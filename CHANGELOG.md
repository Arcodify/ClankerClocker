# Changelog

All notable changes to ClankerClocker are documented here. Format loosely
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [1.2.9] - 2026-11-24

### Fixed

- **Snapshots rejected for long window titles.** A window title over 500
  characters failed PocketBase validation (`active_window: Must be no more
  than 500 character(s)`). The snapshot was queued and retried forever, and
  with no snapshots arriving the server saw the user as offline and clocked
  them out. Snapshot and network text fields are now truncated to the
  schema limits before upload, which also unsticks already-queued rows.

## [1.2.8] - 2026-11-24

### Fixed

- **Ending a break didn't always reach the server.** The break `end_time`
  PATCH was fire-and-forget: one failed request left the break open on the
  server while the app showed the user as back at work. Break time kept
  growing in the "today" totals, and once the break outran its allowed
  length the server auto-clocked the user out 1–2 minutes after they'd
  ended it. The PATCH now retries (3 attempts with backoff), falls back to
  closing every open break on the session, and if that still fails the
  close is queued and retried after each successful activity upload.
- **Slow loading everywhere.** `get_list` always sent `perPage=200`, and a
  caller's own `perPage` was appended after it — PocketBase reads the first
  value, so "fetch one" lookups paged through every matching record and the
  admin user list downloaded the entire `work_sessions` table. Small
  `perPage` values are now a real limit, `skipTotal` drops the per-request
  `COUNT(*)`, and the user list fetches only the fields it uses.
- **macOS input monitoring falsely reported inactive.**
  `CGPreflightListenEventAccess` is cached per process and stays false until
  a relaunch, even after the user grants it. Either Accessibility or Input
  Monitoring is now accepted as enough.

### Added

- **External staff break limit (server).** A break by an external staff
  member that runs past 1 hour ends the session, closed at the one-hour mark
  so the overrun isn't counted as break time.

## [1.2.7] - 2026-11-18

### Fixed

- **macOS input monitoring "working" only while the app was focused.** An
  untrusted event tap still receives events aimed at the app's own window,
  which looked like global monitoring was fine. Health is now judged by the
  actual Accessibility / Input Monitoring grants, and the app asks for
  Input Monitoring through the supported API.
- **Signed out by network blips.** A temporary outage no longer logs out a
  user whose token is still valid; only a real token rejection does.
- macOS unsigned-install script and docs now print clearer TCC
  (permissions) output.

## [1.2.5] - 2026-10-27

### Added

- **Opt-in diagnostics capture** in Settings. Records structured events
  (never key names, typed text, credentials or network payloads), is
  off by default and resets to off on every app restart.

### Fixed

- **Calls with a muted mic counted as idle.** Browsers and meeting clients
  can release the mic while muted; a focused, recognisable meeting window
  now keeps the call protection on as well.
- **Touchpad scrolling ignored as activity.** Two-finger scroll without
  cursor movement now counts as input.
- Windows input hook reliability fix in the vendored `rdev`.

## [1.2.4] - 2026-10-16

### Added

- **Tasks app** for the Plane integration: sidebar, issue list and issue
  detail views replace the old single task panel.

## [1.2.3] - 2026-10-11

### Fixed

- **Server clocked people out mid-call.** The auto-clockout cron now honours
  the latest snapshot's `in_call` flag, matching the client's own idle
  suppression.
- Added `scripts/cleanup_duplicate_break_sessions.sql` to clean up
  duplicate sessions created around breaks.

## [1.2.1] - 2026-09-24

### Fixed

- Project-management settings (`pm_config`: workspace slug / base URL) are
  now saved locally, so they survive restarts.

## [1.2.0] - 2026-09-24

### Added

- **Plane project-management integration (first version)**, configured via
  PocketBase.
- Show/hide toggle on the login password field.

### Changed

- Time-loss calculation moved to a shared `domain::time_loss` module so the
  dashboard and admin views use the same numbers.

## [1.1.11] - 2026-08-18

### Fixed

- **Restart while clocked in created a second session.** The app now
  restores an open session from PocketBase on startup instead of assuming
  it's idle (no repeat clock-in reminder, no duplicate session).
- Dashboard warns when the current session exists only on this machine and
  hasn't been synced to the server yet.

## [1.1.10] - 2026-08-11

### Fixed

- **Offline sessions never reached the server.** A session started without
  credentials (login Skip, or a stale token) only existed in memory. It's
  now created on PocketBase once credentials are available, at login and
  from the offline-sync loop, and queued snapshots are re-tagged to it.
- **Idle timer reset by phantom input.** Linux input handling counted every
  device event (sync markers, key releases, zero-delta keep-alive packets)
  as activity; only real key presses and non-zero motion count now.

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
