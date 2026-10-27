use chrono::Utc;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
static DIAGNOSTICS_ENABLED: AtomicBool = AtomicBool::new(false);
static DIAGNOSTICS_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_WRITE_LOCK: Mutex<()> = Mutex::new(());

struct TeeLogger {
    console: env_logger::Logger,
}

impl log::Log for TeeLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        self.console.enabled(metadata) || DIAGNOSTICS_ENABLED.load(Ordering::Relaxed)
    }

    fn log(&self, record: &log::Record<'_>) {
        self.console.log(record);
        if DIAGNOSTICS_ENABLED.load(Ordering::Relaxed) {
            append_line(json!({
                "timestamp": Utc::now().to_rfc3339(),
                "event": "app_log",
                "details": {
                    "level": record.level().to_string(),
                    "target": record.target(),
                    "message": record.args().to_string(),
                }
            }));
        }
    }

    fn flush(&self) {
        self.console.flush();
    }
}

pub fn init_logger() {
    let console = env_logger::Builder::from_default_env().build();
    let _ = log::set_boxed_logger(Box::new(TeeLogger { console }));
    log::set_max_level(log::LevelFilter::Trace);
}

fn append_line(line: Value) {
    let Some(path) = DIAGNOSTICS_PATH.get() else {
        return;
    };
    let Ok(_guard) = LOG_WRITE_LOCK.lock() else {
        return;
    };
    if fs::metadata(path)
        .map(|m| m.len() >= MAX_LOG_BYTES)
        .unwrap_or(false)
    {
        return;
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

pub struct Diagnostics {
    path: PathBuf,
}

impl Diagnostics {
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join("diagnostics.jsonl");
        let _ = DIAGNOSTICS_PATH.set(path.clone());
        Self { path }
    }

    pub fn enabled(&self) -> bool {
        DIAGNOSTICS_ENABLED.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        if enabled {
            // Each opt-in starts a fresh, self-contained capture. Restarting
            // the app also resets diagnostics to OFF.
            if let Ok(_guard) = LOG_WRITE_LOCK.lock() {
                let _ = fs::write(&self.path, "");
            }
            DIAGNOSTICS_ENABLED.store(true, Ordering::Relaxed);
            self.record(
                "diagnostics_enabled",
                json!({
                    "app_version": env!("CARGO_PKG_VERSION"),
                    "os": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                }),
            );
        } else {
            self.record("diagnostics_disabled", json!({}));
            DIAGNOSTICS_ENABLED.store(false, Ordering::Relaxed);
        }
    }

    /// Append one structured event. Never records key names, typed text,
    /// credentials, or network payloads.
    pub fn record(&self, event: &str, details: Value) {
        if !self.enabled() {
            return;
        }
        let line = json!({
            "timestamp": Utc::now().to_rfc3339(),
            "event": event,
            "details": details,
        });
        append_line(line);
    }

    pub fn export_to(&self, destination: &Path) -> Result<(), String> {
        let _guard = LOG_WRITE_LOCK
            .lock()
            .map_err(|_| "Diagnostic log is busy.".to_string())?;
        if !self.path.exists() {
            return Err("No diagnostic events have been recorded yet.".into());
        }
        fs::copy(&self.path, destination)
            .map(|_| ())
            .map_err(|e| format!("Could not save diagnostic log: {e}"))
    }
}
