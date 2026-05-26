mod commands;
pub mod config;
mod db;
mod monitor;
mod pocketbase;
mod session;

use parking_lot::Mutex;
use std::collections::{HashSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

use db::LocalDb;
use session::{ActivityCounters, ActivitySnapshot, AppConfig, SessionState, SessionStatus};
use pocketbase::PocketBase;

pub struct AppState {
    pub session: Arc<Mutex<SessionState>>,
    pub counters: Arc<Mutex<ActivityCounters>>,
    pub config: Arc<Mutex<AppConfig>>,
    pub input_monitoring: Arc<AtomicBool>,
    pub db: Arc<Mutex<LocalDb>>,
    pub break_id: Arc<Mutex<Option<String>>>,
}

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().expect("no app data dir");
            std::fs::create_dir_all(&data_dir).ok();
            let db_path = data_dir.join("clankerclocker.db");

            let db = LocalDb::open(db_path.to_str().unwrap()).expect("failed to open local db");
            let config = db.load_config().unwrap_or_default();

            let db = Arc::new(Mutex::new(db));
            let config = Arc::new(Mutex::new(config));
            let session: Arc<Mutex<SessionState>> = Arc::new(Mutex::new(SessionState::default()));
            let counters: Arc<Mutex<ActivityCounters>> = Arc::new(Mutex::new(ActivityCounters::default()));
            let break_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
            let input_monitoring = Arc::new(AtomicBool::new(false));

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
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
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
                db: db.clone(),
                break_id,
            });

            // Start input monitor thread (rdev blocks its OS thread)
            monitor::input::start(counters.clone(), input_monitoring.clone());

            // Main background loop: live counters, snapshots, network
            let app_handle = app.handle().clone();
            let session_bg = session.clone();
            let counters_bg = counters.clone();
            let config_bg = config.clone();
            let db_bg = db.clone();

            tauri::async_runtime::spawn(async move {
                let mut net_seen: HashSet<String> = HashSet::new();
                let mut dns_cache: HashMap<String, String> = HashMap::new();
                let mut snapshot_tick: u32 = 0;
                let mut network_tick: u32 = 0;

                loop {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    snapshot_tick += 5;
                    network_tick += 5;

                    let status = session_bg.lock().status.clone();
                    if status == SessionStatus::Idle {
                        continue;
                    }

                    // Active window poll every 5s
                    let (active_app, active_window) =
                        tauri::async_runtime::spawn_blocking(monitor::window::get_active_window)
                            .await
                            .unwrap_or_default();

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
                            (cfg.pb_url.clone(), cfg.pb_token.clone(), sess.session_id.clone())
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
                            let conns = monitor::network::sample_connections(&mut seen_clone, &mut cache_clone);
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
                                (cfg.pb_url.clone(), cfg.pb_token.clone(), sess.session_id.clone())
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
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
