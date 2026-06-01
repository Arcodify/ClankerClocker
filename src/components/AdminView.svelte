<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { teamStatus, errorMessage, formatDuration } from "../lib/stores";
  import type { TeamMember, ActivitySnapshot, NetworkConnection } from "../lib/types";

  const dispatch = createEventDispatcher();
  let loading = true;
  let refreshInterval: ReturnType<typeof setInterval>;

  // Detail view
  let selected: TeamMember | null = null;
  let snapshots: ActivitySnapshot[] = [];
  let netConns: NetworkConnection[] = [];
  let detailLoading = false;
  let netFilter = "";
  let netPage = 0;
  const NET_PAGE_SIZE = 10;
  let activeTab: "activity" | "network" | "calendar" = "activity";

  // Calendar state
  let calYear = new Date().getFullYear();
  let calMonth = new Date().getMonth(); // 0-based
  let calData: Record<string, { session_count: number; total_work_seconds: number; total_break_seconds: number }> = {};
  let calLoading = false;

  onMount(async () => {
    await refresh();
    refreshInterval = setInterval(refresh, 30_000);
    return () => clearInterval(refreshInterval);
  });
  onDestroy(() => clearInterval(refreshInterval));

  async function refresh() {
    try {
      const members = await invoke<TeamMember[]>("get_team_status");
      teamStatus.set(members);
      if (selected) {
        const updated = members.find(m => m.session_id === selected!.session_id);
        if (updated) selected = updated;
      }
    } catch (e) {
      errorMessage.set(String(e));
    } finally {
      loading = false;
    }
  }

  async function selectMember(m: TeamMember) {
    selected = m;
    snapshots = []; netConns = [];
    detailLoading = true; activeTab = "activity";
    try {
      const [snaps, nets] = await Promise.all([
        invoke<ActivitySnapshot[]>("get_user_activity", { sessionId: m.session_id }),
        invoke<NetworkConnection[]>("get_user_network", { sessionId: m.session_id }),
      ]);
      snapshots = snaps;
      netConns = nets;
    } catch (_) {}
    detailLoading = false;
  }

  function back() { selected = null; snapshots = []; netConns = []; }

  async function loadCalendar() {
    if (!selected) return;
    calLoading = true;
    const ym = `${calYear}-${String(calMonth + 1).padStart(2, "0")}`;
    try {
      calData = await invoke<typeof calData>("get_user_monthly_sessions", {
        userId: selected.user_id,
        yearMonth: ym,
      });
    } catch (_) { calData = {}; }
    calLoading = false;
  }

  $: if (activeTab === "calendar") loadCalendar();

  function prevMonth() {
    if (calMonth === 0) { calMonth = 11; calYear--; } else calMonth--;
    loadCalendar();
  }
  function nextMonth() {
    if (calMonth === 11) { calMonth = 0; calYear++; } else calMonth++;
    loadCalendar();
  }

  // Build calendar grid for the current calYear/calMonth
  $: calGrid = (() => {
    const firstDay = new Date(calYear, calMonth, 1).getDay(); // 0=Sun
    const daysInMonth = new Date(calYear, calMonth + 1, 0).getDate();
    const cells: Array<{ day: number | null; date: string | null }> = [];
    for (let i = 0; i < firstDay; i++) cells.push({ day: null, date: null });
    for (let d = 1; d <= daysInMonth; d++) {
      const date = `${calYear}-${String(calMonth + 1).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
      cells.push({ day: d, date });
    }
    return cells;
  })();

  const MONTH_NAMES = ["January","February","March","April","May","June","July","August","September","October","November","December"];
  const today = new Date().toISOString().slice(0, 10);

  function workIntensity(date: string | null): number {
    if (!date) return 0;
    const d = calData[date];
    if (!d) return 0;
    // 0–1 scale, 8h = 1.0
    return Math.min(d.total_work_seconds / 28800, 1);
  }

  function cellColor(date: string | null): string {
    const i = workIntensity(date);
    if (i === 0) return "transparent";
    const alpha = 0.2 + i * 0.8;
    return `hsl(var(--success) / ${alpha.toFixed(2)})`;
  }

  // ── Chart ────────────────────────────────────────────────────────────────────
  const CHART_W = 560; const CHART_H = 80; const PAD_B = 20;
  $: maxKS = Math.max(1, ...snapshots.map(s => s.keystrokes));
  $: chartBars = snapshots.map((s, i) => {
    const bw = Math.max(1, CHART_W / Math.max(snapshots.length, 1));
    const h = Math.max(1, Math.round((s.keystrokes / maxKS) * (CHART_H - PAD_B)));
    return { x: i * bw, y: CHART_H - PAD_B - h, w: Math.max(1, bw - 1), h, idle: s.idle_seconds > 25 };
  });
  $: xLabels = (() => {
    if (snapshots.length < 2) return [];
    const step = Math.ceil(snapshots.length / 5);
    return snapshots
      .filter((_, i) => i % step === 0)
      .map((s, j) => ({ x: j * step * (CHART_W / snapshots.length) + (CHART_W / snapshots.length) / 2, label: fmtTime(s.timestamp) }));
  })();

  // ── App usage ────────────────────────────────────────────────────────────────
  $: appUsage = (() => {
    const m = new Map<string, number>();
    for (const s of snapshots) if (s.active_app) m.set(s.active_app, (m.get(s.active_app) ?? 0) + 30);
    return [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, 6);
  })();
  $: totalAppSecs = appUsage.reduce((s, [, v]) => s + v, 0) || 1;

  // ── Network ──────────────────────────────────────────────────────────────────
  $: domainGroups = (() => {
    const m = new Map<string, number>();
    for (const c of netConns) { const h = c.remote_host || c.remote_ip; m.set(h, (m.get(h) ?? 0) + 1); }
    return [...m.entries()].sort((a, b) => b[1] - a[1]);
  })();
  $: filteredNet = netConns.filter(c =>
    !netFilter ||
    (c.remote_host || c.remote_ip).toLowerCase().includes(netFilter.toLowerCase()) ||
    c.process_name.toLowerCase().includes(netFilter.toLowerCase())
  ).slice().reverse();

  $: netPageCount = Math.max(1, Math.ceil(filteredNet.length / NET_PAGE_SIZE));
  $: netPageClamped = Math.min(netPage, netPageCount - 1);
  $: netPageSlice = filteredNet.slice(netPageClamped * NET_PAGE_SIZE, (netPageClamped + 1) * NET_PAGE_SIZE);

  // Reset to page 0 when filter changes
  $: { netFilter; netPage = 0; }

  function downloadCSV(data: NetworkConnection[], name: string) {
    const headers = ["Timestamp","Process","Host","IP","Remote Port","Local Port"];
    const rows = data.map(c => [
      new Date(c.timestamp).toISOString(),
      c.process_name,
      c.remote_host || c.remote_ip,
      c.remote_ip,
      c.remote_port,
      c.local_port,
    ]);
    const csv = [headers, ...rows].map(r => r.map(v => `"${String(v).replace(/"/g, '""')}"`).join(",")).join("\r\n");
    const blob = new Blob([csv], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url; a.download = name; a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function downloadActivityCSV(data: ActivitySnapshot[], name: string) {
    const headers = ["Timestamp","Keystrokes","Mouse Clicks","Mouse Distance","Idle Seconds","Active App","Active Window"];
    const rows = data.map(s => [
      new Date(s.timestamp).toISOString(),
      s.keystrokes, s.mouse_clicks, Math.round(s.mouse_distance_px),
      s.idle_seconds, s.active_app, s.active_window,
    ]);
    const csv = [headers, ...rows].map(r => r.map(v => `"${String(v).replace(/"/g, '""')}"`).join(",")).join("\r\n");
    const blob = new Blob([csv], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url; a.download = name; a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  // ── Summary ──────────────────────────────────────────────────────────────────
  $: totalKeys = snapshots.reduce((s, x) => s + x.keystrokes, 0);
  $: totalClicks = snapshots.reduce((s, x) => s + x.mouse_clicks, 0);
  $: idlePct = snapshots.length > 0 ? Math.round((snapshots.filter(s => s.idle_seconds > 25).length / snapshots.length) * 100) : 0;

  function elapsed(ci: string) { return formatDuration(Math.floor((Date.now() - new Date(ci).getTime()) / 1000)); }
  function displayName(m: TeamMember) { return m.user_name || m.user_email.split("@")[0]; }
  function fmtTime(ts: string) { return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }); }
</script>

<!-- ═══════════════════════════ TEAM LIST ═══════════════════════════════════ -->
{#if !selected}
<div class="admin">
  <header>
    <button class="app-button app-button-ghost app-button-sm back-btn" on:click={() => dispatch("back")}>← Back</button>
    <span class="text-title title">Team</span>
    <span class="app-badge app-badge-secondary count">{$teamStatus.length} active</span>
    <button class="app-button app-button-ghost app-button-icon icon-btn" on:click={refresh} class:spinning={loading}>↻</button>
  </header>
  <div class="body">
    {#if loading}
      <div class="text-small placeholder">Loading team…</div>
    {:else if $teamStatus.length === 0}
      <div class="text-small placeholder">No one is clocked in right now.</div>
    {:else}
      {#each $teamStatus as member}
        <button class="app-card app-card-compact member-card" on:click={() => selectMember(member)}>
          <div class="member-top">
            <div class="dot" class:on-break={member.status === "on_break"}></div>
            <div class="minfo">
              <span class="text-body mname">{displayName(member)}</span>
              <span class="text-caption memail">{member.user_email}</span>
            </div>
            <div class="mright">
              {#if member.status === "on_break"}<span class="app-badge app-badge-warning">On Break</span>{:else}<span class="app-badge app-badge-success">Active</span>{/if}
              <span class="chevron">›</span>
            </div>
          </div>
          <div class="mstats">
            <div class="ms"><span class="msv">{elapsed(member.clock_in)}</span><span class="msl">clocked in</span></div>
            <div class="ms"><span class="msv">{member.break_count}</span><span class="msl">breaks</span></div>
            {#if member.total_break_seconds > 0}
              <div class="ms"><span class="msv">{formatDuration(member.total_break_seconds)}</span><span class="msl">break time</span></div>
            {/if}
          </div>
          {#if member.active_app}
            <div class="mapp"><span class="adot">▶</span>{member.active_app}</div>
          {/if}
        </button>
      {/each}
    {/if}
  </div>
</div>

<!-- ═══════════════════════════ MEMBER DETAIL ════════════════════════════════ -->
{:else}
<div class="admin">
  <header>
    <button class="app-button app-button-ghost app-button-sm back-btn" on:click={back}>← Team</button>
    <div class="dtitle">
      <span class="text-title mname">{displayName(selected)}</span>
      {#if selected.status === "on_break"}<span class="app-badge app-badge-warning">On Break</span>{:else}<span class="app-badge app-badge-success">Active</span>{/if}
    </div>
    <button class="app-button app-button-ghost app-button-icon icon-btn" on:click={() => selectMember(selected!)} class:spinning={detailLoading}>↻</button>
  </header>

  <div class="body">
    <!-- Summary row -->
    <div class="sum-row">
      <div class="app-card sum-card"><span class="text-title sumv">{elapsed(selected.clock_in)}</span><span class="text-caption suml">Elapsed</span></div>
      <div class="app-card sum-card"><span class="text-title sumv">{selected.break_count}</span><span class="text-caption suml">Breaks</span></div>
      <div class="app-card sum-card"><span class="text-title sumv">{totalKeys.toLocaleString()}</span><span class="text-caption suml">Keystrokes</span></div>
      <div class="app-card sum-card"><span class="text-title sumv">{totalClicks.toLocaleString()}</span><span class="text-caption suml">Clicks</span></div>
      <div class="app-card sum-card"><span class="text-title sumv">{idlePct}%</span><span class="text-caption suml">Idle</span></div>
    </div>

    <!-- Tabs -->
    <div class="tabs">
      <button class="tab" class:active={activeTab === "activity"} on:click={() => (activeTab = "activity")}>Activity</button>
      <button class="tab" class:active={activeTab === "network"} on:click={() => (activeTab = "network")}>
        Network {#if netConns.length > 0}<span class="tc">{netConns.length}</span>{/if}
      </button>
      <button class="tab" class:active={activeTab === "calendar"} on:click={() => (activeTab = "calendar")}>Calendar</button>
    </div>

    {#if detailLoading}
      <div class="text-small placeholder">Loading data…</div>

    <!-- ─── ACTIVITY TAB ─── -->
    {:else if activeTab === "activity"}

      <!-- Keystroke chart -->
      <div class="app-card app-card-compact card">
        <div class="app-card-title">
          Keystroke Timeline <span class="app-card-description csub">{snapshots.length} snapshots · 30s each</span>
          {#if snapshots.length > 0}
            <button class="app-button app-button-info-outline app-button-sm csv-btn" style="margin-left:auto"
              on:click={() => downloadActivityCSV(snapshots, `activity-${selected?.user_email}-${new Date().toISOString().slice(0,10)}.csv`)}>
              ↓ CSV
            </button>
          {/if}
        </div>
        {#if snapshots.length === 0}
          <div class="text-small nodata">No activity snapshots yet for this session.</div>
        {:else}
          <div class="chart-wrap">
            <svg viewBox="0 0 {CHART_W} {CHART_H}" preserveAspectRatio="none" class="chart-svg">
              {#each chartBars as bar}
                <rect x={bar.x} y={bar.y} width={bar.w} height={bar.h} fill={bar.idle ? "hsl(var(--muted))" : "hsl(var(--success))"} rx="1"/>
              {/each}
              <line x1="0" y1={CHART_H - PAD_B} x2={CHART_W} y2={CHART_H - PAD_B} stroke="hsl(var(--border))" stroke-width="1"/>
            </svg>
            <div class="xlabels">
              {#each xLabels as l}<span class="xlabel" style="left:{(l.x/CHART_W*100).toFixed(1)}%">{l.label}</span>{/each}
            </div>
          </div>
          <div class="legend"><span class="ldot success-dot"></span>Active <span class="ldot idle-dot"></span>Idle</div>
        {/if}
      </div>

      <!-- App usage -->
      {#if appUsage.length > 0}
        <div class="app-card app-card-compact card">
          <div class="app-card-title">App Usage <span class="app-card-description csub">today's session</span></div>
          <div class="app-list">
            {#each appUsage as [app, secs]}
              <div class="arow">
                <span class="aname">{app}</span>
                <div class="bwrap"><div class="bfill" style="width:{Math.round(secs/totalAppSecs*100)}%"></div></div>
                <span class="adur">{formatDuration(secs)}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Snapshot table -->
      {#if snapshots.length > 0}
        <div class="app-card app-card-compact card">
          <div class="app-card-title">Recent Activity <span class="app-card-description csub">last 10</span></div>
          <div class="snap-table">
            <div class="snap-head"><span>Time</span><span>Keys</span><span>Clicks</span><span>Idle</span><span>App</span></div>
            {#each snapshots.slice(-10).reverse() as s}
              <div class="snap-row" class:idle-row={s.idle_seconds > 25}>
                <span class="ftime">{fmtTime(s.timestamp)}</span>
                <span>{s.keystrokes}</span>
                <span>{s.mouse_clicks}</span>
                <span>{s.idle_seconds}s</span>
                <span class="sapp" title={s.active_window}>{s.active_app || "—"}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

    <!-- ─── NETWORK TAB ─── -->
    {:else if activeTab === "network"}

      {#if domainGroups.length > 0}
        <div class="app-card app-card-compact card">
          <div class="app-card-title">Top Domains <span class="app-card-description csub">{domainGroups.length} unique</span></div>
          <div class="app-list">
            {#each domainGroups.slice(0, 10) as [host, count]}
              <div class="arow">
                <span class="aname">{host}</span>
                <div class="bwrap"><div class="bfill dbfill" style="width:{Math.max(Math.round(count/netConns.length*100), 2)}%"></div></div>
                <span class="adur">{count}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <div class="app-card app-card-compact card">
        <div class="app-card-title">
          Connection Log
          <span class="app-badge app-badge-secondary net-total">{filteredNet.length} records</span>
          <input class="app-input app-input-sm nfilter" placeholder="filter host / process…" bind:value={netFilter}/>
          <button class="app-button app-button-info-outline app-button-sm csv-btn" on:click={() => downloadCSV(filteredNet, `network-${selected?.user_email}-${new Date().toISOString().slice(0,10)}.csv`)}
            title="Download CSV" disabled={netConns.length === 0}>
            ↓ CSV
          </button>
        </div>
        {#if netConns.length === 0}
          <div class="text-small nodata">No network connections recorded for this session.</div>
        {:else}
          <div class="net-table">
            <div class="net-head"><span>Time</span><span>Process</span><span>Host / Service</span><span>Port</span></div>
            {#each netPageSlice as c}
              <div class="net-row">
                <span class="ftime">{fmtTime(c.timestamp)}</span>
                <span class="nproc">{c.process_name || "?"}</span>
                <span class="nhost" title={c.remote_ip}>{c.remote_host || c.remote_ip}</span>
                <span class="nport">:{c.remote_port}</span>
              </div>
            {/each}
          </div>

          <!-- Pagination -->
          <div class="pagination">
            <button class="app-button app-button-outline app-button-icon pg-btn" on:click={() => (netPage = 0)} disabled={netPageClamped === 0}>«</button>
            <button class="app-button app-button-outline app-button-icon pg-btn" on:click={() => (netPage = Math.max(0, netPageClamped - 1))} disabled={netPageClamped === 0}>‹</button>
            <span class="pg-info">Page {netPageClamped + 1} of {netPageCount} &nbsp;·&nbsp; {filteredNet.length} total</span>
            <button class="app-button app-button-outline app-button-icon pg-btn" on:click={() => (netPage = Math.min(netPageCount - 1, netPageClamped + 1))} disabled={netPageClamped >= netPageCount - 1}>›</button>
            <button class="app-button app-button-outline app-button-icon pg-btn" on:click={() => (netPage = netPageCount - 1)} disabled={netPageClamped >= netPageCount - 1}>»</button>
          </div>
        {/if}
      </div>

    <!-- ─── CALENDAR TAB ─── -->
    {:else}
      <div class="app-card app-card-compact card">
        <div class="cal-header">
          <button class="app-button app-button-outline app-button-icon cal-nav" on:click={prevMonth}>‹</button>
          <span class="cal-month">{MONTH_NAMES[calMonth]} {calYear}</span>
          <button class="app-button app-button-outline app-button-icon cal-nav" on:click={nextMonth} disabled={calYear === new Date().getFullYear() && calMonth === new Date().getMonth()}>›</button>
        </div>

        {#if calLoading}
          <div class="text-small nodata">Loading…</div>
        {:else}
          <!-- Day-of-week headers -->
          <div class="cal-grid">
            {#each ["Su","Mo","Tu","We","Th","Fr","Sa"] as d}
              <div class="cal-dow">{d}</div>
            {/each}

            {#each calGrid as cell}
              <div
                class="cal-cell"
                class:cal-today={cell.date === today}
                class:cal-empty={!cell.day}
                class:cal-worked={cell.date && calData[cell.date]}
                style="background: {cellColor(cell.date)}"
                title={cell.date && calData[cell.date]
                  ? `${calData[cell.date].session_count} session(s) · ${formatDuration(calData[cell.date].total_work_seconds)} worked`
                  : ""}
              >
                {#if cell.day}
                  <span class="cal-day-num">{cell.day}</span>
                  {#if cell.date && calData[cell.date]}
                    <span class="cal-dur">{formatDuration(calData[cell.date].total_work_seconds)}</span>
                  {/if}
                {/if}
              </div>
            {/each}
          </div>

          <!-- Legend -->
          <div class="cal-legend">
            <div class="cal-leg-grad"></div>
            <div class="cal-leg-labels"><span>0h</span><span>4h</span><span>8h+</span></div>
          </div>

          <!-- Monthly summary -->
          {#if Object.keys(calData).length > 0}
            {@const monthDays = Object.values(calData)}
            {@const totalDays = monthDays.length}
            {@const totalSecs = monthDays.reduce((s, d) => s + d.total_work_seconds, 0)}
            <div class="cal-summary">
              <div class="app-card sum-card sm"><span class="text-title sumv">{totalDays}</span><span class="text-caption suml">Days worked</span></div>
              <div class="app-card sum-card sm"><span class="text-title sumv">{formatDuration(totalSecs)}</span><span class="text-caption suml">Total time</span></div>
              <div class="app-card sum-card sm"><span class="text-title sumv">{formatDuration(Math.round(totalSecs / totalDays))}</span><span class="text-caption suml">Avg / day</span></div>
            </div>
          {/if}
        {/if}
      </div>
    {/if}
  </div>
</div>
{/if}

<style>
  .admin { display: flex; flex-direction: column; height: 100vh; padding: 0 12px 12px; }
  header { display: flex; align-items: center; gap: 8px; padding: 12px 4px; border-bottom: 1px solid hsl(var(--border) / 0.72); flex-shrink: 0; }
  .title { flex: 1; }
  .dtitle { flex: 1; display: flex; align-items: center; gap: 8px; min-width: 0; }
  .dtitle .mname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .icon-btn { font-size: 16px; }
  .spinning { animation: spin 0.6s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .body { flex: 1; overflow-y: auto; padding: 12px 0 0; display: flex; flex-direction: column; gap: 10px; }
  .body::-webkit-scrollbar { width: 3px; }
  .body::-webkit-scrollbar-thumb { background: hsl(var(--border)); border-radius: 2px; }
  .placeholder { text-align: center; margin-top: 40px; }

  /* ── Team list ── */
  .member-card { display: flex; flex-direction: column; gap: 8px; cursor: pointer; text-align: left; width: 100%; transition: border-color 0.15s; }
  .member-card:hover { border-color: hsl(var(--ring) / 0.85); box-shadow: var(--shadow-md); }
  .member-top { display: flex; align-items: center; gap: 10px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: hsl(var(--success)); flex-shrink: 0; }
  .dot.on-break { background: hsl(var(--warning)); }
  .minfo { flex: 1; min-width: 0; }
  .mname { display: block; }
  .memail { display: block; margin-top: 1px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mright { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }
  .chevron { color: hsl(var(--muted-foreground) / 0.6); font-size: 18px; }
  .mstats { display: flex; gap: 14px; padding-top: 2px; }
  .ms { display: flex; flex-direction: column; }
  .msv { font-size: 14px; font-weight: 600; color: hsl(var(--secondary-foreground)); font-variant-numeric: tabular-nums; }
  .msl { font-size: 9px; color: hsl(var(--muted-foreground)); text-transform: uppercase; letter-spacing: 0.4px; }
  .mapp { font-size: 11px; color: hsl(var(--muted-foreground)); border-top: 1px solid hsl(var(--border) / 0.72); padding-top: 8px; display: flex; align-items: center; gap: 6px; }
  .adot { color: hsl(var(--success)); font-size: 8px; }

  /* ── Detail summary ── */
  .sum-row { display: flex; gap: 8px; flex-wrap: wrap; }
  .sum-card { flex: 1; min-width: 56px; padding: 10px; display: flex; flex-direction: column; align-items: center; }
  .sum-card.sm { padding: 7px 8px; }
  .sumv { font-variant-numeric: tabular-nums; }
  .suml { font-size: 8px; letter-spacing: 0.4px; margin-top: 2px; text-align: center; }

  /* ── Tabs ── */
  .tabs { display: flex; gap: 4px; background: hsl(var(--background) / 0.58); border: 1px solid hsl(var(--border) / 0.72); border-radius: var(--radius); padding: 4px; flex-shrink: 0; }
  .tab { flex: 1; background: none; border: none; color: hsl(var(--muted-foreground)); font-size: 12px; font-weight: 650; padding: 7px 4px; border-radius: var(--radius-md); cursor: pointer; display: flex; align-items: center; justify-content: center; gap: 4px; transition: background-color 0.15s, color 0.15s; }
  .tab.active { background: hsl(var(--accent)); color: hsl(var(--accent-foreground)); box-shadow: var(--shadow-sm); }
  .tc { background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); font-size: 10px; padding: 1px 5px; border-radius: var(--radius); }

  /* ── Cards ── */
  .csub { font-weight: 400; text-transform: none; letter-spacing: 0; }
  .nodata { text-align: center; padding: 20px 0; }

  /* ── Chart ── */
  .chart-wrap { position: relative; margin-bottom: 4px; }
  .chart-svg { width: 100%; height: 80px; display: block; }
  .xlabels { position: relative; height: 18px; }
  .xlabel { position: absolute; transform: translateX(-50%); font-size: 9px; color: hsl(var(--muted-foreground)); white-space: nowrap; }
  .legend { display: flex; align-items: center; gap: 5px; font-size: 10px; color: hsl(var(--muted-foreground)); margin-top: 4px; }
  .ldot { display: inline-block; width: 8px; height: 8px; border-radius: 2px; }
  .success-dot { background: hsl(var(--success)); }
  .idle-dot { background: hsl(var(--muted)); border: 1px solid hsl(var(--border)); margin-left: 8px; }

  /* ── App bars ── */
  .app-list { display: flex; flex-direction: column; gap: 7px; }
  .arow { display: flex; align-items: center; gap: 8px; }
  .aname { font-size: 11px; color: hsl(var(--secondary-foreground)); min-width: 90px; max-width: 90px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bwrap { flex: 1; height: 7px; background: hsl(var(--background) / 0.7); border-radius: 999px; overflow: hidden; }
  .bfill { height: 100%; background: hsl(var(--success)); border-radius: 999px; min-width: 2px; }
  .dbfill { background: hsl(var(--info)); }
  .adur { font-size: 10px; color: hsl(var(--muted-foreground)); min-width: 32px; text-align: right; font-variant-numeric: tabular-nums; }

  /* ── Snapshot table ── */
  .snap-table { font-size: 11px; }
  .snap-head { display: grid; grid-template-columns: 50px 42px 42px 42px 1fr; color: hsl(var(--muted-foreground)); font-size: 9px; text-transform: uppercase; letter-spacing: 0.4px; padding: 0 0 6px; border-bottom: 1px solid hsl(var(--border) / 0.72); gap: 6px; }
  .snap-row { display: grid; grid-template-columns: 50px 42px 42px 42px 1fr; padding: 6px 0; border-bottom: 1px solid hsl(var(--background) / 0.7); gap: 6px; align-items: center; color: hsl(var(--secondary-foreground)); }
  .snap-row.idle-row { color: hsl(var(--muted-foreground)); }
  .ftime { font-variant-numeric: tabular-nums; color: hsl(var(--muted-foreground)); }
  .sapp { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  /* ── Network ── */
  .nfilter { margin-left: auto; width: 160px; }
  .net-table { font-size: 11px; }
  .net-table::-webkit-scrollbar { width: 2px; }
  .net-table::-webkit-scrollbar-thumb { background: hsl(var(--border)); }
  .net-head { display: grid; grid-template-columns: 46px 80px 1fr 44px; color: hsl(var(--muted-foreground)); font-size: 9px; text-transform: uppercase; letter-spacing: 0.4px; padding: 0 0 6px; border-bottom: 1px solid hsl(var(--border) / 0.72); gap: 6px; position: sticky; top: 0; background: hsl(var(--card)); }
  .net-row { display: grid; grid-template-columns: 46px 80px 1fr 44px; padding: 6px 0; border-bottom: 1px solid hsl(var(--background) / 0.7); gap: 6px; align-items: center; }
  .nproc { color: hsl(var(--muted-foreground)); font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nhost { color: hsl(var(--secondary-foreground)); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nport { color: hsl(var(--muted-foreground)); font-variant-numeric: tabular-nums; }
  /* ── Calendar ── */
  .cal-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px; }
  .cal-month { font-size: 13px; font-weight: 700; color: hsl(var(--secondary-foreground)); }
  .cal-nav { font-size: 16px; width: 28px; height: 28px; }
  .cal-nav:disabled { opacity: 0.3; cursor: not-allowed; }
  .cal-grid { display: grid; grid-template-columns: repeat(7, 1fr); gap: 4px; }
  .cal-dow { text-align: center; font-size: 9px; font-weight: 600; color: hsl(var(--muted-foreground)); text-transform: uppercase; padding: 4px 0; }
  .cal-cell {
    aspect-ratio: 1;
    border-radius: var(--radius-md);
    display: flex; flex-direction: column; align-items: center; justify-content: center;
    cursor: default; position: relative;
    border: 1px solid transparent;
    transition: border-color 0.1s;
  }
  .cal-cell.cal-empty { }
  .cal-cell.cal-worked { cursor: default; }
  .cal-cell.cal-today { border-color: hsl(var(--success)) !important; }
  .cal-day-num { font-size: 11px; font-weight: 600; color: hsl(var(--secondary-foreground)); line-height: 1; }
  .cal-cell.cal-worked .cal-day-num { color: hsl(var(--foreground)); }
  .cal-cell.cal-today .cal-day-num { color: hsl(var(--success)); }
  .cal-dur { font-size: 7px; color: hsl(var(--foreground) / 0.5); margin-top: 1px; line-height: 1; }
  .cal-legend { margin-top: 10px; display: flex; flex-direction: column; gap: 2px; }
  .cal-leg-grad { height: 6px; border-radius: 3px; background: linear-gradient(to right, hsl(var(--background)), hsl(var(--success) / 0.3), hsl(var(--success))); }
  .cal-leg-labels { display: flex; justify-content: space-between; font-size: 9px; color: hsl(var(--muted-foreground)); }
  .cal-summary { display: flex; gap: 6px; margin-top: 10px; }

  /* ── Pagination ── */
  .pagination { display: flex; align-items: center; gap: 4px; padding-top: 10px; border-top: 1px solid hsl(var(--border) / 0.72); }
  .pg-btn { font-size: 13px; width: 28px; height: 26px; }
  .pg-btn:disabled { opacity: 0.3; cursor: not-allowed; }
  .pg-info { flex: 1; text-align: center; font-size: 10px; color: hsl(var(--muted-foreground)); }

  /* ── CSV button ── */
  .csv-btn { white-space: nowrap; }
  .csv-btn:disabled { opacity: 0.3; cursor: not-allowed; }

  /* ── Network total badge ── */
  .net-total { font-weight: 400; text-transform: none; letter-spacing: 0; }
</style>
