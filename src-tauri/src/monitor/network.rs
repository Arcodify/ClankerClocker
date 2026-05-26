use std::collections::{HashSet, HashMap};
use std::net::IpAddr;
use std::sync::mpsc;
use std::time::Duration;
use chrono::Utc;
use crate::session::NetworkConnection;

pub fn sample_connections(
    seen: &mut HashSet<String>,
    dns_cache: &mut HashMap<String, String>,
) -> Vec<NetworkConnection> {
    #[cfg(target_os = "linux")]
    return linux_connections(seen, dns_cache);
    #[cfg(target_os = "macos")]
    return macos_connections(seen, dns_cache);
    #[cfg(target_os = "windows")]
    return windows_connections(seen, dns_cache);
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    { let _ = (seen, dns_cache); Vec::new() }
}

// ── Well-known port → service label ─────────────────────────────────────────

fn port_service(port: u16) -> Option<&'static str> {
    match port {
        80   => Some("HTTP"),
        443  => Some("HTTPS"),
        22   => Some("SSH"),
        21   => Some("FTP"),
        25   => Some("SMTP"),
        465  => Some("SMTPS"),
        587  => Some("SMTP"),
        993  => Some("IMAP"),
        995  => Some("POP3"),
        53   => Some("DNS"),
        3306 => Some("MySQL"),
        5432 => Some("PostgreSQL"),
        6379 => Some("Redis"),
        27017 => Some("MongoDB"),
        9200 => Some("Elasticsearch"),
        2375 => Some("Docker"),
        2376 => Some("Docker-TLS"),
        5000 => Some("Registry"),
        8080 => Some("HTTP-Alt"),
        8443 => Some("HTTPS-Alt"),
        3000 => Some("Dev-Server"),
        4000 => Some("Dev-Server"),
        5173 => Some("Vite"),
        1194 => Some("OpenVPN"),
        1337 => Some("Tailscale"),
        41641 => Some("Tailscale"),
        _    => None,
    }
}

// Try to extract a meaningful label from a raw PTR record.
// e.g. "lhr25s12-in-f14.1e100.net" → "google.com"
//      "142.250.80.46" → keep as-is
fn clean_hostname(ptr: &str, ip: &str) -> String {
    if ptr == ip || ptr.is_empty() { return ip.to_string(); }

    // Map well-known CDN hostnames to their parent service
    let lower = ptr.to_lowercase();
    let friendly = if lower.contains("googlevideo") || lower.contains("1e100") || lower.contains("googleusercontent") {
        Some("google.com")
    } else if lower.contains("youtube") {
        Some("youtube.com")
    } else if lower.contains("facebook") || lower.contains("fbcdn") || lower.contains("tfbnw") {
        Some("facebook.com")
    } else if lower.contains("instagram") {
        Some("instagram.com")
    } else if lower.contains("amazonaws") || lower.contains("awsglobalaccelerator") {
        Some("aws.amazon.com")
    } else if lower.contains("cloudfront") {
        Some("cloudfront.net")
    } else if lower.contains("akamai") || lower.contains("akamaitechnologies") || lower.contains("akamaiedge") {
        Some("akamai-cdn")
    } else if lower.contains("cloudflare") {
        Some("cloudflare.com")
    } else if lower.contains("fastly") {
        Some("fastly.com")
    } else if lower.contains("netflix") {
        Some("netflix.com")
    } else if lower.contains("slack") {
        Some("slack.com")
    } else if lower.contains("discord") {
        Some("discord.com")
    } else if lower.contains("github") {
        Some("github.com")
    } else if lower.contains("microsoft") || lower.contains(".msn.") || lower.contains(".live.") {
        Some("microsoft.com")
    } else if lower.contains("apple") || lower.contains("icloud") {
        Some("apple.com")
    } else if lower.contains("twitch") {
        Some("twitch.tv")
    } else if lower.contains("spotify") {
        Some("spotify.com")
    } else if lower.contains("dropbox") {
        Some("dropbox.com")
    } else if lower.contains("docker") {
        Some("docker.io")
    } else {
        None
    };

    if let Some(f) = friendly {
        return f.to_string();
    }

    // Strip generic ISP PTR records (contain the IP digits → useless)
    let ip_compressed: String = ip.chars().filter(|c| c.is_ascii_digit()).collect();
    let ptr_compressed: String = lower.chars().filter(|c| c.is_ascii_digit()).collect();
    if ptr_compressed.len() >= 6 && ptr_compressed.contains(&ip_compressed[..6]) {
        return ip.to_string();
    }

    ptr.to_string()
}

// ── Parallel DNS with timeout ────────────────────────────────────────────────

fn resolve_batch(ips: &[String], cache: &mut HashMap<String, String>) {
    let uncached: Vec<String> = ips.iter()
        .filter(|ip| !cache.contains_key(*ip))
        .cloned()
        .collect();
    if uncached.is_empty() { return; }

    let handles: Vec<(String, mpsc::Receiver<String>)> = uncached.into_iter().map(|ip| {
        let (tx, rx) = mpsc::channel();
        let ip2 = ip.clone();
        std::thread::spawn(move || {
            let result = if let Ok(addr) = ip2.parse::<IpAddr>() {
                dns_lookup::lookup_addr(&addr).unwrap_or_default()
            } else {
                String::new()
            };
            let _ = tx.send(result);
        });
        (ip, rx)
    }).collect();

    // Collect all results with a shared deadline of 800ms
    let deadline = std::time::Instant::now() + Duration::from_millis(800);
    for (ip, rx) in handles {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let raw = if remaining.is_zero() {
            String::new()
        } else {
            rx.recv_timeout(remaining).unwrap_or_default()
        };
        let cleaned = clean_hostname(&raw, &ip);
        cache.insert(ip, cleaned);
    }
}

// ── Linux ────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn linux_connections(seen: &mut HashSet<String>, dns_cache: &mut HashMap<String, String>) -> Vec<NetworkConnection> {
    use std::process::Command;
    let out = Command::new("ss")
        .args(["-tnp", "--no-header"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    parse_ss_output(&out, seen, dns_cache)
}

#[cfg(target_os = "linux")]
fn parse_ss_output(output: &str, seen: &mut HashSet<String>, dns_cache: &mut HashMap<String, String>) -> Vec<NetworkConnection> {
    let now = Utc::now();
    let mut raw_entries: Vec<(String, u16, u16, String)> = Vec::new(); // (remote_ip, remote_port, local_port, process_name)

    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 || parts[0] != "ESTAB" { continue; }

        let (remote_ip, remote_port) = parse_addr(parts[4]);
        let (_, local_port) = parse_addr(parts[3]);
        if remote_ip.is_empty() || is_private_or_loopback(&remote_ip) { continue; }

        let key = format!("{}:{}-{}", remote_ip, remote_port, local_port);
        if seen.contains(&key) { continue; }
        seen.insert(key);

        let proc_info = parts.get(5).copied().unwrap_or("");
        let process_name = extract_process_linux(proc_info);
        raw_entries.push((remote_ip, remote_port, local_port, process_name));
    }

    // Resolve all new IPs in parallel
    let ips: Vec<String> = raw_entries.iter().map(|(ip, ..)| ip.clone()).collect();
    resolve_batch(&ips, dns_cache);

    raw_entries.into_iter().map(|(remote_ip, remote_port, local_port, process_name)| {
        let host = dns_cache.get(&remote_ip).cloned().unwrap_or_else(|| remote_ip.clone());
        let host = annotate_with_service(host, remote_port);
        NetworkConnection { timestamp: now, process_name, remote_host: host, remote_ip, remote_port, local_port }
    }).collect()
}

#[cfg(target_os = "linux")]
fn extract_process_linux(proc_info: &str) -> String {
    if let Some(start) = proc_info.find('"') {
        let rest = &proc_info[start + 1..];
        if let Some(end) = rest.find('"') { return rest[..end].to_string(); }
    }
    String::new()
}

// ── macOS ────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn macos_connections(seen: &mut HashSet<String>, dns_cache: &mut HashMap<String, String>) -> Vec<NetworkConnection> {
    use std::process::Command;
    let out = Command::new("lsof")
        .args(["-i", "TCP", "-n", "-P", "-s", "TCP:ESTABLISHED"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let now = Utc::now();
    let mut raw: Vec<(String, u16, u16, String)> = Vec::new();
    for line in out.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 { continue; }
        let process_name = parts[0].to_string();
        if let Some((local, remote)) = parts[8].split_once("->") {
            let (remote_ip, remote_port) = parse_addr(remote);
            let (_, local_port) = parse_addr(local);
            if remote_ip.is_empty() || is_private_or_loopback(&remote_ip) { continue; }
            let key = format!("{}:{}-{}", remote_ip, remote_port, local_port);
            if seen.contains(&key) { continue; }
            seen.insert(key);
            raw.push((remote_ip, remote_port, local_port, process_name));
        }
    }
    let ips: Vec<String> = raw.iter().map(|(ip, ..)| ip.clone()).collect();
    resolve_batch(&ips, dns_cache);
    raw.into_iter().map(|(remote_ip, remote_port, local_port, process_name)| {
        let host = dns_cache.get(&remote_ip).cloned().unwrap_or_else(|| remote_ip.clone());
        let host = annotate_with_service(host, remote_port);
        NetworkConnection { timestamp: now, process_name, remote_host: host, remote_ip, remote_port, local_port }
    }).collect()
}

// ── Windows ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn windows_connections(seen: &mut HashSet<String>, dns_cache: &mut HashMap<String, String>) -> Vec<NetworkConnection> {
    use std::process::Command;
    let out = Command::new("netstat").args(["-ano"])
        .output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default();
    let now = Utc::now();
    let mut raw: Vec<(String, u16, u16, String)> = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 || parts[0] != "TCP" || parts[3] != "ESTABLISHED" { continue; }
        let (remote_ip, remote_port) = parse_addr(parts[2]);
        let (_, local_port) = parse_addr(parts[1]);
        if remote_ip.is_empty() || is_private_or_loopback(&remote_ip) { continue; }
        let key = format!("{}:{}-{}", remote_ip, remote_port, local_port);
        if seen.contains(&key) { continue; }
        seen.insert(key);
        let pid: u32 = parts[4].parse().unwrap_or(0);
        let process_name = get_process_name_windows(pid);
        raw.push((remote_ip, remote_port, local_port, process_name));
    }
    let ips: Vec<String> = raw.iter().map(|(ip, ..)| ip.clone()).collect();
    resolve_batch(&ips, dns_cache);
    raw.into_iter().map(|(remote_ip, remote_port, local_port, process_name)| {
        let host = dns_cache.get(&remote_ip).cloned().unwrap_or_else(|| remote_ip.clone());
        let host = annotate_with_service(host, remote_port);
        NetworkConnection { timestamp: now, process_name, remote_host: host, remote_ip, remote_port, local_port }
    }).collect()
}

#[cfg(target_os = "windows")]
fn get_process_name_windows(pid: u32) -> String {
    use std::process::Command;
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
        .output().ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().and_then(|l| l.split(',').next()).map(|s| s.trim_matches('"').to_string()))
        .unwrap_or_default()
}

// ── Shared helpers ───────────────────────────────────────────────────────────

fn parse_addr(addr: &str) -> (String, u16) {
    if let Some(bracket_end) = addr.rfind(']') {
        let ip = addr[1..bracket_end].to_string();
        let port = addr[bracket_end + 2..].parse().unwrap_or(0);
        return (ip, port);
    }
    if let Some(colon) = addr.rfind(':') {
        let ip = addr[..colon].to_string();
        let port = addr[colon + 1..].parse().unwrap_or(0);
        return (ip, port);
    }
    (addr.to_string(), 0)
}

fn is_private_or_loopback(ip: &str) -> bool {
    if ip.starts_with("127.") || ip == "::1" { return true; }
    if ip.starts_with("169.254.") { return true; }
    // Skip RFC-1918 private ranges
    if ip.starts_with("10.") { return true; }
    if ip.starts_with("192.168.") { return true; }
    if let Some(second) = ip.split('.').nth(1) {
        if ip.starts_with("172.") {
            if let Ok(n) = second.parse::<u8>() {
                if (16..=31).contains(&n) { return true; }
            }
        }
    }
    false
}

fn annotate_with_service(host: String, port: u16) -> String {
    if let Some(svc) = port_service(port) {
        // Don't double-annotate if already a well-known service name
        if !host.contains('(') {
            return format!("{} ({})", host, svc);
        }
    }
    host
}
