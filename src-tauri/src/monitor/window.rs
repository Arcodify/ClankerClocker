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

    // Wayland/GNOME fallback
    let out = Command::new("gdbus")
        .args([
            "call", "--session",
            "--dest", "org.gnome.Shell",
            "--object-path", "/org/gnome/Shell",
            "--method", "org.gnome.Shell.Eval",
            "global.get_window_actors().filter(w=>w.meta_window.has_focus())[0]?.meta_window.title ?? ''",
        ])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    // gdbus returns: ('Window Title', true,)
    let title = out
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string();

    (String::new(), title)
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
