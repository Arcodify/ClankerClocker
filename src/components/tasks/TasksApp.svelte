<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import type { PlaneIssue, SessionState, TaskRecord } from "../../lib/types";
  import {
    loadBasics,
    loadView,
    issuesByView,
    viewKey,
    refreshTotals,
    projects,
    issueKey,
    links,
    openInPlane,
    type TaskView,
  } from "../../lib/plane";
  import Sidebar from "./Sidebar.svelte";
  import IssueList from "./IssueList.svelte";
  import IssueDetail from "./IssueDetail.svelte";
  import Dialog from "../Dialog.svelte";

  let view: TaskView = { kind: "mine" };
  let selectedId: string | null = null;
  let loading = false;
  let bootError = "";
  let error = "";
  let errorTimer: ReturnType<typeof setTimeout>;

  let activeTask: TaskRecord | null = null;
  let sessionStatus: SessionState["status"] = "idle";
  let taskSeconds = 0;
  let ticker: ReturnType<typeof setInterval>;
  let showEndDialog = false;
  let busy = false;

  let list: IssueList;

  $: issues = $issuesByView[viewKey(view)] ?? [];
  $: selected = issues.find((i) => i.id === selectedId) ?? null;
  $: clockedIn = sessionStatus !== "idle";
  $: activeIssue = activeTask?.plane_issue_id
    ? Object.values($issuesByView).flat().find((i) => i.id === activeTask!.plane_issue_id) ?? null
    : null;

  function showError(e: unknown) {
    error = String(e);
    clearTimeout(errorTimer);
    errorTimer = setTimeout(() => (error = ""), 5000);
  }

  async function openView(v: TaskView, force = false) {
    view = v;
    selectedId = null;
    if (!force && $issuesByView[viewKey(v)]) return;
    loading = true;
    try {
      await loadView(v, force);
    } catch (e) {
      showError(e);
    } finally {
      loading = false;
    }
  }

  function setActiveTask(t: TaskRecord | null) {
    activeTask = t;
    taskSeconds = t ? Math.max(0, Math.floor((Date.now() - new Date(t.started_at).getTime()) / 1000)) : 0;
  }

  async function startTracking(issue: PlaneIssue) {
    if (!clockedIn) return showError("Clock in before starting a task");
    busy = true;
    try {
      setActiveTask(
        await invoke<TaskRecord>("start_task", {
          name: issue.name,
          planeProjectId: issue.project,
          planeIssueId: issue.id,
        }),
      );
      refreshTotals();
    } catch (e) {
      showError(e);
    } finally {
      busy = false;
    }
  }

  async function endTask(complete: boolean) {
    showEndDialog = false;
    const t = activeTask;
    if (!t) return;
    try {
      await invoke("end_task", {
        taskId: t.id,
        planeProjectId: t.plane_project_id,
        planeIssueId: t.plane_issue_id,
        complete,
      });
      setActiveTask(null);
      refreshTotals();
      if (complete) loadView(view, true).catch(() => {});
    } catch (e) {
      showError(e);
    }
  }

  function fmtClock(s: number) {
    const h = Math.floor(s / 3600).toString().padStart(2, "0");
    const m = Math.floor((s % 3600) / 60).toString().padStart(2, "0");
    const sec = (s % 60).toString().padStart(2, "0");
    return `${h}:${m}:${sec}`;
  }

  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const typing = target.tagName === "INPUT" || target.tagName === "SELECT" || target.tagName === "TEXTAREA";
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      list?.focusSearch();
      return;
    }
    if (typing) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const ids = list?.visibleIds() ?? [];
      if (!ids.length) return;
      const idx = selectedId ? ids.indexOf(selectedId) : -1;
      const next = e.key === "ArrowDown" ? Math.min(ids.length - 1, idx + 1) : Math.max(0, idx - 1);
      selectedId = ids[next];
    } else if (e.key === "Enter" && selected) {
      startTracking(selected);
    } else if (e.key.toLowerCase() === "o" && selected) {
      openInPlane(links.issue(selected));
    } else if (e.key.toLowerCase() === "n") {
      e.preventDefault();
      list?.focusQuickAdd();
    }
  }

  const unlistens: Promise<UnlistenFn>[] = [];

  onMount(async () => {
    unlistens.push(listen<TaskRecord | null>("task-changed", (e) => setActiveTask(e.payload)));
    unlistens.push(
      listen<SessionState>("session-update", (e) => {
        sessionStatus = e.payload.status;
        if (e.payload.status === "idle") setActiveTask(null);
      }),
    );
    ticker = setInterval(() => {
      if (activeTask) taskSeconds += 1;
    }, 1000);

    invoke<SessionState>("get_session_state")
      .then((s) => (sessionStatus = s.status))
      .catch(() => {});
    invoke<TaskRecord | null>("get_active_task")
      .then(setActiveTask)
      .catch(() => {});

    loading = true;
    try {
      await loadBasics();
      await loadView(view);
    } catch (e) {
      bootError = String(e);
    } finally {
      loading = false;
    }
  });

  onDestroy(() => {
    clearInterval(ticker);
    unlistens.forEach((p) => p.then((f) => f()));
  });
</script>

<svelte:window on:keydown={onKey} />

<div class="shell">
  {#if activeTask}
    <div class="now-bar">
      <span class="live-dot"></span>
      <span class="now-label">Working on</span>
      {#if activeIssue}<span class="key">{issueKey(activeIssue, $projects)}</span>{/if}
      <button class="now-name" on:click={() => activeIssue && (selectedId = activeIssue.id)} title={activeTask.name}>
        {activeTask.name}
      </button>
      <span class="now-timer">{fmtClock(taskSeconds)}</span>
      {#if activeIssue}
        <button class="ghost" on:click={() => activeIssue && openInPlane(links.issue(activeIssue))} title="Open in Plane">↗</button>
      {/if}
      <button class="btn-stop" on:click={() => (showEndDialog = true)}>End</button>
    </div>
  {/if}

  {#if bootError}
    <div class="boot-error">
      <div class="be-title">Can't reach Plane</div>
      <div class="be-msg">{bootError}</div>
      <div class="be-hint">Check your Plane API key in the main window's Settings → API Keys, and that an admin has configured the workspace.</div>
    </div>
  {:else}
    <div class="panes">
      <Sidebar {view} on:select={(e) => openView(e.detail)} on:error={(e) => showError(e.detail)} />
      <IssueList
        bind:this={list}
        {view}
        {issues}
        {loading}
        {selectedId}
        activeIssueId={activeTask?.plane_issue_id ?? null}
        on:select={(e) => (selectedId = e.detail)}
        on:start={(e) => startTracking(e.detail)}
        on:refresh={() => openView(view, true)}
        on:error={(e) => showError(e.detail)}
      />
      <IssueDetail
        issue={selected}
        {clockedIn}
        {busy}
        isActive={!!selected && activeTask?.plane_issue_id === selected.id}
        hasActive={!!activeTask}
        on:start={(e) => startTracking(e.detail)}
        on:end={() => (showEndDialog = true)}
        on:error={(e) => showError(e.detail)}
      />
    </div>
  {/if}

  {#if error}
    <button class="toast" on:click={() => (error = "")}>{error}</button>
  {/if}
</div>

<Dialog
  open={showEndDialog}
  title="End task"
  body={`End "${activeTask?.name ?? ""}"?`}
  confirmLabel={activeTask?.plane_issue_id ? "Complete" : "Done"}
  secondaryLabel="Stop"
  cancelLabel="Keep Working"
  on:confirm={() => endTask(true)}
  on:secondary={() => endTask(false)}
  on:cancel={() => (showEndDialog = false)}
  on:dismiss={() => (showEndDialog = false)}
/>

<style>
  :global(*, *::before, *::after) { box-sizing: border-box; margin: 0; padding: 0; }
  :global(html) { color-scheme: dark; }
  :global(body) {
    font-family: "Inter", system-ui, sans-serif;
    background: #0d0d0f;
    color: #e8e8ec;
    height: 100vh;
    overflow: hidden;
  }
  :global(button) { font-family: inherit; }
  .shell { height: 100vh; display: flex; flex-direction: column; }
  .panes { flex: 1; min-height: 0; display: grid; grid-template-columns: 230px minmax(0, 1fr) 300px; }

  .now-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
    background: #111118;
    border-bottom: 1px solid #1a1a24;
    font-size: 13px;
  }
  .live-dot { width: 7px; height: 7px; border-radius: 50%; background: #22c55e; box-shadow: 0 0 6px #22c55e; }
  .now-label { font-size: 10px; font-weight: 700; letter-spacing: 0.6px; text-transform: uppercase; color: #5a5a72; }
  .key { font-size: 11px; color: #7c8aa6; font-variant-numeric: tabular-nums; }
  .now-name {
    flex: 1;
    min-width: 0;
    text-align: left;
    background: none;
    border: none;
    color: #e0e0ec;
    font-size: 13px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .now-timer { color: #93c5fd; font-weight: 700; font-variant-numeric: tabular-nums; }
  .ghost {
    background: none;
    border: 1px solid #2a2a38;
    color: #9ca3af;
    border-radius: 6px;
    padding: 4px 8px;
    cursor: pointer;
  }
  .ghost:hover { color: #e0e0ec; background: #1a1a24; }
  .btn-stop {
    background: #1f2937;
    color: #fca5a5;
    border: 1px solid #3a2020;
    border-radius: 6px;
    padding: 5px 12px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-stop:hover { background: #2a1d1d; }

  .boot-error { margin: auto; max-width: 420px; text-align: center; display: flex; flex-direction: column; gap: 8px; }
  .be-title { font-size: 16px; font-weight: 700; }
  .be-msg { font-size: 12px; color: #fca5a5; word-break: break-word; }
  .be-hint { font-size: 12px; color: #6b7280; }

  .toast {
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
    max-width: 70vw;
    cursor: pointer;
    z-index: 100;
  }
</style>
