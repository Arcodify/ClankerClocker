<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import AdminView from "./components/AdminView.svelte";
  import Dashboard from "./components/Dashboard.svelte";
  import Login from "./components/Login.svelte";
  import Settings from "./components/Settings.svelte";
  import {
    authToken,
    elapsedSeconds,
    errorMessage,
    latestActivity,
    networkFeed,
    session,
    settings,
    userId,
    userName,
    view,
  } from "./lib/stores";
  import type {
    ActivitySnapshot,
    NetworkConnection,
    SessionState,
  } from "./lib/types";

  let ticker: ReturnType<typeof setInterval>;

  onMount(async () => {
    try {
      const saved = await invoke<{
        pb_url: string;
        pb_email: string;
        pb_token: string;
        user_id: string;
        user_name: string;
        user_email: string;
        token_saved_at: string;
        default_pb_url: string;
      }>("get_settings");

      // Always restore URL and email so Login form is pre-filled
      settings.update((s) => ({
        ...s,
        pb_url: saved.pb_url,
        pb_email: saved.pb_email,
      }));

      if (saved.pb_token && saved.token_saved_at) {
        const ageMs = Date.now() - new Date(saved.token_saved_at).getTime();
        if (ageMs > 86_400_000) {
          // Token older than 24 h — clear it and stay on login (email still pre-filled)
          await invoke("clear_auth").catch(() => {});
        } else {
          authToken.set(saved.pb_token);
          userId.set(saved.user_id);
          userName.set(saved.user_name || saved.user_email);
          view.set("dashboard");
        }
      }
    } catch (_) {}

    // Restore running session from the Rust side (survives UI restarts)
    try {
      const state = await invoke<SessionState>("get_session_state");
      session.set(state);
      if (state.status !== "idle" && state.clock_in) {
        const start = new Date(state.clock_in).getTime();
        elapsedSeconds.set(
          Math.floor((Date.now() - start) / 1000) - state.total_break_seconds,
        );
        startTicker();
        view.set("dashboard");
      }
    } catch (_) {}

    // Real-time events from Rust daemon
    await listen<SessionState>("session-update", (e) => {
      session.set(e.payload);
      if (e.payload.status === "idle") {
        clearInterval(ticker);
        elapsedSeconds.set(0);
      } else if (e.payload.status === "active" && e.payload.clock_in) {
        const start = new Date(e.payload.clock_in).getTime();
        elapsedSeconds.set(
          Math.floor((Date.now() - start) / 1000) -
            e.payload.total_break_seconds,
        );
        startTicker();
      }
    });

    await listen<ActivitySnapshot>("activity-update", (e) => {
      latestActivity.set(e.payload);
    });

    await listen<NetworkConnection[]>("network-update", (e) => {
      networkFeed.update((feed) => [...e.payload, ...feed].slice(0, 50));
    });

    return () => clearInterval(ticker);
  });

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
  {:else}
    <Settings on:back={() => view.set("dashboard")} />
  {/if}

  {#if $errorMessage}
    <div class="error-toast">{$errorMessage}</div>
  {/if}
</main>

<style>
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
    background: hsl(var(--destructive-muted));
    border: 1px solid hsl(var(--destructive));
    color: hsl(var(--destructive-muted-foreground));
    padding: 8px 18px;
    border-radius: var(--radius);
    font-size: 12px;
    z-index: 100;
    max-width: 340px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
