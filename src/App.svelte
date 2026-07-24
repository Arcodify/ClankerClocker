<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
  import {
    session, latestActivity, networkFeed, settings,
    authToken, userId, isAdmin, userName,
    errorMessage, view, elapsedSeconds,
    todayStats,
  } from "./lib/stores";
  import type { SessionState, ActivitySnapshot, NetworkConnection, AppNotification, TimeLossPrompt } from "./lib/types";
  import Login from "./components/Login.svelte";
  import Dashboard from "./components/Dashboard.svelte";
  import Settings from "./components/Settings.svelte";
  import About from "./components/About.svelte";
  import AdminView from "./components/AdminView.svelte";
  import Dialog from "./components/Dialog.svelte";
  import { formatDuration } from "./lib/stores";

  let ticker: ReturnType<typeof setInterval>;
  let showTimeLossDialog = false;
  let timeLossDeficit = 0;

  async function onTimeLossContinue() {
    showTimeLossDialog = false;
    // Session is already marked extended by the backend; this confirms it
    // (and re-applies it in case the earlier PocketBase PATCH failed).
    try { await invoke("extend_session"); } catch (_) {}
  }

  async function onTimeLossClockOut() {
    showTimeLossDialog = false;
    try { await invoke("clock_out", { reason: null }); } catch (_) {}
  }

  onMount(() => {
    console.log("App mounted");
    initialize().catch(err => console.error("Initialize failed:", err));

    // Restore running session from the Rust side (survives UI restarts)
    invoke<SessionState>("get_session_state").then(state => {
      console.log("Restored session:", state.status);
      session.set(state);
      if (state.status !== "idle" && state.clock_in) {
        const start = new Date(state.clock_in).getTime();
        let elapsed =
          Math.floor((Date.now() - start) / 1000) - state.total_break_seconds;
        // An in-progress break isn't in total_break_seconds yet — exclude it.
        if (state.status === "on_break" && state.break_start) {
          elapsed -= Math.floor(
            (Date.now() - new Date(state.break_start).getTime()) / 1000
          );
        }
        elapsedSeconds.set(Math.max(0, elapsed));
        if (state.status === "active") startTicker();
        view.set("dashboard");
      }
    }).catch(err => console.warn("Failed to get session state:", err));

    // Real-time events from Rust daemon
    const unlistens: Array<Promise<any>> = [
      listen<SessionState>("session-update", (e) => {
        session.set(e.payload);
        if (e.payload.status === "idle") {
          clearInterval(ticker);
          elapsedSeconds.set(0);
        } else if (e.payload.status === "on_break") {
          // Break time must not count as work time — freeze the ticker until
          // the break ends (the "active" branch below re-syncs from clock_in).
          clearInterval(ticker);
        } else if (e.payload.status === "active" && e.payload.clock_in) {
          const start = new Date(e.payload.clock_in).getTime();
          elapsedSeconds.set(
            Math.floor((Date.now() - start) / 1000) - e.payload.total_break_seconds
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
      listen<TimeLossPrompt>("time-loss-prompt", (e) => {
        timeLossDeficit = e.payload.deficit_seconds;
        showTimeLossDialog = true;
      }),
      listen<AppNotification>("app-notification", async (e) => {
        // System notification only — no in-app cards, they covered the UI
        // and stacked up stale while the window was hidden in the tray.
        // Sound is played backend-side (src-tauri/src/audio.rs).
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
      })
    ];

    return () => {
      clearInterval(ticker);
      unlistens.forEach(p => p.catch(() => {}).then(u => u && u()));
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
  :global(*, *::before, *::after) { box-sizing: border-box; margin: 0; padding: 0; }
  :global(body) {
    font-family: "Inter", system-ui, sans-serif;
    background: #0d0d0f;
    color: #e8e8ec;
    height: 100vh;
    overflow: hidden;
    user-select: none;
    -webkit-user-select: none;
  }
  main { height: 100vh; display: flex; flex-direction: column; }

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
