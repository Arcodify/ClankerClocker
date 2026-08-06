# ClankerClocker

Employee time & activity tracker for the Arcodify team — a focused desktop tool for attendance tracking, activity summaries, and reminder workflows.

Built with Tauri 2, Svelte 5, and Rust, syncing to a PocketBase backend.

## What it records

- Session/break timing for attendance logs (clock in/out, work schedule)
- Counts of keystrokes, mouse clicks, and mouse movement (not keystroke text/content)
- Active application + window title
- Network connection metadata (process name, host, IP, port)

It does **not** capture screenshots or typed text content. Idle detection triggers an auto clock-out warning, which is paused automatically when a meeting app (Zoom, Teams, Slack, Webex, Google Meet) is detected active.

Snapshot cadence is currently hardcoded: 5s live emission, 30s snapshot.

## Settings

Configured from the in-app Settings page:

- **PocketBase URL** — backend server address
- **Email / Password** — PocketBase login, sign-out available
- **Company Work Schedule** (admin only) — Clock In Time, Clock Out Time, and an "auto clock out after clock-out time" toggle

There is currently no UI for tracking interval or a monitored-apps list — those are hardcoded.

## Development

```bash
npm install
npm run dev          # Vite dev server (frontend only)
npm run tauri dev    # full Tauri app in dev mode
```

## Building

```bash
npm run build              # frontend build
npm run tauri build        # full platform build
npm run bundle:appimage    # Linux AppImage bundle only
```

### Linux prerequisites

```bash
sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev \
  patchelf libssl-dev libasound2-dev pkg-config
```

Also install **`pipewire-alsa`** and **`libasound2-plugins`** on the *runtime* machine (not just build machine) — see [Known Issues](#known-issues-linux) below.

Input tracking on Linux uses `evdev` on raw kernel input, with Hyprland compositor event-stream detection for active window (falls back to X11 otherwise).

### macOS prerequisites

- Xcode Command Line Tools
- Grant the built app **Accessibility** and **Input Monitoring** permissions (System Settings → Privacy & Security) — required for input tracking to work at all
- Input tracking uses a vendored, patched fork of `rdev` (`src-tauri/vendor/rdev-0.5.3`) that fixes a macOS `CGEventTap` auto-disable bug and adds stronger locking to avoid dropped input under contention

For unsigned local distribution (no Apple Developer signing):

```bash
npm run tauri build
npm run install:macos:unsigned   # copies to ~/Applications, strips quarantine, launches
```

See `docs/macos-unsigned-release.md` for details.

If a built/installed app won't launch, or Accessibility/Automation
permissions don't stick between launches, run the `clanker-fix` command
(auto-installed to `~/.local/bin` the first time the app launches — see
`docs/macos-troubleshooting.md`) or `npm run fix:macos` from a checkout.

## Known Issues (Linux)

### Audio device locking on Kali Linux (and other minimal Debian-based installs)

**Symptom**: ClankerClocker's notification sound locks the system audio device, preventing other applications from using sound output, or the app itself produces no sound.

**Cause**: On a minimal install, ALSA's `default` device can resolve directly to raw hardware (e.g. `hw:CARD=PCH`) instead of routing through PipeWire, even when PipeWire/WirePlumber services are active — because the `pipewire-alsa` ALSA plugin isn't installed. Verify with:

```bash
aplay -L | grep -A2 '^default'   # if this shows CARD=... directly, PipeWire isn't in the path
pactl info                        # shows PipeWire's actual default sink for comparison
```

The app's audio code (`src-tauri/src/audio.rs`) deliberately opens the ALSA "default" device rather than enumerating raw hardware, specifically to avoid this class of problem — but it can't route around a missing system ALSA plugin.

**Fix**:

```bash
sudo apt install pipewire-alsa libasound2-plugins
```

Then reboot. After that, ALSA's `default` routes through PipeWire correctly and the app no longer locks the device.

## License

Copyright (c) 2026 Arcodify. All rights reserved.
