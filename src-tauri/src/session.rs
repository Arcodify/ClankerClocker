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
    pub total_break_seconds: i64,
    pub break_count: u32,
}

impl Default for SessionState {
    fn default() -> Self {
        SessionState {
            status: SessionStatus::Idle,
            session_id: None,
            clock_in: None,
            break_start: None,
            total_break_seconds: 0,
            break_count: 0,
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
}

impl BreakConfig {
    pub fn defaults() -> Vec<Self> {
        vec![
            BreakConfig { id: "1".into(), name: "Short Break".into(), type_key: "short".into(), duration_minutes: 15, sort_order: 0 },
            BreakConfig { id: "2".into(), name: "Lunch".into(), type_key: "lunch".into(), duration_minutes: 30, sort_order: 1 },
            BreakConfig { id: "3".into(), name: "Other".into(), type_key: "other".into(), duration_minutes: 0, sort_order: 2 },
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodayStats {
    pub session_count: u32,
    pub total_work_seconds: i64,
    pub break_count: u32,
    pub total_break_seconds: i64,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub pb_url: String,
    pub pb_email: String,
    pub pb_token: String,
    pub user_id: String,
    pub user_name: String,
    pub user_email: String,
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
            token_saved_at: String::new(),
        }
    }
}
