use crate::session::{
    ActivitySnapshot, BreakConfig, NetworkConnection, SessionStatus, TeamMember, TodayBreakdown,
    TodaySessionBreakdown, TodayStats,
};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbAuthResponse {
    pub token: String,
    pub record: PbUser,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbUser {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub clock_in_time: String,
    #[serde(default)]
    pub clock_out_time: String,
    #[serde(default)]
    pub auto_clock_out_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbRecord {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbUserRecord {
    pub id: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub clock_in_time: String,
    #[serde(default)]
    pub clock_out_time: String,
    #[serde(default)]
    pub auto_clock_out_enabled: bool,
}

#[derive(Clone)]
pub struct PocketBase {
    pub base_url: String,
    pub token: String,
    client: Client,
}

impl PocketBase {
    pub fn new(base_url: String, token: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        PocketBase {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            client,
        }
    }

    pub async fn authenticate(
        base_url: &str,
        email: &str,
        password: &str,
    ) -> Result<PbAuthResponse> {
        let client = Client::new();
        let url = format!(
            "{}/api/collections/users/auth-with-password",
            base_url.trim_end_matches('/')
        );
        let resp = client
            .post(&url)
            .json(&json!({ "identity": email, "password": password }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Auth failed: {}", text));
        }
        Ok(resp.json::<PbAuthResponse>().await?)
    }

    async fn post(&self, collection: &str, body: Value) -> Result<PbRecord> {
        let url = format!("{}/api/collections/{}/records", self.base_url, collection);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("PB error: {}", text));
        }
        Ok(resp.json::<PbRecord>().await?)
    }

    async fn patch(&self, collection: &str, id: &str, body: Value) -> Result<()> {
        let url = format!(
            "{}/api/collections/{}/records/{}",
            self.base_url, collection, id
        );
        let resp = self
            .client
            .patch(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("PB patch error: {}", text));
        }
        Ok(())
    }

    async fn get_record<T: for<'de> Deserialize<'de>>(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<T> {
        let url = format!(
            "{}/api/collections/{}/records/{}",
            self.base_url, collection, id
        );
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("PB get error: {}", text));
        }
        Ok(resp.json::<T>().await?)
    }

    pub async fn create_session(
        &self,
        user_id: &str,
        clock_in: &chrono::DateTime<chrono::Utc>,
        user_name: &str,
        user_email: &str,
    ) -> Result<String> {
        let rec = self
            .post(
                "work_sessions",
                json!({
                    "user_id": user_id,
                    "clock_in": clock_in.to_rfc3339(),
                    "status": "active",
                    "total_break_seconds": 0,
                    "user_name": user_name,
                    "user_email": user_email,
                }),
            )
            .await?;
        Ok(rec.id)
    }

    pub async fn get_company_settings(&self) -> Result<Value> {
        // Fetch the first record from the 'company_config' collection
        let data = self.get_list("company_config", "", "&perPage=1").await?;
        let items = data["items"]
            .as_array()
            .ok_or_else(|| anyhow!("Invalid response"))?;
        if items.is_empty() {
            return Err(anyhow!("No company settings found"));
        }
        Ok(items[0].clone())
    }

    pub async fn update_company_settings(
        &self,
        id: &str,
        clock_in: &str,
        clock_out: &str,
        auto_out: bool,
    ) -> Result<()> {
        self.patch(
            "company_config",
            id,
            json!({
                "clock_in_time": clock_in,
                "clock_out_time": clock_out,
                "auto_clock_out_enabled": auto_out,
            }),
        )
        .await
    }

    pub async fn get_user_record(&self, user_id: &str) -> Result<PbUserRecord> {
        self.get_record("users", user_id).await
    }

    /// Close any active/on_break sessions for a user (called before creating a new session).
    pub async fn close_stale_sessions(
        &self,
        user_id: &str,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let filter = format!("user_id='{user_id}'&&(status='active'||status='on_break')");
        let data = self
            .get_list("work_sessions", &filter, "&sort=clock_in")
            .await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        for item in &items {
            let id = item["id"].as_str().unwrap_or("");
            if !id.is_empty() {
                let break_secs = item["total_break_seconds"].as_i64().unwrap_or(0);
                self.close_session(id, now, break_secs).await.ok();
            }
        }
        Ok(())
    }

    pub async fn close_session(
        &self,
        session_id: &str,
        clock_out: &chrono::DateTime<chrono::Utc>,
        total_break_seconds: i64,
    ) -> Result<()> {
        self.patch(
            "work_sessions",
            session_id,
            json!({
                "clock_out": clock_out.to_rfc3339(),
                "status": "completed",
                "total_break_seconds": total_break_seconds
            }),
        )
        .await
    }

    pub async fn update_session_status(
        &self,
        session_id: &str,
        status: &SessionStatus,
    ) -> Result<()> {
        let s = match status {
            SessionStatus::Active => "active",
            SessionStatus::OnBreak => "on_break",
            SessionStatus::Idle => "completed",
        };
        self.patch("work_sessions", session_id, json!({ "status": s }))
            .await
    }

    pub async fn update_session_break_metrics(
        &self,
        session_id: &str,
        break_count: u32,
        total_break_seconds: i64,
    ) -> Result<()> {
        self.patch(
            "work_sessions",
            session_id,
            json!({
                "break_count": break_count,
                "total_break_seconds": total_break_seconds,
            }),
        )
        .await
    }

    pub async fn start_break(
        &self,
        session_id: &str,
        break_type: &str,
        start: &chrono::DateTime<chrono::Utc>,
    ) -> Result<String> {
        let rec = self
            .post(
                "breaks",
                json!({
                    "session_id": session_id,
                    "start_time": start.to_rfc3339(),
                    "type": break_type
                }),
            )
            .await?;
        Ok(rec.id)
    }

    pub async fn end_break(
        &self,
        break_id: &str,
        end: &chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        self.patch("breaks", break_id, json!({ "end_time": end.to_rfc3339() }))
            .await
    }

    pub async fn close_open_breaks(
        &self,
        session_id: &str,
        end: &chrono::DateTime<chrono::Utc>,
    ) -> Result<i64> {
        let filter = format!("session_id='{session_id}'&&end_time=''");
        let data = self.get_list("breaks", &filter, "").await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        let mut closed_seconds = 0i64;

        for item in items {
            let break_id = item["id"].as_str().unwrap_or("");
            let start_time = item["start_time"].as_str().unwrap_or("");
            let Some(start) = Self::parse_pb_datetime(start_time) else {
                continue;
            };
            if break_id.is_empty() {
                continue;
            }

            let secs = (end.signed_duration_since(start)).num_seconds().max(0);
            closed_seconds += secs;
            self.end_break(break_id, end).await?;
        }

        Ok(closed_seconds)
    }

    pub async fn push_snapshot(&self, session_id: &str, snap: &ActivitySnapshot) -> Result<()> {
        self.post(
            "activity_snapshots",
            json!({
                "session_id": session_id,
                "timestamp": snap.timestamp.to_rfc3339(),
                "keystrokes": snap.keystrokes,
                "mouse_clicks": snap.mouse_clicks,
                "active_app": snap.active_app,
                "active_window": snap.active_window,
                "idle_seconds": snap.idle_seconds
            }),
        )
        .await?;
        Ok(())
    }

    pub async fn push_network_connection(
        &self,
        session_id: &str,
        conn: &NetworkConnection,
    ) -> Result<()> {
        self.post(
            "network_connections",
            json!({
                "session_id": session_id,
                "timestamp": conn.timestamp.to_rfc3339(),
                "process_name": conn.process_name,
                "remote_host": conn.remote_host,
                "remote_ip": conn.remote_ip,
                "remote_port": conn.remote_port,
                "local_port": conn.local_port
            }),
        )
        .await?;
        Ok(())
    }

    async fn get_list(&self, collection: &str, filter: &str, extra: &str) -> Result<Value> {
        let url = format!(
            "{}/api/collections/{}/records?filter={}&perPage=200{}",
            self.base_url,
            collection,
            urlencoding::encode(filter),
            extra,
        );
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("PB list error: {}", resp.status()));
        }
        Ok(resp.json::<Value>().await?)
    }

    fn parse_pb_datetime(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        if value.is_empty() {
            return None;
        }
        chrono::DateTime::parse_from_rfc3339(value)
            .or_else(|_| chrono::DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.3fZ"))
            .ok()
            .map(|d| d.with_timezone(&chrono::Utc))
    }

    fn timestamp_is_within_break(
        timestamp: chrono::DateTime<chrono::Utc>,
        intervals: &[(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)],
    ) -> bool {
        intervals
            .iter()
            .any(|(start, end)| timestamp >= *start && timestamp < *end)
    }

    /// Get today's work stats for a given user.
    async fn get_today_breakdown_data(&self, user_id: &str) -> Result<TodayBreakdown> {
        // "Today" follows Nepal time (the company's operating timezone), not UTC —
        // otherwise sessions started shortly after Nepal midnight (which is still
        // daytime UTC-wise) would be counted as "yesterday".
        use chrono::TimeZone;
        let nepal_offset =
            chrono::FixedOffset::east_opt(5 * 3600 + 45 * 60).expect("valid Nepal offset");
        let now_nepal = chrono::Utc::now().with_timezone(&nepal_offset);
        let nepal_midnight = now_nepal.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let boundary_utc = nepal_offset
            .from_local_datetime(&nepal_midnight)
            .single()
            .unwrap()
            .with_timezone(&chrono::Utc);
        let sessions = self
            .get_sessions_in_range(&boundary_utc, &chrono::Utc::now(), Some(user_id))
            .await?;

        let session_count = sessions.len() as u32;
        let mut total_work_seconds = 0i64;
        let mut total_break_seconds = 0i64;
        let mut total_net_loss_seconds = 0i64;
        let mut break_count = 0u32;
        let mut session_summaries: Vec<TodaySessionBreakdown> = Vec::with_capacity(sessions.len());

        for s in sessions {
            total_work_seconds += s.net_seconds;
            total_break_seconds += s.break_seconds;
            total_net_loss_seconds += s.net_loss_seconds;
            break_count += s.break_count;
            session_summaries.push(TodaySessionBreakdown {
                session_id: s.session_id,
                clock_in: s.clock_in,
                clock_out: s.clock_out,
                gross_seconds: s.gross_seconds,
                break_seconds: s.break_seconds,
                net_seconds: s.net_seconds,
                net_loss_seconds: s.net_loss_seconds,
            });
        }

        Ok(TodayBreakdown {
            session_count,
            total_work_seconds,
            break_count,
            total_break_seconds,
            total_net_loss_seconds,
            sessions: session_summaries,
        })
    }

    pub async fn get_today_stats(&self, user_id: &str) -> Result<TodayStats> {
        let data = self.get_today_breakdown_data(user_id).await?;

        Ok(TodayStats {
            session_count: data.session_count,
            total_work_seconds: data.total_work_seconds,
            break_count: data.break_count,
            total_break_seconds: data.total_break_seconds,
            total_net_loss_seconds: data.total_net_loss_seconds,
        })
    }

    pub async fn get_today_breakdown(&self, user_id: &str) -> Result<TodayBreakdown> {
        self.get_today_breakdown_data(user_id).await
    }

    pub async fn get_break_configs(&self) -> Result<Vec<BreakConfig>> {
        let data = self
            .get_list("break_configs", "is_active=true", "&sort=sort_order")
            .await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        let configs = items
            .iter()
            .map(|item| BreakConfig {
                id: item["id"].as_str().unwrap_or("").to_string(),
                name: item["name"].as_str().unwrap_or("").to_string(),
                type_key: item["type_key"].as_str().unwrap_or("other").to_string(),
                duration_minutes: item["duration_minutes"].as_u64().unwrap_or(0) as u32,
                sort_order: item["sort_order"].as_u64().unwrap_or(0) as u32,
                auto_start_enabled: item["auto_start_enabled"].as_bool().unwrap_or(false),
                auto_start_time: item["auto_start_time"]
                    .as_str()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty()),
                auto_end_time: item["auto_end_time"]
                    .as_str()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty()),
            })
            .collect();
        Ok(configs)
    }

    /// Get all currently clocked-in team members (admin view).
    pub async fn get_team_status(&self) -> Result<Vec<TeamMember>> {
        let filter = "status='active'||status='on_break'";
        let data = self.get_list("work_sessions", filter, "").await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();

        let mut tasks = tokio::task::JoinSet::new();
        for (idx, item) in items.into_iter().enumerate() {
            let pb = self.clone();
            tasks.spawn(async move {
                let session_id = item["id"].as_str().unwrap_or("").to_string();
                let user_id = item["user_id"].as_str().unwrap_or("").to_string();
            let status_str = item["status"].as_str().unwrap_or("idle");
            let status = match status_str {
                "active" => SessionStatus::Active,
                "on_break" => SessionStatus::OnBreak,
                _ => SessionStatus::Idle,
            };

            // Read name/email stored on session at clock-in (avoids cross-user PB access rules)
                let mut user_name = item["user_name"].as_str().unwrap_or("").to_string();
                let mut user_email = item["user_email"].as_str().unwrap_or("").to_string();

                let user_fut = pb.get_user_record(&user_id);
                let active_app_fut = pb.get_latest_active_app(&session_id);
                let today_stats_fut = pb.get_today_stats(&user_id);
                let (user_res, active_app_res, today_res) =
                    tokio::join!(user_fut, active_app_fut, today_stats_fut);

                if let Ok(user) = user_res {
                if !user.email.is_empty() {
                    user_email = user.email;
                }
                if !user.name.trim().is_empty() {
                    user_name = user.name.trim().to_string();
                }
            }

                if user_name.is_empty() && !user_email.is_empty() {
                    user_name = user_email.split('@').next().unwrap_or("").to_string();
                }
                if user_email.is_empty() {
                    user_email = user_id.clone();
                }

                let clock_in_str = item["clock_in"].as_str().unwrap_or("");
                let clock_in = Self::parse_pb_datetime(clock_in_str).unwrap_or_else(chrono::Utc::now);

                let total_break_seconds = item["total_break_seconds"].as_i64().unwrap_or(0);
                let break_count = item["break_count"].as_u64().unwrap_or(0) as u32;
                let active_app = active_app_res.unwrap_or_default();
                let (today_total_work_seconds, today_total_break_seconds) = today_res
                    .map(|s| (s.total_work_seconds, s.total_break_seconds))
                    .unwrap_or((0, 0));

                (
                    idx,
                    TeamMember {
                        session_id,
                        user_id,
                        user_name,
                        user_email,
                        status,
                        clock_in,
                        total_break_seconds,
                        break_count,
                        active_app,
                        today_total_work_seconds,
                        today_total_break_seconds,
                    },
                )
            });
        }

        let mut raw_members = Vec::new();
        while let Some(result) = tasks.join_next().await {
            let (_, member) = result.map_err(|e| anyhow!("team status task failed: {e}"))?;
            raw_members.push(member);
        }

        // A user can have multiple active sessions if a previous client crashed
        // before closing its session. Keep only the most recent session per user.
        let mut by_user: HashMap<String, TeamMember> = HashMap::new();
        for member in raw_members {
            let keep = match by_user.get(&member.user_id) {
                None => true,
                Some(existing) => member.clock_in > existing.clock_in,
            };
            if keep {
                by_user.insert(member.user_id.clone(), member);
            }
        }

        let mut members: Vec<TeamMember> = by_user.into_values().collect();
        members.sort_by_key(|m| m.clock_in);

        Ok(members)
    }

    /// Returns all sessions for a user in a given month (YYYY-MM).
    pub async fn get_user_monthly_sessions(
        &self,
        user_id: &str,
        year_month: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let filter = format!("user_id='{user_id}'&&clock_in>='{year_month}-01 00:00:00'&&clock_in<'{year_month}-32 00:00:00'");
        let data = self
            .get_list("work_sessions", &filter, "&sort=clock_in&perPage=200")
            .await?;
        Ok(data["items"].as_array().cloned().unwrap_or_default())
    }

    pub async fn get_session_snapshots(&self, session_id: &str) -> Result<Vec<ActivitySnapshot>> {
        let filter = format!("session_id='{session_id}'");
        let data = self
            .get_list("activity_snapshots", &filter, "&sort=timestamp&perPage=2000")
            .await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        let snaps = items
            .iter()
            .filter_map(|item| {
                let ts_str = item["timestamp"].as_str()?;
                let timestamp = chrono::DateTime::parse_from_rfc3339(ts_str)
                    .or_else(|_| chrono::DateTime::parse_from_str(ts_str, "%Y-%m-%d %H:%M:%S%.3fZ"))
                    .ok()?
                    .with_timezone(&chrono::Utc);
                Some(ActivitySnapshot {
                    timestamp,
                    keystrokes: item["keystrokes"].as_u64().unwrap_or(0),
                    mouse_clicks: item["mouse_clicks"].as_u64().unwrap_or(0),
                    mouse_distance_px: item["mouse_distance_px"].as_f64().unwrap_or(0.0),
                    active_app: item["active_app"].as_str().unwrap_or("").to_string(),
                    active_window: item["active_window"].as_str().unwrap_or("").to_string(),
                    idle_seconds: item["idle_seconds"].as_u64().unwrap_or(0),
                })
            })
            .collect();
        Ok(snaps)
    }

    pub async fn get_session_network(&self, session_id: &str) -> Result<Vec<NetworkConnection>> {
        let filter = format!("session_id='{session_id}'");
        let data = self
            .get_list(
                "network_connections",
                &filter,
                "&sort=timestamp&perPage=500",
            )
            .await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        let conns = items
            .iter()
            .filter_map(|item| {
                let ts_str = item["timestamp"].as_str()?;
                let timestamp = chrono::DateTime::parse_from_rfc3339(ts_str)
                    .or_else(|_| chrono::DateTime::parse_from_str(ts_str, "%Y-%m-%d %H:%M:%S%.3fZ"))
                    .ok()?
                    .with_timezone(&chrono::Utc);
                Some(NetworkConnection {
                    timestamp,
                    process_name: item["process_name"].as_str().unwrap_or("").to_string(),
                    remote_host: item["remote_host"].as_str().unwrap_or("").to_string(),
                    remote_ip: item["remote_ip"].as_str().unwrap_or("").to_string(),
                    remote_port: item["remote_port"].as_u64().unwrap_or(0) as u16,
                    local_port: item["local_port"].as_u64().unwrap_or(0) as u16,
                })
            })
            .collect();
        Ok(conns)
    }

    pub async fn get_all_users(&self) -> Result<Vec<crate::session::UserInfo>> {
        // The users collection list rule restricts non-superadmin tokens to their own
        // record. Derive the full user list from work_sessions instead, which has
        // permissive read rules for authenticated admins.
        let data = self
            .get_list("work_sessions", "", "&sort=-clock_in&perPage=500")
            .await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        let mut users: Vec<crate::session::UserInfo> = items
            .iter()
            .filter_map(|item| {
                let id = item["user_id"].as_str().unwrap_or("").to_string();
                if id.is_empty() || !seen.insert(id.clone()) {
                    return None;
                }
                let email = item["user_email"].as_str().unwrap_or("").to_string();
                let raw_name = item["user_name"].as_str().unwrap_or("").trim().to_string();
                let name = if raw_name.is_empty() {
                    email.split('@').next().unwrap_or("").to_string()
                } else {
                    raw_name
                };
                Some(crate::session::UserInfo { id, name, email, is_admin: false })
            })
            .collect();
        users.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(users)
    }

    pub async fn get_sessions_in_range(
        &self,
        from: &chrono::DateTime<chrono::Utc>,
        to: &chrono::DateTime<chrono::Utc>,
        user_id: Option<&str>,
    ) -> Result<Vec<crate::session::SessionRecord>> {
        let mut filter = format!(
            "clock_in>='{}'&&clock_in<='{}'",
            from.format("%Y-%m-%d %H:%M:%S"),
            to.format("%Y-%m-%d %H:%M:%S"),
        );
        if let Some(uid) = user_id {
            if !uid.is_empty() {
                filter.push_str(&format!("&&user_id='{uid}'"));
            }
        }
        let data = self
            .get_list("work_sessions", &filter, "&sort=clock_in&perPage=500")
            .await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();

        let session_ids: Vec<String> = items
            .iter()
            .filter_map(|item| {
                let id = item["id"].as_str().unwrap_or("");
                if id.is_empty() { None } else { Some(id.to_string()) }
            })
            .collect();

        // Batch-fetch breaks in groups of 15 to stay within URL limits.
        let mut break_secs_by: HashMap<String, i64> = HashMap::new();
        let mut break_intervals_by: HashMap<
            String,
            Vec<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>,
        > = HashMap::new();
        for chunk in session_ids.chunks(15) {
            let f = chunk
                .iter()
                .map(|id| format!("session_id='{id}'"))
                .collect::<Vec<_>>()
                .join("||");
            if let Ok(bd) = self.get_list("breaks", &f, "&perPage=500").await {
                for b in bd["items"].as_array().cloned().unwrap_or_default() {
                    let bs = b["start_time"].as_str().unwrap_or("");
                    let be = b["end_time"].as_str().unwrap_or("");
                    let sid = b["session_id"].as_str().unwrap_or("").to_string();
                    if bs.is_empty() || sid.is_empty() { continue; }
                    if let Some(s) = Self::parse_pb_datetime(bs) {
                        let e = if be.is_empty() {
                            chrono::Utc::now()
                        } else {
                            Self::parse_pb_datetime(be).unwrap_or_else(chrono::Utc::now)
                        };
                        *break_secs_by.entry(sid.clone()).or_insert(0) +=
                            (e - s).num_seconds().max(0);
                        break_intervals_by.entry(sid).or_default().push((s, e));
                    }
                }
            }
        }

        let now = chrono::Utc::now();
        let mut net_loss_by: HashMap<String, i64> = HashMap::new();
        for chunk in session_ids.chunks(10) {
            let f = chunk
                .iter()
                .map(|id| format!("session_id='{id}'"))
                .collect::<Vec<_>>()
                .join("||");
            if let Ok(sdata) = self
                .get_list("activity_snapshots", &f, "&sort=timestamp&perPage=2000")
                .await
            {
                for item in sdata["items"].as_array().cloned().unwrap_or_default() {
                    let sid = item["session_id"].as_str().unwrap_or("").to_string();
                    let ts = item["timestamp"].as_str().unwrap_or("");
                    if sid.is_empty() || ts.is_empty() {
                        continue;
                    }
                    let Some(timestamp) = Self::parse_pb_datetime(ts) else {
                        continue;
                    };
                    let Some(idle_seconds) = item["idle_seconds"].as_u64() else {
                        continue;
                    };
                    if let Some(intervals) = break_intervals_by.get(&sid) {
                        if Self::timestamp_is_within_break(timestamp, intervals) {
                            continue;
                        }
                    }
                    let loss = std::cmp::min(idle_seconds as i64, 30);
                    *net_loss_by.entry(sid).or_insert(0) += loss;
                }
            }
        }

        Ok(items
            .iter()
            .filter_map(|item| {
                let session_id = item["id"].as_str()?.to_string();
                if session_id.is_empty() { return None; }
                let clock_in =
                    Self::parse_pb_datetime(item["clock_in"].as_str().unwrap_or(""))?;
                let clock_out_str = item["clock_out"].as_str().unwrap_or("");
                let clock_out = if clock_out_str.is_empty() {
                    None
                } else {
                    Self::parse_pb_datetime(clock_out_str)
                };
                let effective_end = clock_out.unwrap_or(now);
                let gross_seconds = (effective_end - clock_in).num_seconds().max(0);
                let break_seconds =
                    break_secs_by.get(&session_id).copied().unwrap_or(0);
                let net_seconds = (gross_seconds - break_seconds).max(0);
                let net_loss_seconds = net_loss_by.get(&session_id).copied().unwrap_or(0);
                let user_email = item["user_email"].as_str().unwrap_or("").to_string();
                let raw_name = item["user_name"].as_str().unwrap_or("").trim().to_string();
                let user_name = if raw_name.is_empty() {
                    user_email.split('@').next().unwrap_or("").to_string()
                } else {
                    raw_name
                };
                Some(crate::session::SessionRecord {
                    session_id: session_id.clone(),
                    user_id: item["user_id"].as_str().unwrap_or("").to_string(),
                    user_name,
                    user_email,
                    clock_in,
                    clock_out,
                    status: item["status"].as_str().unwrap_or("completed").to_string(),
                    gross_seconds,
                    break_seconds,
                    net_seconds,
                    net_loss_seconds,
                    break_count: item["break_count"].as_u64().unwrap_or(0) as u32,
                })
            })
            .collect())
    }

    pub async fn get_network_in_range(
        &self,
        from: &chrono::DateTime<chrono::Utc>,
        to: &chrono::DateTime<chrono::Utc>,
        user_id: Option<&str>,
    ) -> Result<crate::session::NetworkReport> {
        let sessions = self.get_sessions_in_range(from, to, user_id).await?;
        let session_ids: Vec<String> =
            sessions.iter().map(|s| s.session_id.clone()).collect();

        let mut all_records: Vec<crate::session::NetworkRecord> = Vec::new();
        let mut host_counts: HashMap<String, u32> = HashMap::new();
        let mut proc_counts: HashMap<String, u32> = HashMap::new();

        for chunk in session_ids.chunks(10) {
            let f = chunk
                .iter()
                .map(|id| format!("session_id='{id}'"))
                .collect::<Vec<_>>()
                .join("||");
            if let Ok(data) = self
                .get_list("network_connections", &f, "&sort=timestamp&perPage=500")
                .await
            {
                for item in data["items"].as_array().cloned().unwrap_or_default() {
                    let ts_str = item["timestamp"].as_str().unwrap_or("");
                    let Some(timestamp) = Self::parse_pb_datetime(ts_str) else {
                        continue;
                    };
                    let sid = item["session_id"].as_str().unwrap_or("").to_string();
                    let sess = sessions.iter().find(|s| s.session_id == sid);
                    let remote_host =
                        item["remote_host"].as_str().unwrap_or("").to_string();
                    let remote_ip =
                        item["remote_ip"].as_str().unwrap_or("").to_string();
                    let process_name =
                        item["process_name"].as_str().unwrap_or("").to_string();

                    let host_key = if !remote_host.is_empty() {
                        remote_host.clone()
                    } else {
                        remote_ip.clone()
                    };
                    if !host_key.is_empty() {
                        *host_counts.entry(host_key).or_insert(0) += 1;
                    }
                    if !process_name.is_empty() {
                        *proc_counts.entry(process_name.clone()).or_insert(0) += 1;
                    }

                    all_records.push(crate::session::NetworkRecord {
                        timestamp,
                        user_id: sess
                            .map(|s| s.user_id.clone())
                            .unwrap_or_default(),
                        user_name: sess
                            .map(|s| s.user_name.clone())
                            .unwrap_or_default(),
                        user_email: sess
                            .map(|s| s.user_email.clone())
                            .unwrap_or_default(),
                        session_id: sid,
                        process_name,
                        remote_host,
                        remote_ip,
                        remote_port: item["remote_port"].as_u64().unwrap_or(0) as u16,
                        local_port: item["local_port"].as_u64().unwrap_or(0) as u16,
                    });
                }
            }
        }

        let mut top_hosts: Vec<crate::session::NetworkStat> = host_counts
            .into_iter()
            .map(|(name, count)| crate::session::NetworkStat { name, count })
            .collect();
        top_hosts.sort_by(|a, b| b.count.cmp(&a.count));
        top_hosts.truncate(20);

        let mut top_processes: Vec<crate::session::NetworkStat> = proc_counts
            .into_iter()
            .map(|(name, count)| crate::session::NetworkStat { name, count })
            .collect();
        top_processes.sort_by(|a, b| b.count.cmp(&a.count));
        top_processes.truncate(10);

        Ok(crate::session::NetworkReport {
            records: all_records,
            top_hosts,
            top_processes,
        })
    }

    pub async fn get_activity_in_range(
        &self,
        from: &chrono::DateTime<chrono::Utc>,
        to: &chrono::DateTime<chrono::Utc>,
        user_id: &str,
    ) -> Result<crate::session::ActivityReport> {
        let sessions = self.get_sessions_in_range(from, to, Some(user_id)).await?;
        let session_ids: Vec<String> =
            sessions.iter().map(|s| s.session_id.clone()).collect();
        let session_count = session_ids.len() as u32;

        let mut total_keystrokes: u64 = 0;
        let mut total_clicks: u64 = 0;
        let mut total_snaps: u32 = 0;
        let mut idle_snaps: u32 = 0;
        let mut app_seconds: HashMap<String, i64> = HashMap::new();

        for chunk in session_ids.chunks(10) {
            let f = chunk
                .iter()
                .map(|id| format!("session_id='{id}'"))
                .collect::<Vec<_>>()
                .join("||");
            if let Ok(data) = self
                .get_list("activity_snapshots", &f, "&sort=timestamp&perPage=2000")
                .await
            {
                for item in data["items"].as_array().cloned().unwrap_or_default() {
                    total_snaps += 1;
                    total_keystrokes += item["keystrokes"].as_u64().unwrap_or(0);
                    total_clicks += item["mouse_clicks"].as_u64().unwrap_or(0);
                    let idle = item["idle_seconds"].as_u64().unwrap_or(0);
                    if idle >= 60 {
                        idle_snaps += 1;
                    } else {
                        let app = item["active_app"].as_str().unwrap_or("").to_string();
                        if !app.is_empty() {
                            *app_seconds.entry(app).or_insert(0) += 30;
                        }
                    }
                }
            }
        }

        let idle_pct = if total_snaps > 0 {
            idle_snaps as f32 / total_snaps as f32 * 100.0
        } else {
            0.0
        };
        let total_tracked: i64 = app_seconds.values().sum();
        let mut top_apps: Vec<crate::session::AppUsage> = app_seconds
            .into_iter()
            .map(|(app, seconds)| crate::session::AppUsage {
                pct: if total_tracked > 0 {
                    seconds as f32 / total_tracked as f32 * 100.0
                } else {
                    0.0
                },
                app,
                seconds,
            })
            .collect();
        top_apps.sort_by(|a, b| b.seconds.cmp(&a.seconds));
        top_apps.truncate(10);

        Ok(crate::session::ActivityReport {
            total_keystrokes,
            total_clicks,
            idle_pct,
            top_apps,
            session_count,
            total_snapshot_count: total_snaps,
        })
    }

    async fn get_latest_active_app(&self, session_id: &str) -> Result<String> {
        let url = format!(
            "{}/api/collections/activity_snapshots/records?filter={}&sort=-timestamp&perPage=1",
            self.base_url,
            urlencoding::encode(&format!("session_id='{session_id}'")),
        );
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(String::new());
        }
        let data: Value = resp.json().await?;
        Ok(data["items"][0]["active_app"]
            .as_str()
            .unwrap_or("")
            .to_string())
    }
}
