use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use parking_lot::Mutex;
use crate::session::ActivityCounters;

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
                                    InputEventKind::Key(key) if ev.value() == 1 => {
                                        if (0x110u16..=0x117).contains(&key.code()) {
                                            mc += 1;
                                        } else {
                                            ks += 1;
                                        }
                                    }
                                    InputEventKind::RelAxis(RelativeAxisType::REL_X) => dx += ev.value() as f64,
                                    InputEventKind::RelAxis(RelativeAxisType::REL_Y) => dy += ev.value() as f64,
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
    if spawned == 0 {
        active_flag.store(false, Ordering::Relaxed);
    }
}

// ── macOS / Windows: rdev (uses platform APIs) ───────────────────────────────

#[cfg(not(target_os = "linux"))]
fn other_start(counters: Arc<Mutex<ActivityCounters>>, active_flag: Arc<AtomicBool>) {
    use rdev::{listen, Event, EventType};

    std::thread::Builder::new()
        .name("input-monitor".into())
        .spawn(move || {
            let flag = active_flag.clone();
            if let Err(e) = listen(move |event: Event| {
                flag.store(true, Ordering::Relaxed);
                let mut c = counters.lock();
                c.last_activity = Some(std::time::Instant::now());
                match event.event_type {
                    EventType::KeyPress(_) => c.keystrokes += 1,
                    EventType::ButtonPress(_) => c.mouse_clicks += 1,
                    EventType::MouseMove { x, y } => {
                        let dx = x - c.last_mouse_x;
                        let dy = y - c.last_mouse_y;
                        c.mouse_distance_px += (dx * dx + dy * dy).sqrt();
                        c.last_mouse_x = x;
                        c.last_mouse_y = y;
                    }
                    _ => {}
                }
            }) {
                active_flag.store(false, Ordering::Relaxed);
                log::warn!("Input monitor failed: {:?}", e);
                log::warn!("On macOS: grant Accessibility in System Settings → Privacy → Accessibility.");
                log::warn!("On Windows: run as administrator if hooks are blocked.");
            }
        })
        .ok();
}
