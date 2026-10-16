<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-shell";
  import { activeTask, taskElapsedSeconds, settings, errorMessage } from "../lib/stores";
  import { planeWebBase } from "../lib/plane";
  import type { PlaneIssue } from "../lib/types";
  import Dialog from "./Dialog.svelte";

  // Browsing, filtering and managing issues lives in the separate Tasks
  // window (src/components/tasks). This panel is just the running-task bar
  // plus one-click restarts, so the 380px dashboard stays uncluttered.

  let showEndDialog = false;
  let starting = false;
  let quickName = "";
  let recent: PlaneIssue[] = [];

  $: taskTimerLabel = (() => {
    const h = Math.floor($taskElapsedSeconds / 3600).toString().padStart(2, "0");
    const m = Math.floor(($taskElapsedSeconds % 3600) / 60).toString().padStart(2, "0");
    const s = ($taskElapsedSeconds % 60).toString().padStart(2, "0");
    return `${h}:${m}:${s}`;
  })();

  $: webBase = planeWebBase($settings.pm_base_url, $settings.pm_workspace_slug);
  $: activeIssueUrl =
    webBase && $activeTask?.plane_project_id && $activeTask?.plane_issue_id
      ? `${webBase}/projects/${$activeTask.plane_project_id}/issues/${$activeTask.plane_issue_id}`
      : "";

  onMount(loadRecent);

  async function loadRecent() {
    if (!$settings.pm_enabled) return;
    try {
      recent = (await invoke<PlaneIssue[]>("list_recent_plane_issues")).slice(0, 3);
    } catch (_) {
      // Non-fatal — the chips just don't show.
    }
  }

  function openTasks() {
    invoke("open_tasks_window").catch((e) => errorMessage.set(String(e)));
  }

  async function startTask(name: string, planeProjectId: string | null, planeIssueId: string | null) {
    if (!name.trim()) return;
    starting = true;
    try {
      const task = await invoke("start_task", { name: name.trim(), planeProjectId, planeIssueId });
      activeTask.set(task as any);
      quickName = "";
    } catch (e) {
      errorMessage.set(String(e));
    } finally {
      starting = false;
    }
  }

  async function endTask(complete: boolean) {
    const task = $activeTask;
    if (!task) return;
    showEndDialog = false;
    try {
      await invoke("end_task", {
        taskId: task.id,
        planeProjectId: task.plane_project_id,
        planeIssueId: task.plane_issue_id,
        complete,
      });
      activeTask.set(null);
      loadRecent();
    } catch (e) {
      errorMessage.set(String(e));
    }
  }
</script>

{#if $activeTask}
  <div class="task-bar">
    <div class="task-info">
      <span class="task-label">Working on</span>
      <span class="task-name" title={$activeTask.name}>{$activeTask.name}</span>
    </div>
    <span class="task-timer">{taskTimerLabel}</span>
    {#if activeIssueUrl}
      <button class="icon-btn" on:click={() => open(activeIssueUrl)} title="Open in Plane">↗</button>
    {/if}
    {#if $settings.pm_enabled}
      <button class="icon-btn" on:click={openTasks} title="Switch task">⇄</button>
    {/if}
    <button class="btn-end" on:click={() => (showEndDialog = true)}>End</button>
  </div>
{:else}
  <div class="task-panel">
    <div class="quick-row">
      <input
        bind:value={quickName}
        placeholder="What are you working on?"
        autocomplete="off"
        disabled={starting}
        on:keydown={(e) => e.key === "Enter" && startTask(quickName, null, null)}
      />
      {#if $settings.pm_enabled}
        <button class="btn-tasks" on:click={openTasks} title="Browse Plane projects, modules and cycles">Tasks</button>
      {/if}
    </div>
    {#if recent.length}
      <div class="recent">
        {#each recent as issue (issue.id)}
          <button
            class="chip"
            disabled={starting}
            on:click={() => startTask(issue.name, issue.project, issue.id)}
            title="Start: {issue.name}"
          >
            ▶ {issue.name}
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<Dialog
  open={showEndDialog}
  title="End task"
  body={`End "${$activeTask?.name ?? ""}"?`}
  confirmLabel="Complete"
  secondaryLabel="Stop"
  cancelLabel="Keep Working"
  on:confirm={() => endTask(true)}
  on:secondary={() => endTask(false)}
  on:cancel={() => (showEndDialog = false)}
  on:dismiss={() => (showEndDialog = false)}
/>

<style>
  .task-panel {
    margin: 0 12px 4px;
    background: #111118;
    border: 1px solid #1a1a24;
    border-radius: 10px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .quick-row { display: flex; gap: 6px; }
  .quick-row input {
    flex: 1;
    min-width: 0;
    background: #13131a;
    border: 1px solid #2a2a36;
    border-radius: 7px;
    padding: 8px 10px;
    color: #e0e0ec;
    font-size: 13px;
    outline: none;
  }
  .quick-row input:focus { border-color: #6366f1; }
  .btn-tasks {
    background: #6366f1;
    color: white;
    border: none;
    border-radius: 7px;
    padding: 0 14px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-tasks:hover { background: #4f46e5; }
  .recent { display: flex; flex-direction: column; gap: 4px; }
  .chip {
    text-align: left;
    background: none;
    border: 1px solid #1e1e2c;
    border-radius: 6px;
    color: #a0a0b8;
    font-size: 12px;
    padding: 6px 9px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chip:hover:not(:disabled) { background: #1a1a24; color: #e0e0ec; }

  .task-bar {
    margin: 0 12px 4px;
    background: #111118;
    border: 1px solid #1a1a24;
    border-radius: 10px;
    padding: 9px 12px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .task-info { display: flex; flex-direction: column; flex: 1; min-width: 0; }
  .task-label {
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: #4a4a62;
  }
  .task-name {
    font-size: 13px;
    color: #e0e0ec;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .task-timer {
    font-size: 13px;
    font-weight: 700;
    color: #93c5fd;
    font-variant-numeric: tabular-nums;
  }
  .icon-btn {
    background: none;
    border: 1px solid #2a2a38;
    border-radius: 6px;
    color: #9ca3af;
    padding: 4px 7px;
    font-size: 12px;
    cursor: pointer;
  }
  .icon-btn:hover { color: #e0e0ec; background: #1a1a24; }
  .btn-end {
    background: #1f2937;
    color: #fca5a5;
    border: 1px solid #3a2020;
    border-radius: 6px;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-end:hover { background: #2a1d1d; }
</style>
