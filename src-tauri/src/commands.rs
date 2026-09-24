use crate::config::DEFAULT_PB_URL;
use crate::domain::time_loss::calculate as calculate_time_loss;
use crate::pm::plane::{Plane, PlaneIssue, PlaneMember, PlaneProject, PlaneState};
use crate::pocketbase::PocketBase;
use crate::session::{
    ActivityCounters, ActivityReport, ActivitySnapshot, AppNotification, BreakConfig,
    NetworkConnection, NetworkReport, SessionRecord, SessionState, SessionStatus, TaskRecord,
    TeamMember, TodayBreakdown, TodayStats, UserInfo, UserSummary,
};
use crate::AppState;
use chrono::Utc;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tauri::{Emitter, Manager, State};
use uuid::Uuid;

fn nepal_range_from_dates(
    from_date: &str,
    to_date: &str,
) -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    use chrono::TimeZone;
    let nepal = chrono::FixedOffset::east_opt(5 * 3600 + 45 * 60).unwrap();
    let now_npl = chrono::Utc::now().with_timezone(&nepal);
    let from = chrono::NaiveDate::parse_from_str(from_date, "%Y-%m-%d")
        .unwrap_or_else(|_| now_npl.date_naive());
    let to = chrono::NaiveDate::parse_from_str(to_date, "%Y-%m-%d")
        .unwrap_or_else(|_| now_npl.date_naive());
    let from_utc = nepal
        .from_local_datetime(&from.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);
    let to_utc = nepal
        .from_local_datetime(&to.and_hms_opt(23, 59, 59).unwrap())
        .single()
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);
    (from_utc, to_utc)
}

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
        // The server-side auto-clockout cron treats an "active" session as
        // idle-eligible, so a failed status PATCH here can get the user
        // clocked out mid-break. Retry once before giving up.
        if let Err(e) = pb
            .update_session_status(&session_id, &SessionStatus::OnBreak)
            .await
        {
            log::warn!("start_break: status PATCH failed ({e}), retrying once");
            if let Err(e) = pb
                .update_session_status(&session_id, &SessionStatus::OnBreak)
                .await
            {
                log::warn!("start_break: status PATCH retry failed for {session_id}: {e}");
            }
        }
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
    // A lost break id (e.g. app restart mid-break) must not trap the user on
    // "On Break" — proceed without it and let close_open_breaks on the server
    // close the orphaned record.
    let break_id = break_id_state.lock().clone();
    if break_id.is_none() && session.lock().status != SessionStatus::OnBreak {
        return Err("No active break".into());
    }
    let break_name = session
        .lock()
        .break_name
        .clone()
        .unwrap_or_else(|| "break".to_string());
    let now = Utc::now();
    let (pb_url, pb_token, break_start, session_id) = {
        let cfg = config.lock();
        let sess = session.lock();
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            // A missing break_start (e.g. state restored after a crash mid-break)
            // must not leave the user stuck on "On Break" — treat it as a
            // zero-length break and still transition back to Active.
            sess.break_start.unwrap_or(now),
            sess.session_id.clone().ok_or("No active session")?,
        )
    };

    let break_duration = (now - break_start).num_seconds().max(0);
    let pb_ready = !pb_url.is_empty() && !pb_token.is_empty();

    // Always update local state first — PB sync is best-effort so a network
    // hiccup can never leave the user stuck on "On Break".
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

    if !session_id.starts_with("local-") && pb_ready {
        let pb = PocketBase::new(pb_url, pb_token);
        if let Some(id) = break_id.as_deref().filter(|id| !id.starts_with("local-")) {
            pb.end_break(id, &now).await.ok();
        } else {
            // Break id was lost — close whatever open break records exist so
            // the server-side cron doesn't see a phantom open break.
            pb.close_open_breaks(&session_id, &now).await.ok();
        }
        // The cron treats an "on_break" record whose status never flipped back
        // as still on break, so retry this PATCH once like start_break does.
        if let Err(e) = pb
            .update_session_status(&session_id, &SessionStatus::Active)
            .await
        {
            log::warn!("end_break: status PATCH failed ({e}), retrying once");
            pb.update_session_status(&session_id, &SessionStatus::Active)
                .await
                .ok();
        }
        pb.update_session_break_metrics(&session_id, break_count, total_break_seconds)
            .await
            .ok();
    }

    notify(
        app,
        &format!("your break {break_name} has ended"),
        &format!("your break {break_name} has ended"),
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
    early_clockout_reason: Option<&str>,
) -> Result<(), String> {
    // End any open break first (best-effort, must not block clock-out).
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

    // Always reset local state immediately — the user must never be
    // stuck in "Clocked In" because of a network failure.
    {
        let mut sess = session.lock();
        *sess = crate::session::SessionState::default();
        let s = sess.clone();
        drop(sess);
        app.emit("session-update", s).ok();
    }
    {
        let mut c = counters.lock();
        *c = ActivityCounters::default();
    }
    update_tray(app, "idle");

    // PB sync is best-effort. Any failure is logged; the stale session is
    // closed automatically by close_stale_sessions() on the next clock-in.
    if !session_id.starts_with("local-") && !pb_url.is_empty() && !pb_token.is_empty() {
        let pb = PocketBase::new(pb_url, pb_token);
        // A task left running when the session ends would otherwise stay
        // open forever (its session_id no longer matches any active
        // session, so find_active_task on the next clock-in never finds it
        // to clean up) — close it out as "stopped", same as breaks above.
        if let Ok(Some(task)) = pb.find_active_task(&session_id).await {
            pb.end_task(&task.id, &now, "stopped").await.ok();
        }
        let extra_break_seconds = pb.close_open_breaks(&session_id, &now).await.unwrap_or(0);
        let total = total_break_seconds + extra_break_seconds;
        if let Err(e) = pb
            .close_session(&session_id, &now, total, early_clockout_reason)
            .await
        {
            log::warn!("clock_out: PB sync failed for session {session_id}: {e}");
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn authenticate_pb(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    pb_url: String,
    pb_email: String,
    pb_password: String,
) -> Result<serde_json::Value, String> {
    let auth = PocketBase::authenticate(&pb_url, &pb_email, &pb_password)
        .await
        .map_err(|e| e.to_string())?;

    let pb = {
        let mut cfg = state.config.lock();
        cfg.pb_url = pb_url;
        cfg.pb_email = pb_email;
        cfg.pb_token = auth.token.clone();
        cfg.user_id = auth.record.id.clone();
        cfg.user_name = auth.record.name.clone();
        cfg.user_email = auth.record.email.clone();
        cfg.is_admin = auth.record.is_admin;
        cfg.pm_api = auth.record.pm_api.clone();
        cfg.is_external_staff = auth.record.is_external_staff;
        cfg.token_saved_at = Utc::now().to_rfc3339();
        PocketBase::new(cfg.pb_url.clone(), cfg.pb_token.clone())
        // cfg guard dropped here, before any await
    };

    if let Ok(settings) = pb.get_company_settings().await {
        let mut cfg = state.config.lock();
        if settings["enable_pm"].as_bool().unwrap_or(false) {
            cfg.pm_enabled = true;
            cfg.pm_config = settings["pm_config"].clone();
        }
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

    let (cfg_save, token_saved_at) = {
        let cfg = state.config.lock();
        (cfg.clone(), cfg.token_saved_at.clone())
    };

    state
        .db
        .lock()
        .save_config(&cfg_save)
        .map_err(|e| e.to_string())?;

    // A session that wAas clocked in before we had credentials (Skip button,
    // or a stale saved token that just got cleared) only exists in memory —
    // sync it up now that we actually have a token.
    if let Some((old_id, new_id)) = sync_local_session_to_pb(
        &app,
        &state.session,
        &pb,
        &cfg_save.user_id,
        &cfg_save.user_name,
        &cfg_save.user_email,
    )
    .await
    {
        state.db.lock().reassign_session_id(&old_id, &new_id).ok();
    }

    Ok(json!({
        "token": auth.token,
        "user_id": auth.record.id,
        "user_name": auth.record.name,
        "user_email": auth.record.email,
        "is_admin": auth.record.is_admin,
        "is_external_staff": auth.record.is_external_staff,
        "token_saved_at": token_saved_at,
    }))
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock();
    Ok(json!({
        "pb_url": cfg.pb_url,
        "pb_email": cfg.pb_email,
        "pb_token": cfg.pb_token,
        "user_id": cfg.user_id,
        "user_name": cfg.user_name,
        "user_email": cfg.user_email,
        "is_admin": cfg.is_admin,
        "is_external_staff": cfg.is_external_staff,
        "clock_in_time": cfg.clock_in_time,
        "clock_out_time": cfg.clock_out_time,
        "auto_clock_out_enabled": cfg.auto_clock_out_enabled,
        "token_saved_at": cfg.token_saved_at,
        "pm_enabled": cfg.pm_enabled,
        "pm_workspace_slug": cfg.pm_config.get("workspace_slug").and_then(|v| v.as_str()).unwrap_or(""),
        "pm_base_url": cfg.pm_config.get("base_url").and_then(|v| v.as_str()).unwrap_or(""),
        "default_pb_url": DEFAULT_PB_URL,
    }))
}

#[tauri::command]
pub async fn get_session_state(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<SessionState, String> {
    // The in-memory session always starts Idle on process start, so a crash
    // or restart while clocked in otherwise looks identical to never having
    // clocked in: the clock-in reminder fires again, and clocking back in
    // creates a second work_sessions record on top of the one still open on
    // the server. Reconcile against PocketBase once, here, before trusting
    // the local Idle default.
    let (status, pb_url, pb_token, user_id) = {
        let sess = state.session.lock();
        let cfg = state.config.lock();
        (
            sess.status.clone(),
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            cfg.user_id.clone(),
        )
    };
    if status == SessionStatus::Idle
        && !pb_url.is_empty()
        && !pb_token.is_empty()
        && !user_id.is_empty()
    {
        let pb = PocketBase::new(pb_url, pb_token);
        if let Ok(Some(restored)) = pb.find_active_session(&user_id).await {
            let mut sess = state.session.lock();
            // Only apply if still idle — avoid clobbering a clock-in that
            // happened locally while this lookup was in flight.
            if sess.status == SessionStatus::Idle {
                *sess = restored;
                update_tray(
                    &app,
                    match sess.status {
                        SessionStatus::Active => "active",
                        SessionStatus::OnBreak => "break",
                        SessionStatus::Idle => "idle",
                    },
                );
            }
        }
    }
    Ok(state.session.lock().clone())
}

/// If the current session was started via `clock_in`'s offline fallback
/// (no PB token/url at the time, e.g. the login screen's Skip button or a
/// stale saved token), it only exists in memory — nothing was ever created
/// on PocketBase, so it never shows up for admins and can never be closed
/// out server-side. Called once real credentials are available (right after
/// login, and periodically from the offline-sync retry loop) to create the
/// session on PocketBase now and swap the in-memory session over to the
/// real id. Returns the (old, new) id pair on success so callers can
/// re-tag any snapshot/network rows already queued under the old id.
pub async fn sync_local_session_to_pb(
    app: &tauri::AppHandle,
    session: &std::sync::Arc<parking_lot::Mutex<SessionState>>,
    pb: &PocketBase,
    user_id: &str,
    user_name: &str,
    user_email: &str,
) -> Option<(String, String)> {
    if user_id.is_empty() {
        return None;
    }
    let (status, old_id, clock_in, break_count, total_break_seconds, extended) = {
        let sess = session.lock();
        (
            sess.status.clone(),
            sess.session_id.clone(),
            sess.clock_in,
            sess.break_count,
            sess.total_break_seconds,
            sess.extended_past_schedule,
        )
    };
    if status == SessionStatus::Idle {
        return None;
    }
    let old_id = old_id.filter(|id| id.starts_with("local-"))?;
    let clock_in = clock_in?;

    let now = Utc::now();
    pb.close_stale_sessions(user_id, &now).await.ok();
    let new_id = pb
        .create_session(user_id, &clock_in, user_name, user_email)
        .await
        .ok()?;

    if extended {
        pb.set_session_extended(&new_id, true).await.ok();
    }
    if break_count > 0 || total_break_seconds > 0 {
        pb.update_session_break_metrics(&new_id, break_count, total_break_seconds)
            .await
            .ok();
    }
    if status == SessionStatus::OnBreak {
        pb.update_session_status(&new_id, &SessionStatus::OnBreak)
            .await
            .ok();
    }

    {
        let mut sess = session.lock();
        if sess.session_id.as_deref() == Some(old_id.as_str()) {
            sess.session_id = Some(new_id.clone());
        }
        let s = sess.clone();
        drop(sess);
        app.emit("session-update", s).ok();
    }

    log::info!("synced offline session {old_id} to PocketBase as {new_id}");
    Some((old_id, new_id))
}

#[tauri::command]
pub async fn clock_in(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    user_id: String,
    pb_token: String,
    force: Option<bool>,
) -> Result<(), String> {
    // Guard: reject if already active or on break (prevents double clock-in).
    {
        let status = state.session.lock().status.clone();
        if status != SessionStatus::Idle {
            return Err("Already clocked in".into());
        }
    }
    let force = force.unwrap_or(false);

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

    // Clocking in after the scheduled clock-out time is always deliberate
    // off-hours work (making up time loss, or external staff). Mark the
    // session extended from the start so the server cron's scheduled close
    // doesn't end it a minute later.
    let past_schedule = {
        let cfg = state.config.lock();
        let now_npt = now.with_timezone(&crate::nepal_offset());
        crate::schedule_datetime(now_npt.date_naive(), &cfg.clock_out_time)
            .map(|due| now_npt >= due)
            .unwrap_or(false)
    };

    // If offline (no token/url), generate a local session ID
    let session_id = if pb_token.is_empty() || pb_url.is_empty() {
        format!("local-{}", uuid::Uuid::new_v4())
    } else {
        let pb = PocketBase::new(pb_url, pb_token);
        // A session left open on another machine (or a prior crash on this
        // one) must not be silently ended — ask the caller to confirm first.
        // The frontend catches the "SESSION_ALREADY_ACTIVE|<clock_in>" error
        // and re-invokes with force=true once the user agrees.
        if !force {
            if let Some(existing) = pb
                .find_active_session(&user_id)
                .await
                .map_err(|e| e.to_string())?
            {
                let started = existing
                    .clock_in
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_default();
                return Err(format!("SESSION_ALREADY_ACTIVE|{started}"));
            }
        }
        // Close the confirmed-stale active session(s) for this user now that
        // the caller has agreed to end them (multi-machine protection).
        pb.close_stale_sessions(&user_id, &now).await.ok();
        let sid = pb
            .create_session(&user_id, &now, &display_name, &user_email)
            .await
            .map_err(|e| e.to_string())?;
        if past_schedule {
            pb.set_session_extended(&sid, true).await.ok();
        }
        sid
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
        sess.extended_past_schedule = past_schedule;
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
pub async fn clock_out(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    reason: Option<String>,
) -> Result<(), String> {
    clock_out_internal(
        &app,
        &state.session,
        &state.counters,
        &state.config,
        &state.break_id,
        reason.as_deref(),
    )
    .await
}

/// Keep working past the scheduled clock-out to make up a time deficit.
/// The flag is set locally and on PocketBase so neither the client loop nor
/// the server cron auto-closes the session at schedule time.
#[tauri::command]
pub async fn extend_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (pb_url, pb_token, session_id) = {
        let cfg = state.config.lock();
        let mut sess = state.session.lock();
        if sess.status == SessionStatus::Idle {
            return Err("Not clocked in".into());
        }
        sess.extended_past_schedule = true;
        let s = sess.clone();
        let session_id = s.session_id.clone().ok_or("No active session")?;
        drop(sess);
        app.emit("session-update", s).ok();
        (cfg.pb_url.clone(), cfg.pb_token.clone(), session_id)
    };

    if !session_id.starts_with("local-") && !pb_url.is_empty() && !pb_token.is_empty() {
        let pb = PocketBase::new(pb_url, pb_token);
        pb.set_session_extended(&session_id, true)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Admin: mark a user as external staff (works outside the company schedule).
#[tauri::command]
pub async fn set_user_external_staff(
    state: State<'_, AppState>,
    user_id: String,
    is_external: bool,
) -> Result<(), String> {
    let (pb_url, pb_token, is_admin) = {
        let cfg = state.config.lock();
        (cfg.pb_url.clone(), cfg.pb_token.clone(), cfg.is_admin)
    };
    if !is_admin {
        return Err("Admin access required".into());
    }
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    PocketBase::new(pb_url, pb_token)
        .set_user_external_staff(&user_id, is_external)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_api_key(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
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

    let key = key.trim();
    if key.is_empty() {
        return Err("API key name is required".into());
    }

    PocketBase::new(pb_url, pb_token)
        .add_user_apis(&user_id, key, value.trim())
        .await
        .map_err(|e| e.to_string())
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

/// Required daily work seconds (schedule span minus scheduled auto-breaks),
/// for admin views that compute per-day time loss client-side. This is the
/// company-wide schedule and does not depend on the logged-in admin's own
/// external-staff status; callers apply the per-employee exemption.
#[tauri::command]
pub fn get_required_seconds(state: State<'_, AppState>) -> i64 {
    let cfg = state.config.lock();
    let breaks = state.break_configs.lock();
    cfg.required_work_seconds(&breaks)
}

#[tauri::command]
pub async fn get_today_stats(state: State<'_, AppState>) -> Result<TodayStats, String> {
    let (
        pb_url,
        pb_token,
        user_id,
        cached_required_seconds,
        sess_elapsed,
        sess_break_secs,
        sess_break_count,
    ) = {
        let cfg = state.config.lock();
        let sess = state.session.lock();
        let breaks = state.break_configs.lock();
        let elapsed = sess
            .clock_in
            .map(|ci| (Utc::now() - ci).num_seconds())
            .unwrap_or(0);
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            cfg.user_id.clone(),
            // Self-exemption: an external-staff user has no required hours
            // for their own dashboard, even though the company schedule
            // itself is a shared, non-zero value used for everyone else.
            if cfg.is_external_staff {
                0
            } else {
                cfg.required_work_seconds(&breaks)
            },
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
            total_net_loss_seconds: 0,
            required_seconds: cached_required_seconds,
            time_loss_seconds: calculate_time_loss(cached_required_seconds, work_secs, false)
                .seconds,
        });
    }

    let pb = PocketBase::new(pb_url, pb_token);
    let required_seconds = if cached_required_seconds == 0 {
        0
    } else {
        pb.get_required_work_seconds()
            .await
            .unwrap_or(cached_required_seconds)
    };
    let breakdown = pb
        .get_today_breakdown(&user_id)
        .await
        .map_err(|e| e.to_string())?;

    // PocketBase calculates open sessions through the current instant, so its
    // totals are authoritative. Do not remove the current session here and
    // re-add a local browser timer: that gave the user and admin two distinct
    // time-loss calculations.
    let total_work_seconds = breakdown.total_work_seconds.max(0);
    let total_break_seconds = breakdown.total_break_seconds.max(0);
    let total_net_loss_seconds = breakdown.total_net_loss_seconds.max(0);
    let break_count = breakdown.break_count;
    let session_count = breakdown.session_count;

    Ok(TodayStats {
        session_count,
        total_work_seconds,
        break_count,
        total_break_seconds,
        total_net_loss_seconds,
        required_seconds,
        time_loss_seconds: calculate_time_loss(required_seconds, total_work_seconds, false).seconds,
    })
}

#[tauri::command]
pub async fn get_user_today_breakdown(
    state: State<'_, AppState>,
    user_id: String,
) -> Result<TodayBreakdown, String> {
    let (pb_url, pb_token) = {
        let cfg = state.config.lock();
        (cfg.pb_url.clone(), cfg.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let pb = PocketBase::new(pb_url, pb_token);
    pb.get_today_breakdown(&user_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_team_status(state: State<'_, AppState>) -> Result<Vec<TeamMember>, String> {
    let (pb_url, pb_token, cached_required_seconds) = {
        let cfg = state.config.lock();
        let breaks = state.break_configs.lock();
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            cfg.required_work_seconds(&breaks),
        )
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let pb = PocketBase::new(pb_url, pb_token);
    let required_seconds = pb
        .get_required_work_seconds()
        .await
        .unwrap_or(cached_required_seconds);
    let mut members = pb.get_team_status().await.map_err(|e| e.to_string())?;
    for member in &mut members {
        member.today_time_loss_seconds = calculate_time_loss(
            required_seconds,
            member.today_total_work_seconds,
            member.is_external_staff,
        )
        .seconds;
    }
    Ok(members)
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
                    .update_company_settings(
                        id,
                        &clock_in_time,
                        &clock_out_time,
                        auto_clock_out_enabled,
                    )
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
pub async fn save_pm_settings(
    state: State<'_, AppState>,
    enabled: bool,
    workspace_slug: String,
    base_url: String,
) -> Result<serde_json::Value, String> {
    let (pb_url, pb_token, is_admin) = {
        let mut cfg = state.config.lock();
        if !cfg.is_admin {
            return Err("Admin access required".into());
        }
        cfg.pm_enabled = enabled;
        cfg.pm_config = json!({ "workspace_slug": workspace_slug, "base_url": base_url });
        (cfg.pb_url.clone(), cfg.pb_token.clone(), cfg.is_admin)
    };

    if is_admin && !pb_url.is_empty() && !pb_token.is_empty() {
        let pb = PocketBase::new(pb_url, pb_token);
        if let Ok(settings) = pb.get_company_settings().await {
            if let Some(id) = settings["id"].as_str() {
                let _ = pb
                    .update_pm_settings(id, enabled, &workspace_slug, &base_url)
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
        "pm_enabled": cfg_clone.pm_enabled,
        "pm_config": cfg_clone.pm_config,
    }))
}

/// Builds a `Plane` client for the current user, resolving the workspace
/// config from `company_config` (via `AppConfig.pm_config`/`pm_enabled`) and
/// the personal API token from `users.pm_api.plane` (see `PbUserRecord`).
/// Errors clearly instead of silently no-op-ing when either is missing.
async fn plane_client_for(state: &State<'_, AppState>) -> Result<Plane, String> {
    let (pb_url, pb_token, user_id, pm_config, pm_enabled) = {
        let cfg = state.config.lock();
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            cfg.user_id.clone(),
            cfg.pm_config.clone(),
            cfg.pm_enabled,
        )
    };
    if !pm_enabled {
        return Err("Plane integration is not enabled".into());
    }
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let pb = PocketBase::new(pb_url, pb_token);
    let user = pb.get_user_record(&user_id).await.map_err(|e| e.to_string())?;
    let token = user
        .pm_api
        .get("plane")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if token.is_empty() {
        return Err("No Plane API key saved — add one in Settings".into());
    }
    let workspace_slug = pm_config
        .get("workspace_slug")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let base_url = pm_config
        .get("base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if workspace_slug.is_empty() || base_url.is_empty() {
        return Err("Plane workspace is not configured — ask an admin to set it up in Settings".into());
    }
    Ok(Plane::new(base_url, workspace_slug, token))
}

#[tauri::command]
pub async fn list_plane_projects(state: State<'_, AppState>) -> Result<Vec<PlaneProject>, String> {
    plane_client_for(&state)
        .await?
        .list_my_projects()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_plane_project_issues(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<PlaneIssue>, String> {
    plane_client_for(&state)
        .await?
        .list_project_issues(&project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_plane_members(state: State<'_, AppState>) -> Result<Vec<PlaneMember>, String> {
    plane_client_for(&state)
        .await?
        .list_workspace_members()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_plane_project_states(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<PlaneState>, String> {
    plane_client_for(&state)
        .await?
        .project_states(&project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_recent_plane_issues(state: State<'_, AppState>) -> Result<Vec<PlaneIssue>, String> {
    let plane = plane_client_for(&state).await?;
    let me = plane.current_user().await.map_err(|e| e.to_string())?;
    plane
        .list_recent_assigned_issues(&me.id, 20)
        .await
        .map_err(|e| e.to_string())
}

/// Starts tracking a task within the current session. Covers all three
/// entry points: an existing Plane issue (`plane_project_id` + `plane_issue_id`
/// both set), a new custom task in a Plane project (`plane_project_id` set,
/// `plane_issue_id` empty — creates the issue first), or a purely local task
/// (both empty). If a task is already running for this session, it's ended
/// as "stopped" first — this is how switching tasks without clocking out
/// works.
#[tauri::command]
pub async fn start_task(
    state: State<'_, AppState>,
    name: String,
    plane_project_id: Option<String>,
    plane_issue_id: Option<String>,
) -> Result<TaskRecord, String> {
    let (pb_url, pb_token, user_id, session_id, status) = {
        let cfg = state.config.lock();
        let sess = state.session.lock();
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            cfg.user_id.clone(),
            sess.session_id.clone(),
            sess.status.clone(),
        )
    };
    if status == SessionStatus::Idle {
        return Err("Clock in before starting a task".into());
    }
    let session_id = session_id.ok_or("No active session")?;
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let pb = PocketBase::new(pb_url, pb_token);
    let now = Utc::now();

    if let Some(existing) = pb
        .find_active_task(&session_id)
        .await
        .map_err(|e| e.to_string())?
    {
        pb.end_task(&existing.id, &now, "stopped")
            .await
            .map_err(|e| e.to_string())?;
    }

    let mut plane_issue_id = plane_issue_id.filter(|s| !s.is_empty());
    let plane_project_id = plane_project_id.filter(|s| !s.is_empty());
    if let Some(project_id) = &plane_project_id {
        if plane_issue_id.is_none() {
            let plane = plane_client_for(&state).await?;
            let issue = plane
                .create_issue(project_id, &name)
                .await
                .map_err(|e| e.to_string())?;
            plane_issue_id = Some(issue.id);
        }
    }

    pb.create_task(
        &user_id,
        &session_id,
        &name,
        plane_project_id.as_deref(),
        plane_issue_id.as_deref(),
        &now,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Ends a task. `complete` marks the linked Plane issue done too (if any) —
/// a Plane-side failure is logged but doesn't fail the command, since the
/// local time record is the source of truth for tracked time regardless of
/// whether Plane is reachable right now.
#[tauri::command]
pub async fn end_task(
    state: State<'_, AppState>,
    task_id: String,
    plane_project_id: Option<String>,
    plane_issue_id: Option<String>,
    complete: bool,
) -> Result<(), String> {
    let (pb_url, pb_token) = {
        let cfg = state.config.lock();
        (cfg.pb_url.clone(), cfg.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let pb = PocketBase::new(pb_url, pb_token);
    let now = Utc::now();
    let status = if complete { "completed" } else { "stopped" };
    pb.end_task(&task_id, &now, status)
        .await
        .map_err(|e| e.to_string())?;

    if complete {
        if let (Some(project_id), Some(issue_id)) = (plane_project_id, plane_issue_id) {
            if let Ok(plane) = plane_client_for(&state).await {
                if let Err(e) = plane.complete_issue(&project_id, &issue_id).await {
                    log::warn!("failed to mark Plane issue {issue_id} complete: {e}");
                }
            }
        }
    }
    Ok(())
}

/// The task currently running for this session, if any — restores the
/// running timer after an app restart, mirroring `get_session_state`.
#[tauri::command]
pub async fn get_active_task(state: State<'_, AppState>) -> Result<Option<TaskRecord>, String> {
    let (pb_url, pb_token, session_id) = {
        let cfg = state.config.lock();
        let sess = state.session.lock();
        (cfg.pb_url.clone(), cfg.pb_token.clone(), sess.session_id.clone())
    };
    let Some(session_id) = session_id else {
        return Ok(None);
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Ok(None);
    }
    PocketBase::new(pb_url, pb_token)
        .find_active_task(&session_id)
        .await
        .map_err(|e| e.to_string())
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

    {
        let mut cfg = state.config.lock();
        if !user.name.trim().is_empty() {
            cfg.user_name = user.name.trim().to_string();
        }
        if !user.email.trim().is_empty() {
            cfg.user_email = user.email.trim().to_string();
        }
        // Only elevate to admin — never revoke via refresh since GET /users/:id
        // may not return is_admin depending on PocketBase collection rules.
        // Admin can only be cleared by signing out.
        cfg.is_admin = cfg.is_admin || user.is_admin;
        cfg.is_external_staff = user.is_external_staff;
    }

    // Also sync company settings during auth refresh — drop lock before await
    if let Ok(settings) = pb.get_company_settings().await {
        let mut cfg = state.config.lock();
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

    let cfg_save = state.config.lock().clone();

    state
        .db
        .lock()
        .save_config(&cfg_save)
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "user_name": cfg_save.user_name,
        "user_email": cfg_save.user_email,
        "is_admin": cfg_save.is_admin,
        "is_external_staff": cfg_save.is_external_staff,
        "clock_in_time": cfg_save.clock_in_time,
        "clock_out_time": cfg_save.clock_out_time,
        "auto_clock_out_enabled": cfg_save.auto_clock_out_enabled,
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
        cfg.is_external_staff = false;
        cfg.token_saved_at = String::new();
    }
    let cfg = state.config.lock().clone();
    state.db.lock().save_config(&cfg).map_err(|e| e.to_string())
}

/// Manually pushes a one-off activity snapshot built from the given
/// keystrokes/mouse_clicks totals, through the same push-or-queue path
/// `Background::push_activity_snapshot` uses for the real 30s snapshots.
/// Wired to the in-app `lestercity2` test shortcut so the PocketBase sync
/// path can be exercised on demand instead of waiting for a real session.
#[tauri::command]
pub async fn push_test_activity_snapshot(
    state: State<'_, AppState>,
    keystrokes: u64,
    mouse_clicks: u64,
) -> Result<(), String> {
    // The local auto-clockout timer (Background::tick_idle_clockout) only
    // ever looks at ActivityCounters.last_activity, which is normally
    // stamped by the real OS-level input monitor. Pushing a synthetic
    // snapshot to PocketBase doesn't touch that, so without this the
    // repeating lestercity2 loop keeps the server-side "offline" check happy
    // while the on-device idle timer keeps climbing in the background and
    // auto-clocks the session out anyway. Treat a manual push as real
    // activity so the two stay consistent.
    state.counters.lock().last_activity = Some(std::time::Instant::now());

    let (pb_url, pb_token, session_id) = {
        let cfg = state.config.lock();
        let sess = state.session.lock();
        (
            cfg.pb_url.clone(),
            cfg.pb_token.clone(),
            sess.session_id.clone(),
        )
    };
    let Some(sid) = session_id else {
        return Err("no active session to attach the snapshot to".into());
    };

    let snap = ActivitySnapshot {
        timestamp: Utc::now(),
        keystrokes,
        mouse_clicks,
        mouse_distance_px: 0.0,
        active_app: "Nvim".into(),
        active_window: "neovim".into(),
        idle_seconds: 0,
        in_call: false,
    };

    if pb_url.is_empty() || pb_token.is_empty() {
        return state
            .db
            .lock()
            .queue_snapshot(&sid, &snap)
            .map_err(|e| e.to_string());
    }

    let pb = PocketBase::new(pb_url, pb_token);
    if pb.push_snapshot(&sid, &snap).await.is_err() {
        state
            .db
            .lock()
            .queue_snapshot(&sid, &snap)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn get_all_users(state: State<'_, AppState>) -> Result<Vec<UserInfo>, String> {
    let (pb_url, pb_token) = {
        let c = state.config.lock();
        (c.pb_url.clone(), c.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    PocketBase::new(pb_url, pb_token)
        .get_all_users()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_sessions_report(
    state: State<'_, AppState>,
    from_date: String,
    to_date: String,
    user_id: Option<String>,
) -> Result<Vec<SessionRecord>, String> {
    let (pb_url, pb_token) = {
        let c = state.config.lock();
        (c.pb_url.clone(), c.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let (from, to) = nepal_range_from_dates(&from_date, &to_date);
    PocketBase::new(pb_url, pb_token)
        .get_sessions_in_range(&from, &to, &Utc::now(), user_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_time_summary(
    state: State<'_, AppState>,
    from_date: String,
    to_date: String,
    user_id: Option<String>,
) -> Result<Vec<UserSummary>, String> {
    let (pb_url, pb_token, required_seconds) = {
        let c = state.config.lock();
        let breaks = state.break_configs.lock();
        (
            c.pb_url.clone(),
            c.pb_token.clone(),
            c.required_work_seconds(&breaks),
        )
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let (from, to) = nepal_range_from_dates(&from_date, &to_date);
    let pb = PocketBase::new(pb_url, pb_token);
    let sessions = pb
        .get_sessions_in_range(&from, &to, &Utc::now(), user_id.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    // External staff have no required hours, so no time loss.
    let external_ids: HashSet<String> = pb
        .get_all_users()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|u| u.is_external_staff)
        .map(|u| u.id)
        .collect();

    let nepal = chrono::FixedOffset::east_opt(5 * 3600 + 45 * 60).unwrap();
    let mut by_user: HashMap<String, UserSummary> = HashMap::new();
    let mut user_days: HashMap<String, HashSet<String>> = HashMap::new();
    let mut day_net: HashMap<(String, String), i64> = HashMap::new();

    for s in &sessions {
        let e = by_user
            .entry(s.user_id.clone())
            .or_insert_with(|| UserSummary {
                user_id: s.user_id.clone(),
                user_name: s.user_name.clone(),
                user_email: s.user_email.clone(),
                session_count: 0,
                days_present: 0,
                total_work_seconds: 0,
                total_break_seconds: 0,
                total_gross_seconds: 0,
                total_net_loss_seconds: 0,
                total_time_loss_seconds: 0,
            });
        e.session_count += 1;
        e.total_work_seconds += s.net_seconds;
        e.total_break_seconds += s.break_seconds;
        e.total_gross_seconds += s.gross_seconds;
        e.total_net_loss_seconds += s.net_loss_seconds;
        let date = s
            .clock_in
            .with_timezone(&nepal)
            .format("%Y-%m-%d")
            .to_string();
        user_days
            .entry(s.user_id.clone())
            .or_default()
            .insert(date.clone());
        *day_net.entry((s.user_id.clone(), date)).or_insert(0) += s.net_seconds;
    }
    for (uid, summary) in by_user.iter_mut() {
        summary.days_present = user_days.get(uid).map(|d| d.len() as u32).unwrap_or(0);
        if required_seconds > 0 && !external_ids.contains(uid) {
            summary.total_time_loss_seconds = day_net
                .iter()
                .filter(|((id, _), _)| id == uid)
                .map(|(_, net)| (required_seconds - net).max(0))
                .sum();
        }
    }
    let mut result: Vec<UserSummary> = by_user.into_values().collect();
    result.sort_by(|a, b| a.user_name.cmp(&b.user_name));
    Ok(result)
}

#[tauri::command]
pub async fn get_network_report(
    state: State<'_, AppState>,
    from_date: String,
    to_date: String,
    user_id: Option<String>,
) -> Result<NetworkReport, String> {
    let (pb_url, pb_token) = {
        let c = state.config.lock();
        (c.pb_url.clone(), c.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    let (from, to) = nepal_range_from_dates(&from_date, &to_date);
    PocketBase::new(pb_url, pb_token)
        .get_network_in_range(&from, &to, user_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_activity_report(
    state: State<'_, AppState>,
    from_date: String,
    to_date: String,
    user_id: String,
) -> Result<ActivityReport, String> {
    let (pb_url, pb_token) = {
        let c = state.config.lock();
        (c.pb_url.clone(), c.pb_token.clone())
    };
    if pb_url.is_empty() || pb_token.is_empty() {
        return Err("Not connected to PocketBase".into());
    }
    if user_id.is_empty() {
        return Err("user_id required for activity report".into());
    }
    let (from, to) = nepal_range_from_dates(&from_date, &to_date);
    PocketBase::new(pb_url, pb_token)
        .get_activity_in_range(&from, &to, &user_id)
        .await
        .map_err(|e| e.to_string())
}

fn notify(app: &tauri::AppHandle, title: &str, body: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        state.audio.play("info");
    }
    let payload = AppNotification {
        title: title.to_string(),
        body: body.to_string(),
        kind: "info".to_string(),
    };
    app.emit("app-notification", payload).ok();
}

fn update_tray(app: &tauri::AppHandle, status: &str) {
    if let Some(tray) = app.tray_by_id("main") {
        let (tooltip, icon_bytes): (&str, &[u8]) = match status {
            "active" => (
                "ClankerClocker — Clocked In",
                include_bytes!("../icons/tray-active.png"),
            ),
            "break" => (
                "ClankerClocker — On Break",
                include_bytes!("../icons/tray-break.png"),
            ),
            _ => (
                "ClankerClocker — Idle",
                include_bytes!("../icons/tray-idle.png"),
            ),
        };
        let _ = tray.set_tooltip(Some(tooltip));
        if let Ok(icon) = tauri::image::Image::from_bytes(icon_bytes) {
            let _ = tray.set_icon(Some(icon));
        }
    }
}
