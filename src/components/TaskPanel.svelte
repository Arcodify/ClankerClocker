<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    activeTask,
    taskElapsedSeconds,
    planeProjects,
    planeIssuesByProject,
    planeStatesByProject,
    planeMembers,
    recentPlaneIssues,
    taskPanelView,
    errorMessage,
  } from "../lib/stores";
  import type { PlaneProject, PlaneIssue, PlaneState, PlaneMember } from "../lib/types";
  import Dialog from "./Dialog.svelte";

  let loadingProjects = false;
  let loadingIssues = false;
  let loadingRecent = false;
  let starting = false;
  let selectedProject: PlaneProject | null = null;
  let customName = "";
  let customProjectId = "";
  let showEndDialog = false;
  let issueSearch = "";
  let expandedIds = new Set<string>();

  $: currentIssues = selectedProject ? $planeIssuesByProject[selectedProject.id] ?? null : null;
  $: currentStates = selectedProject ? $planeStatesByProject[selectedProject.id] ?? null : null;
  $: doneGroupIds = new Set(
    (currentStates ?? []).filter((s) => s.group === "completed" || s.group === "cancelled").map((s) => s.id),
  );
  // Sub-issues are excluded from the top-level list — they show up nested
  // under their parent instead (see childrenOf) so a project's issue picker
  // isn't cluttered with what are really implementation details of another
  // row.
  $: topLevelIssues = (currentIssues ?? [])
    .filter((i) => !i.parent && !doneGroupIds.has(i.state))
    .filter((i) => !issueSearch.trim() || i.name.toLowerCase().includes(issueSearch.trim().toLowerCase()));
  $: memberNameById = new Map($planeMembers.map((m) => [m.id, m.display_name || m.email]));

  function childrenOf(parentId: string): PlaneIssue[] {
    return (currentIssues ?? []).filter((i) => i.parent === parentId && !doneGroupIds.has(i.state));
  }

  function toggleExpanded(id: string) {
    if (expandedIds.has(id)) expandedIds.delete(id);
    else expandedIds.add(id);
    expandedIds = new Set(expandedIds);
  }

  function memberName(id: string | null): string {
    if (!id) return "—";
    return memberNameById.get(id) ?? id.slice(0, 8);
  }

  function shortDate(iso: string): string {
    if (!iso) return "—";
    try {
      return new Date(iso).toLocaleDateString(undefined, { month: "short", day: "numeric" });
    } catch (_) {
      return iso;
    }
  }

  async function ensureMembersLoaded() {
    if ($planeMembers.length > 0) return;
    try {
      planeMembers.set(await invoke<PlaneMember[]>("list_plane_members"));
    } catch (_) {
      // Non-fatal — rows just fall back to showing raw ids.
    }
  }

  $: taskTimerLabel = (() => {
    const h = Math.floor($taskElapsedSeconds / 3600).toString().padStart(2, "0");
    const m = Math.floor(($taskElapsedSeconds % 3600) / 60).toString().padStart(2, "0");
    const s = ($taskElapsedSeconds % 60).toString().padStart(2, "0");
    return `${h}:${m}:${s}`;
  })();

  function openMenu() {
    $taskPanelView = "menu";
  }

  function closePanel() {
    $taskPanelView = "closed";
    selectedProject = null;
    customName = "";
    customProjectId = "";
    issueSearch = "";
    expandedIds = new Set();
  }

  async function openPickProject() {
    $taskPanelView = "pickProject";
    if ($planeProjects.length === 0) {
      loadingProjects = true;
      try {
        planeProjects.set(await invoke<PlaneProject[]>("list_plane_projects"));
      } catch (e) {
        errorMessage.set(String(e));
        $taskPanelView = "menu";
      } finally {
        loadingProjects = false;
      }
    }
  }

  async function pickProject(project: PlaneProject) {
    selectedProject = project;
    issueSearch = "";
    expandedIds = new Set();
    $taskPanelView = "pickIssue";
    ensureMembersLoaded();
    if (!$planeIssuesByProject[project.id]) {
      loadingIssues = true;
      try {
        const [issues, states] = await Promise.all([
          invoke<PlaneIssue[]>("list_plane_project_issues", { projectId: project.id }),
          invoke<PlaneState[]>("list_plane_project_states", { projectId: project.id }),
        ]);
        planeIssuesByProject.update((m) => ({ ...m, [project.id]: issues }));
        planeStatesByProject.update((m) => ({ ...m, [project.id]: states }));
      } catch (e) {
        errorMessage.set(String(e));
      } finally {
        loadingIssues = false;
      }
    }
  }

  function openCustom(prefillProjectId = "") {
    customProjectId = prefillProjectId;
    $taskPanelView = "custom";
  }

  async function openRecent() {
    $taskPanelView = "recent";
    issueSearch = "";
    loadingRecent = true;
    ensureMembersLoaded();
    try {
      recentPlaneIssues.set(await invoke<PlaneIssue[]>("list_recent_plane_issues"));
    } catch (e) {
      errorMessage.set(String(e));
    } finally {
      loadingRecent = false;
    }
  }

  async function startTask(name: string, planeProjectId: string | null, planeIssueId: string | null) {
    if (!name.trim()) return;
    starting = true;
    try {
      const task = await invoke("start_task", {
        name: name.trim(),
        planeProjectId,
        planeIssueId,
      });
      activeTask.set(task as any);
      closePanel();
    } catch (e) {
      errorMessage.set(String(e));
    } finally {
      starting = false;
    }
  }

  function startFromIssue(issue: PlaneIssue) {
    startTask(issue.name, issue.project, issue.id);
  }

  function submitCustom() {
    startTask(customName, customProjectId || null, null);
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
    } catch (e) {
      errorMessage.set(String(e));
    }
  }
</script>

{#if $activeTask}
  <div class="task-bar">
    <div class="task-info">
      <span class="task-label">Working on</span>
      <span class="task-name">{$activeTask.name}</span>
    </div>
    <span class="task-timer">{taskTimerLabel}</span>
    <button class="btn-end" on:click={() => (showEndDialog = true)}>End</button>
  </div>
{:else if $taskPanelView === "closed"}
  <div class="task-panel">
    <button class="btn-track" on:click={openMenu}>+ Track Task</button>
  </div>
{:else}
  <div class="task-panel">
    {#if $taskPanelView === "menu"}
      <div class="menu-row">
        <button on:click={openPickProject}>Pick Project</button>
        <button on:click={() => openCustom()}>Custom Task</button>
        <button on:click={openRecent}>Recent Task</button>
      </div>
      <button class="btn-close" on:click={closePanel}>Cancel</button>
    {:else if $taskPanelView === "pickProject"}
      <div class="panel-title">Pick a project</div>
      {#if loadingProjects}
        <div class="hint">Loading projects…</div>
      {:else if $planeProjects.length === 0}
        <div class="hint">No projects found.</div>
      {:else}
        <div class="list">
          {#each $planeProjects as project}
            <button class="list-row" on:click={() => pickProject(project)}>
              <span class="pill">{project.identifier}</span> {project.name}
            </button>
          {/each}
        </div>
      {/if}
      <button class="btn-close" on:click={closePanel}>Cancel</button>
    {:else if $taskPanelView === "pickIssue" && selectedProject}
      <div class="panel-title">{selectedProject.name}</div>
      {#if loadingIssues}
        <div class="hint">Loading issues…</div>
      {:else if currentIssues && currentIssues.length === 0}
        <div class="hint">No issues in this project yet.</div>
        <div class="inline-custom">
          <input bind:value={customName} placeholder="Task name" autocomplete="off" />
          <button
            class="btn-track"
            disabled={!customName.trim() || starting}
            on:click={() => startTask(customName, selectedProject && selectedProject.id, null)}
          >
            + Create task in this project
          </button>
        </div>
      {:else if currentIssues}
        <input class="search-input" bind:value={issueSearch} placeholder="Search issues…" autocomplete="off" />
        {#if topLevelIssues.length === 0}
          <div class="hint">No open issues match.</div>
          <div class="inline-custom">
            <input bind:value={customName} placeholder="Task name" autocomplete="off" />
            <button
              class="btn-track"
              disabled={!customName.trim() || starting}
              on:click={() => startTask(customName, selectedProject && selectedProject.id, null)}
            >
              + Create task in this project
            </button>
          </div>
        {:else}
          <div class="list">
            {#each topLevelIssues as issue (issue.id)}
              {@const kids = childrenOf(issue.id)}
              <div class="issue-row">
                <div class="issue-main">
                  {#if kids.length > 0}
                    <button class="expand-btn" on:click={() => toggleExpanded(issue.id)}>
                      {expandedIds.has(issue.id) ? "▾" : "▸"}
                    </button>
                  {:else}
                    <span class="expand-spacer"></span>
                  {/if}
                  <button class="list-row issue-title" on:click={() => startFromIssue(issue)}>
                    {issue.name}
                    {#if kids.length > 0}<span class="child-count">{kids.length}</span>{/if}
                  </button>
                </div>
                <div class="issue-meta">
                  <span>{issue.assignees.length ? issue.assignees.map(memberName).join(", ") : "Unassigned"}</span>
                  <span>by {memberName(issue.created_by)}</span>
                  <span>{shortDate(issue.created_at)}</span>
                </div>
                {#if kids.length > 0 && expandedIds.has(issue.id)}
                  <div class="issue-children">
                    {#each kids as child (child.id)}
                      <div class="issue-row child">
                        <button class="list-row issue-title" on:click={() => startFromIssue(child)}>
                          {child.name}
                        </button>
                        <div class="issue-meta">
                          <span>{child.assignees.length ? child.assignees.map(memberName).join(", ") : "Unassigned"}</span>
                          <span>by {memberName(child.created_by)}</span>
                          <span>{shortDate(child.created_at)}</span>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      {/if}
      <button class="btn-close" on:click={closePanel}>Cancel</button>
    {:else if $taskPanelView === "custom"}
      <div class="panel-title">Custom task</div>
      <input bind:value={customName} placeholder="Task name" autocomplete="off" />
      <label class="project-select">
        <span>Project (optional)</span>
        <select bind:value={customProjectId}>
          <option value="">No project (local only)</option>
          {#each $planeProjects as project}
            <option value={project.id}>{project.name}</option>
          {/each}
        </select>
      </label>
      <button class="btn-track" disabled={!customName.trim() || starting} on:click={submitCustom}>
        {customProjectId ? "Create Plane Issue & Start" : "Create Task"}
      </button>
      <button class="btn-close" on:click={closePanel}>Cancel</button>
    {:else if $taskPanelView === "recent"}
      <div class="panel-title">Recent Plane issues</div>
      {#if loadingRecent}
        <div class="hint">Loading…</div>
      {:else if $recentPlaneIssues.length === 0}
        <div class="hint">No recently updated issues assigned to you.</div>
      {:else}
        <input class="search-input" bind:value={issueSearch} placeholder="Search issues…" autocomplete="off" />
        <div class="list">
          {#each $recentPlaneIssues.filter((i) => !issueSearch.trim() || i.name.toLowerCase().includes(issueSearch.trim().toLowerCase())) as issue (issue.id)}
            <div class="issue-row">
              <button class="list-row issue-title" on:click={() => startFromIssue(issue)}>
                {issue.name}
              </button>
              <div class="issue-meta">
                <span>{issue.assignees.length ? issue.assignees.map(memberName).join(", ") : "Unassigned"}</span>
                <span>by {memberName(issue.created_by)}</span>
                <span>{shortDate(issue.created_at)}</span>
              </div>
            </div>
          {/each}
        </div>
      {/if}
      <button class="btn-close" on:click={closePanel}>Cancel</button>
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
    padding: 11px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .btn-track {
    background: #6366f1;
    color: white;
    border: none;
    border-radius: 7px;
    padding: 9px 14px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-track:hover:not(:disabled) { background: #4f46e5; }
  .btn-track:disabled { opacity: 0.5; cursor: not-allowed; }
  .menu-row { display: flex; gap: 8px; flex-wrap: wrap; }
  .menu-row button {
    flex: 1;
    background: #1e1e2c;
    color: #c0c0d0;
    border: 1px solid #2a2a38;
    border-radius: 7px;
    padding: 9px 10px;
    font-size: 12px;
    cursor: pointer;
  }
  .menu-row button:hover { background: #252532; }
  .panel-title {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: #7c8aa6;
  }
  .hint { color: #6b7280; font-size: 12px; }
  .list { display: flex; flex-direction: column; gap: 4px; max-height: 220px; overflow-y: auto; }
  .list-row {
    text-align: left;
    background: none;
    border: 1px solid #1e1e2c;
    border-radius: 6px;
    color: #c0c0d0;
    font-size: 12px;
    padding: 7px 10px;
    cursor: pointer;
  }
  .list-row:hover { background: #1a1a24; }
  .list-row.issue-title { flex: 1; display: flex; align-items: center; gap: 6px; }
  .search-input { width: 100%; box-sizing: border-box; }
  .issue-row {
    display: flex;
    flex-direction: column;
    gap: 3px;
    border-bottom: 1px solid #16161e;
    padding-bottom: 6px;
  }
  .issue-row.child { padding-left: 20px; border-bottom: none; padding-bottom: 0; }
  .issue-main { display: flex; align-items: center; gap: 4px; }
  .expand-btn {
    background: none;
    border: none;
    color: #7c8aa6;
    font-size: 11px;
    cursor: pointer;
    width: 16px;
    flex: 0 0 16px;
  }
  .expand-spacer { display: inline-block; width: 16px; flex: 0 0 16px; }
  .child-count {
    background: #1f2937;
    color: #93c5fd;
    border-radius: 4px;
    padding: 0 5px;
    font-size: 10px;
  }
  .issue-meta {
    display: flex;
    gap: 8px;
    font-size: 10px;
    color: #5a5a72;
    padding-left: 20px;
  }
  .issue-children {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 4px;
  }
  .pill {
    display: inline-block;
    background: #1f2937;
    color: #93c5fd;
    border-radius: 4px;
    padding: 1px 5px;
    font-size: 10px;
    font-weight: 700;
    margin-right: 4px;
  }
  .inline-custom { display: flex; flex-direction: column; gap: 6px; }
  input, select {
    background: #13131a;
    border: 1px solid #2a2a36;
    border-radius: 7px;
    padding: 8px 10px;
    color: #e0e0ec;
    font-size: 13px;
    outline: none;
  }
  /* Without color-scheme: dark, WebKitGTK renders the native option
     popup with its light-theme defaults regardless of the control's own
     CSS, which reads as unreadable black-on-black text. */
  select {
    color-scheme: dark;
  }
  select option {
    background: #13131a;
    color: #e0e0ec;
  }
  input:focus, select:focus { border-color: #6366f1; }
  .project-select { display: flex; flex-direction: column; gap: 4px; font-size: 11px; color: #7c8aa6; }
  .btn-close {
    background: none;
    border: none;
    color: #6b7280;
    font-size: 12px;
    cursor: pointer;
    align-self: flex-start;
    padding: 2px 0;
  }
  .btn-close:hover { color: #c0c0d0; }

  .task-bar {
    margin: 0 12px 4px;
    background: #111118;
    border: 1px solid #1a1a24;
    border-radius: 10px;
    padding: 9px 12px;
    display: flex;
    align-items: center;
    gap: 10px;
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
