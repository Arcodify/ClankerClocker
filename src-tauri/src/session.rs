use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Active,
    OnBreak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub status: SessionStatus,
    pub session_id: Option<String>,
    pub clock_in: Option<DateTime<Utc>>,
    pub break_start: Option<DateTime<Utc>>,
    pub break_name: Option<String>,
    pub total_break_seconds: i64,
    pub break_count: u32,
    /// User chose to keep working past the scheduled clock-out time to make
    /// up a time deficit — scheduled auto-clockout must skip this session.
    #[serde(default)]
    pub extended_past_schedule: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        SessionState {
            status: SessionStatus::Idle,
            session_id: None,
            clock_in: None,
            break_start: None,
            break_name: None,
            total_break_seconds: 0,
            break_count: 0,
            extended_past_schedule: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ActivityCounters {
    pub keystrokes: u64,
    pub mouse_clicks: u64,
    pub mouse_distance_px: f64,
    pub last_mouse_x: f64,
    pub last_mouse_y: f64,
    pub last_activity: Option<std::time::Instant>,
}

impl ActivityCounters {
    pub fn idle_seconds(&self) -> u64 {
        match &self.last_activity {
            Some(t) => t.elapsed().as_secs(),
            None => 0,
        }
    }

    pub fn drain(&mut self) -> (u64, u64, f64) {
        let ks = self.keystrokes;
        let mc = self.mouse_clicks;
        let md = self.mouse_distance_px;
        self.keystrokes = 0;
        self.mouse_clicks = 0;
        self.mouse_distance_px = 0.0;
        (ks, mc, md)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivitySnapshot {
    pub timestamp: DateTime<Utc>,
    pub keystrokes: u64,
    pub mouse_clicks: u64,
    pub mouse_distance_px: f64,
    pub active_app: String,
    pub active_window: String,
    pub idle_seconds: u64,
    /// Whether the microphone was actively captured (see monitor::call) —
    /// treated as "working" even though idle_seconds may be climbing.
    #[serde(default)]
    pub in_call: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConnection {
    pub timestamp: DateTime<Utc>,
    pub process_name: String,
    pub remote_host: String,
    pub remote_ip: String,
    pub remote_port: u16,
    pub local_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakConfig {
    pub id: String,
    pub name: String,
    pub type_key: String,
    pub duration_minutes: u32,
    pub sort_order: u32,
    pub auto_start_enabled: bool,
    pub auto_start_time: Option<String>,
    pub auto_end_time: Option<String>,
}

/// Total seconds of scheduled auto-break windows (auto_start → auto_end).
/// Configs without auto-start or with unparsable times contribute nothing.
pub fn scheduled_break_seconds(configs: &[BreakConfig]) -> i64 {
    let parse = |v: &str| chrono::NaiveTime::parse_from_str(v, "%H:%M").ok();
    configs
        .iter()
        .filter(|c| c.auto_start_enabled)
        .filter_map(|c| {
            let start = parse(c.auto_start_time.as_deref()?)?;
            let end = parse(c.auto_end_time.as_deref()?)?;
            Some((end - start).num_seconds().max(0))
        })
        .sum()
}

impl BreakConfig {
    pub fn defaults() -> Vec<Self> {
        vec![
            BreakConfig {
                id: "1".into(),
                name: "Short Break".into(),
                type_key: "short".into(),
                duration_minutes: 15,
                sort_order: 0,
                auto_start_enabled: false,
                auto_start_time: None,
                auto_end_time: None,
            },
            BreakConfig {
                id: "2".into(),
                name: "Lunch".into(),
                type_key: "lunch".into(),
                duration_minutes: 30,
                sort_order: 1,
                auto_start_enabled: false,
                auto_start_time: None,
                auto_end_time: None,
            },
            BreakConfig {
                id: "3".into(),
                name: "Other".into(),
                type_key: "other".into(),
                duration_minutes: 0,
                sort_order: 2,
                auto_start_enabled: false,
                auto_start_time: None,
                auto_end_time: None,
            },
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodayStats {
    pub session_count: u32,
    pub total_work_seconds: i64,
    pub break_count: u32,
    pub total_break_seconds: i64,
    pub total_net_loss_seconds: i64,
    /// Required work seconds for today: schedule span (clock_in_time →
    /// clock_out_time) minus scheduled auto-break windows. 0 for external
    /// staff, who have no fixed schedule.
    #[serde(default)]
    pub required_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodaySessionBreakdown {
    pub session_id: String,
    pub clock_in: DateTime<Utc>,
    pub clock_out: Option<DateTime<Utc>>,
    pub gross_seconds: i64,
    pub break_seconds: i64,
    pub net_seconds: i64,
    pub net_loss_seconds: i64,
    #[serde(default)]
    pub break_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodayBreakdown {
    pub session_count: u32,
    pub total_work_seconds: i64,
    pub break_count: u32,
    pub total_break_seconds: i64,
    pub total_net_loss_seconds: i64,
    pub sessions: Vec<TodaySessionBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub session_id: String,
    pub user_id: String,
    pub user_name: String,
    pub user_email: String,
    pub status: SessionStatus,
    pub clock_in: DateTime<Utc>,
    pub total_break_seconds: i64,
    pub break_count: u32,
    pub active_app: String,
    /// Title of the most recently reported active window.
    #[serde(default)]
    pub active_window_title: String,
    /// Totals across all of this member's sessions today (Nepal time), including the current one.
    pub today_total_work_seconds: i64,
    pub today_total_break_seconds: i64,
    #[serde(default)]
    pub is_external_staff: bool,
    /// Whether this member's mic was active as of the most recent snapshot.
    #[serde(default)]
    pub in_call: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub pb_url: String,
    pub pb_email: String,
    pub pb_token: String,
    pub user_id: String,
    pub user_name: String,
    pub user_email: String,
    pub is_admin: bool,
    /// External staff work outside the company schedule: no scheduled
    /// auto-clockout, no clock-in reminder, and no required daily hours.
    #[serde(default)]
    pub is_external_staff: bool,
    pub clock_in_time: String,
    pub clock_out_time: String,
    pub auto_clock_out_enabled: bool,
    pub token_saved_at: String, // RFC3339; empty means no saved token
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            pb_url: String::new(),
            pb_email: String::new(),
            pb_token: String::new(),
            user_id: String::new(),
            user_name: String::new(),
            user_email: String::new(),
            is_admin: false,
            is_external_staff: false,
            clock_in_time: "09:00".into(),
            clock_out_time: "18:00".into(),
            auto_clock_out_enabled: true,
            token_saved_at: String::new(),
        }
    }
}

impl AppConfig {
    /// Scheduled work seconds per day (clock_in_time → clock_out_time in NPT),
    /// independent of who is currently logged in. 0 when the schedule is
    /// unparsable. Callers must apply any per-employee external-staff
    /// exemption themselves (see `get_today_stats`/`get_time_summary`).
    pub fn required_seconds(&self) -> i64 {
        let parse = |v: &str| chrono::NaiveTime::parse_from_str(v, "%H:%M").ok();
        match (parse(&self.clock_in_time), parse(&self.clock_out_time)) {
            (Some(start), Some(end)) => (end - start).num_seconds().max(0),
            _ => 0,
        }
    }

    /// Required work seconds per day: the schedule span minus scheduled
    /// auto-break windows (breaks don't count toward required hours).
    pub fn required_work_seconds(&self, breaks: &[BreakConfig]) -> i64 {
        (self.required_seconds() - scheduled_break_seconds(breaks)).max(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub name: String,
    pub email: String,
    pub is_admin: bool,
    #[serde(default)]
    pub is_external_staff: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub user_id: String,
    pub user_name: String,
    pub user_email: String,
    pub clock_in: DateTime<Utc>,
    pub clock_out: Option<DateTime<Utc>>,
    pub status: String,
    pub gross_seconds: i64,
    pub break_seconds: i64,
    pub net_seconds: i64,
    pub net_loss_seconds: i64,
    pub break_count: u32,
    /// Reason the employee gave when clocking out before completing their hours.
    #[serde(default)]
    pub early_clockout_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    pub user_id: String,
    pub user_name: String,
    pub user_email: String,
    pub session_count: u32,
    pub days_present: u32,
    pub total_work_seconds: i64,
    pub total_break_seconds: i64,
    pub total_gross_seconds: i64,
    pub total_net_loss_seconds: i64,
    /// Sum over days present of max(0, required work seconds − day's net
    /// work). 0 for external staff, who have no required hours.
    #[serde(default)]
    pub total_time_loss_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStat {
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRecord {
    pub timestamp: DateTime<Utc>,
    pub user_id: String,
    pub user_name: String,
    pub user_email: String,
    pub session_id: String,
    pub process_name: String,
    pub remote_host: String,
    pub remote_ip: String,
    pub remote_port: u16,
    pub local_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkReport {
    pub records: Vec<NetworkRecord>,
    pub top_hosts: Vec<NetworkStat>,
    pub top_processes: Vec<NetworkStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUsage {
    pub app: String,
    pub seconds: i64,
    pub pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityReport {
    pub total_keystrokes: u64,
    pub total_clicks: u64,
    pub idle_pct: f32,
    /// % of snapshots where the mic was active (excluded from idle_pct).
    #[serde(default)]
    pub call_pct: f32,
    pub top_apps: Vec<AppUsage>,
    /// Same shape as top_apps but keyed by window title.
    #[serde(default)]
    pub top_windows: Vec<AppUsage>,
    pub session_count: u32,
    pub total_snapshot_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppNotification {
    pub title: String,
    pub body: String,
    /// Discriminator the frontend uses to pick a notification sound, e.g.
    /// "clock_in_reminder", "idle_clockout_warning", "idle_clockout",
    /// "scheduled_clockout_warning", "scheduled_clockout", "info".
    pub kind: String,
}
