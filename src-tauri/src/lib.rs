mod audio;
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

use chrono::{DateTime, FixedOffset, NaiveTime, TimeZone, Timelike, Utc};
use db::LocalDb;
use pocketbase::PocketBase;
use session::{
    ActivityCounters, ActivitySnapshot, AppConfig, AppNotification, BreakConfig, SessionState,
    SessionStatus,
};

// store application state for each session breaks, notification, idle and monitoring
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
    pub audio: Arc<audio::AudioPlayer>,
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

            // Gray = idle; commands::update_tray swaps in the green/yellow
            // variants on clock-in and break transitions.
            let icon_bytes = include_bytes!("../icons/tray-idle.png");
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
                                // Use the full clock_out path so open breaks are
                                // closed properly before the process exits.
                                let _ = commands::clock_out_internal(
                                    &app_clone,
                                    &state.session,
                                    &state.counters,
                                    &state.config,
                                    &state.break_id,
                                    None,
                                )
                                .await;
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
                win.set_focus().ok();
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
                audio: Arc::new(audio::AudioPlayer::new()),
            });

            // Start input monitor thread (rdev blocks its OS thread)
            monitor::input::start(counters.clone(), input_monitoring.clone());
            #[cfg(target_os = "linux")]
            monitor::window::start_hyprland_active_window_cache(active_window.clone());

            // Main background loop: idle/scheduled clock-out, auto-breaks,
            // live counters, activity snapshots, network sampling. Each
            // concern lives in its own `tick_*` method on `Background`.
            tauri::async_runtime::spawn(
                Background {
                    app: app.handle().clone(),
                    session: session.clone(),
                    counters: counters.clone(),
                    config: config.clone(),
                    db: db.clone(),
                    break_id: break_id.clone(),
                    break_configs: break_configs.clone(),
                    auto_break_history: auto_break_history.clone(),
                    notification_history: scheduled_notification_history.clone(),
                    pending_auto_breaks: pending_auto_breaks.clone(),
                    input_monitoring: input_monitoring.clone(),
                    active_window: active_window.clone(),
                    debug: DebugOverrides::load(),
                    net_seen: HashSet::new(),
                    dns_cache: HashMap::new(),
                    snapshot_tick: 0,
                    network_tick: 0,
                    break_config_refresh_tick: 60,
                    scheduled_clockout_warned_at: None,
                }
                .run(),
            );

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

                    // Prune synced rows older than 7 days to keep the DB lean.
                    db_sync.lock().cleanup_old_synced(7).ok();
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
            commands::extend_session,
            commands::set_user_external_staff,
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
            commands::get_all_users,
            commands::get_sessions_report,
            commands::get_time_summary,
            commands::get_network_report,
            commands::get_activity_report,
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}

/// Shared state and per-loop scratch for the 5-second background loop.
/// `run()` is the dispatcher; each `tick_*` method owns one concern.
struct Background {
    app: tauri::AppHandle,
    session: Arc<Mutex<SessionState>>,
    counters: Arc<Mutex<ActivityCounters>>,
    config: Arc<Mutex<AppConfig>>,
    db: Arc<Mutex<LocalDb>>,
    break_id: Arc<Mutex<Option<String>>>,
    break_configs: Arc<Mutex<Vec<BreakConfig>>>,
    auto_break_history: Arc<Mutex<HashSet<String>>>,
    notification_history: Arc<Mutex<HashSet<String>>>,
    pending_auto_breaks: Arc<Mutex<HashSet<String>>>,
    input_monitoring: Arc<AtomicBool>,
    active_window: Arc<Mutex<(String, String)>>,
    debug: DebugOverrides,

    // Loop-local scratch state
    net_seen: HashSet<String>,
    dns_cache: HashMap<String, String>,
    snapshot_tick: u32,
    network_tick: u32,
    break_config_refresh_tick: u32,
    scheduled_clockout_warned_at: Option<DateTime<Utc>>,
}

impl Background {
    async fn run(mut self) {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            self.snapshot_tick += 5;
            self.network_tick += 5;
            self.break_config_refresh_tick += 5;

            self.tick_break_config_refresh().await;

            let now = Utc::now();
            let now_npt = now.with_timezone(&nepal_offset());
            let mut config_snapshot = self.config.lock().clone();
            self.debug.apply_to_config(&mut config_snapshot);

            let (active_app, active_window) = self.read_active_window().await;

            if self.status() == SessionStatus::Idle {
                // Clear per-session state so the next session starts fresh.
                self.net_seen.clear();
                self.dns_cache.clear();
                self.auto_break_history.lock().clear();
                self.pending_auto_breaks.lock().clear();
            }

            self.tick_idle_clockout(&config_snapshot, &active_app, &active_window)
                .await;
            self.tick_clock_in_reminder(now_npt, &config_snapshot);
            self.tick_scheduled_clockout(now, now_npt, &config_snapshot)
                .await;
            self.tick_auto_breaks().await;
            self.emit_live_counters(&active_app, &active_window);

            if self.snapshot_tick >= 30 {
                self.snapshot_tick = 0;
                self.push_activity_snapshot(active_app, active_window).await;
            }
            if self.network_tick >= 60 {
                self.network_tick = 0;
                self.push_network_samples().await;
            }
        }
    }

    fn status(&self) -> SessionStatus {
        self.session.lock().status.clone()
    }

    fn pb(&self) -> Option<PocketBase> {
        let cfg = self.config.lock();
        if cfg.pb_url.is_empty() || cfg.pb_token.is_empty() {
            return None;
        }
        Some(PocketBase::new(cfg.pb_url.clone(), cfg.pb_token.clone()))
    }

    fn notify(&self, kind: &str, title: &str, body: &str) {
        if let Some(state) = self.app.try_state::<AppState>() {
            state.audio.play(kind);
        }
        self.app
            .emit(
                "app-notification",
                AppNotification {
                    title: title.into(),
                    body: body.into(),
                    kind: kind.into(),
                },
            )
            .ok();
    }

    /// Fires a notification at most once for `key` (kept in the in-memory
    /// notification history). Returns true when it fired.
    fn notify_once(&self, key: String, kind: &str, title: &str, body: &str) -> bool {
        if self.notification_history.lock().contains(&key) {
            return false;
        }
        self.notification_history.lock().insert(key);
        self.notify(kind, title, body);
        true
    }

    async fn read_active_window(&self) -> (String, String) {
        // Prefer the cache (kept warm by the Hyprland listener on Linux) so
        // conference detection sees the window before idle logic runs.
        let cached = self.active_window.lock().clone();
        if !cached.0.is_empty() || !cached.1.is_empty() {
            return cached;
        }
        tauri::async_runtime::spawn_blocking(monitor::window::get_active_window)
            .await
            .unwrap_or_default()
    }

    async fn tick_break_config_refresh(&mut self) {
        if self.break_config_refresh_tick < 60 {
            return;
        }
        self.break_config_refresh_tick = 0;
        match self.pb() {
            Some(pb) => {
                if let Ok(configs) = pb.get_break_configs().await {
                    *self.break_configs.lock() = configs;
                }
            }
            None => *self.break_configs.lock() = BreakConfig::defaults(),
        }
    }

    async fn tick_idle_clockout(&self, cfg: &AppConfig, active_app: &str, active_window: &str) {
        let (status, session_id) = {
            let sess = self.session.lock();
            (sess.status.clone(), sess.session_id.clone())
        };
        let Some(sid) = session_id else { return };
        let idle_seconds = self.counters.lock().idle_seconds();

        // Once the user is active again, forget this episode's idle
        // notifications so the next idle stretch warns again.
        if idle_seconds < self.debug.idle_warning_seconds {
            let mut history = self.notification_history.lock();
            history.remove(&format!("{sid}:idle_clockout_warning"));
            history.remove(&format!("{sid}:idle_clockout"));
        }

        if status != SessionStatus::Active || !cfg.auto_clock_out_enabled {
            return;
        }
        // Suppress idle warnings/clock-out while the user is in a
        // video/audio conference — talking doesn't move the mouse.
        if is_conference_active(active_app, active_window) {
            return;
        }

        if idle_seconds >= self.debug.idle_warning_seconds
            && idle_seconds < self.debug.idle_clockout_seconds
        {
            self.notify_once(
                format!("{sid}:idle_clockout_warning"),
                "idle_clockout_warning",
                "clock-out warning",
                "You are about to be clocked out for inactivity. Move your mouse or press any key to stay clocked in.",
            );
        }

        if idle_seconds >= self.debug.idle_clockout_seconds {
            self.notify_once(
                format!("{sid}:idle_clockout"),
                "idle_clockout",
                "auto clocked out for idle",
                "You were clocked out automatically after 5 minutes of inactivity.",
            );
            let _ = commands::clock_out_internal(
                &self.app,
                &self.session,
                &self.counters,
                &self.config,
                &self.break_id,
                None,
            )
            .await;
        }
    }

    fn tick_clock_in_reminder(&self, now_npt: DateTime<FixedOffset>, cfg: &AppConfig) {
        // External staff have no fixed shift to be reminded of.
        if cfg.is_external_staff {
            return;
        }
        let Some(clock_in_due) = schedule_datetime(now_npt.date_naive(), &cfg.clock_in_time) else {
            return;
        };
        // Only fire within a short window right at clock-in time, not
        // any time afterwards — otherwise an app restart later in the
        // day (which resets the in-memory history) would re-send it.
        if self.status() == SessionStatus::Idle
            && now_npt >= clock_in_due
            && now_npt < clock_in_due + chrono::Duration::minutes(2)
        {
            self.notify_once(
                reminder_key(now_npt, "clock_in"),
                "clock_in_reminder",
                "your clockin time is here",
                "it's time to clock in for your shift",
            );
        }
    }

    async fn tick_scheduled_clockout(
        &mut self,
        now: DateTime<Utc>,
        now_npt: DateTime<FixedOffset>,
        cfg: &AppConfig,
    ) {
        // External staff work outside the company schedule entirely.
        if cfg.is_external_staff {
            return;
        }
        let (status, extended) = {
            let sess = self.session.lock();
            (sess.status.clone(), sess.extended_past_schedule)
        };
        if status == SessionStatus::Idle || extended {
            return;
        }
        let Some(clock_out_due) = schedule_datetime(now_npt.date_naive(), &cfg.clock_out_time)
        else {
            return;
        };
        if now_npt < clock_out_due {
            return;
        }

        // An employee who still owes hours gets the "keep working?" prompt
        // right away — it closes nothing, so it needs no warning or grace,
        // and waiting here used to lose the race against the server cron.
        if cfg.auto_clock_out_enabled && self.maybe_prompt_time_loss(now_npt, cfg).await {
            self.scheduled_clockout_warned_at = None;
            return;
        }

        // Warning notification fires once per day (key-gated)
        if self.notify_once(
            reminder_key(now_npt, "clock_out"),
            "scheduled_clockout_warning",
            "your clockout time is here",
            if cfg.auto_clock_out_enabled {
                "your clockout time is here. you'll be auto clocked out shortly."
            } else {
                "your clockout time is here"
            },
        ) {
            self.scheduled_clockout_warned_at = Some(now);
        }

        if !cfg.auto_clock_out_enabled {
            return;
        }

        // Auto clock-out only happens after the warning notification
        // (+ sound) above has had a moment to be seen/heard.
        let grace_elapsed = self
            .scheduled_clockout_warned_at
            .map(|warned_at| (now - warned_at).num_seconds() >= self.debug.clockout_grace_seconds)
            .unwrap_or(true);
        if !grace_elapsed {
            return;
        }

        self.notify(
            "scheduled_clockout",
            "auto clocked out",
            "your scheduled clock-out time passed. you've been clocked out automatically.",
        );
        let _ = commands::clock_out_internal(
            &self.app,
            &self.session,
            &self.counters,
            &self.config,
            &self.break_id,
            None,
        )
        .await;
        self.scheduled_clockout_warned_at = None;
    }

    /// At scheduled clock-out with a work-hour deficit, emits the
    /// "time-loss-prompt" event so the UI can ask whether to keep working.
    /// The session is pre-marked as extended (locally and on PocketBase) so
    /// neither this loop nor the server cron closes it while the user
    /// decides; declining just clocks out, ignoring it falls back to the
    /// 5-minute idle auto-clockout. Returns true when the prompt fired.
    async fn maybe_prompt_time_loss(&self, now_npt: DateTime<FixedOffset>, cfg: &AppConfig) -> bool {
        let required = cfg.required_seconds();
        if required <= 0 {
            return false;
        }
        let prompt_key = reminder_key(now_npt, "time_loss_prompt");
        if self.notification_history.lock().contains(&prompt_key) {
            return false;
        }
        let Some(pb) = self.pb() else { return false };
        let user_id = self.config.lock().user_id.clone();
        if user_id.is_empty() {
            return false;
        }
        let Ok(stats) = pb.get_today_stats(&user_id).await else {
            return false; // offline — fall through to the normal auto clock-out
        };
        let deficit = required - stats.total_work_seconds;
        if deficit < 60 {
            return false;
        }

        self.notification_history.lock().insert(prompt_key);

        let session_id = {
            let mut sess = self.session.lock();
            sess.extended_past_schedule = true;
            let s = sess.clone();
            self.app.emit("session-update", s.clone()).ok();
            s.session_id
        };
        if let Some(sid) = session_id.filter(|id| !id.starts_with("local-")) {
            pb.set_session_extended(&sid, true).await.ok();
        }

        self.app
            .emit(
                "time-loss-prompt",
                serde_json::json!({ "deficit_seconds": deficit }),
            )
            .ok();
        let hours = deficit / 3600;
        let minutes = (deficit % 3600) / 60;
        self.notify(
            "scheduled_clockout_warning",
            "your office time has finished",
            &format!(
                "you still have {hours}h {minutes:02}m of time loss today. choose whether to keep working to complete your hours."
            ),
        );
        true
    }

    async fn tick_auto_breaks(&self) {
        let status = self.status();
        let mut configs = self.break_configs.lock().clone();
        self.debug.inject_break_config(&mut configs);
        let history = self.auto_break_history.lock().clone();
        let pending = self.pending_auto_breaks.lock().clone();

        if status == SessionStatus::Active {
            // Breaks queued while the user was already on break start first;
            // otherwise start whichever scheduled break is currently due.
            let next = find_pending_auto_break(&configs, &pending).or_else(|| {
                due_auto_breaks(&configs, &history).first().cloned()
            });
            let Some(config) = next else { return };
            self.pending_auto_breaks.lock().remove(&config.id);
            let started = commands::start_break_internal(
                &self.app,
                &self.session,
                &self.config,
                &self.break_id,
                &config.type_key,
                Some(&config.name),
            )
            .await
            .is_ok();
            if started {
                self.auto_break_history
                    .lock()
                    .insert(auto_break_history_key(&config.id));
            }
        } else if status == SessionStatus::OnBreak {
            // Notify when the current break has run past its planned duration.
            let (break_start, break_name) = {
                let sess = self.session.lock();
                (sess.break_start, sess.break_name.clone())
            };
            if let (Some(start), Some(name)) = (break_start, break_name) {
                let duration = (Utc::now() - start).num_minutes();
                if let Some(config) = configs.iter().find(|c| c.name == name) {
                    if config.duration_minutes > 0 && duration >= config.duration_minutes as i64 {
                        // Key is per break-instance (not per day) so two breaks
                        // of the same type in one day both fire notifications.
                        self.notify_once(
                            format!("break_end:{}:{}", config.id, start.timestamp()),
                            "info",
                            &format!("your break {name} has ended"),
                            "your break time is up — time to get back to work.",
                        );
                    }
                }
            }

            // Scheduled breaks that become due mid-break start once this one ends.
            for config in due_auto_breaks(&configs, &history) {
                self.pending_auto_breaks.lock().insert(config.id.clone());
            }
        }
    }

    fn emit_live_counters(&self, active_app: &str, active_window: &str) {
        let c = self.counters.lock();
        let live = serde_json::json!({
            "keystrokes": c.keystrokes,
            "mouse_clicks": c.mouse_clicks,
            "mouse_distance_px": c.mouse_distance_px,
            "idle_seconds": c.idle_seconds(),
            "active_app": active_app,
            "active_window": active_window,
            "input_monitoring_active": self.input_monitoring.load(Ordering::Relaxed),
        });
        drop(c);
        self.app.emit("live-counters", live).ok();
    }

    /// Every 30s: drain the counters into a snapshot, show it in the UI, and
    /// push it to PocketBase (queued locally when offline).
    async fn push_activity_snapshot(&self, active_app: String, active_window: String) {
        let (ks, mc, md) = self.counters.lock().drain();
        let idle = self.counters.lock().idle_seconds();

        let snap = ActivitySnapshot {
            timestamp: Utc::now(),
            keystrokes: ks,
            mouse_clicks: mc,
            mouse_distance_px: md,
            active_app,
            active_window,
            idle_seconds: idle,
        };
        self.app.emit("activity-update", &snap).ok();

        let Some(sid) = self.session.lock().session_id.clone() else {
            return;
        };
        match self.pb() {
            Some(pb) => {
                if pb.push_snapshot(&sid, &snap).await.is_err() {
                    self.db.lock().queue_snapshot(&sid, &snap).ok();
                }
            }
            None => {
                self.db.lock().queue_snapshot(&sid, &snap).ok();
            }
        }
    }

    /// Every 60s: sample new outbound connections and push them to
    /// PocketBase (queued locally when offline).
    async fn push_network_samples(&mut self) {
        let mut seen_clone = self.net_seen.clone();
        let mut cache_clone = self.dns_cache.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let conns = monitor::network::sample_connections(&mut seen_clone, &mut cache_clone);
            (conns, seen_clone, cache_clone)
        })
        .await;

        let new_conns = match result {
            Ok((conns, updated_seen, updated_cache)) => {
                self.net_seen = updated_seen;
                self.dns_cache = updated_cache;
                conns
            }
            Err(_) => Vec::new(),
        };
        if new_conns.is_empty() {
            return;
        }

        self.app.emit("network-update", &new_conns).ok();

        let Some(sid) = self.session.lock().session_id.clone() else {
            return;
        };
        match self.pb() {
            Some(pb) => {
                for conn in &new_conns {
                    if pb.push_network_connection(&sid, conn).await.is_err() {
                        self.db.lock().queue_network(&sid, &[conn.clone()]).ok();
                    }
                }
            }
            None => {
                self.db.lock().queue_network(&sid, &new_conns).ok();
            }
        }
    }
}

/// Returns true when the user appears to be in an active audio/video conference.
/// Keyboard and mouse are naturally idle during calls, so we must not auto-clock-out.
fn is_conference_active(app: &str, window: &str) -> bool {
    let app_lc = app.to_lowercase();
    let win_lc = window.to_lowercase();

    // App process name covers native clients (Discord, Zoom, Teams, Slack, etc.)
    const CONF_APPS: &[&str] = &[
        "discord", "zoom", "teams", "slack", "webex", "whereby", "skype",
    ];
    if CONF_APPS.iter().any(|a| app_lc.contains(a)) {
        return true;
    }

    // Window title covers browser-embedded meetings (Google Meet, Jitsi, etc.)
    const CONF_TITLES: &[&str] = &[
        "google meet",
        "meet –", // GNOME truncation of "Meet – Google Meet"
        "meet -",
        "zoom meeting",
        "zoom call",
        "teams meeting",
        "slack huddle",
        "webex meeting",
        "jitsi meet",
        "whereby.com",
    ];
    if CONF_TITLES.iter().any(|t| win_lc.contains(t)) {
        return true;
    }

    false
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
            let end_time =
                parse_hhmm(config.auto_end_time.as_deref().unwrap_or("")).unwrap_or(start_time);
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

/// Local-testing-only overrides, read from env vars so the scheduling loop
/// can be exercised in seconds instead of waiting for real clock-in,
/// clock-out, break, or idle thresholds. Only active in debug builds
/// (`npm run tauri dev`) — release builds always use the defaults below and
/// never read these env vars, so this has zero effect for end users.
///
/// - `CLANKER_DEBUG_CLOCK_IN_TIME` / `CLANKER_DEBUG_CLOCK_OUT_TIME` (HH:MM, NPT)
/// - `CLANKER_DEBUG_AUTO_CLOCKOUT=1` forces auto clock-out on
/// - `CLANKER_DEBUG_IDLE_WARNING_SECONDS` / `CLANKER_DEBUG_IDLE_CLOCKOUT_SECONDS`
/// - `CLANKER_DEBUG_CLOCKOUT_GRACE_SECONDS`
/// - `CLANKER_DEBUG_BREAK_AT` (HH:MM, NPT) adds a synthetic auto-start break
#[derive(Debug)]
struct DebugOverrides {
    clock_in_time: Option<String>,
    clock_out_time: Option<String>,
    force_auto_clockout: bool,
    idle_warning_seconds: u64,
    idle_clockout_seconds: u64,
    clockout_grace_seconds: i64,
    break_at: Option<String>,
}

impl DebugOverrides {
    #[cfg(debug_assertions)]
    fn load() -> Self {
        fn env_num<T: std::str::FromStr>(key: &str, default: T) -> T {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        }
        let overrides = DebugOverrides {
            clock_in_time: std::env::var("CLANKER_DEBUG_CLOCK_IN_TIME").ok(),
            clock_out_time: std::env::var("CLANKER_DEBUG_CLOCK_OUT_TIME").ok(),
            force_auto_clockout: std::env::var("CLANKER_DEBUG_AUTO_CLOCKOUT").is_ok(),
            idle_warning_seconds: env_num("CLANKER_DEBUG_IDLE_WARNING_SECONDS", 4 * 60 + 30),
            idle_clockout_seconds: env_num("CLANKER_DEBUG_IDLE_CLOCKOUT_SECONDS", 5 * 60),
            clockout_grace_seconds: env_num("CLANKER_DEBUG_CLOCKOUT_GRACE_SECONDS", 20),
            break_at: std::env::var("CLANKER_DEBUG_BREAK_AT").ok(),
        };
        if overrides.clock_in_time.is_some()
            || overrides.clock_out_time.is_some()
            || overrides.force_auto_clockout
            || overrides.break_at.is_some()
        {
            log::info!("[debug] schedule overrides active: {overrides:?}");
        }
        overrides
    }

    #[cfg(not(debug_assertions))]
    fn load() -> Self {
        DebugOverrides {
            clock_in_time: None,
            clock_out_time: None,
            force_auto_clockout: false,
            idle_warning_seconds: 4 * 60 + 30,
            idle_clockout_seconds: 5 * 60,
            clockout_grace_seconds: 20,
            break_at: None,
        }
    }

    fn apply_to_config(&self, config: &mut AppConfig) {
        if let Some(t) = &self.clock_in_time {
            config.clock_in_time = t.clone();
        }
        if let Some(t) = &self.clock_out_time {
            config.clock_out_time = t.clone();
        }
        if self.force_auto_clockout {
            config.auto_clock_out_enabled = true;
        }
    }

    fn inject_break_config(&self, configs: &mut Vec<BreakConfig>) {
        if let Some(hhmm) = &self.break_at {
            configs.push(BreakConfig {
                id: "debug-break".into(),
                name: "Debug Break".into(),
                type_key: "debug".into(),
                duration_minutes: 1,
                sort_order: 99,
                auto_start_enabled: true,
                auto_start_time: Some(hhmm.clone()),
                auto_end_time: None,
            });
        }
    }
}
