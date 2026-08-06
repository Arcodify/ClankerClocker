# Changelog

All notable changes to ClankerClocker are documented here. Format loosely
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [1.1.9] - 2026-08-06

### Fixed

- **Double-counted work/break time on the dashboard.** The personal
  dashboard's "today" totals could include the currently-open session's
  hours twice — once from the backend aggregate (which already includes the
  live session up to the moment it's queried) and once from the frontend's
  own live-ticking timer for that same session. This was most visible right
  after an auto-update relaunched the app mid-shift: total work time and
  break time would jump up by roughly however long the session had already
  run (e.g. +3h worked, +30m break), and could make "time loss today"
  falsely read as satisfied. `get_today_stats` now excludes the open
  session's own contribution from the aggregate it returns, so the
  frontend's "backend total (closed sessions) + live delta" math is
  actually correct instead of assumed.
- **Automation permission never prompted on macOS.** `Info.plist` was
  missing `NSAppleEventsUsageDescription`; without it, the AppleScript call
  used to read the active app/window title was silently denied by macOS
  instead of surfacing an Automation permission request.

### Added

- **Mic-based call detection**, replacing the old hardcoded app/window-name
  matching (Zoom, Teams, Slack, Discord, Meet, ...) used to suppress idle
  clock-out warnings during meetings. The new check asks the OS directly
  whether the microphone is being captured (CoreAudio on macOS, `pactl` on
  Linux, the CapabilityAccessManager consent store on Windows) — the same
  signal each OS uses for its own mic-in-use indicator. This stays accurate
  through mute toggles (apps keep the stream open and mute locally) and
  doesn't break when a call app's window-title format changes.
  - Live counters, activity snapshots, and the admin team view now surface
    an "In Call" status alongside idle time.
- **Visible, flashing notifications.** Clock-in/out reminders, idle
  warnings, and break-end notices previously surfaced as an easy-to-miss
  background OS toast while the app sat hidden in the tray. The app now
  also shows and focuses its window (flashing the taskbar/dock icon as a
  fallback when focus can't be stolen) and displays an in-app banner that
  flashes on arrival, color-coded by severity. Only the latest notification
  is ever shown — it replaces any previous one and auto-dismisses after
  10s, avoiding the staleness/stacking problem that got an earlier in-app
  notification design removed.
- **`clanker-fix` command** for macOS permission troubleshooting, installed
  automatically to `~/.local/bin` the first time the app launches (and kept
  up to date on every subsequent launch). Interactive menu to relaunch the
  app with quarantine attributes stripped, reset TCC permissions (scoped to
  ClankerClocker only — doesn't touch other apps' grants like a bare
  `sudo tccutil reset` would), or jump straight to the relevant Privacy &
  Security settings panes. See `docs/macos-troubleshooting.md`.

## [1.1.7] - 2026-08-03

Baseline for this changelog. See git history for changes prior to this tag.
