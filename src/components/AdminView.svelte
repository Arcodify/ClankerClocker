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
    // lerp from #0f2d0f (low) to #22c55e (full day)
    const alpha = 0.2 + i * 0.8;
    return `rgba(34, 197, 94, ${alpha.toFixed(2)})`;
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
    <button class="back-btn" on:click={() => dispatch("back")}>← Back</button>
    <span class="title">Team</span>
    <span class="count">{$teamStatus.length} active</span>
    <button class="icon-btn" on:click={refresh} class:spinning={loading}>↻</button>
  </header>
  <div class="body">
    {#if loading}
      <div class="placeholder">Loading team…</div>
    {:else if $teamStatus.length === 0}
      <div class="placeholder">No one is clocked in right now.</div>
    {:else}
      {#each $teamStatus as member}
        <button class="member-card" on:click={() => selectMember(member)}>
          <div class="member-top">
            <div class="dot" class:on-break={member.status === "on_break"}></div>
            <div class="minfo">
              <span class="mname">{displayName(member)}</span>
              <span class="memail">{member.user_email}</span>
            </div>
            <div class="mright">
              {#if member.status === "on_break"}<span class="badge-break">On Break</span>{:else}<span class="badge-active">Active</span>{/if}
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
    <button class="back-btn" on:click={back}>← Team</button>
    <div class="dtitle">
      <span class="mname">{displayName(selected)}</span>
      {#if selected.status === "on_break"}<span class="badge-break">On Break</span>{:else}<span class="badge-active">Active</span>{/if}
    </div>
    <button class="icon-btn" on:click={() => selectMember(selected!)} class:spinning={detailLoading}>↻</button>
  </header>

  <div class="body">
    <!-- Summary row -->
    <div class="sum-row">
      <div class="sum-card"><span class="sumv">{elapsed(selected.clock_in)}</span><span class="suml">Elapsed</span></div>
      <div class="sum-card"><span class="sumv">{selected.break_count}</span><span class="suml">Breaks</span></div>
      <div class="sum-card"><span class="sumv">{totalKeys.toLocaleString()}</span><span class="suml">Keystrokes</span></div>
      <div class="sum-card"><span class="sumv">{totalClicks.toLocaleString()}</span><span class="suml">Clicks</span></div>
      <div class="sum-card"><span class="sumv">{idlePct}%</span><span class="suml">Idle</span></div>
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
      <div class="placeholder">Loading data…</div>

    <!-- ─── ACTIVITY TAB ─── -->
    {:else if activeTab === "activity"}

      <!-- Keystroke chart -->
      <div class="card">
        <div class="card-title">
          Keystroke Timeline <span class="csub">{snapshots.length} snapshots · 30s each</span>
          {#if snapshots.length > 0}
            <button class="csv-btn" style="margin-left:auto"
              on:click={() => downloadActivityCSV(snapshots, `activity-${selected?.user_email}-${new Date().toISOString().slice(0,10)}.csv`)}>
              ↓ CSV
            </button>
          {/if}
        </div>
        {#if snapshots.length === 0}
          <div class="nodata">No activity snapshots yet for this session.</div>
        {:else}
          <div class="chart-wrap">
            <svg viewBox="0 0 {CHART_W} {CHART_H}" preserveAspectRatio="none" class="chart-svg">
              {#each chartBars as bar}
                <rect x={bar.x} y={bar.y} width={bar.w} height={bar.h} fill={bar.idle ? "#1e1e2a" : "#22c55e"} rx="1"/>
              {/each}
              <line x1="0" y1={CHART_H - PAD_B} x2={CHART_W} y2={CHART_H - PAD_B} stroke="#1e1e2a" stroke-width="1"/>
            </svg>
            <div class="xlabels">
              {#each xLabels as l}<span class="xlabel" style="left:{(l.x/CHART_W*100).toFixed(1)}%">{l.label}</span>{/each}
            </div>
          </div>
          <div class="legend"><span class="ldot" style="background:#22c55e"></span>Active <span class="ldot" style="background:#1e1e2a;border:1px solid #2a2a38;margin-left:8px"></span>Idle</div>
        {/if}
      </div>

      <!-- App usage -->
      {#if appUsage.length > 0}
        <div class="card">
          <div class="card-title">App Usage <span class="csub">today's session</span></div>
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
        <div class="card">
          <div class="card-title">Recent Activity <span class="csub">last 10</span></div>
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
        <div class="card">
          <div class="card-title">Top Domains <span class="csub">{domainGroups.length} unique</span></div>
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

      <div class="card">
        <div class="card-title">
          Connection Log
          <span class="net-total">{filteredNet.length} records</span>
          <input class="nfilter" placeholder="filter host / process…" bind:value={netFilter}/>
          <button class="csv-btn" on:click={() => downloadCSV(filteredNet, `network-${selected?.user_email}-${new Date().toISOString().slice(0,10)}.csv`)}
            title="Download CSV" disabled={netConns.length === 0}>
            ↓ CSV
          </button>
        </div>
        {#if netConns.length === 0}
          <div class="nodata">No network connections recorded for this session.</div>
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
            <button class="pg-btn" on:click={() => (netPage = 0)} disabled={netPageClamped === 0}>«</button>
            <button class="pg-btn" on:click={() => (netPage = Math.max(0, netPageClamped - 1))} disabled={netPageClamped === 0}>‹</button>
            <span class="pg-info">Page {netPageClamped + 1} of {netPageCount} &nbsp;·&nbsp; {filteredNet.length} total</span>
            <button class="pg-btn" on:click={() => (netPage = Math.min(netPageCount - 1, netPageClamped + 1))} disabled={netPageClamped >= netPageCount - 1}>›</button>
            <button class="pg-btn" on:click={() => (netPage = netPageCount - 1)} disabled={netPageClamped >= netPageCount - 1}>»</button>
          </div>
        {/if}
      </div>

    <!-- ─── CALENDAR TAB ─── -->
    {:else}
      <div class="card">
        <div class="cal-header">
          <button class="cal-nav" on:click={prevMonth}>‹</button>
          <span class="cal-month">{MONTH_NAMES[calMonth]} {calYear}</span>
          <button class="cal-nav" on:click={nextMonth} disabled={calYear === new Date().getFullYear() && calMonth === new Date().getMonth()}>›</button>
        </div>

        {#if calLoading}
          <div class="nodata">Loading…</div>
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
              <div class="sum-card sm"><span class="sumv">{totalDays}</span><span class="suml">Days worked</span></div>
              <div class="sum-card sm"><span class="sumv">{formatDuration(totalSecs)}</span><span class="suml">Total time</span></div>
              <div class="sum-card sm"><span class="sumv">{formatDuration(Math.round(totalSecs / totalDays))}</span><span class="suml">Avg / day</span></div>
            </div>
          {/if}
        {/if}
      </div>
    {/if}
  </div>
</div>
{/if}

<style>
  .admin { display: flex; flex-direction: column; height: 100vh; }
  header { display: flex; align-items: center; gap: 8px; padding: 12px 16px; border-bottom: 1px solid #16161e; flex-shrink: 0; }
  .back-btn { background: none; border: none; color: #5a5a72; font-size: 13px; cursor: pointer; padding: 4px 8px; border-radius: 4px; }
  .back-btn:hover { color: #c0c0d0; background: #1e1e2a; }
  .title { flex: 1; font-size: 14px; font-weight: 600; color: #c0c0d0; }
  .dtitle { flex: 1; display: flex; align-items: center; gap: 8px; min-width: 0; }
  .dtitle .mname { font-size: 14px; font-weight: 600; color: #d8d8ec; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { font-size: 11px; color: #4a4a62; background: #1a1a22; padding: 2px 8px; border-radius: 10px; }
  .icon-btn { background: none; border: none; color: #5a5a72; font-size: 16px; cursor: pointer; padding: 3px 6px; border-radius: 4px; }
  .icon-btn:hover { color: #c0c0d0; }
  .spinning { animation: spin 0.6s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .body { flex: 1; overflow-y: auto; padding: 10px 12px; display: flex; flex-direction: column; gap: 8px; }
  .body::-webkit-scrollbar { width: 3px; }
  .body::-webkit-scrollbar-thumb { background: #2a2a36; border-radius: 2px; }
  .placeholder { text-align: center; color: #4a4a62; font-size: 13px; margin-top: 40px; }

  /* ── Team list ── */
  .member-card { background: #111118; border: 1px solid #1a1a24; border-radius: 10px; padding: 12px 14px; display: flex; flex-direction: column; gap: 8px; cursor: pointer; text-align: left; width: 100%; transition: border-color 0.15s; }
  .member-card:hover { border-color: #2a2a3a; }
  .member-top { display: flex; align-items: center; gap: 10px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: #22c55e; flex-shrink: 0; }
  .dot.on-break { background: #f59e0b; }
  .minfo { flex: 1; min-width: 0; }
  .mname { display: block; font-size: 13px; font-weight: 600; color: #d8d8ec; }
  .memail { display: block; font-size: 10px; color: #4a4a62; margin-top: 1px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mright { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }
  .chevron { color: #2e2e42; font-size: 18px; }
  .mstats { display: flex; gap: 14px; }
  .ms { display: flex; flex-direction: column; }
  .msv { font-size: 14px; font-weight: 600; color: #c0c0d8; font-variant-numeric: tabular-nums; }
  .msl { font-size: 9px; color: #4a4a62; text-transform: uppercase; letter-spacing: 0.4px; }
  .mapp { font-size: 11px; color: #5a5a72; border-top: 1px solid #1a1a24; padding-top: 7px; display: flex; align-items: center; gap: 6px; }
  .adot { color: #22c55e; font-size: 8px; }
  .badge-active { font-size: 9px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.6px; color: #22c55e; background: #0a1f0a; border: 1px solid #1a3a1a; padding: 2px 6px; border-radius: 3px; white-space: nowrap; }
  .badge-break { font-size: 9px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.6px; color: #f59e0b; background: #1c1a10; border: 1px solid #3a2e00; padding: 2px 6px; border-radius: 3px; white-space: nowrap; }

  /* ── Detail summary ── */
  .sum-row { display: flex; gap: 6px; flex-wrap: wrap; }
  .sum-card { flex: 1; min-width: 56px; background: #111118; border: 1px solid #1a1a24; border-radius: 8px; padding: 8px 10px; display: flex; flex-direction: column; align-items: center; }
  .sum-card.sm { padding: 7px 8px; }
  .sumv { font-size: 14px; font-weight: 700; color: #d8d8ec; font-variant-numeric: tabular-nums; }
  .suml { font-size: 8px; color: #4a4a62; text-transform: uppercase; letter-spacing: 0.4px; margin-top: 2px; text-align: center; }

  /* ── Tabs ── */
  .tabs { display: flex; gap: 2px; background: #0e0e16; border-radius: 8px; padding: 3px; flex-shrink: 0; }
  .tab { flex: 1; background: none; border: none; color: #4a4a62; font-size: 12px; font-weight: 600; padding: 6px 4px; border-radius: 6px; cursor: pointer; display: flex; align-items: center; justify-content: center; gap: 4px; }
  .tab.active { background: #1a1a24; color: #c0c0d0; }
  .tc { background: #252532; color: #8080a0; font-size: 10px; padding: 1px 5px; border-radius: 8px; }

  /* ── Cards ── */
  .card { background: #111118; border: 1px solid #1a1a24; border-radius: 10px; padding: 12px 14px; }
  .card-title { font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.8px; color: #4a4a62; margin-bottom: 10px; display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .csub { color: #2e2e42; font-weight: 400; text-transform: none; letter-spacing: 0; }
  .nodata { font-size: 12px; color: #3a3a52; text-align: center; padding: 20px 0; }

  /* ── Chart ── */
  .chart-wrap { position: relative; margin-bottom: 4px; }
  .chart-svg { width: 100%; height: 80px; display: block; }
  .xlabels { position: relative; height: 18px; }
  .xlabel { position: absolute; transform: translateX(-50%); font-size: 9px; color: #3a3a52; white-space: nowrap; }
  .legend { display: flex; align-items: center; gap: 5px; font-size: 10px; color: #3a3a52; margin-top: 4px; }
  .ldot { display: inline-block; width: 8px; height: 8px; border-radius: 2px; }

  /* ── App bars ── */
  .app-list { display: flex; flex-direction: column; gap: 7px; }
  .arow { display: flex; align-items: center; gap: 8px; }
  .aname { font-size: 11px; color: #9090b0; min-width: 90px; max-width: 90px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bwrap { flex: 1; height: 6px; background: #0e0e16; border-radius: 3px; overflow: hidden; }
  .bfill { height: 100%; background: #22c55e; border-radius: 3px; min-width: 2px; }
  .dbfill { background: #3b82f6; }
  .adur { font-size: 10px; color: #4a4a62; min-width: 32px; text-align: right; font-variant-numeric: tabular-nums; }

  /* ── Snapshot table ── */
  .snap-table { font-size: 11px; }
  .snap-head { display: grid; grid-template-columns: 50px 42px 42px 42px 1fr; color: #3a3a52; font-size: 9px; text-transform: uppercase; letter-spacing: 0.4px; padding: 0 0 5px; border-bottom: 1px solid #1a1a24; gap: 6px; }
  .snap-row { display: grid; grid-template-columns: 50px 42px 42px 42px 1fr; padding: 5px 0; border-bottom: 1px solid #0e0e16; gap: 6px; align-items: center; color: #9090b0; }
  .snap-row.idle-row { color: #4a4a62; }
  .ftime { font-variant-numeric: tabular-nums; color: #5a5a72; }
  .sapp { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  /* ── Network ── */
  .nfilter { margin-left: auto; background: #0e0e16; border: 1px solid #1e1e2a; border-radius: 5px; padding: 3px 8px; font-size: 10px; color: #8080a0; outline: none; width: 160px; }
  .nfilter:focus { border-color: #3b82f6; }
  .net-table { font-size: 11px; max-height: 300px; overflow-y: auto; }
  .net-table::-webkit-scrollbar { width: 2px; }
  .net-table::-webkit-scrollbar-thumb { background: #2a2a36; }
  .net-head { display: grid; grid-template-columns: 46px 80px 1fr 44px; color: #3a3a52; font-size: 9px; text-transform: uppercase; letter-spacing: 0.4px; padding: 0 0 5px; border-bottom: 1px solid #1a1a24; gap: 6px; position: sticky; top: 0; background: #111118; }
  .net-row { display: grid; grid-template-columns: 46px 80px 1fr 44px; padding: 5px 0; border-bottom: 1px solid #0e0e16; gap: 6px; align-items: center; }
  .nproc { color: #7878a0; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nhost { color: #a0a0bc; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nport { color: #3a3a52; font-variant-numeric: tabular-nums; }
  /* ── Calendar ── */
  .cal-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px; }
  .cal-month { font-size: 13px; font-weight: 700; color: #c0c0d0; }
  .cal-nav { background: none; border: 1px solid #2a2a36; color: #7070909; color: #7070a0; font-size: 16px; width: 28px; height: 28px; border-radius: 6px; cursor: pointer; display: flex; align-items: center; justify-content: center; }
  .cal-nav:hover:not(:disabled) { border-color: #3a3a52; color: #c0c0d0; }
  .cal-nav:disabled { opacity: 0.3; cursor: not-allowed; }
  .cal-grid { display: grid; grid-template-columns: repeat(7, 1fr); gap: 3px; }
  .cal-dow { text-align: center; font-size: 9px; font-weight: 600; color: #3a3a52; text-transform: uppercase; padding: 4px 0; }
  .cal-cell {
    aspect-ratio: 1;
    border-radius: 6px;
    display: flex; flex-direction: column; align-items: center; justify-content: center;
    cursor: default; position: relative;
    border: 1px solid transparent;
    transition: border-color 0.1s;
  }
  .cal-cell.cal-empty { }
  .cal-cell.cal-worked { cursor: default; }
  .cal-cell.cal-today { border-color: #22c55e !important; }
  .cal-day-num { font-size: 11px; font-weight: 600; color: #8080a0; line-height: 1; }
  .cal-cell.cal-worked .cal-day-num { color: #d8d8ec; }
  .cal-cell.cal-today .cal-day-num { color: #22c55e; }
  .cal-dur { font-size: 7px; color: rgba(255,255,255,0.5); margin-top: 1px; line-height: 1; }
  .cal-legend { margin-top: 10px; display: flex; flex-direction: column; gap: 2px; }
  .cal-leg-grad { height: 6px; border-radius: 3px; background: linear-gradient(to right, #0e0e16, rgba(34,197,94,0.3), #22c55e); }
  .cal-leg-labels { display: flex; justify-content: space-between; font-size: 9px; color: #3a3a52; }
  .cal-summary { display: flex; gap: 6px; margin-top: 10px; }

  /* ── Pagination ── */
  .pagination { display: flex; align-items: center; gap: 4px; padding-top: 10px; border-top: 1px solid #1a1a24; }
  .pg-btn { background: #0e0e16; border: 1px solid #1e1e2a; color: #6060808; color: #6060a0; font-size: 13px; width: 28px; height: 26px; border-radius: 5px; cursor: pointer; display: flex; align-items: center; justify-content: center; }
  .pg-btn:hover:not(:disabled) { border-color: #3a3a52; color: #c0c0d0; }
  .pg-btn:disabled { opacity: 0.3; cursor: not-allowed; }
  .pg-info { flex: 1; text-align: center; font-size: 10px; color: #4a4a62; }

  /* ── CSV button ── */
  .csv-btn { background: #0e1a1a; border: 1px solid #1a3a3a; color: #3b82f6; font-size: 10px; font-weight: 700; padding: 3px 8px; border-radius: 4px; cursor: pointer; white-space: nowrap; }
  .csv-btn:hover:not(:disabled) { border-color: #3b82f6; }
  .csv-btn:disabled { opacity: 0.3; cursor: not-allowed; }

  /* ── Network total badge ── */
  .net-total { font-size: 9px; color: #3a3a52; background: #0e0e16; padding: 2px 6px; border-radius: 8px; font-weight: 400; text-transform: none; letter-spacing: 0; }
</style>
