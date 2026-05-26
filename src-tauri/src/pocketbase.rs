use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::session::{ActivitySnapshot, BreakConfig, NetworkConnection, SessionStatus, TeamMember, TodayStats};

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbRecord {
    pub id: String,
}

pub struct PocketBase {
    pub base_url: String,
    pub token: String,
    client: Client,
}

impl PocketBase {
    pub fn new(base_url: String, token: String) -> Self {
        PocketBase {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            client: Client::new(),
        }
    }

    pub async fn authenticate(base_url: &str, email: &str, password: &str) -> Result<PbAuthResponse> {
        let client = Client::new();
        let url = format!("{}/api/collections/users/auth-with-password", base_url.trim_end_matches('/'));
        let resp = client.post(&url)
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
        let resp = self.client.post(&url)
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
        let url = format!("{}/api/collections/{}/records/{}", self.base_url, collection, id);
        let resp = self.client.patch(&url)
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

    pub async fn create_session(&self, user_id: &str, clock_in: &chrono::DateTime<chrono::Utc>) -> Result<String> {
        let rec = self.post("work_sessions", json!({
            "user_id": user_id,
            "clock_in": clock_in.to_rfc3339(),
            "status": "active",
            "total_break_seconds": 0
        })).await?;
        Ok(rec.id)
    }

    pub async fn close_session(
        &self,
        session_id: &str,
        clock_out: &chrono::DateTime<chrono::Utc>,
        total_break_seconds: i64,
    ) -> Result<()> {
        self.patch("work_sessions", session_id, json!({
            "clock_out": clock_out.to_rfc3339(),
            "status": "completed",
            "total_break_seconds": total_break_seconds
        })).await
    }

    pub async fn update_session_status(&self, session_id: &str, status: &SessionStatus) -> Result<()> {
        let s = match status {
            SessionStatus::Active => "active",
            SessionStatus::OnBreak => "on_break",
            SessionStatus::Idle => "completed",
        };
        self.patch("work_sessions", session_id, json!({ "status": s })).await
    }

    pub async fn start_break(&self, session_id: &str, break_type: &str, start: &chrono::DateTime<chrono::Utc>) -> Result<String> {
        let rec = self.post("breaks", json!({
            "session_id": session_id,
            "start_time": start.to_rfc3339(),
            "type": break_type
        })).await?;
        Ok(rec.id)
    }

    pub async fn end_break(&self, break_id: &str, end: &chrono::DateTime<chrono::Utc>) -> Result<()> {
        self.patch("breaks", break_id, json!({ "end_time": end.to_rfc3339() })).await
    }

    pub async fn push_snapshot(&self, session_id: &str, snap: &ActivitySnapshot) -> Result<()> {
        self.post("activity_snapshots", json!({
            "session_id": session_id,
            "timestamp": snap.timestamp.to_rfc3339(),
            "keystrokes": snap.keystrokes,
            "mouse_clicks": snap.mouse_clicks,
            "active_app": snap.active_app,
            "active_window": snap.active_window,
            "idle_seconds": snap.idle_seconds
        })).await?;
        Ok(())
    }

    pub async fn push_network_connection(&self, session_id: &str, conn: &NetworkConnection) -> Result<()> {
        self.post("network_connections", json!({
            "session_id": session_id,
            "timestamp": conn.timestamp.to_rfc3339(),
            "process_name": conn.process_name,
            "remote_host": conn.remote_host,
            "remote_ip": conn.remote_ip,
            "remote_port": conn.remote_port,
            "local_port": conn.local_port
        })).await?;
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
        let resp = self.client.get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("PB list error: {}", resp.status()));
        }
        Ok(resp.json::<Value>().await?)
    }

    /// Get today's work stats for a given user.
    pub async fn get_today_stats(&self, user_id: &str) -> Result<TodayStats> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let filter = format!("user_id='{user_id}'&&clock_in>='{today} 00:00:00'");
        let data = self.get_list("work_sessions", &filter, "").await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();

        let session_count = items.len() as u32;
        let mut total_work_seconds = 0i64;
        let mut session_ids: Vec<String> = Vec::new();

        for item in &items {
            session_ids.push(item["id"].as_str().unwrap_or("").to_string());
            let clock_in = item["clock_in"].as_str().unwrap_or("");
            let clock_out = item["clock_out"].as_str().unwrap_or("");
            let total_break = item["total_break_seconds"].as_i64().unwrap_or(0);

            if !clock_in.is_empty() {
                let start = chrono::DateTime::parse_from_rfc3339(clock_in)
                    .or_else(|_| chrono::DateTime::parse_from_str(clock_in, "%Y-%m-%d %H:%M:%S%.3fZ"))
                    .map(|d| d.with_timezone(&chrono::Utc));
                let end = if clock_out.is_empty() {
                    Ok(chrono::Utc::now())
                } else {
                    chrono::DateTime::parse_from_rfc3339(clock_out)
                        .or_else(|_| chrono::DateTime::parse_from_str(clock_out, "%Y-%m-%d %H:%M:%S%.3fZ"))
                        .map(|d| d.with_timezone(&chrono::Utc))
                };
                if let (Ok(s), Ok(e)) = (start, end) {
                    total_work_seconds += (e - s).num_seconds() - total_break;
                }
            }
        }

        // Count breaks across all today's sessions
        let mut break_count = 0u32;
        let mut total_break_seconds = 0i64;
        if !session_ids.is_empty() {
            let ids_filter = session_ids.iter()
                .map(|id| format!("session_id='{id}'"))
                .collect::<Vec<_>>()
                .join("||");
            if let Ok(bdata) = self.get_list("breaks", &ids_filter, "").await {
                let bitems = bdata["items"].as_array().cloned().unwrap_or_default();
                break_count = bitems.len() as u32;
                for b in &bitems {
                    let bs = b["start_time"].as_str().unwrap_or("");
                    let be = b["end_time"].as_str().unwrap_or("");
                    if !bs.is_empty() && !be.is_empty() {
                        let s = chrono::DateTime::parse_from_rfc3339(bs).ok();
                        let e = chrono::DateTime::parse_from_rfc3339(be).ok();
                        if let (Some(s), Some(e)) = (s, e) {
                            total_break_seconds += (e - s).num_seconds();
                        }
                    }
                }
            }
        }

        Ok(TodayStats { session_count, total_work_seconds, break_count, total_break_seconds })
    }

    pub async fn get_break_configs(&self) -> Result<Vec<BreakConfig>> {
        let data = self.get_list("break_configs", "is_active=true", "&sort=sort_order").await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        let configs = items.iter().map(|item| BreakConfig {
            id: item["id"].as_str().unwrap_or("").to_string(),
            name: item["name"].as_str().unwrap_or("").to_string(),
            type_key: item["type_key"].as_str().unwrap_or("other").to_string(),
            duration_minutes: item["duration_minutes"].as_u64().unwrap_or(0) as u32,
            sort_order: item["sort_order"].as_u64().unwrap_or(0) as u32,
        }).collect();
        Ok(configs)
    }

    async fn get_user(&self, user_id: &str) -> (String, String) {
        let url = format!("{}/api/collections/users/records/{}", self.base_url, user_id);
        let Ok(resp) = self.client.get(&url).bearer_auth(&self.token).send().await else {
            return (String::new(), String::new());
        };
        let Ok(data) = resp.json::<Value>().await else {
            return (String::new(), String::new());
        };
        let name = data["name"].as_str().unwrap_or("").to_string();
        let email = data["email"].as_str().unwrap_or("").to_string();
        (name, email)
    }

    /// Get all currently clocked-in team members (admin view).
    pub async fn get_team_status(&self) -> Result<Vec<TeamMember>> {
        let filter = "status='active'||status='on_break'";
        let data = self.get_list("work_sessions", filter, "").await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();

        let mut members = Vec::new();
        for item in &items {
            let session_id = item["id"].as_str().unwrap_or("").to_string();
            let user_id = item["user_id"].as_str().unwrap_or("").to_string();
            let status_str = item["status"].as_str().unwrap_or("idle");
            let status = match status_str {
                "active" => SessionStatus::Active,
                "on_break" => SessionStatus::OnBreak,
                _ => SessionStatus::Idle,
            };

            let (user_name, user_email) = self.get_user(&user_id).await;

            let clock_in_str = item["clock_in"].as_str().unwrap_or("");
            let clock_in = chrono::DateTime::parse_from_rfc3339(clock_in_str)
                .or_else(|_| chrono::DateTime::parse_from_str(clock_in_str, "%Y-%m-%d %H:%M:%S%.3fZ"))
                .map(|d| d.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            let total_break_seconds = item["total_break_seconds"].as_i64().unwrap_or(0);
            let break_count = item["break_count"].as_u64().unwrap_or(0) as u32;

            // Get last known active app from latest snapshot
            let active_app = self.get_latest_active_app(&session_id).await.unwrap_or_default();

            members.push(TeamMember {
                session_id,
                user_id,
                user_name,
                user_email,
                status,
                clock_in,
                total_break_seconds,
                break_count,
                active_app,
            });
        }

        Ok(members)
    }

    /// Returns all sessions for a user in a given month (YYYY-MM).
    pub async fn get_user_monthly_sessions(&self, user_id: &str, year_month: &str) -> Result<Vec<serde_json::Value>> {
        let filter = format!("user_id='{user_id}'&&clock_in>='{year_month}-01 00:00:00'&&clock_in<'{year_month}-32 00:00:00'");
        let data = self.get_list("work_sessions", &filter, "&sort=clock_in&perPage=200").await?;
        Ok(data["items"].as_array().cloned().unwrap_or_default())
    }

    pub async fn get_session_snapshots(&self, session_id: &str) -> Result<Vec<ActivitySnapshot>> {
        let filter = format!("session_id='{session_id}'");
        let data = self.get_list("activity_snapshots", &filter, "&sort=timestamp&perPage=500").await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        let snaps = items.iter().filter_map(|item| {
            let ts_str = item["timestamp"].as_str()?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(ts_str)
                .or_else(|_| chrono::DateTime::parse_from_str(ts_str, "%Y-%m-%d %H:%M:%S%.3fZ"))
                .ok()?.with_timezone(&chrono::Utc);
            Some(ActivitySnapshot {
                timestamp,
                keystrokes: item["keystrokes"].as_u64().unwrap_or(0),
                mouse_clicks: item["mouse_clicks"].as_u64().unwrap_or(0),
                mouse_distance_px: item["mouse_distance_px"].as_f64().unwrap_or(0.0),
                active_app: item["active_app"].as_str().unwrap_or("").to_string(),
                active_window: item["active_window"].as_str().unwrap_or("").to_string(),
                idle_seconds: item["idle_seconds"].as_u64().unwrap_or(0),
            })
        }).collect();
        Ok(snaps)
    }

    pub async fn get_session_network(&self, session_id: &str) -> Result<Vec<NetworkConnection>> {
        let filter = format!("session_id='{session_id}'");
        let data = self.get_list("network_connections", &filter, "&sort=timestamp&perPage=500").await?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        let conns = items.iter().filter_map(|item| {
            let ts_str = item["timestamp"].as_str()?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(ts_str)
                .or_else(|_| chrono::DateTime::parse_from_str(ts_str, "%Y-%m-%d %H:%M:%S%.3fZ"))
                .ok()?.with_timezone(&chrono::Utc);
            Some(NetworkConnection {
                timestamp,
                process_name: item["process_name"].as_str().unwrap_or("").to_string(),
                remote_host: item["remote_host"].as_str().unwrap_or("").to_string(),
                remote_ip: item["remote_ip"].as_str().unwrap_or("").to_string(),
                remote_port: item["remote_port"].as_u64().unwrap_or(0) as u16,
                local_port: item["local_port"].as_u64().unwrap_or(0) as u16,
            })
        }).collect();
        Ok(conns)
    }

    async fn get_latest_active_app(&self, session_id: &str) -> Result<String> {
        let url = format!(
            "{}/api/collections/activity_snapshots/records?filter={}&sort=-timestamp&perPage=1",
            self.base_url,
            urlencoding::encode(&format!("session_id='{session_id}'")),
        );
        let resp = self.client.get(&url).bearer_auth(&self.token).send().await?;
        if !resp.status().is_success() { return Ok(String::new()); }
        let data: Value = resp.json().await?;
        Ok(data["items"][0]["active_app"].as_str().unwrap_or("").to_string())
    }
}

