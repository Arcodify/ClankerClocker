<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { PlaneIssue, PlanePriority } from "../../lib/types";
  import {
    projects,
    members,
    planeMe,
    statesByProject,
    labelsByProject,
    taskTotals,
    updateIssue,
    issueKey,
    formatTracked,
    links,
    openInPlane,
    PRIORITIES,
    PRIORITY_GLYPH,
    STATE_GROUP_ORDER,
  } from "../../lib/plane";

  export let issue: PlaneIssue | null;
  export let clockedIn: boolean;
  export let busy = false;
  export let isActive = false;
  export let hasActive = false;

  const dispatch = createEventDispatcher<{ start: PlaneIssue; end: void; error: string }>();

  let copied = false;

  $: states = issue
    ? [...($statesByProject[issue.project] ?? [])].sort(
        (a, b) => STATE_GROUP_ORDER.indexOf(a.group as any) - STATE_GROUP_ORDER.indexOf(b.group as any),
      )
    : [];
  $: currentState = states.find((s) => s.id === issue?.state);
  $: isDone = currentState?.group === "completed" || currentState?.group === "cancelled";
  $: project = issue ? $projects.find((p) => p.id === issue!.project) : undefined;
  $: labels = issue
    ? ($labelsByProject[issue.project] ?? []).filter((l) => issue!.labels.includes(l.id))
    : [];
  $: memberById = new Map($members.map((m) => [m.id, m]));
  $: unassigned = $members.filter((m) => !issue?.assignees.includes(m.id));

  async function save(patch: Partial<PlaneIssue>) {
    if (!issue) return;
    try {
      await updateIssue(issue, patch);
    } catch (e) {
      dispatch("error", String(e));
    }
  }

  function markDone() {
    const done = states.find((s) => s.group === "completed");
    if (done) save({ state: done.id });
  }

  function addAssignee(e: Event) {
    const sel = e.target as HTMLSelectElement;
    if (issue && sel.value) save({ assignees: [...issue.assignees, sel.value] });
    sel.value = "";
  }

  function removeAssignee(id: string) {
    if (issue) save({ assignees: issue.assignees.filter((a) => a !== id) });
  }

  async function copyLink() {
    if (!issue) return;
    try {
      await navigator.clipboard.writeText(links.issue(issue));
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      dispatch("error", "Couldn't copy link");
    }
  }

  function memberName(id: string) {
    const m = memberById.get(id);
    return m?.display_name || m?.email || id.slice(0, 8);
  }
</script>

<aside class="detail">
  {#if !issue}
    <div class="placeholder">
      <div>Select an issue</div>
      <div class="keys">
        <span><kbd>↑</kbd><kbd>↓</kbd> move</span>
        <span><kbd>Enter</kbd> track</span>
        <span><kbd>O</kbd> open in Plane</span>
        <span><kbd>N</kbd> new issue</span>
        <span><kbd>Ctrl</kbd><kbd>K</kbd> search</span>
      </div>
    </div>
  {:else}
    <div class="top">
      <span class="key">{issueKey(issue, $projects) || project?.identifier || ""}</span>
      <span class="spacer"></span>
      <button class="link" on:click={copyLink} title="Copy link">{copied ? "Copied" : "Copy link"}</button>
      <button class="link strong" on:click={() => issue && openInPlane(links.issue(issue))} title="Open in Plane">
        Open in Plane ↗
      </button>
    </div>

    <h2 class="name">{issue.name}</h2>

    {#if issue.description_stripped?.trim()}
      <p class="desc">{issue.description_stripped.trim()}</p>
    {/if}

    <div class="actions">
      {#if isActive}
        <button class="btn stop" on:click={() => dispatch("end")}>■ End tracking</button>
      {:else}
        <button
          class="btn primary"
          disabled={!clockedIn || busy}
          title={clockedIn ? "" : "Clock in from the main window first"}
          on:click={() => issue && dispatch("start", issue)}
        >
          ▶ {hasActive ? "Switch to this" : "Start tracking"}
        </button>
      {/if}
      {#if !isDone}
        <button class="btn" on:click={markDone}>✓ Done</button>
      {/if}
    </div>
    {#if !clockedIn}
      <div class="note">Clock in from the main window to track time.</div>
    {/if}

    <dl class="props">
      <dt>State</dt>
      <dd>
        <select value={issue.state} on:change={(e) => save({ state: e.currentTarget.value })}>
          {#each states as s (s.id)}
            <option value={s.id}>{s.name}</option>
          {/each}
        </select>
      </dd>

      <dt>Priority</dt>
      <dd>
        <select
          value={issue.priority ?? "none"}
          on:change={(e) => save({ priority: e.currentTarget.value as PlanePriority })}
        >
          {#each PRIORITIES as p}
            <option value={p}>{PRIORITY_GLYPH[p]} {p[0].toUpperCase() + p.slice(1)}</option>
          {/each}
        </select>
      </dd>

      <dt>Assignees</dt>
      <dd class="chips">
        {#each issue.assignees as a (a)}
          <span class="chip">
            {memberName(a)}{a === $planeMe?.id ? " (you)" : ""}
            <button on:click={() => removeAssignee(a)} title="Remove">×</button>
          </span>
        {/each}
        {#if unassigned.length}
          <select class="add-sel" on:change={addAssignee}>
            <option value="">+ Add</option>
            {#if $planeMe && !issue.assignees.includes($planeMe.id)}
              <option value={$planeMe.id}>Me</option>
            {/if}
            {#each unassigned.filter((m) => m.id !== $planeMe?.id) as m (m.id)}
              <option value={m.id}>{m.display_name || m.email}</option>
            {/each}
          </select>
        {/if}
      </dd>

      <dt>Start</dt>
      <dd>
        <input
          type="date"
          value={issue.start_date ?? ""}
          on:change={(e) => save({ start_date: e.currentTarget.value || null })}
        />
      </dd>

      <dt>Due</dt>
      <dd>
        <input
          type="date"
          value={issue.target_date ?? ""}
          on:change={(e) => save({ target_date: e.currentTarget.value || null })}
        />
      </dd>

      {#if labels.length}
        <dt>Labels</dt>
        <dd class="chips">
          {#each labels as l (l.id)}
            <span class="label" style="--c:{l.color || '#6b7280'}">{l.name}</span>
          {/each}
        </dd>
      {/if}

      <dt>Project</dt>
      <dd>
        {#if project}
          <button class="plain" on:click={() => openInPlane(links.project(project!.id))}>{project.name} ↗</button>
        {/if}
      </dd>

      <dt>Tracked</dt>
      <dd class="tracked">{formatTracked($taskTotals[issue.id]) || "—"}</dd>
    </dl>
  {/if}
</aside>

<style>
  .detail {
    background: #0b0b0e;
    padding: 14px 16px;
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    font-size: 13px;
  }
  .placeholder {
    margin: auto;
    text-align: center;
    color: #4a4a62;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .keys { display: flex; flex-direction: column; gap: 6px; font-size: 11px; }
  kbd {
    background: #1a1a24;
    border: 1px solid #2a2a38;
    border-radius: 4px;
    padding: 0 5px;
    margin-right: 2px;
    font-family: inherit;
    font-size: 10px;
    color: #9ca3af;
  }
  .top { display: flex; align-items: center; gap: 8px; }
  .key { font-size: 12px; color: #7c8aa6; font-weight: 600; }
  .spacer { flex: 1; }
  .link { background: none; border: none; color: #6b7280; font-size: 12px; cursor: pointer; }
  .link:hover { color: #e0e0ec; }
  .link.strong { color: #818cf8; }
  .link.strong:hover { color: #c7d2fe; }
  .name { font-size: 16px; font-weight: 700; line-height: 1.35; color: #f0f0f5; user-select: text; }
  .desc {
    font-size: 12px;
    color: #8b8ba0;
    line-height: 1.5;
    display: -webkit-box;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
    user-select: text;
  }
  .actions { display: flex; gap: 8px; }
  .btn {
    background: #1e1e2c;
    color: #c0c0d0;
    border: 1px solid #2a2a38;
    border-radius: 7px;
    padding: 8px 12px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn:hover:not(:disabled) { background: #252532; }
  .btn:disabled { opacity: 0.45; cursor: not-allowed; }
  .btn.primary { flex: 1; background: #6366f1; border-color: #6366f1; color: white; }
  .btn.primary:hover:not(:disabled) { background: #4f46e5; }
  .btn.stop { flex: 1; color: #fca5a5; border-color: #3a2020; background: #1f1414; }
  .note { font-size: 11px; color: #6b7280; margin-top: -6px; }

  .props {
    display: grid;
    grid-template-columns: 76px minmax(0, 1fr);
    gap: 8px 10px;
    align-items: center;
    border-top: 1px solid #16161e;
    padding-top: 12px;
  }
  dt { font-size: 11px; color: #5a5a72; }
  dd { min-width: 0; }
  select, input[type="date"] {
    width: 100%;
    background: #13131a;
    border: 1px solid #22222e;
    border-radius: 6px;
    padding: 5px 8px;
    color: #e0e0ec;
    font-size: 12px;
    outline: none;
    color-scheme: dark;
    font-family: inherit;
  }
  select option { background: #13131a; color: #e0e0ec; }
  select:focus, input:focus { border-color: #6366f1; }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: #1a1a24;
    border: 1px solid #252532;
    border-radius: 12px;
    padding: 2px 4px 2px 8px;
    font-size: 11px;
    color: #c0c0d0;
  }
  .chip button { background: none; border: none; color: #6b7280; cursor: pointer; font-size: 13px; line-height: 1; }
  .chip button:hover { color: #fca5a5; }
  .add-sel { width: auto; padding: 2px 6px; font-size: 11px; color: #818cf8; }
  .label {
    font-size: 11px;
    border-radius: 10px;
    padding: 1px 8px;
    border: 1px solid var(--c);
    color: var(--c);
  }
  .plain { background: none; border: none; color: #c0c0d0; cursor: pointer; font-size: 12px; text-align: left; padding: 0; }
  .plain:hover { color: #e0e0ec; }
  .tracked { color: #93c5fd; font-variant-numeric: tabular-nums; }
</style>
