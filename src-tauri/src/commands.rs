use tauri::{Emitter, State};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use crate::AppState;
use crate::config::DEFAULT_PB_URL;
use crate::session::{ActivitySnapshot, BreakConfig, NetworkConnection, SessionStatus, SessionState, TodayStats, TeamMember};
use crate::pocketbase::PocketBase;

#[tauri::command]
pub async fn authenticate_pb(
    state: State<'_, AppState>,
    pb_url: String,
    pb_email: String,
    pb_password: String,
) -> Result<serde_json::Value, String> {
    let auth = PocketBase::authenticate(&pb_url, &pb_email, &pb_password)
        .await
        .map_err(|e| e.to_string())?;

    let mut cfg = state.config.lock();
    cfg.pb_url = pb_url;
    cfg.pb_email = pb_email;
    cfg.pb_token = auth.token.clone();
    cfg.user_id = auth.record.id.clone();
    cfg.user_name = auth.record.name.clone();
    cfg.user_email = auth.record.email.clone();
    cfg.token_saved_at = Utc::now().to_rfc3339();
    drop(cfg);

    // Persist to local db
    {
        let cfg = state.config.lock().clone();
        let db = state.db.lock();
        db.save_config(&cfg).map_err(|e| e.to_string())?;
    }

    Ok(json!({
        "token": auth.token,
        "user_id": auth.record.id,
        "user_name": auth.record.name,
        "user_email": auth.record.email,
        "is_admin": auth.record.is_admin,
        "token_saved_at": state.config.lock().token_saved_at,
    }))
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock();
    Ok(json!({
        "pb_url": cfg.pb_url,
        "pb_email": cfg.pb_email,
        "pb_token": cfg.pb_token,
        "user_id": cfg.user_id,
        "user_name": cfg.user_name,
        "user_email": cfg.user_email,
        "token_saved_at": cfg.token_saved_at,
        "default_pb_url": DEFAULT_PB_URL,
    }))
}

#[tauri::command]
pub async fn get_session_state(state: State<'_, AppState>) -> Result<SessionState, String> {
    Ok(state.session.lock().clone())
}

#[tauri::command]
pub async fn clock_in(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    user_id: String,
    pb_token: String,
) -> Result<(), String> {
    let (pb_url, user_name, user_email) = {
        let cfg = state.config.lock();
        (cfg.pb_url.clone(), cfg.user_name.clone(), cfg.user_email.clone())
    };
    let now = Utc::now();

    // If offline (no token/url), generate a local session ID
    let session_id = if pb_token.is_empty() || pb_url.is_empty() {
        format!("local-{}", uuid::Uuid::new_v4())
    } else {
        let pb = PocketBase::new(pb_url, pb_token);
        // Close any stale active sessions for this user first (multi-machine protection)
        pb.close_stale_sessions(&user_id, &now).await.ok();
        pb.create_session(&user_id, &now, &user_name, &user_email).await.map_err(|e| e.to_string())?
    };

    {
        let mut sess = state.session.lock();
        sess.status = SessionStatus::Active;
        sess.session_id = Some(session_id);
        sess.clock_in = Some(now);
        sess.break_start = None;
        sess.total_break_seconds = 0;
        let s = sess.clone();
        drop(sess);
        app.emit("session-update", s).ok();
    }

    // Update tray icon
    update_tray(&app, "active");

    Ok(())
}

#[tauri::command]
pub async fn clock_out(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (pb_url, pb_token, session_id, total_break_seconds) = {
        let cfg = state.config.lock();
        let sess = state.session.lock();
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            sess.session_id.clone().ok_or("No active session")?,
            sess.total_break_seconds,
        )
    };

    let now = Utc::now();

    // Skip PB call for local/offline sessions
    if !session_id.starts_with("local-") && !pb_url.is_empty() && !pb_token.is_empty() {
        let pb = PocketBase::new(pb_url, pb_token);
        pb.close_session(&session_id, &now, total_break_seconds)
            .await
            .map_err(|e| e.to_string())?;
    }

    {
        let mut sess = state.session.lock();
        *sess = crate::session::SessionState::default();
        let s = sess.clone();
        drop(sess);
        app.emit("session-update", s).ok();
    }

    // Reset activity counters
    {
        let mut counters = state.counters.lock();
        *counters = crate::session::ActivityCounters::default();
    }

    update_tray(&app, "idle");
    Ok(())
}

#[tauri::command]
pub async fn start_break(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    break_type: String,
) -> Result<(), String> {
    let (pb_url, pb_token, session_id) = {
        let cfg = state.config.lock();
        let sess = state.session.lock();
        if sess.status != SessionStatus::Active {
            return Err("Not clocked in".into());
        }
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            sess.session_id.clone().ok_or("No active session")?,
        )
    };

    let now = Utc::now();

    let break_id = if session_id.starts_with("local-") || pb_url.is_empty() || pb_token.is_empty() {
        format!("local-break-{}", Uuid::new_v4())
    } else {
        let pb = PocketBase::new(pb_url, pb_token);
        pb.start_break(&session_id, &break_type, &now)
            .await
            .map_err(|e| e.to_string())?
    };

    {
        let mut sess = state.session.lock();
        sess.status = SessionStatus::OnBreak;
        sess.break_start = Some(now);
        sess.break_count += 1;
        let s = sess.clone();
        drop(sess);
        *state.break_id.lock() = Some(break_id);
        app.emit("session-update", s).ok();
    }

    update_tray(&app, "break");
    Ok(())
}

#[tauri::command]
pub async fn end_break(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let break_id = state.break_id.lock().clone().ok_or("No active break")?;
    let (pb_url, pb_token, break_start) = {
        let cfg = state.config.lock();
        let sess = state.session.lock();
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            sess.break_start.ok_or("No break start time")?,
        )
    };

    let now = Utc::now();
    let break_duration = (now - break_start).num_seconds();

    if !break_id.starts_with("local-") && !pb_url.is_empty() && !pb_token.is_empty() {
        let pb = PocketBase::new(pb_url, pb_token);
        pb.end_break(&break_id, &now).await.map_err(|e| e.to_string())?;
    }

    {
        let mut sess = state.session.lock();
        sess.status = SessionStatus::Active;
        sess.break_start = None;
        sess.total_break_seconds += break_duration;
        let s = sess.clone();
        drop(sess);
        *state.break_id.lock() = None;
        app.emit("session-update", s).ok();
    }

    update_tray(&app, "active");
    Ok(())
}

#[tauri::command]
pub async fn get_today_stats(state: State<'_, AppState>) -> Result<TodayStats, String> {
    let (pb_url, pb_token, user_id, sess_elapsed, sess_break_secs, sess_break_count) = {
        let cfg = state.config.lock();
        let sess = state.session.lock();
        let elapsed = sess.clock_in
            .map(|ci| (Utc::now() - ci).num_seconds())
            .unwrap_or(0);
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            cfg.user_id.clone(),
            elapsed,
            sess.total_break_seconds,
            sess.break_count,
        )
    };

    // If offline, return only current session data
    if pb_url.is_empty() || pb_token.is_empty() || user_id.is_empty() {
        let work_secs = (sess_elapsed - sess_break_secs).max(0);
        return Ok(TodayStats {
            session_count: 1,
            total_work_seconds: work_secs,
            break_count: sess_break_count,
            total_break_seconds: sess_break_secs,
        });
    }

    let pb = PocketBase::new(pb_url, pb_token);
    pb.get_today_stats(&user_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_team_status(state: State<'_, AppState>) -> Result<Vec<TeamMember>, String> {
    let (pb_url, pb_token) = {
        let cfg = state.config.lock();
        (cfg.pb_url.clone(), cfg.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let pb = PocketBase::new(pb_url, pb_token);
    pb.get_team_status().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_user_monthly_sessions(
    state: State<'_, AppState>,
    user_id: String,
    year_month: String, // "YYYY-MM"
) -> Result<serde_json::Value, String> {
    let (pb_url, pb_token) = {
        let cfg = state.config.lock();
        (cfg.pb_url.clone(), cfg.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let pb = PocketBase::new(pb_url, pb_token);
    let sessions = pb.get_user_monthly_sessions(&user_id, &year_month)
        .await.map_err(|e| e.to_string())?;

    // Aggregate per day: date → { sessions, total_work_seconds, total_break_seconds }
    let mut days: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
    for s in &sessions {
        let clock_in = s["clock_in"].as_str().unwrap_or("");
        if clock_in.is_empty() { continue; }
        let date = &clock_in[..10]; // "YYYY-MM-DD"
        let clock_out = s["clock_out"].as_str().unwrap_or("");
        let break_secs = s["total_break_seconds"].as_i64().unwrap_or(0);
        let work_secs: i64 = if !clock_in.is_empty() {
            let ci = chrono::DateTime::parse_from_rfc3339(clock_in)
                .or_else(|_| chrono::DateTime::parse_from_str(clock_in, "%Y-%m-%d %H:%M:%S%.3fZ"))
                .map(|d| d.with_timezone(&chrono::Utc)).ok();
            let co = if clock_out.is_empty() {
                Some(chrono::Utc::now())
            } else {
                chrono::DateTime::parse_from_rfc3339(clock_out)
                    .or_else(|_| chrono::DateTime::parse_from_str(clock_out, "%Y-%m-%d %H:%M:%S%.3fZ"))
                    .map(|d| d.with_timezone(&chrono::Utc)).ok()
            };
            match (ci, co) {
                (Some(i), Some(o)) => ((o - i).num_seconds() - break_secs).max(0),
                _ => 0,
            }
        } else { 0 };

        let entry = days.entry(date.to_string()).or_insert_with(|| serde_json::json!({
            "date": date,
            "session_count": 0,
            "total_work_seconds": 0i64,
            "total_break_seconds": 0i64,
            "sessions": []
        }));
        *entry["session_count"].as_i64().get_or_insert(0) += 0; // just update below
        if let Some(obj) = entry.as_object_mut() {
            let sc = obj["session_count"].as_i64().unwrap_or(0) + 1;
            obj["session_count"] = serde_json::json!(sc);
            let tw = obj["total_work_seconds"].as_i64().unwrap_or(0) + work_secs;
            obj["total_work_seconds"] = serde_json::json!(tw);
            let tb = obj["total_break_seconds"].as_i64().unwrap_or(0) + break_secs;
            obj["total_break_seconds"] = serde_json::json!(tb);
        }
    }

    Ok(serde_json::json!(days))
}

#[tauri::command]
pub async fn get_user_activity(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<ActivitySnapshot>, String> {
    let (pb_url, pb_token) = {
        let cfg = state.config.lock();
        (cfg.pb_url.clone(), cfg.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let pb = PocketBase::new(pb_url, pb_token);
    pb.get_session_snapshots(&session_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_user_network(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<NetworkConnection>, String> {
    let (pb_url, pb_token) = {
        let cfg = state.config.lock();
        (cfg.pb_url.clone(), cfg.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let pb = PocketBase::new(pb_url, pb_token);
    pb.get_session_network(&session_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_break_configs(state: State<'_, AppState>) -> Result<Vec<BreakConfig>, String> {
    let (pb_url, pb_token) = {
        let cfg = state.config.lock();
        (cfg.pb_url.clone(), cfg.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Ok(BreakConfig::defaults());
    }
    let pb = PocketBase::new(pb_url, pb_token);
    Ok(pb.get_break_configs().await.unwrap_or_else(|_| BreakConfig::defaults()))
}

#[tauri::command]
pub async fn clear_auth(state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut cfg = state.config.lock();
        cfg.pb_token = String::new();
        cfg.token_saved_at = String::new();
    }
    let cfg = state.config.lock().clone();
    state.db.lock().save_config(&cfg).map_err(|e| e.to_string())
}

fn update_tray(app: &tauri::AppHandle, status: &str) {
    if let Some(tray) = app.tray_by_id("main") {
        let tooltip = match status {
            "active" => "ClankerClocker — Clocked In",
            "break" => "ClankerClocker — On Break",
            _ => "ClankerClocker — Idle",
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
}
