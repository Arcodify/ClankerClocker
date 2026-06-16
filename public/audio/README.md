# Notification sounds

Drop custom sound files here, replacing the placeholders with the same name.
These correspond to the `kind` field of `app-notification` events
(see `src-tauri/src/session.rs` / `lib.rs` and `src/App.svelte`).

- `clock_in_reminder.mp3` — time to clock in
- `idle_clockout_warning.mp3` — heads up, about to be clocked out for inactivity
- `idle_clockout.mp3` — clocked out due to inactivity
- `scheduled_clockout_warning.mp3` — heads up, scheduled clock-out is near
- `scheduled_clockout.mp3` — scheduled clock-out happened
- `info.mp3` — generic info notification (e.g. break ended)

Any audio format supported by the browser's `<audio>` element works (mp3, wav, ogg).
