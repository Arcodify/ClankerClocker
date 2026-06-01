<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { check as checkUpdate } from "@tauri-apps/plugin-updater";
  import { createEventDispatcher, onDestroy, onMount } from "svelte";
  import {
    authToken,
    errorMessage,
    formatDuration,
    formattedElapsed,
    isAdmin,
    latestActivity,
    networkFeed,
    session,
    todayStats,
    userId,
    userName,
  } from "../lib/stores";
  import type { BreakConfig, LiveCounters } from "../lib/types";

  const dispatch = createEventDispatcher();
  let loading = false;
  let showBreakMenu = false;

  let breakConfigs: BreakConfig[] = [];
  let liveCounters: LiveCounters | null = null;
  let unlisten: (() => void) | null = null;

  let updateVersion: string | null = null;
  let updateInstalling = false;
  let updateDone = false;

  $: isOffline = !$authToken;
  $: statusColor =
    $session.status === "active"
      ? "hsl(var(--success))"
      : $session.status === "on_break"
        ? "hsl(var(--warning))"
        : "hsl(var(--muted-foreground))";
  $: statusLabel =
    $session.status === "active"
      ? "Clocked In"
      : $session.status === "on_break"
        ? "On Break"
        : "Clocked Out";
  $: breakSubtitle =
    $session.break_count > 0
      ? `${$session.break_count} break${$session.break_count > 1 ? "s" : ""} · ${formatDuration($session.total_break_seconds)} break time`
      : null;

  $: monitoringWarning =
    $session.status !== "idle" &&
    liveCounters !== null &&
    !liveCounters.input_monitoring_active;
  $: platformName = navigator.userAgent.includes("Windows")
    ? "windows"
    : navigator.userAgent.includes("Mac")
      ? "macos"
      : "linux";
  $: monitoringWarningText =
    platformName === "windows"
      ? "Input monitoring looks inactive. Run the app as administrator and check whether antivirus is blocking the tray icon or input hooks."
      : platformName === "macos"
        ? "Input monitoring looks inactive. Grant Accessibility in System Settings -> Privacy & Security -> Accessibility."
        : "Input monitoring looks inactive. Run: sudo usermod -aG input $USER, then log out/in.";

  onMount(async () => {
    await refreshTodayStats();
    await loadBreakConfigs();

    unlisten = await listen<LiveCounters>("live-counters", (e) => {
      liveCounters = e.payload;
    });

    checkUpdate()
      .then((update) => {
        if (update) updateVersion = update.version;
      })
      .catch(() => {});
  });

  onDestroy(() => {
    unlisten?.();
  });

  $: if ($session.status === "idle") {
    liveCounters = null;
    refreshTodayStats();
  }

  async function loadBreakConfigs() {
    try {
      breakConfigs = await invoke<BreakConfig[]>("get_break_configs");
    } catch (_) {
      breakConfigs = [
        {
          id: "1",
          name: "Short Break",
          type_key: "short",
          duration_minutes: 15,
          sort_order: 0,
          auto_start_enabled: false,
          auto_start_time: null,
          auto_end_time: null,
        },
        {
          id: "2",
          name: "Lunch",
          type_key: "lunch",
          duration_minutes: 30,
          sort_order: 1,
          auto_start_enabled: false,
          auto_start_time: null,
          auto_end_time: null,
        },
        {
          id: "3",
          name: "Other",
          type_key: "other",
          duration_minutes: 0,
          sort_order: 2,
          auto_start_enabled: false,
          auto_start_time: null,
          auto_end_time: null,
        },
      ];
    }
  }

  async function refreshTodayStats() {
    try {
      const stats = await invoke<typeof $todayStats>("get_today_stats");
      todayStats.set(stats);
    } catch (_) {}
  }

  async function clockIn() {
    loading = true;
    try {
      await invoke("clock_in", {
        userId: $userId || "offline",
        pbToken: $authToken || "",
      });
      liveCounters = {
        keystrokes: 0,
        mouse_clicks: 0,
        mouse_distance_px: 0,
        idle_seconds: 0,
        active_app: "",
        active_window: "",
        input_monitoring_active: true,
      };
      setTimeout(refreshTodayStats, 500);
    } catch (e) {
      errorMessage.set(String(e));
    } finally {
      loading = false;
    }
  }

  async function clockOut() {
    loading = true;
    try {
      await invoke("clock_out");
      setTimeout(refreshTodayStats, 500);
    } catch (e) {
      errorMessage.set(String(e));
    } finally {
      loading = false;
    }
  }

  async function startBreak(breakType: string) {
    showBreakMenu = false;
    try {
      await invoke("start_break", { breakType });
    } catch (e) {
      errorMessage.set(String(e));
    }
  }

  async function endBreak() {
    try {
      await invoke("end_break");
    } catch (e) {
      errorMessage.set(String(e));
    }
  }

  function breakIcon(typeKey: string): string {
    switch (typeKey) {
      case "short":
        return "☕";
      case "lunch":
        return "🍽";
      default:
        return "⏸";
    }
  }

  function fmtNum(n: number): string {
    return n.toLocaleString();
  }

  async function installUpdate() {
    if (!updateVersion || updateInstalling) return;
    updateInstalling = true;
    try {
      const update = await checkUpdate();
      if (update) {
        await update.downloadAndInstall();
        updateDone = true;
        updateVersion = null;
      }
    } catch (e) {
      errorMessage.set("Update failed: " + String(e));
    } finally {
      updateInstalling = false;
    }
  }

  function formatBreakLabel(config: BreakConfig): string {
    const duration =
      config.duration_minutes > 0 ? ` (${config.duration_minutes}m)` : "";
    if (
      config.auto_start_enabled &&
      config.auto_start_time &&
      config.auto_end_time
    ) {
      return `${config.name}${duration} · Auto ${config.auto_start_time}-${config.auto_end_time} NPT`;
    }
    return `${config.name}${duration}`;
  }
</script>

<div class="dashboard">
  <!-- Header -->
  <header>
    <div class="status-dot" style="background:{statusColor}"></div>
    <span class="text-small status-label">{statusLabel}</span>
    <span class="text-small user-name"
      >{$userName || ($isOffline ? "Offline" : "")}</span
    >
    {#if isOffline}
      <button
        class="app-badge app-badge-warning badge-offline"
        on:click={() => dispatch("settings")}
        title="Connect"
      >
        offline
      </button>
    {/if}
    {#if $isAdmin}
      <button
        class="app-button app-button-ghost app-button-icon icon-btn"
        on:click={() => dispatch("admin")}
        title="Team Status">👥</button
      >
    {/if}
    {#if updateVersion}
      <button
        class="app-button app-button-ghost app-button-icon icon-btn update-btn"
        on:click={installUpdate}
        disabled={updateInstalling}
        title="Update to v{updateVersion}"
      >
        {updateInstalling ? "…" : "↑"}
      </button>
    {/if}
    <button
      class="app-button app-button-ghost app-button-icon icon-btn"
      on:click={() => dispatch("settings")}
      title="Settings">⚙</button
    >
  </header>

  {#if updateDone}
    <div class="app-banner app-banner-success update-banner">
      Update installed — restart the app to apply.
    </div>
  {/if}

  <div class="body">
    <!-- Timer -->
    <div class="timer-block">
      <div class="text-display timer">{$formattedElapsed}</div>
      {#if $session.status === "on_break" && $session.break_start}
        <div class="text-small text-warning timer-sub">On Break</div>
      {:else if breakSubtitle && $session.status !== "idle"}
        <div class="text-small timer-sub">{breakSubtitle}</div>
      {:else if $session.status === "idle"}
        <div class="text-small timer-sub">Ready to clock in</div>
      {/if}
    </div>

    <!-- Monitoring warning -->
    {#if monitoringWarning}
      <div class="app-banner app-banner-warning warn-banner">
        ⚠ {monitoringWarningText}
      </div>
    {/if}

    <!-- Action buttons -->
    <div class="actions">
      {#if $session.status === "idle"}
        <button
          class="app-button app-button-success app-button-lg primary-action"
          on:click={clockIn}
          disabled={loading}
        >
          {loading ? "…" : "Clock In"}
        </button>
      {:else if $session.status === "active"}
        <div class="action-row">
          <div class="break-wrap">
            <button
              class="app-button app-button-warning-outline app-button-lg break-trigger"
              on:click={() => (showBreakMenu = !showBreakMenu)}
            >
              Break ▾
            </button>
            {#if showBreakMenu}
              <div class="break-menu">
                {#each breakConfigs as bc}
                  <button on:click={() => startBreak(bc.type_key)}>
                    {breakIcon(bc.type_key)}
                    {formatBreakLabel(bc)}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
          <button
            class="app-button app-button-destructive app-button-lg"
            on:click={clockOut}
            disabled={loading}
          >
            {loading ? "…" : "Clock Out"}
          </button>
        </div>
      {:else}
        <button
          class="app-button app-button-warning app-button-lg primary-action"
          on:click={endBreak}>End Break</button
        >
      {/if}
    </div>

    <!-- Today's Summary -->
    {#if $todayStats}
      <div class="app-card app-card-compact card">
        <div class="app-card-title">Today</div>
        <div class="today-grid">
          <div class="today-stat">
            <span class="text-h3 ts-val">{$todayStats.session_count}</span>
            <span class="text-caption ts-lbl"
              >session{$todayStats.session_count !== 1 ? "s" : ""}</span
            >
          </div>
          <div class="today-stat">
            <span class="text-h3 ts-val"
              >{formatDuration($todayStats.total_work_seconds)}</span
            >
            <span class="text-caption ts-lbl">worked</span>
          </div>
          <div class="today-stat">
            <span class="text-h3 ts-val">{$todayStats.break_count}</span>
            <span class="text-caption ts-lbl"
              >break{$todayStats.break_count !== 1 ? "s" : ""}</span
            >
          </div>
          <div class="today-stat">
            <span class="text-h3 ts-val"
              >{formatDuration($todayStats.total_break_seconds)}</span
            >
            <span class="text-caption ts-lbl">break time</span>
          </div>
        </div>
      </div>
    {/if}

    <!-- Live Activity (updated every 5s) -->
    {#if $session.status !== "idle" && liveCounters !== null}
      <div class="app-card app-card-compact card">
        <div class="app-card-title">
          Activity
          <span class="app-card-description card-sub">live</span>
          <span class="live-dot"></span>
        </div>
        <div class="stat-row">
          <div class="stat">
            <span class="text-h2 sv">{fmtNum(liveCounters.keystrokes)}</span>
            <span class="text-caption sl">keys</span>
          </div>
          <div class="stat-divider"></div>
          <div class="stat">
            <span class="text-h2 sv">{fmtNum(liveCounters.mouse_clicks)}</span>
            <span class="text-caption sl">clicks</span>
          </div>
          <div class="stat-divider"></div>
          <div class="stat">
            <span class="text-h2 sv">{liveCounters.idle_seconds}s</span>
            <span class="text-caption sl">idle</span>
          </div>
        </div>
        {#if liveCounters.active_app}
          <div class="active-app-row">
            <span class="app-icon">▶</span>
            <span class="app-name">{liveCounters.active_app}</span>
            {#if liveCounters.active_window && liveCounters.active_window !== liveCounters.active_app}
              <span class="win-title">— {liveCounters.active_window}</span>
            {/if}
          </div>
        {/if}
      </div>
    {:else if $session.status !== "idle" && $latestActivity}
      <!-- Fallback to last 30s snapshot if live not yet received -->
      <div class="app-card app-card-compact card">
        <div class="app-card-title">
          Activity <span class="app-card-description card-sub">last 30s</span>
        </div>
        <div class="stat-row">
          <div class="stat">
            <span class="text-h2 sv">{fmtNum($latestActivity.keystrokes)}</span>
            <span class="text-caption sl">keys</span>
          </div>
          <div class="stat-divider"></div>
          <div class="stat">
            <span class="text-h2 sv"
              >{fmtNum($latestActivity.mouse_clicks)}</span
            >
            <span class="text-caption sl">clicks</span>
          </div>
          <div class="stat-divider"></div>
          <div class="stat">
            <span class="text-h2 sv">{$latestActivity.idle_seconds}s</span>
            <span class="text-caption sl">idle</span>
          </div>
        </div>
        {#if $latestActivity.active_app}
          <div class="active-app-row">
            <span class="app-icon">▶</span>
            <span class="app-name">{$latestActivity.active_app}</span>
            {#if $latestActivity.active_window && $latestActivity.active_window !== $latestActivity.active_app}
              <span class="win-title">— {$latestActivity.active_window}</span>
            {/if}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Network feed -->
    {#if $session.status !== "idle" && $networkFeed.length > 0}
      <div class="app-card app-card-compact card">
        <div class="app-card-title">
          Network <span class="app-card-description card-sub">recent</span>
        </div>
        <div class="net-list">
          {#each $networkFeed.slice(0, 7) as conn}
            <div class="net-row">
              <span class="net-proc">{conn.process_name || "?"}</span>
              <span class="net-arrow">→</span>
              <span class="net-host">{conn.remote_host || conn.remote_ip}</span>
              <span class="net-port">:{conn.remote_port}</span>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if $session.status === "idle" && !$todayStats}
      <div class="text-small idle-hint">
        No sessions today yet. Clock in to start tracking.
      </div>
    {/if}
  </div>
</div>

<!-- Close break menu on outside click -->
{#if showBreakMenu}
  <div
    class="overlay"
    on:click={() => (showBreakMenu = false)}
    role="presentation"
  ></div>
{/if}

<style>
  .dashboard {
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding: 0 12px 12px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 4px;
    border-bottom: 1px solid hsl(var(--border) / 0.72);
    flex-shrink: 0;
  }
  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
    box-shadow: 0 0 0 4px currentColor;
    opacity: 0.85;
    transition: background 0.3s;
  }
  .status-label {
    font-weight: 600;
  }
  .user-name {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .icon-btn {
    font-size: 15px;
    line-height: 1;
    flex-shrink: 0;
  }
  .update-btn {
    color: hsl(var(--success));
  }
  .update-banner {
    margin: 0 12px;
    flex-shrink: 0;
  }
  .badge-offline {
    cursor: pointer;
  }

  .body {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 0 0 4px;
  }
  .body::-webkit-scrollbar {
    width: 3px;
  }
  .body::-webkit-scrollbar-track {
    background: transparent;
  }
  .body::-webkit-scrollbar-thumb {
    background: hsl(var(--border));
    border-radius: 2px;
  }

  .timer-block {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 30px 12px 18px;
  }
  .timer {
    letter-spacing: 0;
    font-variant-numeric: tabular-nums;
    text-shadow: 0 18px 42px hsl(var(--primary) / 0.18);
  }
  .timer-sub {
    margin-top: 6px;
  }

  .warn-banner {
    margin: 0;
  }
  .actions {
    display: flex;
    justify-content: center;
    padding: 0 4px 4px;
  }
  .primary-action {
    min-width: 150px;
  }
  .break-trigger {
    padding-left: 18px;
    padding-right: 18px;
  }

  .action-row {
    display: flex;
    gap: 8px;
  }
  .break-wrap {
    position: relative;
  }
  .break-menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    background: hsl(var(--popover));
    border: 1px solid hsl(var(--border));
    border-radius: var(--radius);
    overflow: hidden;
    z-index: 20;
    min-width: 160px;
    box-shadow: var(--shadow-popover);
  }
  .break-menu button {
    display: block;
    width: 100%;
    padding: 9px 14px;
    text-align: left;
    background: none;
    border: none;
    color: hsl(var(--popover-foreground));
    font-size: 13px;
    cursor: pointer;
  }
  .break-menu button:hover {
    background: hsl(var(--accent));
  }

  .overlay {
    position: fixed;
    inset: 0;
    z-index: 10;
  }

  /* Cards */
  .card {
    margin: 0;
  }
  .card-sub {
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
  }

  .live-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: hsl(var(--success));
    animation: pulse 2s ease-in-out infinite;
    margin-left: auto;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.3;
    }
  }

  /* Today grid */
  .today-grid {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr 1fr;
    gap: 6px;
  }
  .today-stat {
    display: flex;
    flex-direction: column;
    align-items: center;
    background: hsl(var(--background) / 0.55);
    border: 1px solid hsl(var(--border) / 0.5);
    border-radius: var(--radius-md);
    padding: 10px 5px;
  }
  .ts-val {
    font-size: 18px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .ts-lbl {
    letter-spacing: 0.4px;
    margin-top: 2px;
  }

  /* Activity stats */
  .stat-row {
    display: flex;
    align-items: center;
    gap: 0;
    margin-bottom: 12px;
  }
  .stat {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .sv {
    font-size: 20px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .sl {
    letter-spacing: 0.4px;
  }
  .stat-divider {
    width: 1px;
    height: 28px;
    background: hsl(var(--border));
    flex-shrink: 0;
  }

  .active-app-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    border-top: 1px solid hsl(var(--border) / 0.7);
    padding-top: 9px;
    overflow: hidden;
  }
  .app-icon {
    color: hsl(var(--success));
    font-size: 9px;
    flex-shrink: 0;
  }
  .app-name {
    color: hsl(var(--secondary-foreground));
    font-weight: 500;
    flex-shrink: 0;
  }
  .win-title {
    color: hsl(var(--muted-foreground));
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Network */
  .net-list {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .net-row {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
  }
  .net-proc {
    font-weight: 600;
    min-width: 70px;
    max-width: 70px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .net-arrow {
    color: hsl(var(--muted-foreground) / 0.6);
    flex-shrink: 0;
  }
  .net-host {
    flex: 1;
    color: hsl(var(--secondary-foreground));
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .net-port {
    color: hsl(var(--muted-foreground));
    flex-shrink: 0;
  }

  .idle-hint {
    margin: 20px 4px;
    font-size: 12px;
    text-align: center;
    line-height: 1.6;
  }
</style>
