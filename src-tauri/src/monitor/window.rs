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
    use std::process::Command;

    // PowerShell: get foreground window via P/Invoke, return "AppName|WindowTitle"
    let script = r#"
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public class WinFocus {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
}
"@
$hwnd = [WinFocus]::GetForegroundWindow()
$sb = New-Object System.Text.StringBuilder 256
[WinFocus]::GetWindowText($hwnd, $sb, 256) | Out-Null
$pid = 0
[WinFocus]::GetWindowThreadProcessId($hwnd, [ref]$pid) | Out-Null
$proc = Get-Process -Id $pid -ErrorAction SilentlyContinue
"$($proc.Name)|$($sb.ToString())"
"#;

    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let out = out.trim();
    if let Some((app, win)) = out.split_once('|') {
        (app.trim().to_string(), win.trim().to_string())
    } else {
        (String::new(), out.to_string())
    }
}
