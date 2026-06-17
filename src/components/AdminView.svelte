<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { LogicalSize } from "@tauri-apps/api/dpi";
  import { teamStatus, formatDuration } from "../lib/stores";
  import type {
    TeamMember, ActivitySnapshot, NetworkConnection, TodayBreakdown,
    UserInfo, SessionRecord, UserSummary, NetworkReport, ActivityReport,
  } from "../lib/types";

  const dispatch = createEventDispatcher();

  // ── Nepal time helpers (must come before any state that calls them) ─────────
  const NPT_OFFSET = (5 * 60 + 45) * 60 * 1000;

  function nepalToday(): string {
    return new Date(Date.now() + NPT_OFFSET).toISOString().slice(0, 10);
  }
  function nptDate(iso: string): string {
    return new Date(new Date(iso).getTime() + NPT_OFFSET).toISOString().slice(0, 10);
  }
  function nptTime(iso: string): string {
    return new Date(new Date(iso).getTime() + NPT_OFFSET).toISOString().slice(11, 16);
  }
  function nptDateTime(iso: string): string {
    return `${nptDate(iso)} ${nptTime(iso)}`;
  }

  // ── Window resize ──────────────────────────────────────────────────────────
  const ADMIN_W = 1050, ADMIN_H = 700, MAIN_W = 380, MAIN_H = 660;

  // ── Tab / filter state ─────────────────────────────────────────────────────
  type MainTab = "live" | "attendance" | "summary" | "activity" | "network";
  const MAIN_TABS: { id: MainTab; label: string }[] = [
    { id: "live", label: "Live" },
    { id: "attendance", label: "Attendance" },
    { id: "summary", label: "Summary" },
    { id: "activity", label: "Activity" },
    { id: "network", label: "Network" },
  ];
  const PRESETS: { id: "today" | "week" | "month" | "custom"; label: string }[] = [
    { id: "today", label: "Today" },
    { id: "week", label: "This Week" },
    { id: "month", label: "This Month" },
    { id: "custom", label: "Custom" },
  ];
  const LIVE_DETAIL_TABS: { id: "activity" | "network" | "breakdown"; label: string }[] = [
    { id: "activity", label: "Activity" },
    { id: "network", label: "Network" },
    { id: "breakdown", label: "Breakdown" },
  ];
  let activeTab: MainTab = "live";
  let filterPreset: "today" | "week" | "month" | "custom" = "today";
  let fromDate = nepalToday();
  let toDate = nepalToday();
  let selectedUserId = "";
  let allUsers: UserInfo[] = [];

  // ── Live tab ───────────────────────────────────────────────────────────────
  let teamMembers: TeamMember[] = [];
  let liveLoading = true;
  let liveInterval: ReturnType<typeof setInterval>;
  // per-member detail (live tab)
  let liveSelected: TeamMember | null = null;
  let liveSnapshots: ActivitySnapshot[] = [];
  let liveNetConns: NetworkConnection[] = [];
  let liveBreakdown: TodayBreakdown | null = null;
  let liveDetailTab: "activity" | "network" | "breakdown" = "activity";
  let liveDetailLoading = false;
  let liveNetFilter = "";
  let liveNetPage = 0;
  const LIVE_NET_PER_PAGE = 10;

  // ── Attendance tab ─────────────────────────────────────────────────────────
  interface DailyRow {
    user_id: string; user_name: string; user_email: string; date: string;
    first_clock_in: string; last_clock_out: string | null;
    gross_seconds: number; break_seconds: number; net_seconds: number;
    break_count: number; session_count: number; status: string;
  }
  let attendanceSessions: SessionRecord[] = [];
  let attendanceLoading = false;
  let attendanceSortKey: keyof SessionRecord = "clock_in";
  let attendanceSortAsc = false;
  let attendancePage = 0;
  let attendanceExpand = false;
  const ATT_PER_PAGE = 25;

  // ── Summary tab ────────────────────────────────────────────────────────────
  let summaryData: UserSummary[] = [];
  let summaryLoading = false;

  // ── Activity tab ───────────────────────────────────────────────────────────
  let activityReport: ActivityReport | null = null;
  let activityLoading = false;
  let activityUserId = "";

  // ── Network tab ────────────────────────────────────────────────────────────
  let networkReport: NetworkReport | null = null;
  let networkLoading = false;
  let networkFilter = "";
  let networkPage = 0;
  let networkViewMode: "stats" | "table" = "stats";
  const NET_PER_PAGE = 25;

  function setPreset(p: typeof filterPreset) {
    filterPreset = p;
    const now = new Date(Date.now() + NPT_OFFSET);
    if (p === "today") {
      fromDate = toDate = now.toISOString().slice(0, 10);
    } else if (p === "week") {
      const day = now.getUTCDay();
      const monday = new Date(now.getTime() - (day === 0 ? 6 : day - 1) * 86400000);
      fromDate = monday.toISOString().slice(0, 10);
      toDate = now.toISOString().slice(0, 10);
    } else if (p === "month") {
      fromDate = `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, "0")}-01`;
      toDate = now.toISOString().slice(0, 10);
    }
    if (p !== "custom") applyFilters();
  }

  // ── Mount / Destroy ────────────────────────────────────────────────────────
  onMount(async () => {
    try {
      const win = getCurrentWindow();
      await win.setSize(new LogicalSize(ADMIN_W, ADMIN_H));
    } catch (_) {}
    await loadUsers();
    await refreshLive();
    liveInterval = setInterval(refreshLive, 30_000);
  });

  onDestroy(async () => {
    clearInterval(liveInterval);
    try {
      const win = getCurrentWindow();
      await win.setSize(new LogicalSize(MAIN_W, MAIN_H));
    } catch (_) {}
  });

  // ── Data loaders ───────────────────────────────────────────────────────────
  async function loadUsers() {
    try { allUsers = await invoke<UserInfo[]>("get_all_users"); } catch (_) { allUsers = []; }
  }

  async function refreshLive() {
    try {
      teamMembers = await invoke<TeamMember[]>("get_team_status");
      teamStatus.set(teamMembers);
      if (liveSelected) {
        const updated = teamMembers.find(m => m.session_id === liveSelected!.session_id);
        if (updated) liveSelected = updated;
      }
    } catch (_) {}
    liveLoading = false;
  }

  async function selectLiveMember(m: TeamMember) {
    liveSelected = m;
    liveSnapshots = [];
    liveNetConns = [];
    liveBreakdown = null;
    liveDetailTab = "activity";
    liveDetailLoading = true;
    try {
      liveSnapshots = await invoke<ActivitySnapshot[]>("get_user_activity", { sessionId: m.session_id });
      liveBreakdown = await invoke<TodayBreakdown>("get_user_today_breakdown", { userId: m.user_id });
    } catch (_) {}
    liveDetailLoading = false;
  }

  async function loadLiveNetwork() {
    if (!liveSelected) return;
    liveDetailLoading = true;
    try { liveNetConns = await invoke<NetworkConnection[]>("get_user_network", { sessionId: liveSelected.session_id }); } catch (_) {}
    liveDetailLoading = false;
  }

  async function loadAttendance() {
    attendanceLoading = true;
    attendancePage = 0;
    try {
      attendanceSessions = await invoke<SessionRecord[]>("get_sessions_report", {
        fromDate, toDate, userId: selectedUserId || null,
      });
    } catch (_) { attendanceSessions = []; }
    attendanceLoading = false;
  }

  async function loadSummary() {
    summaryLoading = true;
    try {
      summaryData = await invoke<UserSummary[]>("get_time_summary", {
        fromDate, toDate, userId: selectedUserId || null,
      });
    } catch (_) { summaryData = []; }
    summaryLoading = false;
  }

  async function loadActivity() {
    const uid = activityUserId || selectedUserId;
    if (!uid) { activityReport = null; return; }
    activityLoading = true;
    try {
      activityReport = await invoke<ActivityReport>("get_activity_report", {
        fromDate, toDate, userId: uid,
      });
    } catch (_) { activityReport = null; }
    activityLoading = false;
  }

  async function loadNetwork() {
    networkLoading = true;
    networkPage = 0;
    try {
      networkReport = await invoke<NetworkReport>("get_network_report", {
        fromDate, toDate, userId: selectedUserId || null,
      });
    } catch (_) { networkReport = null; }
    networkLoading = false;
  }

  function applyFilters() {
    attendanceSessions = [];
    summaryData = [];
    activityReport = null;
    networkReport = null;
    attendancePage = 0;
    networkPage = 0;
    if (activeTab === "attendance") loadAttendance();
    else if (activeTab === "summary") loadSummary();
    else if (activeTab === "activity") loadActivity();
    else if (activeTab === "network") loadNetwork();
  }

  function switchTab(tab: MainTab) {
    activeTab = tab;
    if (tab === "attendance" && attendanceSessions.length === 0) loadAttendance();
    else if (tab === "summary" && summaryData.length === 0) loadSummary();
    else if (tab === "activity" && !activityReport) loadActivity();
    else if (tab === "network" && !networkReport) loadNetwork();
  }

  // ── Sort / filter derived state ────────────────────────────────────────────
  $: sortedSessions = [...attendanceSessions].sort((a, b) => {
    const va = a[attendanceSortKey] ?? "";
    const vb = b[attendanceSortKey] ?? "";
    const r = String(va).localeCompare(String(vb), undefined, { numeric: true });
    return attendanceSortAsc ? r : -r;
  });

  $: dailyRows = (() => {
    const groups = new Map<string, DailyRow>();
    for (const s of attendanceSessions) {
      const date = nptDate(s.clock_in);
      const key = `${s.user_id}::${date}`;
      const row = groups.get(key);
      if (!row) {
        groups.set(key, {
          user_id: s.user_id, user_name: s.user_name, user_email: s.user_email,
          date, first_clock_in: s.clock_in, last_clock_out: s.clock_out,
          gross_seconds: s.gross_seconds, break_seconds: s.break_seconds,
          net_seconds: s.net_seconds, break_count: s.break_count,
          session_count: 1, status: s.status,
        });
      } else {
        if (s.clock_in < row.first_clock_in) row.first_clock_in = s.clock_in;
        if (s.clock_out === null || row.last_clock_out === null) row.last_clock_out = null;
        else if (s.clock_out > row.last_clock_out) row.last_clock_out = s.clock_out;
        row.gross_seconds += s.gross_seconds;
        row.break_seconds += s.break_seconds;
        row.net_seconds += s.net_seconds;
        row.break_count += s.break_count;
        row.session_count += 1;
        if (s.status === "active") row.status = "active";
      }
    }
    return [...groups.values()].sort((a, b) =>
      b.date.localeCompare(a.date) || a.user_name.toLowerCase().localeCompare(b.user_name.toLowerCase())
    );
  })();

  $: attRowCount = attendanceExpand ? sortedSessions.length : dailyRows.length;
  $: pagedSessions = sortedSessions.slice(attendancePage * ATT_PER_PAGE, (attendancePage + 1) * ATT_PER_PAGE);
  $: pagedDaily = dailyRows.slice(attendancePage * ATT_PER_PAGE, (attendancePage + 1) * ATT_PER_PAGE);
  $: attPages = Math.max(1, Math.ceil(attRowCount / ATT_PER_PAGE));

  function sortBy(key: keyof SessionRecord) {
    if (attendanceSortKey === key) attendanceSortAsc = !attendanceSortAsc;
    else { attendanceSortKey = key; attendanceSortAsc = false; }
  }

  $: filteredNetRecords = (networkReport?.records ?? []).filter(r => {
    if (!networkFilter) return true;
    const f = networkFilter.toLowerCase();
    return r.remote_host.toLowerCase().includes(f)
      || r.remote_ip.toLowerCase().includes(f)
      || r.process_name.toLowerCase().includes(f);
  });
  $: netPages = Math.max(1, Math.ceil(filteredNetRecords.length / NET_PER_PAGE));
  $: pagedNetRecords = filteredNetRecords.slice(networkPage * NET_PER_PAGE, (networkPage + 1) * NET_PER_PAGE);

  $: liveFilteredNet = liveNetConns.filter(r => {
    if (!liveNetFilter) return true;
    const f = liveNetFilter.toLowerCase();
    return r.remote_host.toLowerCase().includes(f) || r.process_name.toLowerCase().includes(f);
  });
  $: liveNetPaged = liveFilteredNet.slice(liveNetPage * LIVE_NET_PER_PAGE, (liveNetPage + 1) * LIVE_NET_PER_PAGE);
  $: liveNetPages = Math.max(1, Math.ceil(liveFilteredNet.length / LIVE_NET_PER_PAGE));

  $: liveAppUsage = computeAppUsage(liveSnapshots);

  function computeAppUsage(snaps: ActivitySnapshot[]) {
    const counts: Record<string, number> = {};
    for (const s of snaps) {
      if (s.idle_seconds < 30 && s.active_app) {
        counts[s.active_app] = (counts[s.active_app] ?? 0) + 30;
      }
    }
    const total = Object.values(counts).reduce((a, b) => a + b, 0) || 1;
    return Object.entries(counts)
      .map(([app, seconds]) => ({ app, seconds, pct: Math.round(seconds / total * 100) }))
      .sort((a, b) => b.seconds - a.seconds)
      .slice(0, 8);
  }

  // ── Download helpers ───────────────────────────────────────────────────────
  function csvEscape(v: unknown): string {
    const s = String(v ?? "").replace(/"/g, '""');
    return `"${s}"`;
  }

  function triggerDownload(filename: string, content: string, mime: string) {
    const blob = new Blob([content], { type: mime });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url; a.download = filename; a.click();
    URL.revokeObjectURL(url);
  }

  function downloadAttendanceCSV() {
    if (attendanceExpand) {
      const hdrs = ["User","Email","Date (NPT)","Clock In (NPT)","Clock Out (NPT)","Gross (h)","Break (h)","Net Work (h)","Breaks","Status"];
      const rows = sortedSessions.map(s => [
        s.user_name, s.user_email,
        nptDate(s.clock_in), nptTime(s.clock_in),
        s.clock_out ? nptTime(s.clock_out) : "",
        (s.gross_seconds / 3600).toFixed(2),
        (s.break_seconds / 3600).toFixed(2),
        (s.net_seconds / 3600).toFixed(2),
        String(s.break_count), s.status,
      ].map(csvEscape).join(","));
      triggerDownload(`attendance_sessions_${fromDate}_to_${toDate}.csv`, [hdrs.join(","), ...rows].join("\n"), "text/csv");
    } else {
      const hdrs = ["User","Email","Date (NPT)","First In (NPT)","Last Out (NPT)","Sessions","Gross (h)","Break (h)","Net Work (h)","Breaks","Status"];
      const rows = dailyRows.map(r => [
        r.user_name, r.user_email, r.date,
        nptTime(r.first_clock_in),
        r.last_clock_out ? nptTime(r.last_clock_out) : "",
        String(r.session_count),
        (r.gross_seconds / 3600).toFixed(2),
        (r.break_seconds / 3600).toFixed(2),
        (r.net_seconds / 3600).toFixed(2),
        String(r.break_count), r.status,
      ].map(csvEscape).join(","));
      triggerDownload(`attendance_daily_${fromDate}_to_${toDate}.csv`, [hdrs.join(","), ...rows].join("\n"), "text/csv");
    }
  }

  function downloadAttendanceJSON() {
    const data = attendanceExpand ? sortedSessions : dailyRows;
    const suffix = attendanceExpand ? "sessions" : "daily";
    triggerDownload(`attendance_${suffix}_${fromDate}_to_${toDate}.json`, JSON.stringify(data, null, 2), "application/json");
  }

  function downloadSummaryCSV() {
    const hdrs = ["User","Email","Days Present","Sessions","Total Work (h)","Total Break (h)","Total Gross (h)","Avg Daily Work (h)"];
    const rows = summaryData.map(s => [
      s.user_name, s.user_email,
      String(s.days_present), String(s.session_count),
      (s.total_work_seconds / 3600).toFixed(2),
      (s.total_break_seconds / 3600).toFixed(2),
      (s.total_gross_seconds / 3600).toFixed(2),
      (s.days_present > 0 ? s.total_work_seconds / s.days_present / 3600 : 0).toFixed(2),
    ].map(csvEscape).join(","));
    triggerDownload(`summary_${fromDate}_to_${toDate}.csv`, [hdrs.join(","), ...rows].join("\n"), "text/csv");
  }

  function downloadSummaryJSON() {
    triggerDownload(`summary_${fromDate}_to_${toDate}.json`, JSON.stringify(summaryData, null, 2), "application/json");
  }

  function downloadNetworkCSV() {
    const hdrs = ["Time (NPT)","User","Email","Process","Host","IP","Port"];
    const rows = filteredNetRecords.map(r => [
      nptDateTime(r.timestamp),
      r.user_name, r.user_email, r.process_name,
      r.remote_host, r.remote_ip, String(r.remote_port),
    ].map(csvEscape).join(","));
    triggerDownload(`network_${fromDate}_to_${toDate}.csv`, [hdrs.join(","), ...rows].join("\n"), "text/csv");
  }

  function downloadNetworkJSON() {
    triggerDownload(`network_${fromDate}_to_${toDate}.json`, JSON.stringify(networkReport, null, 2), "application/json");
  }

  function downloadActivityCSV() {
    if (!activityReport) return;
    const hdrs = ["App","Seconds","Percentage"];
    const rows = activityReport.top_apps.map(a => [a.app, String(a.seconds), a.pct.toFixed(1) + "%"].map(csvEscape).join(","));
    const meta = [
      `"Total Keystrokes","${activityReport.total_keystrokes}"`,
      `"Total Clicks","${activityReport.total_clicks}"`,
      `"Idle %","${activityReport.idle_pct.toFixed(1)}%"`,
    ];
    triggerDownload(`activity_${fromDate}_to_${toDate}.csv`, [...meta, "", hdrs.join(","), ...rows].join("\n"), "text/csv");
  }

  function downloadActivityJSON() {
    if (!activityReport) return;
    triggerDownload(`activity_${fromDate}_to_${toDate}.json`, JSON.stringify(activityReport, null, 2), "application/json");
  }

  // ── Misc ──────────────────────────────────────────────────────────────────
  function elapsed(iso: string): string {
    const secs = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
  }

  function hhmm(secs: number): string { return formatDuration(secs); }

  const SORT_ICON = (key: keyof SessionRecord) =>
    attendanceSortKey === key ? (attendanceSortAsc ? "↑" : "↓") : "";
</script>

<div class="admin">
  <!-- ─── HEADER ─────────────────────────────────────────────────────────── -->
  <header>
    <button class="back-btn" on:click={() => dispatch("back")}>← Back</button>
    <span class="title">Admin Panel</span>
    <!-- Main tabs -->
    <div class="main-tabs">
      {#each MAIN_TABS as t}
        <button class="mtab" class:active={activeTab === t.id} on:click={() => switchTab(t.id)}>
          {t.label}
        </button>
      {/each}
    </div>
  </header>

  <!-- ─── FILTER BAR (hidden on live) ─────────────────────────────────────── -->
  {#if activeTab !== "live"}
    <div class="filter-bar">
      <div class="preset-btns">
        {#each PRESETS as p}
          <button class="preset-btn" class:active={filterPreset === p.id} on:click={() => setPreset(p.id)}>
            {p.label}
          </button>
        {/each}
      </div>
      {#if filterPreset === "custom"}
        <div class="date-range">
          <input type="date" class="date-in" bind:value={fromDate} max={toDate} />
          <span class="date-sep">→</span>
          <input type="date" class="date-in" bind:value={toDate} min={fromDate} max={nepalToday()} />
        </div>
      {/if}
      <select class="user-sel" bind:value={selectedUserId}>
        <option value="">All Users</option>
        {#each allUsers as u}
          <option value={u.id}>{u.name}</option>
        {/each}
      </select>
      <button class="apply-btn" on:click={applyFilters}>Apply</button>
    </div>
  {/if}

  <!-- ─── BODY ──────────────────────────────────────────────────────────── -->
  <div class="body">

    <!-- ════════ LIVE TAB ════════════════════════════════════════════════════ -->
    {#if activeTab === "live"}
      {#if liveLoading}
        <div class="placeholder">Loading…</div>
      {:else if teamMembers.length === 0}
        <div class="placeholder">No one is clocked in right now.</div>
      {:else if !liveSelected}
        <div class="live-header">
          <span class="live-count">{teamMembers.length} online</span>
          <button class="csv-btn" on:click={() => {
            const hdrs = ["Name","Email","Status","Since","Today Work","Today Break"];
            const rows = teamMembers.map(m => [m.user_name,m.user_email,m.status,nptDateTime(m.clock_in),hhmm(m.today_total_work_seconds),hhmm(m.today_total_break_seconds)].map(csvEscape).join(","));
            triggerDownload("live_" + nepalToday() + ".csv", [hdrs.join(","), ...rows].join("\n"), "text/csv");
          }}>CSV</button>
        </div>
        {#each teamMembers as m}
          <button class="member-card" on:click={() => selectLiveMember(m)}>
            <div class="member-top">
              <span class="dot" class:on-break={m.status === "on_break"}></span>
              <div class="minfo">
                <span class="mname">{m.user_name}</span>
                <span class="memail">{m.user_email}</span>
              </div>
              <div class="mright">
                <span class={m.status === "on_break" ? "badge-break" : "badge-active"}>
                  {m.status === "on_break" ? "On Break" : "Active"}
                </span>
                <span class="chevron">›</span>
              </div>
            </div>
            <div class="mstats">
              <div class="ms"><span class="msv">{elapsed(m.clock_in)}</span><span class="msl">Since In</span></div>
              <div class="ms"><span class="msv">{hhmm(m.today_total_work_seconds)}</span><span class="msl">Work</span></div>
              <div class="ms"><span class="msv">{hhmm(m.today_total_break_seconds)}</span><span class="msl">Break</span></div>
              <div class="ms"><span class="msv">{m.break_count}</span><span class="msl">Breaks</span></div>
            </div>
            {#if m.active_app}
              <div class="mapp"><span class="adot">●</span>{m.active_app}</div>
            {/if}
          </button>
        {/each}
      {:else}
        <!-- Live member detail -->
        <div class="detail-header">
          <button class="back-btn" on:click={() => liveSelected = null}>← Team</button>
          <div class="dtitle">
            <span class="dot" class:on-break={liveSelected.status === "on_break"}></span>
            <span class="mname">{liveSelected.user_name}</span>
          </div>
          <span class={liveSelected.status === "on_break" ? "badge-break" : "badge-active"}>
            {liveSelected.status === "on_break" ? "On Break" : "Active"}
          </span>
        </div>
        <div class="sum-row">
          <div class="sum-card"><span class="sumv">{elapsed(liveSelected.clock_in)}</span><span class="suml">Since In</span></div>
          <div class="sum-card"><span class="sumv">{hhmm(liveSelected.today_total_work_seconds)}</span><span class="suml">Today Work</span></div>
          <div class="sum-card"><span class="sumv">{hhmm(liveSelected.today_total_break_seconds)}</span><span class="suml">Today Break</span></div>
          <div class="sum-card"><span class="sumv">{liveSelected.break_count}</span><span class="suml">Breaks</span></div>
        </div>
        <div class="tabs">
          {#each LIVE_DETAIL_TABS as t}
            <button class="tab" class:active={liveDetailTab === t.id}
              on:click={() => {
                liveDetailTab = t.id;
                if (t.id === "network" && liveNetConns.length === 0) loadLiveNetwork();
              }}>{t.label}</button>
          {/each}
        </div>

        {#if liveDetailLoading}
          <div class="card"><div class="nodata">Loading…</div></div>
        {:else if liveDetailTab === "activity"}
          <div class="card">
            <div class="card-title">App Usage</div>
            {#if liveAppUsage.length === 0}
              <div class="nodata">No activity data.</div>
            {:else}
              <div class="app-list">
                {#each liveAppUsage as a}
                  <div class="arow">
                    <span class="aname" title={a.app}>{a.app}</span>
                    <div class="bwrap"><div class="bfill" style="width:{a.pct}%"></div></div>
                    <span class="adur">{hhmm(a.seconds)}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {:else if liveDetailTab === "network"}
          <div class="card">
            <div class="card-title">Connections <input class="nfilter" bind:value={liveNetFilter} placeholder="filter host/proc…" /></div>
            {#if liveNetConns.length === 0}
              <div class="nodata">No connections.</div>
            {:else}
              <div class="net-head"><span>Time</span><span>Process</span><span>Host</span><span>Port</span></div>
              {#each liveNetPaged as r}
                <div class="net-row">
                  <span class="ftime">{nptTime(r.timestamp)}</span>
                  <span class="nproc" title={r.process_name}>{r.process_name}</span>
                  <span class="nhost" title={r.remote_host || r.remote_ip}>{r.remote_host || r.remote_ip}</span>
                  <span class="nport">{r.remote_port}</span>
                </div>
              {/each}
              {#if liveNetPages > 1}
                <div class="pagination">
                  <button class="pg-btn" disabled={liveNetPage === 0} on:click={() => liveNetPage--}>‹</button>
                  <span class="pg-info">{liveNetPage + 1} / {liveNetPages}</span>
                  <button class="pg-btn" disabled={liveNetPage >= liveNetPages - 1} on:click={() => liveNetPage++}>›</button>
                </div>
              {/if}
            {/if}
          </div>
        {:else}
          <div class="card">
            <div class="card-title">Today Breakdown</div>
            {#if !liveBreakdown || liveBreakdown.sessions.length === 0}
              <div class="nodata">No sessions today.</div>
            {:else}
              {#each liveBreakdown.sessions as s}
                <div class="break-row">
                  <div class="break-main">
                    <span class="break-time">{nptTime(s.clock_in)}{#if s.clock_out} – {nptTime(s.clock_out)}{/if}</span>
                  </div>
                  <div class="break-stats">
                    <span><b>Gross</b> {hhmm(s.gross_seconds)}</span>
                    <span><b>Break</b> {hhmm(s.break_seconds)}</span>
                    <span><b>Net</b> {hhmm(s.net_seconds)}</span>
                  </div>
                </div>
              {/each}
            {/if}
          </div>
        {/if}
      {/if}

    <!-- ════════ ATTENDANCE TAB ═══════════════════════════════════════════════ -->
    {:else if activeTab === "attendance"}
      <div class="tab-header">
        <span class="tab-count">{attendanceExpand ? `${sortedSessions.length} sessions` : `${dailyRows.length} records`}</span>
        <label class="expand-toggle">
          <input type="checkbox" bind:checked={attendanceExpand} on:change={() => { attendancePage = 0; }} />
          Show individual sessions
        </label>
        <div class="dl-btns">
          <button class="csv-btn" disabled={attRowCount === 0} on:click={downloadAttendanceCSV}>CSV</button>
          <button class="csv-btn" disabled={attRowCount === 0} on:click={downloadAttendanceJSON}>JSON</button>
        </div>
      </div>
      {#if attendanceLoading}
        <div class="placeholder">Loading…</div>
      {:else if attRowCount === 0}
        <div class="placeholder">No sessions found for the selected range.</div>
      {:else if !attendanceExpand}
        <!-- Daily aggregated view (default) -->
        <div class="scroll-x">
          <table class="data-table">
            <thead>
              <tr>
                <th>User</th>
                <th>Date (NPT)</th>
                <th>First In</th>
                <th>Last Out</th>
                <th>Sessions</th>
                <th>Gross</th>
                <th>Break</th>
                <th>Net Work</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              {#each pagedDaily as r}
                <tr>
                  <td class="td-user"><span class="td-name">{r.user_name}</span><span class="td-email">{r.user_email}</span></td>
                  <td class="mono">{r.date}</td>
                  <td class="mono">{nptTime(r.first_clock_in)}</td>
                  <td class="mono">{r.last_clock_out ? nptTime(r.last_clock_out) : "—"}</td>
                  <td class="mono">{r.session_count}</td>
                  <td class="mono">{hhmm(r.gross_seconds)}</td>
                  <td class="mono">{hhmm(r.break_seconds)}</td>
                  <td class="mono bold">{hhmm(r.net_seconds)}</td>
                  <td><span class="status-badge" class:badge-active={r.status === "active"} class:badge-done={r.status === "completed"}>{r.status}</span></td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        {#if attPages > 1}
          <div class="pagination">
            <button class="pg-btn" disabled={attendancePage === 0} on:click={() => attendancePage--}>‹</button>
            <span class="pg-info">{attendancePage + 1} / {attPages} (showing {pagedDaily.length} of {dailyRows.length})</span>
            <button class="pg-btn" disabled={attendancePage >= attPages - 1} on:click={() => attendancePage++}>›</button>
          </div>
        {/if}
      {:else}
        <!-- Individual sessions view -->
        <div class="scroll-x">
          <table class="data-table">
            <thead>
              <tr>
                <th on:click={() => sortBy("user_name")}>User {SORT_ICON("user_name")}</th>
                <th on:click={() => sortBy("clock_in")}>Date (NPT) {SORT_ICON("clock_in")}</th>
                <th on:click={() => sortBy("clock_in")}>Clock In</th>
                <th on:click={() => sortBy("clock_out")}>Clock Out</th>
                <th on:click={() => sortBy("gross_seconds")}>Gross {SORT_ICON("gross_seconds")}</th>
                <th on:click={() => sortBy("break_seconds")}>Break {SORT_ICON("break_seconds")}</th>
                <th on:click={() => sortBy("net_seconds")}>Net Work {SORT_ICON("net_seconds")}</th>
                <th>Breaks</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              {#each pagedSessions as s}
                <tr>
                  <td class="td-user"><span class="td-name">{s.user_name}</span><span class="td-email">{s.user_email}</span></td>
                  <td class="mono">{nptDate(s.clock_in)}</td>
                  <td class="mono">{nptTime(s.clock_in)}</td>
                  <td class="mono">{s.clock_out ? nptTime(s.clock_out) : "—"}</td>
                  <td class="mono">{hhmm(s.gross_seconds)}</td>
                  <td class="mono">{hhmm(s.break_seconds)}</td>
                  <td class="mono bold">{hhmm(s.net_seconds)}</td>
                  <td class="mono">{s.break_count}</td>
                  <td><span class="status-badge" class:badge-active={s.status === "active"} class:badge-done={s.status === "completed"}>{s.status}</span></td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        {#if attPages > 1}
          <div class="pagination">
            <button class="pg-btn" disabled={attendancePage === 0} on:click={() => attendancePage--}>‹</button>
            <span class="pg-info">{attendancePage + 1} / {attPages} (showing {pagedSessions.length} of {sortedSessions.length})</span>
            <button class="pg-btn" disabled={attendancePage >= attPages - 1} on:click={() => attendancePage++}>›</button>
          </div>
        {/if}
      {/if}

    <!-- ════════ SUMMARY TAB ══════════════════════════════════════════════════ -->
    {:else if activeTab === "summary"}
      <div class="tab-header">
        <span class="tab-count">{summaryData.length} users</span>
        <div class="dl-btns">
          <button class="csv-btn" disabled={summaryData.length === 0} on:click={downloadSummaryCSV}>CSV</button>
          <button class="csv-btn" disabled={summaryData.length === 0} on:click={downloadSummaryJSON}>JSON</button>
        </div>
      </div>
      {#if summaryLoading}
        <div class="placeholder">Loading…</div>
      {:else if summaryData.length === 0}
        <div class="placeholder">No data for the selected range.</div>
      {:else}
        <div class="scroll-x">
          <table class="data-table">
            <thead>
              <tr>
                <th>User</th>
                <th>Days Present</th>
                <th>Sessions</th>
                <th>Total Work</th>
                <th>Total Break</th>
                <th>Total Gross</th>
                <th>Avg Daily Work</th>
              </tr>
            </thead>
            <tbody>
              {#each summaryData as s}
                {@const avgDaily = s.days_present > 0 ? s.total_work_seconds / s.days_present : 0}
                <tr>
                  <td class="td-user"><span class="td-name">{s.user_name}</span><span class="td-email">{s.user_email}</span></td>
                  <td class="mono">{s.days_present}</td>
                  <td class="mono">{s.session_count}</td>
                  <td class="mono bold">{hhmm(s.total_work_seconds)}</td>
                  <td class="mono">{hhmm(s.total_break_seconds)}</td>
                  <td class="mono dim">{hhmm(s.total_gross_seconds)}</td>
                  <td class="mono">{hhmm(Math.round(avgDaily))}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <!-- Totals row -->
        {#if summaryData.length > 1}
          {@const totWork = summaryData.reduce((a, s) => a + s.total_work_seconds, 0)}
          {@const totBreak = summaryData.reduce((a, s) => a + s.total_break_seconds, 0)}
          <div class="sum-row" style="margin-top:10px">
            <div class="sum-card"><span class="sumv">{summaryData.length}</span><span class="suml">Users</span></div>
            <div class="sum-card"><span class="sumv">{hhmm(totWork)}</span><span class="suml">Total Work</span></div>
            <div class="sum-card"><span class="sumv">{hhmm(totBreak)}</span><span class="suml">Total Break</span></div>
            <div class="sum-card"><span class="sumv">{hhmm(Math.round(totWork / summaryData.length))}</span><span class="suml">Avg / User</span></div>
          </div>
        {/if}
      {/if}

    <!-- ════════ ACTIVITY TAB ═════════════════════════════════════════════════ -->
    {:else if activeTab === "activity"}
      <div class="tab-header">
        <span class="tab-count">Activity Analytics</span>
        <div class="dl-btns">
          <button class="csv-btn" disabled={!activityReport} on:click={downloadActivityCSV}>CSV</button>
          <button class="csv-btn" disabled={!activityReport} on:click={downloadActivityJSON}>JSON</button>
        </div>
      </div>
      <!-- User selector for activity (must pick one user) -->
      <div class="act-user-bar">
        <span class="act-label">Select user:</span>
        <select class="user-sel wide" bind:value={activityUserId} on:change={loadActivity}>
          <option value="">— choose a user —</option>
          {#each allUsers as u}
            <option value={u.id}>{u.name} ({u.email})</option>
          {/each}
        </select>
      </div>

      {#if !activityUserId && !selectedUserId}
        <div class="placeholder">Select a user above to see their activity analytics.</div>
      {:else if activityLoading}
        <div class="placeholder">Loading activity data…</div>
      {:else if !activityReport}
        <div class="placeholder">No activity data found for the selected range.</div>
      {:else}
        <!-- Summary stats -->
        <div class="sum-row">
          <div class="sum-card"><span class="sumv">{activityReport.session_count}</span><span class="suml">Sessions</span></div>
          <div class="sum-card"><span class="sumv">{activityReport.total_keystrokes.toLocaleString()}</span><span class="suml">Keystrokes</span></div>
          <div class="sum-card"><span class="sumv">{activityReport.total_clicks.toLocaleString()}</span><span class="suml">Clicks</span></div>
          <div class="sum-card"><span class="sumv" class:idle-high={activityReport.idle_pct > 40}>{activityReport.idle_pct.toFixed(1)}%</span><span class="suml">Idle</span></div>
          <div class="sum-card"><span class="sumv">{activityReport.total_snapshot_count}</span><span class="suml">Snapshots</span></div>
        </div>
        <!-- App usage -->
        <div class="card">
          <div class="card-title">App Usage (active time only)</div>
          {#if activityReport.top_apps.length === 0}
            <div class="nodata">No app usage data.</div>
          {:else}
            <div class="app-list">
              {#each activityReport.top_apps as a}
                <div class="arow">
                  <span class="aname wide" title={a.app}>{a.app}</span>
                  <div class="bwrap"><div class="bfill" style="width:{Math.max(a.pct, 1)}%"></div></div>
                  <span class="adur wide">{hhmm(a.seconds)} ({a.pct.toFixed(1)}%)</span>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

    <!-- ════════ NETWORK TAB ══════════════════════════════════════════════════ -->
    {:else if activeTab === "network"}
      <div class="tab-header">
        <span class="tab-count">{networkReport ? networkReport.records.length : 0} connections</span>
        <div class="view-toggle">
          <button class="preset-btn" class:active={networkViewMode === "stats"} on:click={() => networkViewMode = "stats"}>Stats</button>
          <button class="preset-btn" class:active={networkViewMode === "table"} on:click={() => networkViewMode = "table"}>Table</button>
        </div>
        <div class="dl-btns">
          <button class="csv-btn" disabled={!networkReport} on:click={downloadNetworkCSV}>CSV</button>
          <button class="csv-btn" disabled={!networkReport} on:click={downloadNetworkJSON}>JSON</button>
        </div>
      </div>

      {#if networkLoading}
        <div class="placeholder">Loading…</div>
      {:else if !networkReport}
        <div class="placeholder">No network data for the selected range.</div>
      {:else if networkViewMode === "stats"}
        <!-- Top hosts -->
        <div class="card">
          <div class="card-title">Top Domains / Hosts <span class="csub">({networkReport.top_hosts.length} unique)</span></div>
          {#if networkReport.top_hosts.length === 0}
            <div class="nodata">No connections recorded.</div>
          {:else}
            {@const maxHostCount = networkReport.top_hosts[0]?.count ?? 1}
            <div class="app-list">
              {#each networkReport.top_hosts.slice(0, 15) as h}
                <div class="arow">
                  <span class="aname net-host" title={h.name}>{h.name}</span>
                  <div class="bwrap"><div class="bfill dbfill" style="width:{Math.round(h.count / maxHostCount * 100)}%"></div></div>
                  <span class="adur wide">{h.count}×</span>
                </div>
              {/each}
            </div>
          {/if}
        </div>
        <!-- Top processes -->
        <div class="card">
          <div class="card-title">Top Processes</div>
          {#if networkReport.top_processes.length === 0}
            <div class="nodata">No process data.</div>
          {:else}
            {@const maxProcCount = networkReport.top_processes[0]?.count ?? 1}
            <div class="app-list">
              {#each networkReport.top_processes as p}
                <div class="arow">
                  <span class="aname" title={p.name}>{p.name}</span>
                  <div class="bwrap"><div class="bfill" style="width:{Math.round(p.count / maxProcCount * 100)}%"></div></div>
                  <span class="adur wide">{p.count}×</span>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        <!-- Table view -->
        <div class="net-filter-row">
          <input class="nfilter wide-filter" bind:value={networkFilter} on:input={() => networkPage = 0} placeholder="Filter by host, IP, or process…" />
          <span class="pg-info">{filteredNetRecords.length} shown</span>
        </div>
        {#if filteredNetRecords.length === 0}
          <div class="placeholder">No connections match the filter.</div>
        {:else}
          <div class="scroll-x">
            <table class="data-table">
              <thead>
                <tr>
                  <th>Time (NPT)</th>
                  <th>User</th>
                  <th>Process</th>
                  <th>Host</th>
                  <th>IP</th>
                  <th>Port</th>
                </tr>
              </thead>
              <tbody>
                {#each pagedNetRecords as r}
                  <tr>
                    <td class="mono">{nptTime(r.timestamp)}</td>
                    <td class="td-user"><span class="td-name">{r.user_name}</span></td>
                    <td class="mono">{r.process_name}</td>
                    <td class="mono" title={r.remote_host}>{r.remote_host || "—"}</td>
                    <td class="mono dim">{r.remote_ip}</td>
                    <td class="mono">{r.remote_port}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          {#if netPages > 1}
            <div class="pagination">
              <button class="pg-btn" disabled={networkPage === 0} on:click={() => networkPage--}>‹</button>
              <span class="pg-info">{networkPage + 1} / {netPages} ({filteredNetRecords.length} total)</span>
              <button class="pg-btn" disabled={networkPage >= netPages - 1} on:click={() => networkPage++}>›</button>
            </div>
          {/if}
        {/if}
      {/if}
    {/if}
  </div>
</div>

<style>
  .admin { display: flex; flex-direction: column; height: 100vh; background: #0b0b12; color: #c0c0d0; font-family: system-ui, sans-serif; font-size: 13px; }

  /* ── Header ── */
  header { display: flex; align-items: center; gap: 10px; padding: 10px 14px; border-bottom: 1px solid #16161e; flex-shrink: 0; flex-wrap: wrap; }
  .back-btn { background: none; border: none; color: #5a5a72; font-size: 12px; cursor: pointer; padding: 4px 8px; border-radius: 4px; white-space: nowrap; }
  .back-btn:hover { color: #c0c0d0; background: #1e1e2a; }
  .title { font-size: 13px; font-weight: 700; color: #8080a0; white-space: nowrap; }
  .main-tabs { display: flex; gap: 2px; background: #0e0e18; border-radius: 8px; padding: 3px; margin-left: auto; }
  .mtab { background: none; border: none; color: #4a4a62; font-size: 12px; font-weight: 600; padding: 5px 10px; border-radius: 6px; cursor: pointer; white-space: nowrap; }
  .mtab.active { background: #1a1a28; color: #d0d0e8; }
  .mtab:hover:not(.active) { color: #8080a0; }

  /* ── Filter bar ── */
  .filter-bar { display: flex; align-items: center; gap: 8px; padding: 8px 14px; border-bottom: 1px solid #14141e; background: #0d0d16; flex-shrink: 0; flex-wrap: wrap; }
  .preset-btns { display: flex; gap: 3px; }
  .preset-btn { background: #111118; border: 1px solid #1e1e2c; color: #5a5a72; font-size: 11px; font-weight: 600; padding: 4px 10px; border-radius: 5px; cursor: pointer; white-space: nowrap; }
  .preset-btn.active { background: #1e1e30; border-color: #3b3b58; color: #c0c0e0; }
  .preset-btn:hover:not(.active) { border-color: #2e2e44; color: #8080a0; }
  .date-range { display: flex; align-items: center; gap: 6px; }
  .date-in { background: #111118; border: 1px solid #1e1e2c; color: #c0c0d0; font-size: 12px; padding: 4px 8px; border-radius: 5px; outline: none; }
  .date-in:focus { border-color: #3b82f6; }
  .date-sep { color: #3a3a52; font-size: 11px; }
  .user-sel {
    background: #111118;
    border: 1px solid #1e1e2c;
    color: #c0c0d0;
    font-size: 12px;
    padding: 6px 10px;
    border-radius: 6px;
    outline: none;
    cursor: pointer;
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
    color-scheme: dark;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.02);
  }
  .user-sel option {
    background: #111118;
    color: #e4e4f0;
  }
  .user-sel.wide { min-width: 220px; }
  .user-sel:focus { border-color: #3b82f6; }
  .apply-btn { background: #1a2a4a; border: 1px solid #2a3a6a; color: #3b82f6; font-size: 12px; font-weight: 700; padding: 4px 14px; border-radius: 5px; cursor: pointer; white-space: nowrap; }
  .apply-btn:hover { background: #1e3060; border-color: #3b82f6; }

  /* ── Body ── */
  .body { flex: 1; overflow-y: auto; padding: 10px 14px; display: flex; flex-direction: column; gap: 8px; }
  .body::-webkit-scrollbar { width: 4px; }
  .body::-webkit-scrollbar-thumb { background: #2a2a38; border-radius: 2px; }
  .placeholder { text-align: center; color: #3a3a52; font-size: 13px; margin-top: 40px; }

  /* ── Tab toolbar ── */
  .tab-header { display: flex; align-items: center; gap: 10px; flex-shrink: 0; }
  .tab-count { font-size: 11px; color: #4a4a62; }
  .dl-btns { display: flex; gap: 5px; margin-left: auto; }
  .expand-toggle { display: flex; align-items: center; gap: 5px; font-size: 11px; color: #4a4a62; cursor: pointer; white-space: nowrap; }
  .expand-toggle input { cursor: pointer; accent-color: #7c6af7; }
  .view-toggle { display: flex; gap: 3px; }
  .net-filter-row { display: flex; align-items: center; gap: 10px; }

  /* ── Live tab ── */
  .live-header { display: flex; align-items: center; gap: 8px; }
  .live-count { font-size: 11px; color: #4a4a62; }
  .member-card { background: #111118; border: 1px solid #1a1a24; border-radius: 10px; padding: 12px 14px; display: flex; flex-direction: column; gap: 8px; cursor: pointer; text-align: left; width: 100%; transition: border-color 0.15s; }
  .member-card:hover { border-color: #2e2e42; }
  .member-top { display: flex; align-items: center; gap: 10px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: #22c55e; flex-shrink: 0; }
  .dot.on-break { background: #f59e0b; }
  .minfo { flex: 1; min-width: 0; }
  .mname { display: block; font-size: 13px; font-weight: 600; color: #d8d8ec; }
  .memail { display: block; font-size: 10px; color: #4a4a62; margin-top: 1px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mright { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }
  .chevron { color: #2e2e42; font-size: 18px; }
  .mstats { display: flex; gap: 16px; }
  .ms { display: flex; flex-direction: column; }
  .msv { font-size: 14px; font-weight: 700; color: #c0c0d8; font-variant-numeric: tabular-nums; }
  .msl { font-size: 9px; color: #4a4a62; text-transform: uppercase; letter-spacing: 0.4px; }
  .mapp { font-size: 11px; color: #5a5a72; border-top: 1px solid #1a1a24; padding-top: 7px; display: flex; align-items: center; gap: 6px; }
  .adot { color: #22c55e; font-size: 8px; }
  .badge-active { font-size: 9px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.6px; color: #22c55e; background: #0a1f0a; border: 1px solid #1a3a1a; padding: 2px 6px; border-radius: 3px; white-space: nowrap; }
  .badge-break { font-size: 9px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.6px; color: #f59e0b; background: #1c1a10; border: 1px solid #3a2e00; padding: 2px 6px; border-radius: 3px; white-space: nowrap; }
  .badge-done { font-size: 9px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.6px; color: #6060a0; background: #111118; border: 1px solid #2a2a3a; padding: 2px 6px; border-radius: 3px; }
  .status-badge { font-size: 9px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.6px; padding: 2px 6px; border-radius: 3px; }
  .detail-header { display: flex; align-items: center; gap: 8px; }
  .dtitle { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; }

  /* ── Tabs (live detail) ── */
  .tabs { display: flex; gap: 2px; background: #0e0e16; border-radius: 8px; padding: 3px; flex-shrink: 0; }
  .tab { flex: 1; background: none; border: none; color: #4a4a62; font-size: 12px; font-weight: 600; padding: 5px 4px; border-radius: 6px; cursor: pointer; }
  .tab.active { background: #1a1a24; color: #c0c0d0; }

  /* ── Summary cards ── */
  .sum-row { display: flex; gap: 6px; flex-wrap: wrap; }
  .sum-card { flex: 1; min-width: 70px; background: #111118; border: 1px solid #1a1a24; border-radius: 8px; padding: 10px 12px; display: flex; flex-direction: column; align-items: center; }
  .sumv { font-size: 15px; font-weight: 700; color: #d8d8ec; font-variant-numeric: tabular-nums; }
  .suml { font-size: 9px; color: #4a4a62; text-transform: uppercase; letter-spacing: 0.4px; margin-top: 2px; text-align: center; }
  .idle-high { color: #ef4444; }

  /* ── Cards ── */
  .card { background: #111118; border: 1px solid #1a1a24; border-radius: 10px; padding: 12px 14px; }
  .card-title { font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.8px; color: #4a4a62; margin-bottom: 10px; display: flex; align-items: center; gap: 8px; }
  .csub { color: #2e2e42; font-weight: 400; text-transform: none; letter-spacing: 0; }
  .nodata { font-size: 12px; color: #3a3a52; text-align: center; padding: 16px 0; }

  /* ── App bars ── */
  .app-list { display: flex; flex-direction: column; gap: 8px; }
  .arow { display: flex; align-items: center; gap: 10px; }
  .aname { font-size: 11px; color: #9090b0; min-width: 100px; max-width: 100px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .aname.wide { min-width: 160px; max-width: 160px; }
  .aname.net-host { min-width: 200px; max-width: 200px; color: #8080c0; }
  .bwrap { flex: 1; height: 6px; background: #0e0e16; border-radius: 3px; overflow: hidden; min-width: 40px; }
  .bfill { height: 100%; background: #22c55e; border-radius: 3px; min-width: 2px; transition: width 0.3s; }
  .dbfill { background: #3b82f6; }
  .adur { font-size: 10px; color: #4a4a62; min-width: 32px; text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .adur.wide { min-width: 80px; }

  /* ── Data table ── */
  .scroll-x { overflow-x: auto; border-radius: 8px; }
  .scroll-x::-webkit-scrollbar { height: 4px; }
  .scroll-x::-webkit-scrollbar-thumb { background: #2a2a38; }
  .data-table { width: 100%; border-collapse: collapse; font-size: 12px; white-space: nowrap; }
  .data-table th { text-align: left; font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.5px; color: #4a4a62; padding: 8px 10px; border-bottom: 1px solid #1a1a24; background: #0d0d16; cursor: pointer; user-select: none; }
  .data-table th:hover { color: #8080a0; }
  .data-table td { padding: 7px 10px; border-bottom: 1px solid #111118; color: #a0a0bc; vertical-align: middle; }
  .data-table tr:hover td { background: #0f0f1a; }
  .data-table tr:last-child td { border-bottom: none; }
  .td-user { display: flex; flex-direction: column; gap: 1px; min-width: 120px; }
  .td-name { font-size: 12px; font-weight: 600; color: #d0d0e8; }
  .td-email { font-size: 10px; color: #4a4a62; }
  .mono { font-variant-numeric: tabular-nums; }
  .bold { font-weight: 700; color: #d0d0e8; }
  .dim { color: #5a5a72; }

  /* ── Network tab ── */
  .nfilter { background: #0e0e16; border: 1px solid #1e1e2a; border-radius: 5px; padding: 3px 8px; font-size: 11px; color: #8080a0; outline: none; width: 150px; }
  .nfilter:focus { border-color: #3b82f6; }
  .wide-filter { width: 280px; }
  .net-head { display: grid; grid-template-columns: 50px 80px 1fr 48px; color: #3a3a52; font-size: 9px; text-transform: uppercase; letter-spacing: 0.4px; padding: 0 0 5px; border-bottom: 1px solid #1a1a24; gap: 6px; }
  .net-row { display: grid; grid-template-columns: 50px 80px 1fr 48px; padding: 5px 0; border-bottom: 1px solid #0e0e16; gap: 6px; align-items: center; font-size: 11px; }
  .nproc { color: #7878a0; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nhost { color: #a0a0bc; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nport { color: #3a3a52; font-variant-numeric: tabular-nums; }
  .ftime { font-variant-numeric: tabular-nums; color: #5a5a72; font-size: 11px; }

  /* ── Activity tab ── */
  .act-user-bar { display: flex; align-items: center; gap: 8px; }
  .act-label { font-size: 11px; color: #4a4a62; white-space: nowrap; }

  /* ── Breakdown ── */
  .break-row { background: #0e0e16; border: 1px solid #1a1a24; border-radius: 8px; padding: 8px 12px; margin-bottom: 6px; }
  .break-main { display: flex; justify-content: space-between; align-items: center; }
  .break-time { font-size: 12px; font-weight: 600; color: #d8d8ec; }
  .break-stats { display: flex; flex-wrap: wrap; gap: 12px; margin-top: 6px; font-size: 11px; color: #7b7b96; }
  .break-stats b { color: #d8d8ec; font-weight: 600; }

  /* ── Pagination ── */
  .pagination { display: flex; align-items: center; gap: 6px; padding-top: 10px; border-top: 1px solid #1a1a24; margin-top: 4px; }
  .pg-btn { background: #0e0e16; border: 1px solid #1e1e2a; color: #6060a0; font-size: 14px; width: 28px; height: 26px; border-radius: 5px; cursor: pointer; display: flex; align-items: center; justify-content: center; }
  .pg-btn:hover:not(:disabled) { border-color: #3a3a52; color: #c0c0d0; }
  .pg-btn:disabled { opacity: 0.3; cursor: not-allowed; }
  .pg-info { flex: 1; text-align: center; font-size: 10px; color: #4a4a62; }

  /* ── Download button ── */
  .csv-btn { background: #0e1820; border: 1px solid #1a3040; color: #3b82f6; font-size: 10px; font-weight: 700; padding: 4px 10px; border-radius: 4px; cursor: pointer; white-space: nowrap; }
  .csv-btn:hover:not(:disabled) { border-color: #3b82f6; background: #0e2030; }
  .csv-btn:disabled { opacity: 0.35; cursor: not-allowed; }
</style>
