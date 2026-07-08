use crate::session::ActivityCounters;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub fn start(counters: Arc<Mutex<ActivityCounters>>, active_flag: Arc<AtomicBool>) {
    #[cfg(target_os = "linux")]
    linux_start(counters, active_flag);

    #[cfg(not(target_os = "linux"))]
    other_start(counters, active_flag);
}

// ── Linux: evdev (raw kernel events, works on both X11 and Wayland) ──────────

#[cfg(target_os = "linux")]
fn linux_start(counters: Arc<Mutex<ActivityCounters>>, active_flag: Arc<AtomicBool>) {
    use evdev::{Device, EventType, InputEventKind, RelativeAxisType};

    let devices: Vec<(_, Device)> = evdev::enumerate().collect();
    let mut spawned = 0u32;

    for (_path, device) in devices {
        let caps = device.supported_events();
        if !caps.contains(EventType::KEY) && !caps.contains(EventType::RELATIVE) {
            continue;
        }

        let counters_c = counters.clone();
        let flag_c = active_flag.clone();

        std::thread::Builder::new()
            .name("input-dev".into())
            .spawn(move || {
                let mut dev = device;
                loop {
                    match dev.fetch_events() {
                        Ok(events) => {
                            let mut ks = 0u64;
                            let mut mc = 0u64;
                            let mut dx = 0.0f64;
                            let mut dy = 0.0f64;
                            let mut got = false;

                            for ev in events {
                                got = true;
                                match ev.kind() {
                                    // FIX: classify keys by their own key code only.
                                    // Some keyboards also report a RELATIVE axis, which
                                    // previously caused all their keypresses to be
                                    // miscounted as mouse clicks. Click detection itself
                                    // is unchanged.
                                    InputEventKind::Key(key) if ev.value() == 1 => {
                                        if is_mouse_button_code(key.code()) {
                                            mc += 1;
                                        } else {
                                            ks += 1;
                                        }
                                    }
                                    InputEventKind::RelAxis(RelativeAxisType::REL_X) => {
                                        dx += ev.value() as f64
                                    }
                                    InputEventKind::RelAxis(RelativeAxisType::REL_Y) => {
                                        dy += ev.value() as f64
                                    }
                                    _ => {}
                                }
                            }

                            if got {
                                let mut c = counters_c.lock();
                                c.last_activity = Some(std::time::Instant::now());
                                c.keystrokes += ks;
                                c.mouse_clicks += mc;
                                if dx != 0.0 || dy != 0.0 {
                                    c.mouse_distance_px += (dx * dx + dy * dy).sqrt();
                                }
                                flag_c.store(true, Ordering::Relaxed);
                            }
                        }
                        Err(e) => {
                            log::warn!("input device error: {:?}", e);
                            break;
                        }
                    }
                }
            })
            .ok();
        spawned += 1;
    }

    eprintln!("[input-monitor] monitoring {} devices", spawned);

    // --- FIX: evdev permission failure on Ubuntu ---
    // On Ubuntu, /dev/input/* requires the user to be in the `input` group (not default).
    // If no devices opened, log a clear fix instead of failing silently.
    if spawned > 0 {
        active_flag.store(true, Ordering::Relaxed);
    } else {
        log::warn!(
            "[input-monitor] No /dev/input devices readable. \
             Fix: sudo usermod -aG input $USER  then log out and back in."
        );
        active_flag.store(false, Ordering::Relaxed);
    }
}

#[cfg(target_os = "linux")]
fn is_mouse_button_code(code: u16) -> bool {
    // BTN_MOUSE through BTN_TASK cover standard mouse buttons and extras.
    (0x110u16..=0x117).contains(&code)
}

// ── macOS / Windows: rdev (uses platform APIs) ───────────────────────────────
// Unchanged.

#[cfg(not(target_os = "linux"))]
fn other_start(counters: Arc<Mutex<ActivityCounters>>, active_flag: Arc<AtomicBool>) {
    use rdev::{listen, Event, EventType};

    std::thread::Builder::new()
        .name("input-monitor".into())
        .spawn(move || {
            loop {
                let counters_ref = counters.clone();
                let active_flag_ref = active_flag.clone();

                if let Err(e) = listen(move |event: Event| {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut c = counters_ref.lock();
                        c.last_activity = Some(std::time::Instant::now());
                        match event.event_type {
                            EventType::KeyPress(_) => {
                                c.keystrokes += 1;
                                active_flag_ref.store(true, Ordering::Relaxed);
                            }
                            EventType::ButtonPress(_) => {
                                c.mouse_clicks += 1;
                                active_flag_ref.store(true, Ordering::Relaxed);
                            }
                            EventType::MouseMove { x, y } => {
                                if x.is_finite() && y.is_finite() {
                                    if c.last_mouse_x != 0.0 || c.last_mouse_y != 0.0 {
                                        let dx = x - c.last_mouse_x;
                                        let dy = y - c.last_mouse_y;
                                        let dist = (dx * dx + dy * dy).sqrt();
                                        if dist.is_finite() && dist < 5000.0 {
                                            c.mouse_distance_px += dist;
                                        }
                                    }
                                    c.last_mouse_x = x;
                                    c.last_mouse_y = y;
                                    active_flag_ref.store(true, Ordering::Relaxed);
                                }
                            }
                            _ => {}
                        }
                    }));
                }) {
                    active_flag.store(false, Ordering::Relaxed);
                    log::warn!("Input monitor failed: {:?}", e);
                    #[cfg(target_os = "macos")]
                    log::warn!("Grant Accessibility in System Settings -> Privacy & Security -> Accessibility.");
                    #[cfg(target_os = "windows")]
                    log::warn!("Run as administrator and check whether antivirus is blocking the tray icon or input hooks.");
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    continue;
                }

                break;
            }
        })
        .ok();
}