mod commands;
pub mod config;
mod db;
mod monitor;
mod pocketbase;
mod session;

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

use chrono::{FixedOffset, NaiveTime, TimeZone, Timelike, Utc};
use db::LocalDb;
use pocketbase::PocketBase;
use session::{
    ActivityCounters, ActivitySnapshot, AppConfig, AppNotification, BreakConfig, SessionState,
    SessionStatus,
};

pub struct AppState {
    pub session: Arc<Mutex<SessionState>>,
    pub counters: Arc<Mutex<ActivityCounters>>,
    pub config: Arc<Mutex<AppConfig>>,
    pub input_monitoring: Arc<AtomicBool>,
    pub active_window: Arc<Mutex<(String, String)>>,
    pub db: Arc<Mutex<LocalDb>>,
    pub break_id: Arc<Mutex<Option<String>>>,
    pub break_configs: Arc<Mutex<Vec<BreakConfig>>>,
    pub auto_break_history: Arc<Mutex<HashSet<String>>>,
    pub scheduled_notification_history: Arc<Mutex<HashSet<String>>>,
    pub pending_auto_breaks: Arc<Mutex<HashSet<String>>>,
}

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().expect("no app data dir");
            std::fs::create_dir_all(&data_dir).ok();
            let db_path = data_dir.join("clankerclocker.db");

            let db = LocalDb::open(db_path.to_str().unwrap()).expect("failed to open local db");
            let config = db.load_config().unwrap_or_default();

            let db = Arc::new(Mutex::new(db));
            let config = Arc::new(Mutex::new(config));
            let session: Arc<Mutex<SessionState>> = Arc::new(Mutex::new(SessionState::default()));
            let counters: Arc<Mutex<ActivityCounters>> =
                Arc::new(Mutex::new(ActivityCounters::default()));
            let break_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
            let break_configs: Arc<Mutex<Vec<BreakConfig>>> =
                Arc::new(Mutex::new(BreakConfig::defaults()));
            let auto_break_history: Arc<Mutex<HashSet<String>>> =
                Arc::new(Mutex::new(HashSet::new()));
            let scheduled_notification_history: Arc<Mutex<HashSet<String>>> =
                Arc::new(Mutex::new(HashSet::new()));
            let pending_auto_breaks: Arc<Mutex<HashSet<String>>> =
                Arc::new(Mutex::new(HashSet::new()));
            let input_monitoring = Arc::new(AtomicBool::new(false));
            let active_window = Arc::new(Mutex::new((String::new(), String::new())));

            // System tray
            let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            let icon_bytes = include_bytes!("../icons/icon.png");
            let tray_icon = tauri::image::Image::from_bytes(icon_bytes).expect("invalid tray icon");

            TrayIconBuilder::with_id("main")
                .icon(tray_icon)
                .tooltip("ClankerClocker — Idle")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            win.show().ok();
                            win.set_focus().ok();
                        }
                    }
                    "quit" => {
                        let app_clone = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Some(state) = app_clone.try_state::<AppState>() {
                                let (pb_url, pb_token, session_id, total_break_secs) = {
                                    let cfg = state.config.lock();
                                    let sess = state.session.lock();
                                    (
                                        cfg.pb_url.clone(),
                                        cfg.pb_token.clone(),
                                        sess.session_id.clone(),
                                        sess.total_break_seconds,
                                    )
                                };
                                if let Some(sid) = session_id {
                                    if !sid.starts_with("local-")
                                        && !pb_url.is_empty()
                                        && !pb_token.is_empty()
                                    {
                                        let pb = PocketBase::new(pb_url, pb_token);
                                        let _ = pb
                                            .close_session(&sid, &Utc::now(), total_break_secs)
                                            .await;
                                    }
                                    *state.session.lock() = SessionState::default();
                                }
                            }
                            app_clone.exit(0);
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            if win.is_visible().unwrap_or(false) {
                                win.hide().ok();
                            } else {
                                win.show().ok();
                                win.set_focus().ok();
                            }
                        }
                    }
                })
                .build(app)?;

            // Intercept close → minimize to tray
            if let Some(win) = app.get_webview_window("main") {
                let win_clone = win.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        win_clone.hide().ok();
                    }
                });
                win.show().ok();
            }

            app.manage(AppState {
                session: session.clone(),
                counters: counters.clone(),
                config: config.clone(),
                input_monitoring: input_monitoring.clone(),
                active_window: active_window.clone(),
                db: db.clone(),
                break_id: break_id.clone(),
                break_configs: break_configs.clone(),
                auto_break_history: auto_break_history.clone(),
                scheduled_notification_history: scheduled_notification_history.clone(),
                pending_auto_breaks: pending_auto_breaks.clone(),
            });

            // Start input monitor thread (rdev blocks its OS thread)
            monitor::input::start(counters.clone(), input_monitoring.clone());
            #[cfg(target_os = "linux")]
            monitor::window::start_hyprland_active_window_cache(active_window.clone());

            // Main background loop: live counters, snapshots, network
            let app_handle = app.handle().clone();
            let session_bg = session.clone();
            let counters_bg = counters.clone();
            let config_bg = config.clone();
            let db_bg = db.clone();
            let break_configs_bg = break_configs.clone();
            let break_id_bg = break_id.clone();
            let auto_break_history_bg = auto_break_history.clone();
            let scheduled_notification_history_bg = scheduled_notification_history.clone();
            let pending_auto_breaks_bg = pending_auto_breaks.clone();
            let input_monitoring_bg = input_monitoring.clone();
            let active_window_bg = active_window.clone();

            tauri::async_runtime::spawn(async move {
                let mut net_seen: HashSet<String> = HashSet::new();
                let mut dns_cache: HashMap<String, String> = HashMap::new();
                let mut snapshot_tick: u32 = 0;
                let mut network_tick: u32 = 0;
                let mut break_config_refresh_tick: u32 = 60;
                const IDLE_WARNING_SECONDS: u64 = 4 * 60 + 30;
                const IDLE_CLOCKOUT_SECONDS: u64 = 5 * 60;

                loop {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    snapshot_tick += 5;
                    network_tick += 5;
                    break_config_refresh_tick += 5;

                    if break_config_refresh_tick >= 60 {
                        break_config_refresh_tick = 0;
                        let (pb_url, pb_token) = {
                            let cfg = config_bg.lock();
                            (cfg.pb_url.clone(), cfg.pb_token.clone())
                        };

                        if !pb_url.is_empty() && !pb_token.is_empty() {
                            let pb = PocketBase::new(pb_url, pb_token);
                            if let Ok(configs) = pb.get_break_configs().await {
                                *break_configs_bg.lock() = configs;
                            }
                        } else {
                            *break_configs_bg.lock() = BreakConfig::defaults();
                        }
                    }

                    let now = Utc::now();
                    let now_npt = now.with_timezone(&nepal_offset());
                    let session_snapshot = session_bg.lock().clone();
                    let config_snapshot = config_bg.lock().clone();
                    let status = session_snapshot.status.clone();
                    let session_id = session_snapshot.session_id.clone();
                    let idle_seconds = counters_bg.lock().idle_seconds();

                    if status == SessionStatus::Idle {
                        auto_break_history_bg.lock().clear();
                        pending_auto_breaks_bg.lock().clear();
                    }

                    if let Some(sid) = session_id.as_ref() {
                        let should_warn = status == SessionStatus::Active
                            && config_snapshot.auto_clock_out_enabled
                            && idle_seconds >= IDLE_WARNING_SECONDS
                            && idle_seconds < IDLE_CLOCKOUT_SECONDS;

                        if should_warn {
                            let key = format!("{}:idle_clockout_warning", sid);
                            if !scheduled_notification_history_bg.lock().contains(&key) {
                                scheduled_notification_history_bg.lock().insert(key);
                                app_handle
                                    .emit(
                                        "app-notification",
                                        AppNotification {
                                            title: "clock-out warning".into(),
                                            body: "You are about to be clocked out for inactivity. Move your mouse or press any key to stay clocked in.".into(),
                                        },
                                    )
                                    .ok();
                            }
                        }

                        let should_idle_clockout = status == SessionStatus::Active
                            && config_snapshot.auto_clock_out_enabled
                            && idle_seconds >= IDLE_CLOCKOUT_SECONDS;

                        if should_idle_clockout {
                            let key = format!("{}:idle_clockout", sid);
                            if !scheduled_notification_history_bg.lock().contains(&key) {
                                scheduled_notification_history_bg.lock().insert(key);
                                app_handle
                                    .emit(
                                        "app-notification",
                                        AppNotification {
                                            title: "auto clocked out for idle".into(),
                                            body: "You were clocked out automatically after 5 minutes of inactivity.".into(),
                                        },
                                    )
                                    .ok();
                            }

                            let _ = commands::clock_out_internal(
                                &app_handle,
                                &session_bg,
                                &counters_bg,
                                &config_bg,
                                &break_id_bg,
                            )
                            .await;
                        }
                    }

                    if let Some(clock_in_due) =
                        schedule_datetime(now_npt.date_naive(), &config_snapshot.clock_in_time)
                    {
                        let key = reminder_key(now_npt, "clock_in");
                        let should_notify = status == SessionStatus::Idle
                            && now_npt >= clock_in_due
                            && !scheduled_notification_history_bg.lock().contains(&key);
                        if should_notify {
                            scheduled_notification_history_bg.lock().insert(key);
                            app_handle
                                .emit(
                                    "app-notification",
                                    AppNotification {
                                        title: "your clockin time is here".into(),
                                        body: "it's time to clock in for your shift".into(),
                                    },
                                )
                                .ok();
                        }
                    }

                    if let Some(clock_out_due) =
                        schedule_datetime(now_npt.date_naive(), &config_snapshot.clock_out_time)
                    {
                        let past_due = (status == SessionStatus::Active
                            || status == SessionStatus::OnBreak)
                            && now_npt >= clock_out_due;

                        if past_due {
                            // Notification fires once per day (key-gated)
                            let key = reminder_key(now_npt, "clock_out");
                            if !scheduled_notification_history_bg.lock().contains(&key) {
                                scheduled_notification_history_bg.lock().insert(key);
                                app_handle
                                    .emit(
                                        "app-notification",
                                        AppNotification {
                                            title: "your clockout time is here".into(),
                                            body: if config_snapshot.auto_clock_out_enabled {
                                                "your clockout time is here. auto clocking out now."
                                                    .into()
                                            } else {
                                                "your clockout time is here".into()
                                            },
                                        },
                                    )
                                    .ok();
                            }

                            // Auto clock-out retries every loop until it succeeds.
                            // After success the session status becomes Idle so past_due
                            // turns false and this block stops executing.
                            if config_snapshot.auto_clock_out_enabled {
                                let _ = commands::clock_out_internal(
                                    &app_handle,
                                    &session_bg,
                                    &counters_bg,
                                    &config_bg,
                                    &break_id_bg,
                                )
                                .await;
                            }
                        }
                    }

                    let configs = break_configs_bg.lock().clone();
                    let history = auto_break_history_bg.lock().clone();
                    let pending = pending_auto_breaks_bg.lock().clone();

                    if status == SessionStatus::Active {
                        if let Some(config) = find_pending_auto_break(&configs, &pending) {
                            pending_auto_breaks_bg.lock().remove(&config.id);
                            if commands::start_break_internal(
                                &app_handle,
                                &session_bg,
                                &config_bg,
                                &break_id_bg,
                                &config.type_key,
                                Some(&config.name),
                            )
                            .await
                            .is_ok()
                            {
                                auto_break_history_bg
                                    .lock()
                                    .insert(auto_break_history_key(&config.id));
                            }
                        } else {
                            let due = due_auto_breaks(&configs, &history);
                            if let Some(config) = due.first() {
                                if commands::start_break_internal(
                                    &app_handle,
                                    &session_bg,
                                    &config_bg,
                                    &break_id_bg,
                                    &config.type_key,
                                    Some(&config.name),
                                )
                                .await
                                .is_ok()
                                {
                                    auto_break_history_bg
                                        .lock()
                                        .insert(auto_break_history_key(&config.id));
                                }
                            }
                        }
                    } else if status == SessionStatus::OnBreak {
                        // Check if current break should end notification
                        let (break_start, break_name) = {
                            let sess = session_bg.lock();
                            (sess.break_start, sess.break_name.clone())
                        };
                        if let (Some(start), Some(name)) = (break_start, break_name) {
                            let duration = (Utc::now() - start).num_minutes();
                            // Find matching config to get planned duration
                            if let Some(config) = configs.iter().find(|c| c.name == name) {
                                if config.duration_minutes > 0
                                    && duration >= config.duration_minutes as i64
                                {
                                    let key = reminder_key(now_npt, &format!("break_end:{}", config.id));
                                    if !scheduled_notification_history_bg.lock().contains(&key) {
                                        scheduled_notification_history_bg.lock().insert(key);
                                        app_handle
                                            .emit(
                                                "app-notification",
                                                AppNotification {
                                                    title: format!("your break {name} is ending"),
                                                    body: format!("your break {name} is ending"),
                                                },
                                            )
                                            .ok();
                                    }
                                }
                            }
                        }

                        for config in due_auto_breaks(&configs, &history) {
                            pending_auto_breaks_bg.lock().insert(config.id.clone());
                        }
                    }

                    // Prefer the compositor event cache; fall back to a fresh poll if empty.
                    let (active_app, active_window) = {
                        let cached = active_window_bg.lock().clone();
                        if !cached.0.is_empty() || !cached.1.is_empty() {
                            cached
                        } else {
                            tauri::async_runtime::spawn_blocking(monitor::window::get_active_window)
                                .await
                                .unwrap_or_default()
                        }
                    };

                    // Emit live counters every 5s without draining
                    {
                        let c = counters_bg.lock();
                        let live = serde_json::json!({
                            "keystrokes": c.keystrokes,
                            "mouse_clicks": c.mouse_clicks,
                            "mouse_distance_px": c.mouse_distance_px,
                            "idle_seconds": c.idle_seconds(),
                            "active_app": &active_app,
                            "active_window": &active_window,
                            "input_monitoring_active": input_monitoring_bg.load(Ordering::Relaxed),
                        });
                        drop(c);
                        app_handle.emit("live-counters", live).ok();
                    }

                    // Activity snapshot every 30s (drains counters)
                    if snapshot_tick >= 30 {
                        snapshot_tick = 0;
                        let (ks, mc, md) = counters_bg.lock().drain();
                        let idle = counters_bg.lock().idle_seconds();

                        let snap = ActivitySnapshot {
                            timestamp: chrono::Utc::now(),
                            keystrokes: ks,
                            mouse_clicks: mc,
                            mouse_distance_px: md,
                            active_app: active_app.clone(),
                            active_window: active_window.clone(),
                            idle_seconds: idle,
                        };

                        app_handle.emit("activity-update", &snap).ok();

                        let (pb_url, pb_token, session_id) = {
                            let cfg = config_bg.lock();
                            let sess = session_bg.lock();
                            (
                                cfg.pb_url.clone(),
                                cfg.pb_token.clone(),
                                sess.session_id.clone(),
                            )
                        };

                        if let Some(sid) = session_id {
                            if !pb_url.is_empty() && !pb_token.is_empty() {
                                let pb = PocketBase::new(pb_url, pb_token);
                                if pb.push_snapshot(&sid, &snap).await.is_err() {
                                    db_bg.lock().queue_snapshot(&sid, &snap).ok();
                                }
                            } else {
                                db_bg.lock().queue_snapshot(&sid, &snap).ok();
                            }
                        }
                    }

                    // Network connections every 60s
                    if network_tick >= 60 {
                        network_tick = 0;

                        let mut seen_clone = net_seen.clone();
                        let mut cache_clone = dns_cache.clone();
                        let result = tauri::async_runtime::spawn_blocking(move || {
                            let conns = monitor::network::sample_connections(
                                &mut seen_clone,
                                &mut cache_clone,
                            );
                            (conns, seen_clone, cache_clone)
                        })
                        .await;

                        let new_conns = match result {
                            Ok((conns, updated_seen, updated_cache)) => {
                                net_seen = updated_seen;
                                dns_cache = updated_cache;
                                conns
                            }
                            Err(_) => Vec::new(),
                        };

                        if !new_conns.is_empty() {
                            app_handle.emit("network-update", &new_conns).ok();

                            let (pb_url, pb_token, session_id) = {
                                let cfg = config_bg.lock();
                                let sess = session_bg.lock();
                                (
                                    cfg.pb_url.clone(),
                                    cfg.pb_token.clone(),
                                    sess.session_id.clone(),
                                )
                            };

                            if let Some(sid) = session_id {
                                if !pb_url.is_empty() && !pb_token.is_empty() {
                                    let pb = PocketBase::new(pb_url, pb_token);
                                    for conn in &new_conns {
                                        if pb.push_network_connection(&sid, conn).await.is_err() {
                                            db_bg.lock().queue_network(&sid, &[conn.clone()]).ok();
                                        }
                                    }
                                } else {
                                    db_bg.lock().queue_network(&sid, &new_conns).ok();
                                }
                            }
                        }
                    }
                }
            });

            // Offline sync retry every 5 minutes
            let config_sync = config.clone();
            let db_sync = db.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(300)).await;
                    let (pb_url, pb_token) = {
                        let cfg = config_sync.lock();
                        (cfg.pb_url.clone(), cfg.pb_token.clone())
                    };
                    if pb_url.is_empty() || pb_token.is_empty() {
                        continue;
                    }
                    let pb = PocketBase::new(pb_url, pb_token);

                    let snaps = { db_sync.lock().get_unsynced_snapshots().unwrap_or_default() };
                    for (id, sid, snap) in snaps {
                        if pb.push_snapshot(&sid, &snap).await.is_ok() {
                            db_sync.lock().mark_snapshot_synced(id).ok();
                        }
                    }

                    let conns = { db_sync.lock().get_unsynced_network().unwrap_or_default() };
                    for (id, sid, conn) in conns {
                        if pb.push_network_connection(&sid, &conn).await.is_ok() {
                            db_sync.lock().mark_network_synced(id).ok();
                        }
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::authenticate_pb,
            commands::get_settings,
            commands::get_session_state,
            commands::clock_in,
            commands::clock_out,
            commands::start_break,
            commands::end_break,
            commands::get_today_stats,
            commands::get_team_status,
            commands::get_break_configs,
            commands::get_user_activity,
            commands::get_user_network,
            commands::get_user_monthly_sessions,
            commands::get_user_today_breakdown,
            commands::save_work_schedule,
            commands::refresh_auth_state,
            commands::clear_auth,
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}

fn auto_break_history_key(config_id: &str) -> String {
    let nepal_offset = FixedOffset::east_opt(5 * 3600 + 45 * 60).expect("valid Nepal offset");
    let now_nepal = Utc::now().with_timezone(&nepal_offset);
    format!("{}:{}", now_nepal.format("%Y-%m-%d"), config_id)
}

fn parse_hhmm(value: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M").ok()
}

fn nepal_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 45 * 60).expect("valid Nepal offset")
}

fn schedule_datetime(date: chrono::NaiveDate, hhmm: &str) -> Option<chrono::DateTime<FixedOffset>> {
    let time = parse_hhmm(hhmm)?;
    let naive = date.and_hms_opt(time.hour(), time.minute(), 0)?;
    nepal_offset().from_local_datetime(&naive).single()
}

fn reminder_key(now: chrono::DateTime<FixedOffset>, kind: &str) -> String {
    format!("{}:{}", now.format("%Y-%m-%d"), kind)
}

fn due_auto_breaks(configs: &[BreakConfig], history: &HashSet<String>) -> Vec<BreakConfig> {
    let now_npt = Utc::now().with_timezone(&nepal_offset());
    let now_time = NaiveTime::from_hms_opt(now_npt.hour(), now_npt.minute(), 0);
    let Some(now_time) = now_time else {
        return Vec::new();
    };

    let mut due: Vec<BreakConfig> = configs
        .iter()
        .filter(|config| config.auto_start_enabled)
        .filter(|config| {
            let key = auto_break_history_key(&config.id);
            if history.contains(&key) {
                return false;
            }
            let Some(start_time) = parse_hhmm(config.auto_start_time.as_deref().unwrap_or(""))
            else {
                return false;
            };
            let Some(end_time) = parse_hhmm(config.auto_end_time.as_deref().unwrap_or("")) else {
                return false;
            };
            now_time >= start_time && now_time <= end_time
        })
        .cloned()
        .collect();
    due.sort_by_key(|config| config.sort_order);
    due
}

fn find_pending_auto_break(
    configs: &[BreakConfig],
    pending: &HashSet<String>,
) -> Option<BreakConfig> {
    configs
        .iter()
        .filter(|config| pending.contains(&config.id))
        .min_by_key(|config| config.sort_order)
        .cloned()
}
