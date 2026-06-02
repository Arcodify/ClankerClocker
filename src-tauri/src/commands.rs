use crate::config::DEFAULT_PB_URL;
use crate::pocketbase::PocketBase;
use crate::session::{
    ActivityCounters, ActivitySnapshot, AppNotification, BreakConfig, NetworkConnection,
    SessionState, SessionStatus, TeamMember, TodayStats,
};
use crate::AppState;
use chrono::Utc;
use serde_json::json;
use tauri::{Emitter, State};
use uuid::Uuid;

pub async fn start_break_internal(
    app: &tauri::AppHandle,
    session: &std::sync::Arc<parking_lot::Mutex<SessionState>>,
    config: &std::sync::Arc<parking_lot::Mutex<crate::session::AppConfig>>,
    break_id_state: &std::sync::Arc<parking_lot::Mutex<Option<String>>>,
    break_type: &str,
    break_name: Option<&str>,
) -> Result<(), String> {
    let (pb_url, pb_token, session_id) = {
        let cfg = config.lock();
        let sess = session.lock();
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
        let pb = PocketBase::new(pb_url.clone(), pb_token.clone());
        pb.start_break(&session_id, break_type, &now)
            .await
            .map_err(|e| e.to_string())?
    };

    let (break_count, total_break_seconds) = {
        let mut sess = session.lock();
        sess.status = SessionStatus::OnBreak;
        sess.break_start = Some(now);
        sess.break_name = break_name
            .map(|name| name.to_string())
            .or_else(|| Some(break_type.to_string()));
        sess.break_count += 1;
        let s = sess.clone();
        let break_count = s.break_count;
        let total_break_seconds = s.total_break_seconds;
        drop(sess);
        *break_id_state.lock() = Some(break_id);
        app.emit("session-update", s).ok();
        (break_count, total_break_seconds)
    };

    if !session_id.starts_with("local-") && !pb_url.is_empty() && !pb_token.is_empty() {
        let pb = PocketBase::new(pb_url, pb_token);
        pb.update_session_status(&session_id, &SessionStatus::OnBreak)
            .await
            .ok();
        pb.update_session_break_metrics(&session_id, break_count, total_break_seconds)
            .await
            .ok();
    }

    let label = break_name.unwrap_or(break_type);
    notify(
        app,
        &format!("your {label} is starting"),
        &format!("your {label} is starting"),
    );
    update_tray(app, "break");
    Ok(())
}

pub async fn end_break_internal(
    app: &tauri::AppHandle,
    session: &std::sync::Arc<parking_lot::Mutex<SessionState>>,
    config: &std::sync::Arc<parking_lot::Mutex<crate::session::AppConfig>>,
    break_id_state: &std::sync::Arc<parking_lot::Mutex<Option<String>>>,
) -> Result<(u32, i64), String> {
    let break_id = break_id_state.lock().clone().ok_or("No active break")?;
    let break_name = session
        .lock()
        .break_name
        .clone()
        .unwrap_or_else(|| "break".to_string());
    let (pb_url, pb_token, break_start, session_id) = {
        let cfg = config.lock();
        let sess = session.lock();
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            sess.break_start.ok_or("No break start time")?,
            sess.session_id.clone().ok_or("No active session")?,
        )
    };

    let now = Utc::now();
    let break_duration = (now - break_start).num_seconds();
    let can_sync = !break_id.starts_with("local-") && !pb_url.is_empty() && !pb_token.is_empty();

    if can_sync {
        let pb = PocketBase::new(pb_url.clone(), pb_token.clone());
        pb.end_break(&break_id, &now)
            .await
            .map_err(|e| e.to_string())?;
    }

    let (break_count, total_break_seconds) = {
        let mut sess = session.lock();
        sess.status = SessionStatus::Active;
        sess.break_start = None;
        sess.break_name = None;
        sess.total_break_seconds += break_duration;
        let s = sess.clone();
        let break_count = s.break_count;
        let total_break_seconds = s.total_break_seconds;
        drop(sess);
        *break_id_state.lock() = None;
        app.emit("session-update", s).ok();
        (break_count, total_break_seconds)
    };

    if can_sync && !session_id.starts_with("local-") {
        let pb = PocketBase::new(pb_url, pb_token);
        pb.update_session_status(&session_id, &SessionStatus::Active)
            .await
            .ok();
        pb.update_session_break_metrics(&session_id, break_count, total_break_seconds)
            .await
            .ok();
    }

    notify(
        app,
        &format!("your break {break_name} is ending"),
        &format!("your break {break_name} is ending"),
    );
    update_tray(app, "active");
    Ok((break_count, total_break_seconds))
}

pub async fn clock_out_internal(
    app: &tauri::AppHandle,
    session: &std::sync::Arc<parking_lot::Mutex<SessionState>>,
    counters: &std::sync::Arc<parking_lot::Mutex<ActivityCounters>>,
    config: &std::sync::Arc<parking_lot::Mutex<crate::session::AppConfig>>,
    break_id_state: &std::sync::Arc<parking_lot::Mutex<Option<String>>>,
) -> Result<(), String> {
    let status = { session.lock().status.clone() };
    if status == SessionStatus::OnBreak {
        end_break_internal(app, session, config, break_id_state)
            .await
            .ok();
    }

    let (pb_url, pb_token, session_id, total_break_seconds) = {
        let cfg = config.lock();
        let sess = session.lock();
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            sess.session_id.clone().ok_or("No active session")?,
            sess.total_break_seconds,
        )
    };

    let now = Utc::now();

    if !session_id.starts_with("local-") && !pb_url.is_empty() && !pb_token.is_empty() {
        let pb = PocketBase::new(pb_url, pb_token);
        pb.close_session(&session_id, &now, total_break_seconds)
            .await
            .map_err(|e| e.to_string())?;
    }

    {
        let mut sess = session.lock();
        *sess = crate::session::SessionState::default();
        let s = sess.clone();
        drop(sess);
        app.emit("session-update", s).ok();
    }

    {
        let mut counters = counters.lock();
        *counters = ActivityCounters::default();
    }

    update_tray(app, "idle");
    Ok(())
}

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
    cfg.is_admin = auth.record.is_admin;
    cfg.token_saved_at = Utc::now().to_rfc3339();

    // Fetch company settings on login
    let pb = PocketBase::new(cfg.pb_url.clone(), cfg.pb_token.clone());
    if let Ok(settings) = pb.get_company_settings().await {
        if let Some(ci) = settings["clock_in_time"].as_str() {
            cfg.clock_in_time = ci.to_string();
        }
        if let Some(co) = settings["clock_out_time"].as_str() {
            cfg.clock_out_time = co.to_string();
        }
        if let Some(ao) = settings["auto_clock_out_enabled"].as_bool() {
            cfg.auto_clock_out_enabled = ao;
        }
    }
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
        "is_admin": cfg.is_admin,
        "clock_in_time": cfg.clock_in_time,
        "clock_out_time": cfg.clock_out_time,
        "auto_clock_out_enabled": cfg.auto_clock_out_enabled,
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
        (
            cfg.pb_url.clone(),
            cfg.user_name.clone(),
            cfg.user_email.clone(),
        )
    };
    let display_name = if user_name.trim().is_empty() {
        user_email
            .split('@')
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    } else {
        user_name.trim().to_string()
    };
    let now = Utc::now();

    // If offline (no token/url), generate a local session ID
    let session_id = if pb_token.is_empty() || pb_url.is_empty() {
        format!("local-{}", uuid::Uuid::new_v4())
    } else {
        let pb = PocketBase::new(pb_url, pb_token);
        // Close any stale active sessions for this user first (multi-machine protection)
        pb.close_stale_sessions(&user_id, &now).await.ok();
        pb.create_session(&user_id, &now, &display_name, &user_email)
            .await
            .map_err(|e| e.to_string())?
    };

    {
        let mut sess = state.session.lock();
        sess.status = SessionStatus::Active;
        sess.session_id = Some(session_id);
        sess.clock_in = Some(now);
        sess.break_start = None;
        sess.break_name = None;
        sess.total_break_seconds = 0;
        sess.break_count = 0;
        let s = sess.clone();
        drop(sess);
        state.auto_break_history.lock().clear();
        app.emit("session-update", s).ok();
    }

    // Update tray icon
    update_tray(&app, "active");

    Ok(())
}

#[tauri::command]
pub async fn clock_out(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    clock_out_internal(
        &app,
        &state.session,
        &state.counters,
        &state.config,
        &state.break_id,
    )
    .await
}

#[tauri::command]
pub async fn start_break(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    break_type: String,
    break_name: Option<String>,
) -> Result<(), String> {
    start_break_internal(
        &app,
        &state.session,
        &state.config,
        &state.break_id,
        &break_type,
        break_name.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn end_break(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    end_break_internal(&app, &state.session, &state.config, &state.break_id)
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn get_today_stats(state: State<'_, AppState>) -> Result<TodayStats, String> {
    let (pb_url, pb_token, user_id, sess_elapsed, sess_break_secs, sess_break_count) = {
        let cfg = state.config.lock();
        let sess = state.session.lock();
        let elapsed = sess
            .clock_in
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
    pb.get_today_stats(&user_id)
        .await
        .map_err(|e| e.to_string())
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
    let sessions = pb
        .get_user_monthly_sessions(&user_id, &year_month)
        .await
        .map_err(|e| e.to_string())?;

    // Aggregate per day: date → { sessions, total_work_seconds, total_break_seconds }
    let mut days: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for s in &sessions {
        let clock_in = s["clock_in"].as_str().unwrap_or("");
        if clock_in.is_empty() {
            continue;
        }
        let date = &clock_in[..10]; // "YYYY-MM-DD"
        let clock_out = s["clock_out"].as_str().unwrap_or("");
        let break_secs = s["total_break_seconds"].as_i64().unwrap_or(0);
        let work_secs: i64 = if !clock_in.is_empty() {
            let ci = chrono::DateTime::parse_from_rfc3339(clock_in)
                .or_else(|_| chrono::DateTime::parse_from_str(clock_in, "%Y-%m-%d %H:%M:%S%.3fZ"))
                .map(|d| d.with_timezone(&chrono::Utc))
                .ok();
            let co = if clock_out.is_empty() {
                Some(chrono::Utc::now())
            } else {
                chrono::DateTime::parse_from_rfc3339(clock_out)
                    .or_else(|_| {
                        chrono::DateTime::parse_from_str(clock_out, "%Y-%m-%d %H:%M:%S%.3fZ")
                    })
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .ok()
            };
            match (ci, co) {
                (Some(i), Some(o)) => ((o - i).num_seconds() - break_secs).max(0),
                _ => 0,
            }
        } else {
            0
        };

        let entry = days.entry(date.to_string()).or_insert_with(|| {
            serde_json::json!({
                "date": date,
                "session_count": 0,
                "total_work_seconds": 0i64,
                "total_break_seconds": 0i64,
                "sessions": []
            })
        });
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
    pb.get_session_snapshots(&session_id)
        .await
        .map_err(|e| e.to_string())
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
    pb.get_session_network(&session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_break_configs(state: State<'_, AppState>) -> Result<Vec<BreakConfig>, String> {
    let (pb_url, pb_token) = {
        let cfg = state.config.lock();
        (cfg.pb_url.clone(), cfg.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        let defaults = BreakConfig::defaults();
        *state.break_configs.lock() = defaults.clone();
        return Ok(defaults);
    }
    let pb = PocketBase::new(pb_url, pb_token);
    let configs = pb
        .get_break_configs()
        .await
        .unwrap_or_else(|_| BreakConfig::defaults());
    *state.break_configs.lock() = configs.clone();
    Ok(configs)
}

#[tauri::command]
pub async fn save_work_schedule(
    state: State<'_, AppState>,
    clock_in_time: String,
    clock_out_time: String,
    auto_clock_out_enabled: bool,
) -> Result<serde_json::Value, String> {
    let (pb_url, pb_token, is_admin) = {
        let mut cfg = state.config.lock();
        cfg.clock_in_time = clock_in_time.clone();
        cfg.clock_out_time = clock_out_time.clone();
        cfg.auto_clock_out_enabled = auto_clock_out_enabled;
        (cfg.pb_url.clone(), cfg.pb_token.clone(), cfg.is_admin)
    };

    // If admin, update company-level policy in PocketBase
    if is_admin && !pb_url.is_empty() && !pb_token.is_empty() {
        let pb = PocketBase::new(pb_url, pb_token);
        if let Ok(settings) = pb.get_company_settings().await {
            if let Some(id) = settings["id"].as_str() {
                let _ = pb
                    .update_company_settings(id, &clock_in_time, &clock_out_time, auto_clock_out_enabled)
                    .await;
            }
        }
    }

    let cfg_clone = state.config.lock().clone();
    state
        .db
        .lock()
        .save_config(&cfg_clone)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "clock_in_time": cfg_clone.clock_in_time,
        "clock_out_time": cfg_clone.clock_out_time,
        "auto_clock_out_enabled": cfg_clone.auto_clock_out_enabled,
    }))
}

#[tauri::command]
pub async fn refresh_auth_state(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let (pb_url, pb_token, user_id) = {
        let cfg = state.config.lock();
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            cfg.user_id.clone(),
        )
    };

    if pb_url.is_empty() || pb_token.is_empty() || user_id.is_empty() {
        return Err("Not authenticated".into());
    }

    let pb = PocketBase::new(pb_url, pb_token);
    let user = pb
        .get_user_record(&user_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut cfg = state.config.lock();
    if !user.name.trim().is_empty() {
        cfg.user_name = user.name.trim().to_string();
    }
    if !user.email.trim().is_empty() {
        cfg.user_email = user.email.trim().to_string();
    }
    cfg.is_admin = user.is_admin;
    
    // Also sync company settings during auth refresh
    if let Ok(settings) = pb.get_company_settings().await {
        if let Some(ci) = settings["clock_in_time"].as_str() {
            cfg.clock_in_time = ci.to_string();
        }
        if let Some(co) = settings["clock_out_time"].as_str() {
            cfg.clock_out_time = co.to_string();
        }
        if let Some(ao) = settings["auto_clock_out_enabled"].as_bool() {
            cfg.auto_clock_out_enabled = ao;
        }
    }

    let cfg_clone = cfg.clone();
    drop(cfg);
    state
        .db
        .lock()
        .save_config(&cfg_clone)
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "user_name": cfg_clone.user_name,
        "user_email": cfg_clone.user_email,
        "is_admin": cfg_clone.is_admin,
        "clock_in_time": cfg_clone.clock_in_time,
        "clock_out_time": cfg_clone.clock_out_time,
        "auto_clock_out_enabled": cfg_clone.auto_clock_out_enabled,
    }))
}

#[tauri::command]
pub async fn clear_auth(state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut cfg = state.config.lock();
        cfg.pb_token = String::new();
        cfg.user_id = String::new();
        cfg.user_name = String::new();
        cfg.user_email = String::new();
        cfg.is_admin = false;
        cfg.token_saved_at = String::new();
    }
    let cfg = state.config.lock().clone();
    state.db.lock().save_config(&cfg).map_err(|e| e.to_string())
}

fn notify(app: &tauri::AppHandle, title: &str, body: &str) {
    let payload = AppNotification {
        title: title.to_string(),
        body: body.to_string(),
    };
    app.emit("app-notification", payload).ok();
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
