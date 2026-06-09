# Changelog

## Unreleased

### Linux

- Fixed mouse clicks being misclassified as keypresses on pointer devices.
- Improved active-window detection on Hyprland by listening to the compositor event stream and caching the latest focused window.
- Added Hyprland fallback lookups so app names can be recovered from the focused client or its PID when class data is missing.
- Kept the existing X11 path intact and left non-Hyprland Linux as a fallback path.

### macOS

- Clarified that macOS input tracking needs both Accessibility and Input Monitoring permissions.
- Kept the global input monitor on the `rdev` path, with a stronger lock strategy so input events are not dropped under contention.
- Added an unsigned install helper for local distribution so users do not need to run `xattr -cr` by hand every time.

### Packaging and release flow

- Added `npm run install:macos:unsigned` to copy the app into `~/Applications`, strip quarantine once, and launch it.
- Added a short unsigned macOS release guide under `docs/macos-unsigned-release.md`.

### UI text

- Updated the dashboard and settings copy so the permission and monitoring requirements are clearer across platforms.

### Backend and sync

- Preserved the 5-second live activity emission and 30-second snapshot emission, but now the active-window value comes from a compositor-aware cache when available.
- Left the existing PocketBase sync paths in place.
