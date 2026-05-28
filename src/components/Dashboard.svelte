<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { check as checkUpdate } from "@tauri-apps/plugin-updater";
  import { listen } from "@tauri-apps/api/event";
  import {
    session, latestActivity, networkFeed,
    formattedElapsed, formatDuration,
    errorMessage, authToken, userId, isAdmin, userName,
    todayStats,
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
  $: statusColor = $session.status === "active" ? "#22c55e" : $session.status === "on_break" ? "#f59e0b" : "#6b7280";
  $: statusLabel = $session.status === "active" ? "Clocked In" : $session.status === "on_break" ? "On Break" : "Clocked Out";
  $: breakSubtitle = $session.break_count > 0
    ? `${$session.break_count} break${$session.break_count > 1 ? "s" : ""} · ${formatDuration($session.total_break_seconds)} break time`
    : null;

  $: monitoringWarning = $session.status !== "idle" && liveCounters !== null
    && !liveCounters.input_monitoring_active;
  $: platformName = navigator.userAgent.includes("Windows")
    ? "windows"
    : navigator.userAgent.includes("Mac")
      ? "macos"
      : "linux";
  $: monitoringWarningText = platformName === "windows"
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

    checkUpdate().then(update => {
      if (update) updateVersion = update.version;
    }).catch(() => {});
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
        { id: "1", name: "Short Break", type_key: "short", duration_minutes: 15, sort_order: 0, auto_start_enabled: false, auto_start_time: null, auto_end_time: null },
        { id: "2", name: "Lunch", type_key: "lunch", duration_minutes: 30, sort_order: 1, auto_start_enabled: false, auto_start_time: null, auto_end_time: null },
        { id: "3", name: "Other", type_key: "other", duration_minutes: 0, sort_order: 2, auto_start_enabled: false, auto_start_time: null, auto_end_time: null },
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
      case "short": return "☕";
      case "lunch": return "🍽";
      default: return "⏸";
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
    const duration = config.duration_minutes > 0 ? ` (${config.duration_minutes}m)` : "";
    if (config.auto_start_enabled && config.auto_start_time && config.auto_end_time) {
      return `${config.name}${duration} · Auto ${config.auto_start_time}-${config.auto_end_time} NPT`;
    }
    return `${config.name}${duration}`;
  }
</script>

<div class="dashboard">
  <!-- Header -->
  <header>
    <div class="status-dot" style="background:{statusColor}"></div>
    <span class="status-label">{statusLabel}</span>
    <span class="user-name">{$userName || ($isOffline ? "Offline" : "")}</span>
    {#if isOffline}
      <button class="badge badge-offline" on:click={() => dispatch("settings")} title="Connect">
        offline
      </button>
    {/if}
    {#if $isAdmin}
      <button class="icon-btn" on:click={() => dispatch("admin")} title="Team Status">👥</button>
    {/if}
    {#if updateVersion}
      <button class="icon-btn update-btn" on:click={installUpdate} disabled={updateInstalling} title="Update to v{updateVersion}">
        {updateInstalling ? "…" : "↑"}
      </button>
    {/if}
    <button class="icon-btn" on:click={() => dispatch("settings")} title="Settings">⚙</button>
  </header>

  {#if updateDone}
    <div class="update-banner">Update installed — restart the app to apply.</div>
  {/if}

  <div class="body">
    <!-- Timer -->
    <div class="timer-block">
      <div class="timer">{$formattedElapsed}</div>
      {#if $session.status === "on_break" && $session.break_start}
        <div class="timer-sub break-sub">On Break</div>
      {:else if breakSubtitle && $session.status !== "idle"}
        <div class="timer-sub">{breakSubtitle}</div>
      {:else if $session.status === "idle"}
        <div class="timer-sub">Ready to clock in</div>
      {/if}
    </div>

    <!-- Monitoring warning -->
    {#if monitoringWarning}
      <div class="warn-banner">
        ⚠ {monitoringWarningText}
      </div>
    {/if}

    <!-- Action buttons -->
    <div class="actions">
      {#if $session.status === "idle"}
        <button class="btn btn-in" on:click={clockIn} disabled={loading}>
          {loading ? "…" : "Clock In"}
        </button>
      {:else if $session.status === "active"}
        <div class="btn-row">
          <div class="break-wrap">
            <button class="btn btn-break" on:click={() => (showBreakMenu = !showBreakMenu)}>
              Break ▾
            </button>
            {#if showBreakMenu}
              <div class="break-menu">
                {#each breakConfigs as bc}
                  <button on:click={() => startBreak(bc.type_key)}>
                    {breakIcon(bc.type_key)} {formatBreakLabel(bc)}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
          <button class="btn btn-out" on:click={clockOut} disabled={loading}>
            {loading ? "…" : "Clock Out"}
          </button>
        </div>
      {:else}
        <button class="btn btn-resume" on:click={endBreak}>End Break</button>
      {/if}
    </div>

    <!-- Today's Summary -->
    {#if $todayStats}
      <div class="card">
        <div class="card-title">Today</div>
        <div class="today-grid">
          <div class="today-stat">
            <span class="ts-val">{$todayStats.session_count}</span>
            <span class="ts-lbl">session{$todayStats.session_count !== 1 ? "s" : ""}</span>
          </div>
          <div class="today-stat">
            <span class="ts-val">{formatDuration($todayStats.total_work_seconds)}</span>
            <span class="ts-lbl">worked</span>
          </div>
          <div class="today-stat">
            <span class="ts-val">{$todayStats.break_count}</span>
            <span class="ts-lbl">break{$todayStats.break_count !== 1 ? "s" : ""}</span>
          </div>
          <div class="today-stat">
            <span class="ts-val">{formatDuration($todayStats.total_break_seconds)}</span>
            <span class="ts-lbl">break time</span>
          </div>
        </div>
      </div>
    {/if}

    <!-- Live Activity (updated every 5s) -->
    {#if $session.status !== "idle" && liveCounters !== null}
      <div class="card">
        <div class="card-title">
          Activity
          <span class="card-sub">live</span>
          <span class="live-dot"></span>
        </div>
        <div class="stat-row">
          <div class="stat">
            <span class="sv">{fmtNum(liveCounters.keystrokes)}</span>
            <span class="sl">keys</span>
          </div>
          <div class="stat-divider"></div>
          <div class="stat">
            <span class="sv">{fmtNum(liveCounters.mouse_clicks)}</span>
            <span class="sl">clicks</span>
          </div>
          <div class="stat-divider"></div>
          <div class="stat">
            <span class="sv">{liveCounters.idle_seconds}s</span>
            <span class="sl">idle</span>
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
      <div class="card">
        <div class="card-title">Activity <span class="card-sub">last 30s</span></div>
        <div class="stat-row">
          <div class="stat">
            <span class="sv">{fmtNum($latestActivity.keystrokes)}</span>
            <span class="sl">keys</span>
          </div>
          <div class="stat-divider"></div>
          <div class="stat">
            <span class="sv">{fmtNum($latestActivity.mouse_clicks)}</span>
            <span class="sl">clicks</span>
          </div>
          <div class="stat-divider"></div>
          <div class="stat">
            <span class="sv">{$latestActivity.idle_seconds}s</span>
            <span class="sl">idle</span>
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
      <div class="card">
        <div class="card-title">Network <span class="card-sub">recent</span></div>
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
      <div class="idle-hint">
        No sessions today yet. Clock in to start tracking.
      </div>
    {/if}
  </div>
</div>

<!-- Close break menu on outside click -->
{#if showBreakMenu}
  <div class="overlay" on:click={() => (showBreakMenu = false)} role="presentation"></div>
{/if}

<style>
  .dashboard { display: flex; flex-direction: column; height: 100vh; }

  header {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 12px 16px;
    border-bottom: 1px solid #16161e;
    flex-shrink: 0;
  }
  .status-dot { width: 7px; height: 7px; border-radius: 50%; flex-shrink: 0; transition: background 0.3s; }
  .status-label { font-size: 12px; font-weight: 600; color: #7070a0; }
  .user-name { flex: 1; font-size: 12px; color: #4a4a62; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .icon-btn {
    background: none; border: none; color: #4a4a62;
    font-size: 15px; cursor: pointer; padding: 3px 5px;
    border-radius: 4px; line-height: 1; flex-shrink: 0;
  }
  .icon-btn:hover { color: #c0c0d0; background: #1e1e2a; }
  .update-btn { color: #22c55e; }
  .update-btn:hover { color: #4ade80; background: #0f2a1a; }
  .update-banner { margin: 0 12px; padding: 8px 12px; background: #0f2a1a; border: 1px solid #166534; border-radius: 8px; font-size: 11px; color: #4ade80; flex-shrink: 0; }

  .badge {
    font-size: 9px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.8px;
    padding: 2px 6px; border-radius: 3px; cursor: pointer; border: none;
  }
  .badge-offline { background: #1c1a10; border: 1px solid #3a2e00; color: #8a6a00; }
  .badge-offline:hover { border-color: #f59e0b; color: #f59e0b; }

  .body {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 0 0 12px;
  }
  .body::-webkit-scrollbar { width: 3px; }
  .body::-webkit-scrollbar-track { background: transparent; }
  .body::-webkit-scrollbar-thumb { background: #2a2a36; border-radius: 2px; }

  .timer-block {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 28px 16px 16px;
  }
  .timer {
    font-size: 54px;
    font-weight: 200;
    letter-spacing: 3px;
    font-variant-numeric: tabular-nums;
    color: #f0f0f8;
    line-height: 1;
  }
  .timer-sub { margin-top: 6px; font-size: 12px; color: #5a5a72; }
  .break-sub { color: #f59e0b; }

  .warn-banner {
    margin: 0 12px;
    padding: 8px 12px;
    background: #1a120a;
    border: 1px solid #4a2800;
    border-radius: 8px;
    font-size: 11px;
    color: #c06a00;
    line-height: 1.5;
  }
  .actions {
    display: flex;
    justify-content: center;
    padding: 0 16px 4px;
  }
  .btn {
    padding: 10px 32px;
    border-radius: 8px;
    border: none;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s;
  }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-in { background: #22c55e; color: #080c10; min-width: 150px; }
  .btn-in:hover:not(:disabled) { background: #16a34a; }
  .btn-out { background: #ef4444; color: white; }
  .btn-out:hover:not(:disabled) { background: #dc2626; }
  .btn-break { background: #1e1e2c; color: #c0a000; border: 1px solid #3a3000; font-size: 13px; padding: 10px 18px; }
  .btn-break:hover { background: #252530; }
  .btn-resume { background: #f59e0b; color: #080c10; min-width: 150px; font-weight: 700; }
  .btn-resume:hover { background: #d97706; }

  .btn-row { display: flex; gap: 8px; }
  .break-wrap { position: relative; }
  .break-menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    background: #16161e;
    border: 1px solid #2a2a38;
    border-radius: 8px;
    overflow: hidden;
    z-index: 20;
    min-width: 160px;
    box-shadow: 0 8px 24px rgba(0,0,0,0.5);
  }
  .break-menu button {
    display: block; width: 100%;
    padding: 9px 14px; text-align: left;
    background: none; border: none;
    color: #c0c0d0; font-size: 13px; cursor: pointer;
  }
  .break-menu button:hover { background: #252532; }

  .overlay {
    position: fixed; inset: 0; z-index: 10;
  }

  /* Cards */
  .card {
    margin: 0 12px;
    background: #111118;
    border: 1px solid #1a1a24;
    border-radius: 10px;
    padding: 11px 14px;
  }
  .card-title {
    font-size: 10px; font-weight: 700;
    text-transform: uppercase; letter-spacing: 0.8px;
    color: #4a4a62; margin-bottom: 10px;
    display: flex; align-items: center; gap: 6px;
  }
  .card-sub { color: #2e2e42; font-weight: 400; text-transform: none; letter-spacing: 0; }

  .live-dot {
    width: 5px; height: 5px; border-radius: 50%;
    background: #22c55e;
    animation: pulse 2s ease-in-out infinite;
    margin-left: auto;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  /* Today grid */
  .today-grid {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr 1fr;
    gap: 4px;
  }
  .today-stat {
    display: flex; flex-direction: column; align-items: center;
    background: #0e0e16; border-radius: 6px; padding: 8px 4px;
  }
  .ts-val { font-size: 18px; font-weight: 600; color: #e0e0ec; font-variant-numeric: tabular-nums; }
  .ts-lbl { font-size: 9px; color: #4a4a62; text-transform: uppercase; letter-spacing: 0.4px; margin-top: 2px; }

  /* Activity stats */
  .stat-row {
    display: flex;
    align-items: center;
    gap: 0;
    margin-bottom: 10px;
  }
  .stat { flex: 1; display: flex; flex-direction: column; align-items: center; }
  .sv { font-size: 20px; font-weight: 600; color: #d8d8ec; font-variant-numeric: tabular-nums; }
  .sl { font-size: 9px; color: #4a4a62; text-transform: uppercase; letter-spacing: 0.4px; }
  .stat-divider { width: 1px; height: 28px; background: #1e1e2a; flex-shrink: 0; }

  .active-app-row {
    display: flex; align-items: center; gap: 6px;
    font-size: 11px; color: #606078;
    border-top: 1px solid #1a1a24; padding-top: 8px;
    overflow: hidden;
  }
  .app-icon { color: #22c55e; font-size: 9px; flex-shrink: 0; }
  .app-name { color: #9090b0; font-weight: 500; flex-shrink: 0; }
  .win-title { color: #4a4a62; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  /* Network */
  .net-list { display: flex; flex-direction: column; gap: 5px; }
  .net-row { display: flex; align-items: center; gap: 5px; font-size: 11px; }
  .net-proc {
    color: #7878a0; font-weight: 600;
    min-width: 70px; max-width: 70px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .net-arrow { color: #2e2e42; flex-shrink: 0; }
  .net-host { flex: 1; color: #a0a0bc; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .net-port { color: #3a3a52; flex-shrink: 0; }

  .idle-hint {
    margin: 20px 16px;
    font-size: 12px;
    color: #3a3a52;
    text-align: center;
    line-height: 1.6;
  }
</style>
