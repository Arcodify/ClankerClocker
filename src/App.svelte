<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import {
    isPermissionGranted,
    requestPermission,
    sendNotification,
  } from "@tauri-apps/plugin-notification";
  import {
    session,
    latestActivity,
    networkFeed,
    settings,
    authToken,
    userId,
    isAdmin,
    userName,
    errorMessage,
    view,
    elapsedSeconds,
    todayStats,
  } from "./lib/stores";
  import type {
    SessionState,
    ActivitySnapshot,
    NetworkConnection,
    AppNotification,
    TimeLossPrompt,
    LiveCounters,
  } from "./lib/types";
  import Login from "./components/Login.svelte";
  import Dashboard from "./components/Dashboard.svelte";
  import Settings from "./components/Settings.svelte";
  import About from "./components/About.svelte";
  import AdminView from "./components/AdminView.svelte";
  import Dialog from "./components/Dialog.svelte";
  import NotificationBanner from "./components/NotificationBanner.svelte";
  import { formatDuration } from "./lib/stores";

  let ticker: ReturnType<typeof setInterval>;
  let showTimeLossDialog = false;
  let timeLossDeficit = 0;

  // Only the latest notification is ever shown — stacking them was the
  // exact problem that got the old in-app cards removed. Auto-dismiss keeps
  // it from ever going stale if nobody's at the keyboard.
  let activeNotification: AppNotification | null = null;
  let notificationTimeout: ReturnType<typeof setTimeout>;
  function showNotificationBanner(n: AppNotification) {
    clearTimeout(notificationTimeout);
    activeNotification = null;
    // Re-triggers the banner's mount animation even if a notification of
    // the same kind is already showing.
    requestAnimationFrame(() => {
      activeNotification = n;
      notificationTimeout = setTimeout(
        () => (activeNotification = null),
        10_000,
      );
    });
  }

  // In-app test shortcuts: typing any sequence anywhere while the window has
  // focus fires an action — no OS-level grab needed, so this works fine
  // under Wayland compositors like Hyprland where tauri-plugin-global-shortcut
  // doesn't (see src-tauri git history).
  //   lestercity1 — one-off: records keystrokes/mouse clicks for 1 minute,
  //                 no push.
  //   lestercity2 — repeating: records continuously and pushes a snapshot to
  //                 PocketBase every 1 minute, until lestercity3 is typed or
  //                 the session clocks out.
  //   lestercity3 — stops the lestercity2 repeating push.
  const RECORD_SHORTCUT = "lestercity1";
  const PUSH_SHORTCUT = "lestercity2";
  const STOP_SHORTCUT = "lestercity3";
  const RECORDING_DURATION_MS = 60_000;
  let shortcutBuffer = "";

  async function notifySystem(title: string, body: string) {
    try {
      let permission = await isPermissionGranted();
      if (!permission) {
        const res = await requestPermission();
        permission = res === "granted";
      }
      if (permission) sendNotification({ title, body });
    } catch (_) {}
  }

  // Counters reset every ~30s server-side (Background::push_activity_snapshot
  // drains them for syncing), so `live-counters` values follow a sawtooth,
  // not a straight climb. Track deltas sample-to-sample instead of a single
  // start/end diff, and treat any decrease as "counter was drained" — the
  // new (lower) value is activity that happened entirely since the drain.
  let recording = false;
  let recordingHasBaseline = false;
  let recordingLastKeystrokes = 0;
  let recordingLastMouseClicks = 0;
  let recordedKeystrokes = 0;
  let recordedMouseClicks = 0;

  function onLiveCountersForRecording(payload: LiveCounters) {
    if (!recording) return;
    if (!recordingHasBaseline) {
      recordingHasBaseline = true;
      recordingLastKeystrokes = payload.keystrokes;
      recordingLastMouseClicks = payload.mouse_clicks;
      return;
    }
    recordedKeystrokes +=
      payload.keystrokes >= recordingLastKeystrokes
        ? payload.keystrokes - recordingLastKeystrokes
        : payload.keystrokes;
    recordedMouseClicks +=
      payload.mouse_clicks >= recordingLastMouseClicks
        ? payload.mouse_clicks - recordingLastMouseClicks
        : payload.mouse_clicks;
    recordingLastKeystrokes = payload.keystrokes;
    recordingLastMouseClicks = payload.mouse_clicks;
  }

  let repeatingPush = false;
  let repeatIntervalId: ReturnType<typeof setInterval> | null = null;

  function startRecording() {
    if (repeatingPush) {
      notifySystem("recording unavailable", "a repeating push is active — press lestercity3 first");
      return;
    }
    recording = true;
    recordingHasBaseline = false;
    recordedKeystrokes = 0;
    recordedMouseClicks = 0;
    notifySystem("recording started", "capturing keystrokes and mouse clicks for 1 minute…");
    setTimeout(() => {
      recording = false;
      notifySystem(
        "recording finished",
        `${recordedKeystrokes} keystrokes, ${recordedMouseClicks} mouse clicks in the last 1 min`,
      );
    }, RECORDING_DURATION_MS);
  }

  async function pushSnapshot(keystrokes: number, mouseClicks: number) {
    try {
      await invoke("push_test_activity_snapshot", { keystrokes, mouseClicks });
      notifySystem(
        "snapshot pushed",
        `sent ${keystrokes} keystrokes, ${mouseClicks} mouse clicks to PocketBase`,
      );
    } catch (err) {
      notifySystem("push failed", String(err));
    }
  }

  function startRepeatingPush() {
    if (repeatingPush) return;
    if (recording) {
      notifySystem("push unavailable", "a one-off recording is in progress — wait for it to finish");
      return;
    }
    repeatingPush = true;
    recording = true;
    recordingHasBaseline = false;
    recordedKeystrokes = 0;
    recordedMouseClicks = 0;
    notifySystem(
      "repeating push started",
      "sending keystrokes/mouse clicks to PocketBase every 1 min until lestercity3 or clock-out",
    );
    repeatIntervalId = setInterval(() => {
      const ks = recordedKeystrokes;
      const mc = recordedMouseClicks;
      // Reset the accumulation window for the next interval immediately —
      // onLiveCountersForRecording keeps accumulating into it while this push
      // is in flight, so no activity is dropped between intervals.
      recordedKeystrokes = 0;
      recordedMouseClicks = 0;
      pushSnapshot(ks, mc);
    }, RECORDING_DURATION_MS);
  }

  function stopRepeatingPush(reason: string) {
    if (!repeatingPush) return;
    repeatingPush = false;
    recording = false;
    if (repeatIntervalId !== null) {
      clearInterval(repeatIntervalId);
      repeatIntervalId = null;
    }
    notifySystem("repeating push stopped", reason);
  }

  function handleShortcutKeydown(e: KeyboardEvent) {
    if (e.key.length !== 1) return;
    const maxLen = Math.max(
      RECORD_SHORTCUT.length,
      PUSH_SHORTCUT.length,
      STOP_SHORTCUT.length,
    );
    shortcutBuffer = (shortcutBuffer + e.key).slice(-maxLen);
    if (shortcutBuffer.endsWith(RECORD_SHORTCUT)) {
      shortcutBuffer = "";
      startRecording();
    } else if (shortcutBuffer.endsWith(PUSH_SHORTCUT)) {
      shortcutBuffer = "";
      startRepeatingPush();
    } else if (shortcutBuffer.endsWith(STOP_SHORTCUT)) {
      shortcutBuffer = "";
      stopRepeatingPush("stopped via lestercity3");
    }
  }

  async function onTimeLossContinue() {
    showTimeLossDialog = false;
    // Session is already marked extended by the backend; this confirms it
    // (and re-applies it in case the earlier PocketBase PATCH failed).
    try {
      await invoke("extend_session");
    } catch (_) {}
  }

  async function onTimeLossClockOut() {
    showTimeLossDialog = false;
    try {
      await invoke("clock_out", { reason: null });
    } catch (_) {}
  }

  onMount(() => {
    console.log("App mounted");
    initialize().catch((err) => console.error("Initialize failed:", err));

    // Restore running session from the Rust side (survives UI restarts)
    invoke<SessionState>("get_session_state")
      .then((state) => {
        console.log("Restored session:", state.status);
        session.set(state);
        if (state.status !== "idle" && state.clock_in) {
          if (state.status === "on_break" && state.break_start) {
            const breakStart = new Date(state.break_start).getTime();
            elapsedSeconds.set(
              Math.max(0, Math.floor((Date.now() - breakStart) / 1000)),
            );
          } else {
            const start = new Date(state.clock_in).getTime();
            const elapsed =
              Math.floor((Date.now() - start) / 1000) -
              state.total_break_seconds;
            elapsedSeconds.set(Math.max(0, elapsed));
          }
          if (state.status === "active" || state.status === "on_break")
            startTicker();
          view.set("dashboard");
        }
      })
      .catch((err) => console.warn("Failed to get session state:", err));

    // Real-time events from Rust daemon
    const unlistens: Array<Promise<any>> = [
      listen<SessionState>("session-update", (e) => {
        session.set(e.payload);
        if (e.payload.status === "idle") {
          clearInterval(ticker);
          elapsedSeconds.set(0);
          stopRepeatingPush("clocked out");
        } else if (e.payload.status === "on_break" && e.payload.break_start) {
          // The timer shows the current break's running duration (not counted
          // toward work time); the "active" branch below re-syncs from
          // clock_in once the break ends.
          const start = new Date(e.payload.break_start).getTime();
          elapsedSeconds.set(
            Math.max(0, Math.floor((Date.now() - start) / 1000)),
          );
          startTicker();
        } else if (e.payload.status === "active" && e.payload.clock_in) {
          const start = new Date(e.payload.clock_in).getTime();
          elapsedSeconds.set(
            Math.floor((Date.now() - start) / 1000) -
              e.payload.total_break_seconds,
          );
          startTicker();
        }
      }),
      listen<ActivitySnapshot>("activity-update", (e) => {
        latestActivity.set(e.payload);
      }),
      listen<NetworkConnection[]>("network-update", (e) => {
        networkFeed.update((feed) => [...e.payload, ...feed].slice(0, 50));
      }),
      listen<LiveCounters>("live-counters", (e) => {
        onLiveCountersForRecording(e.payload);
      }),
      listen<TimeLossPrompt>("time-loss-prompt", (e) => {
        timeLossDeficit = e.payload.deficit_seconds;
        showTimeLossDialog = true;
      }),
      listen<AppNotification>("app-notification", async (e) => {
        // Backend already shows+focuses the window and flashes the
        // taskbar/dock icon (src-tauri/src/lib.rs Background::notify) before
        // this fires, so the in-app banner below is what's actually seen.
        // The OS notification is a fallback for the rare case focus can't be
        // stolen. Sound is played backend-side (src-tauri/src/audio.rs).
        showNotificationBanner(e.payload);
        try {
          let permission = await isPermissionGranted();
          if (!permission) {
            const res = await requestPermission();
            permission = res === "granted";
          }
          if (permission) {
            sendNotification({ title: e.payload.title, body: e.payload.body });
          }
        } catch (_) {}
      }),
    ];

    window.addEventListener("keydown", handleShortcutKeydown);

    return () => {
      clearInterval(ticker);
      clearTimeout(notificationTimeout);
      unlistens.forEach((p) => p.catch(() => {}).then((u) => u && u()));
      window.removeEventListener("keydown", handleShortcutKeydown);
      if (repeatIntervalId !== null) clearInterval(repeatIntervalId);
    };
  });

  async function initialize() {
    try {
      console.log("Invoking get_settings...");
      const saved = await invoke<any>("get_settings");
      console.log("Settings loaded", !!saved.pb_token);

      // Always restore connection and schedule data so the forms are pre-filled
      settings.update((s) => ({
        ...s,
        pb_url: saved.pb_url || "",
        pb_email: saved.pb_email || "",
        is_admin: !!saved.is_admin,
        clock_in_time: saved.clock_in_time || s.clock_in_time,
        clock_out_time: saved.clock_out_time || s.clock_out_time,
        auto_clock_out_enabled: saved.auto_clock_out_enabled !== false,
      }));

      if (saved.pb_token && saved.token_saved_at) {
        const ageMs = Date.now() - new Date(saved.token_saved_at).getTime();
        if (ageMs > 86_400_000) {
          console.log("Token expired");
          await invoke("clear_auth").catch(() => {});
        } else {
          authToken.set(saved.pb_token);
          userId.set(saved.user_id);
          userName.set(saved.user_name || saved.user_email);
          isAdmin.set(!!saved.is_admin);

          view.set("dashboard");
          refreshAuth().catch(() => {});
        }
      }
    } catch (err) {
      console.error("Initialize error:", err);
    } finally {
      setInterval(refreshAuth, 300_000);
    }
  }

  async function refreshAuth() {
    try {
      const refreshed = await invoke<{
        user_name: string;
        user_email: string;
        is_admin: boolean;
        clock_in_time: string;
        clock_out_time: string;
        auto_clock_out_enabled: boolean;
      }>("refresh_auth_state");
      if (refreshed.user_name) userName.set(refreshed.user_name);
      isAdmin.set(refreshed.is_admin);
      settings.update((s) => ({
        ...s,
        is_admin: refreshed.is_admin,
        pb_email: refreshed.user_email || s.pb_email,
        clock_in_time: refreshed.clock_in_time || s.clock_in_time,
        clock_out_time: refreshed.clock_out_time || s.clock_out_time,
        auto_clock_out_enabled: refreshed.auto_clock_out_enabled,
      }));
    } catch (_) {}
  }

  function startTicker() {
    clearInterval(ticker);
    ticker = setInterval(() => elapsedSeconds.update((s) => s + 1), 1000);
  }

  function onLoginDone() {
    view.set("dashboard");
  }

  function onSkip() {
    view.set("dashboard");
  }

  // Auto-dismiss error after 3.5s
  let errorTimeout: ReturnType<typeof setTimeout>;
  $: if ($errorMessage) {
    clearTimeout(errorTimeout);
    errorTimeout = setTimeout(() => errorMessage.set(""), 3500);
  }
</script>

<main>
  {#if activeNotification}
    {#key activeNotification}
      <NotificationBanner
        notification={activeNotification}
        on:dismiss={() => (activeNotification = null)}
      />
    {/key}
  {/if}

  {#if $view === "login"}
    <Login on:done={onLoginDone} on:skip={onSkip} />
  {:else if $view === "dashboard"}
    <Dashboard
      on:settings={() => view.set("settings")}
      on:admin={() => view.set("admin")}
    />
  {:else if $view === "admin"}
    <AdminView on:back={() => view.set("dashboard")} />
  {:else if $view === "about"}
    <About on:back={() => view.set("settings")} />
  {:else}
    <Settings on:back={() => view.set("dashboard")} />
  {/if}

  {#if $errorMessage}
    <div class="error-toast">{$errorMessage}</div>
  {/if}

  <Dialog
    open={showTimeLossDialog}
    title="Office time is over"
    body={`Your office time has finished, but you have ${formatDuration(timeLossDeficit)} of time loss today.\nDo you want to keep working now to complete your hours?`}
    confirmLabel="Keep Working"
    cancelLabel="Clock Out"
    on:confirm={onTimeLossContinue}
    on:cancel={onTimeLossClockOut}
    on:dismiss={() => (showTimeLossDialog = false)}
  />
</main>

<style>
  :global(*, *::before, *::after) {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
  }
  :global(body) {
    font-family: "Inter", system-ui, sans-serif;
    background: #0d0d0f;
    color: #e8e8ec;
    height: 100vh;
    overflow: hidden;
    user-select: none;
    -webkit-user-select: none;
  }
  main {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }

  .error-toast {
    position: fixed;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    background: #2a0a0a;
    border: 1px solid #7f1d1d;
    color: #fca5a5;
    padding: 8px 18px;
    border-radius: 8px;
    font-size: 12px;
    z-index: 100;
    max-width: 340px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
