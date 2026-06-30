/// Returns (app_name, window_title) of the currently focused window.
pub fn get_active_window() -> (String, String) {
    #[cfg(target_os = "linux")]
    return linux_active_window();

    #[cfg(target_os = "macos")]
    return macos_active_window();

    #[cfg(target_os = "windows")]
    return windows_active_window();

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    (String::new(), String::new())
}

#[cfg(target_os = "linux")]
pub fn start_hyprland_active_window_cache(
    cache: std::sync::Arc<parking_lot::Mutex<(String, String)>>,
) {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    let runtime_dir = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(v) => v,
        Err(_) => return,
    };
    let instance = match std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
        Ok(v) => v,
        Err(_) => return,
    };

    let mut socket_path = PathBuf::from(runtime_dir);
    socket_path.push("hypr");
    socket_path.push(instance);
    socket_path.push(".socket2.sock");

    std::thread::Builder::new()
        .name("hypr-activewindow".into())
        .spawn(move || {
            loop {
                match UnixStream::connect(&socket_path) {
                    Ok(stream) => {
                        let reader = BufReader::new(stream);
                        for line in reader.lines().flatten() {
                            if let Some(data) = line.strip_prefix("activewindow>>") {
                                if let Some((app, title)) = data.split_once(',') {
                                    *cache.lock() =
                                        (app.trim().to_string(), title.trim().to_string());
                                }
                            } else if line.starts_with("activewindowv2>>") {
                                if let Ok((app, title)) = fetch_hyprland_active_window() {
                                    *cache.lock() = (app, title);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("Hyprland activewindow socket failed: {:?}", e);
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                }
            }
        })
        .ok();
}

#[cfg(target_os = "linux")]
fn fetch_hyprland_active_window() -> Result<(String, String), String> {
    use std::process::Command;

    let out = Command::new("hyprctl")
        .args(["-j", "activewindow"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8(out.stderr).unwrap_or_else(|_| "hyprctl failed".into()));
    }
    let out = String::from_utf8(out.stdout).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    let app = hyprland_window_app(&json, &json);
    let title = hyprland_window_title(&json, &json);
    Ok((app, title))
}

#[cfg(target_os = "linux")]
fn linux_active_window() -> (String, String) {
    use std::process::Command;

    // X11 path via xdotool
    let wid = Command::new("xdotool")
        .arg("getactivewindow")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(wid) = wid {
        let title = Command::new("xdotool")
            .args(["getwindowname", &wid])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let app = Command::new("xdotool")
            .args(["getwindowpid", &wid])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u32>().ok())
            .and_then(|pid| std::fs::read_to_string(format!("/proc/{}/comm", pid)).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        return (app, title);
    }

    // Hyprland fallback
    if let Some((app, title)) = hyprland_active_window() {
        return normalize_window(app, title);
    }

    // Sway fallback
    if let Some((app, title)) = sway_active_window() {
        return normalize_window(app, title);
    }

    // GNOME fallback
    if let Some((app, title)) = gnome_active_window() {
        return normalize_window(app, title);
    }

    (String::new(), String::new())
}

#[cfg(target_os = "linux")]
fn normalize_window(app: String, title: String) -> (String, String) {
    let app = app.trim().to_string();
    let title = title.trim().to_string();
    if app.is_empty() && !title.is_empty() {
        (title.clone(), title)
    } else {
        (app, title)
    }
}

#[cfg(target_os = "linux")]
fn hyprland_active_window() -> Option<(String, String)> {
    if let Some(result) = hyprland_active_window_json(&["activewindow", "-j"]) {
        return Some(result);
    }

    hyprland_focused_client()
}

#[cfg(target_os = "linux")]
fn hyprland_active_window_json(args: &[&str]) -> Option<(String, String)> {
    use std::process::Command;

    let out = Command::new("hyprctl")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())?;

    let json: serde_json::Value = serde_json::from_str(&out).ok()?;
    let app = hyprland_window_app(&json, &json);
    let title = hyprland_window_title(&json, &json);

    if app.is_empty() && title.is_empty() {
        None
    } else {
        Some((app, title))
    }
}

#[cfg(target_os = "linux")]
fn hyprland_focused_client() -> Option<(String, String)> {
    use std::process::Command;

    let out = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())?;

    let json: serde_json::Value = serde_json::from_str(&out).ok()?;
    let focused = find_hyprland_focused_client(&json)?;
    let app = hyprland_window_app(focused, &json);
    let title = hyprland_window_title(focused, &json);

    if app.is_empty() && title.is_empty() {
        None
    } else {
        Some((app, title))
    }
}

#[cfg(target_os = "linux")]
fn hyprland_window_app<'a>(win: &'a serde_json::Value, root: &'a serde_json::Value) -> String {
    let pid = win.get("pid").and_then(|v| v.as_u64());
    let address = win.get("address").and_then(|v| v.as_str());

    let from_window = win
        .get("class")
        .and_then(|v| v.as_str())
        .or_else(|| win.get("initialClass").and_then(|v| v.as_str()))
        .or_else(|| win.get("app_id").and_then(|v| v.as_str()))
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string());

    if let Some(app) = from_window {
        return app;
    }

    if let Some(addr) = address {
        if let Some(client) = find_hyprland_client_by_address(root, addr) {
            return hyprland_window_app(client, root);
        }
    }

    pid.and_then(process_name_from_pid).unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn hyprland_window_title<'a>(win: &'a serde_json::Value, root: &'a serde_json::Value) -> String {
    let from_window = win
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| win.get("initialTitle").and_then(|v| v.as_str()))
        .or_else(|| win.get("name").and_then(|v| v.as_str()))
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string());

    if let Some(title) = from_window {
        return title;
    }

    if let Some(addr) = win.get("address").and_then(|v| v.as_str()) {
        if let Some(client) = find_hyprland_client_by_address(root, addr) {
            return hyprland_window_title(client, root);
        }
    }

    String::new()
}

#[cfg(target_os = "linux")]
fn find_hyprland_client_by_address<'a>(
    node: &'a serde_json::Value,
    address: &str,
) -> Option<&'a serde_json::Value> {
    if let Some(items) = node.as_array() {
        for item in items {
            if let Some(found) = find_hyprland_client_by_address(item, address) {
                return Some(found);
            }
        }
        return None;
    }

    if node.get("address").and_then(|v| v.as_str()) == Some(address) {
        return Some(node);
    }

    for key in ["clients", "nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(|v| v.as_array()) {
            for child in children {
                if let Some(found) = find_hyprland_client_by_address(child, address) {
                    return Some(found);
                }
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn process_name_from_pid(pid: u64) -> Option<String> {
    let path = format!("/proc/{pid}/comm");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "linux")]
fn find_hyprland_focused_client(node: &serde_json::Value) -> Option<&serde_json::Value> {
    if let Some(items) = node.as_array() {
        for item in items {
            let is_focused = item.get("focused").and_then(|v| v.as_bool()).unwrap_or(false)
                || item.get("focusHistoryID").and_then(|v| v.as_i64()) == Some(0);
            if is_focused {
                return Some(item);
            }
            if let Some(found) = find_hyprland_focused_client(item) {
                return Some(found);
            }
        }
        return None;
    }

    if node.get("focused").and_then(|v| v.as_bool()).unwrap_or(false)
        || node.get("focusHistoryID").and_then(|v| v.as_i64()) == Some(0)
    {
        return Some(node);
    }

    for key in ["clients", "nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(|v| v.as_array()) {
            for child in children {
                if let Some(found) = find_hyprland_focused_client(child) {
                    return Some(found);
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn sway_active_window() -> Option<(String, String)> {
    use std::process::Command;

    let out = Command::new("swaymsg")
        .args(["-t", "get_tree"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())?;

    let json: serde_json::Value = serde_json::from_str(&out).ok()?;

    fn find_focused(node: &serde_json::Value) -> Option<&serde_json::Value> {
        if node.get("focused").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Some(node);
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = node.get(key).and_then(|v| v.as_array()) {
                for child in children {
                    if let Some(found) = find_focused(child) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    let node = find_focused(&json)?;
    let app = node
        .get("app_id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            node.get("window_properties")
                .and_then(|w| w.get("class"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_string();
    let title = node
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some((app, title))
}

#[cfg(target_os = "linux")]
fn gnome_active_window() -> Option<(String, String)> {
    // Try Eval first (works on GNOME ≤40). On Ubuntu 21.10+ Eval is blocked,
    // so if it returns None we fall through to the xprop-based path.
    gnome_active_window_eval().or_else(gnome_active_window_xprop)
}

// --- FIX 1: gnome_active_window_eval ---
// Ubuntu 21.10+ (GNOME 41+) disables org.gnome.Shell.Eval for security.
// When blocked, gdbus still exits 0 but returns ('false', false,).
// The old code never checked the success boolean, so "false" leaked into the UI.
// Now we split on the LAST comma to get the boolean, and reject if it's not "true".
#[cfg(target_os = "linux")]
fn gnome_active_window_eval() -> Option<(String, String)> {
    use std::process::Command;

    let script = r#"(() => {
        const w = global.display.focus_window;
        if (!w) return '|';
        const app = (typeof w.get_wm_class === 'function' ? w.get_wm_class() : '') || w.wm_class || '';
        const title = w.title || '';
        return `${app}|${title}`;
    })()"#;

    let out = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Shell",
            "--object-path",
            "/org/gnome/Shell",
            "--method",
            "org.gnome.Shell.Eval",
            script,
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())?;

    // gdbus returns: ('result', success_bool,)
    // Split on the LAST comma so window titles containing commas don't break parsing.
    let trimmed = out.trim().trim_start_matches('(').trim_end_matches(')');
    let last_comma = trimmed.rfind(',')?;
    let success_str = trimmed[last_comma + 1..].trim();
    let result_str = trimmed[..last_comma].trim().trim_matches('\'').trim_matches('"');

    // Eval was blocked — don't use this result.
    if success_str != "true" {
        return None;
    }

    if result_str == "|" || result_str.is_empty() {
        return None;
    }

    let (app, title) = result_str
        .split_once('|')
        .map(|(a, t)| (a.trim().to_string(), t.trim().to_string()))
        .unwrap_or_else(|| (result_str.trim().to_string(), String::new()));

    if app.is_empty() && title.is_empty() { None } else { Some((app, title)) }
}

// --- FIX 2: gnome_active_window_xprop ---
// Ubuntu GNOME 41+ fallback. xprop reads _NET_ACTIVE_WINDOW from the X11 root,
// which works on both X11 and Ubuntu's Wayland session (via XWayland).
// No extra dependencies needed — xprop ships with Ubuntu by default (x11-utils).
//
// NOTE: changed signature from `fn gnome_active_window_xprop(_: ())` to take
// zero arguments, since `Option::or_else` requires `FnOnce() -> Option<T>`
// (zero parameters), not a function taking a `()` tuple as its single argument.
#[cfg(target_os = "linux")]
fn gnome_active_window_xprop() -> Option<(String, String)> {
    use std::process::Command;

    // Get the focused window ID from the root window property.
    let root_out = Command::new("xprop")
        .args(["-root", "_NET_ACTIVE_WINDOW"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())?;

    // Output: _NET_ACTIVE_WINDOW(WINDOW): window id # 0x3e00006
    let win_id = root_out.split_whitespace().last()?.trim().to_string();
    if win_id == "0x0" || win_id.is_empty() {
        return None;
    }

    // WM_CLASS gives us the app name (second token is the class, e.g. "Firefox").
    let wm_class = Command::new("xprop")
        .args(["-id", &win_id, "WM_CLASS"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let app = wm_class
        .split('=').nth(1).unwrap_or("")
        .split(',').nth(1)
        .unwrap_or_else(|| wm_class.split('=').nth(1).unwrap_or(""))
        .trim().trim_matches('"').trim().to_string();

    // _NET_WM_NAME gives us the window title.
    let wm_name = Command::new("xprop")
        .args(["-id", &win_id, "_NET_WM_NAME"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let title = wm_name
        .split('=').nth(1).unwrap_or("")
        .trim().trim_matches('"').trim().to_string();

    if app.is_empty() && title.is_empty() {
        return None;
    }

    // Prefer the process name from /proc/<pid>/comm for consistency with the xdotool path.
    let pid_out = Command::new("xprop")
        .args(["-id", &win_id, "_NET_WM_PID"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let proc_name = pid_out
        .split('=').nth(1)
        .and_then(|s| s.trim().parse::<u64>().ok())
        .and_then(process_name_from_pid)
        .unwrap_or_default();

    let resolved_app = if !proc_name.is_empty() { proc_name }
                       else if !app.is_empty()   { app }
                       else                       { title.clone() };

    Some((resolved_app, title))
}

#[cfg(target_os = "macos")]
fn macos_active_window() -> (String, String) {
    use std::process::Command;

    let script = r#"tell application "System Events"
        set fa to first application process whose frontmost is true
        set appName to name of fa
        try
            set winTitle to name of first window of fa
        on error
            set winTitle to ""
        end try
    end tell
    return appName & "|" & winTitle"#;

    let out = Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let out = out.trim();
    if let Some((app, win)) = out.split_once('|') {
        (app.trim().to_string(), win.trim().to_string())
    } else {
        (out.to_string(), String::new())
    }
}

#[cfg(target_os = "windows")]
fn windows_active_window() -> (String, String) {
    use std::path::Path;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return (String::new(), String::new());
        }

        let title_len = GetWindowTextLengthW(hwnd);
        let mut title_buf = vec![0u16; title_len.saturating_add(1) as usize];
        if !title_buf.is_empty() {
            GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
        }
        let title = String::from_utf16_lossy(&title_buf)
            .trim_matches(char::from(0))
            .trim()
            .to_string();

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return (String::new(), title);
        }

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return (String::new(), title);
        }

        let mut name_buf = vec![0u16; 260];
        let mut len = name_buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, name_buf.as_mut_ptr(), &mut len);
        CloseHandle(process);

        let app = if ok != 0 && len > 0 {
            let raw = String::from_utf16_lossy(&name_buf[..len as usize]);
            Path::new(&raw)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(raw.as_str())
                .to_string()
        } else {
            String::new()
        };

        (app, title)
    }
}