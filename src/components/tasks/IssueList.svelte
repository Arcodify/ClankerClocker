<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import type { PlaneIssue, PlaneLabel } from "../../lib/types";
  import {
    projects,
    members,
    planeMe,
    statesByProject,
    labelsByProject,
    modulesByProject,
    cyclesByProject,
    taskTotals,
    createIssue,
    loadView,
    issueKey,
    formatTracked,
    links,
    openInPlane,
    loadFilters,
    saveFilters,
    filtersActive,
    viewProjectId,
    DEFAULT_FILTERS,
    STATE_GROUP_ORDER,
    STATE_GROUP_LABEL,
    PRIORITIES,
    PRIORITY_GLYPH,
    type IssueFilters,
    type TaskView,
  } from "../../lib/plane";

  export let view: TaskView;
  export let issues: PlaneIssue[];
  export let loading = false;
  export let selectedId: string | null;
  export let activeIssueId: string | null;

  const dispatch = createEventDispatcher<{
    select: string;
    start: PlaneIssue;
    refresh: void;
    error: string;
  }>();

  let search = "";
  let searchEl: HTMLInputElement;
  let quickEl: HTMLInputElement;
  let quickName = "";
  let quickProjectId = "";
  let creating = false;
  let filters: IssueFilters = { ...DEFAULT_FILTERS };
  let collapsedGroups = new Set<string>();
  let expandedParents = new Set<string>();
  let lastViewKey = "";

  // Swap in the persisted filters whenever the view changes.
  $: {
    const key = JSON.stringify(view);
    if (key !== lastViewKey) {
      lastViewKey = key;
      filters = loadFilters(view);
      search = "";
      expandedParents = new Set();
    }
  }
  $: saveFilters(view, filters);

  $: projectId = viewProjectId(view);
  $: if (!quickProjectId && $projects.length) quickProjectId = $projects[0].id;

  $: stateGroupOf = (i: PlaneIssue): string =>
    ($statesByProject[i.project] ?? []).find((s) => s.id === i.state)?.group ?? "backlog";
  $: stateNameOf = (i: PlaneIssue): string =>
    ($statesByProject[i.project] ?? []).find((s) => s.id === i.state)?.name ?? "";

  // Labels available for filtering — those of every project in this list.
  $: viewLabels = (() => {
    const seen = new Map<string, PlaneLabel>();
    for (const pid of new Set(issues.map((i) => i.project))) {
      for (const l of $labelsByProject[pid] ?? []) if (!seen.has(l.name)) seen.set(l.name, l);
    }
    return [...seen.values()].sort((a, b) => a.name.localeCompare(b.name));
  })();
  $: labelNamesById = new Map(
    Object.values($labelsByProject).flat().map((l) => [l.id, l.name] as [string, string]),
  );

  $: memberById = new Map($members.map((m) => [m.id, m]));

  function matches(i: PlaneIssue, f: IssueFilters, q: string): boolean {
    const g = stateGroupOf(i);
    const done = g === "completed" || g === "cancelled";
    if (f.state === "open" && done) return false;
    if (f.state === "done" && !done) return false;
    if (f.assignee === "me" && !i.assignees.includes($planeMe?.id ?? "")) return false;
    if (f.assignee === "unassigned" && i.assignees.length) return false;
    if (!["any", "me", "unassigned"].includes(f.assignee) && !i.assignees.includes(f.assignee)) return false;
    if (f.priority !== "any" && (i.priority ?? "none") !== f.priority) return false;
    if (f.label !== "any" && !i.labels.some((id) => labelNamesById.get(id) === f.label)) return false;
    if (q) {
      const hay = `${issueKey(i, $projects)} ${i.name}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  }

  $: query = search.trim().toLowerCase();
  $: filtered = issues.filter((i) => matches(i, filters, query));
  $: filteredIds = new Set(filtered.map((i) => i.id));
  // Sub-issues nest under their parent when the parent is also listed;
  // otherwise (or while searching) they're shown as ordinary rows.
  $: nest = !query;
  $: childrenByParent = (() => {
    const m = new Map<string, PlaneIssue[]>();
    if (!nest) return m;
    for (const i of filtered) {
      if (i.parent && filteredIds.has(i.parent)) {
        if (!m.has(i.parent)) m.set(i.parent, []);
        m.get(i.parent)!.push(i);
      }
    }
    return m;
  })();
  $: topLevel = filtered.filter((i) => !(nest && i.parent && filteredIds.has(i.parent)));

  const PRIORITY_RANK: Record<string, number> = { urgent: 0, high: 1, medium: 2, low: 3, none: 4 };
  $: groups = STATE_GROUP_ORDER.map((g) => ({
    group: g,
    items: topLevel
      .filter((i) => stateGroupOf(i) === g)
      .sort(
        (a, b) =>
          PRIORITY_RANK[a.priority ?? "none"] - PRIORITY_RANK[b.priority ?? "none"] ||
          b.updated_at.localeCompare(a.updated_at),
      ),
  })).filter((x) => x.items.length);

  export function visibleIds(): string[] {
    const ids: string[] = [];
    for (const g of groups) {
      if (collapsedGroups.has(g.group)) continue;
      for (const i of g.items) {
        ids.push(i.id);
        if (expandedParents.has(i.id)) for (const c of childrenByParent.get(i.id) ?? []) ids.push(c.id);
      }
    }
    return ids;
  }

  export function focusSearch() {
    searchEl?.focus();
    searchEl?.select();
  }

  export async function focusQuickAdd() {
    await tick();
    quickEl?.focus();
  }

  // Keep the keyboard-selected row in view.
  $: if (selectedId) {
    tick().then(() =>
      document.querySelector(`[data-issue="${selectedId}"]`)?.scrollIntoView({ block: "nearest" }),
    );
  }

  function toggleGroup(g: string) {
    if (collapsedGroups.has(g)) collapsedGroups.delete(g);
    else collapsedGroups.add(g);
    collapsedGroups = new Set(collapsedGroups);
  }

  function toggleParent(id: string) {
    if (expandedParents.has(id)) expandedParents.delete(id);
    else expandedParents.add(id);
    expandedParents = new Set(expandedParents);
  }

  function initials(id: string): string {
    const m = memberById.get(id);
    const name = (m?.display_name || m?.email || "?").trim();
    return name.slice(0, 2).toUpperCase();
  }

  function memberName(id: string): string {
    const m = memberById.get(id);
    return m?.display_name || m?.email || id.slice(0, 8);
  }

  async function quickAdd() {
    const name = quickName.trim();
    const pid = projectId ?? quickProjectId;
    if (!name || !pid) return;
    creating = true;
    try {
      const issue = await createIssue(pid, {
        name,
        // "My issues" only lists issues assigned to me — a new one created
        // there would otherwise vanish from the list it was typed into.
        assignees: view.kind === "mine" && $planeMe ? [$planeMe.id] : undefined,
        module_id: view.kind === "module" ? view.moduleId : undefined,
        cycle_id: view.kind === "cycle" ? view.cycleId : undefined,
      });
      quickName = "";
      await loadView(view, true);
      dispatch("select", issue.id);
    } catch (e) {
      dispatch("error", String(e));
    } finally {
      creating = false;
    }
  }

  $: title = (() => {
    if (view.kind === "mine") return "My issues";
    const p = $projects.find((x) => x.id === view.projectId);
    if (view.kind === "project") return p?.name ?? "Project";
    if (view.kind === "module")
      return ($modulesByProject[view.projectId] ?? []).find((m) => m.id === view.moduleId)?.name ?? "Module";
    return ($cyclesByProject[view.projectId] ?? []).find((c) => c.id === view.cycleId)?.name ?? "Cycle";
  })();
  $: subtitle = view.kind === "mine" ? "" : $projects.find((x) => x.id === viewProjectId(view))?.identifier ?? "";
</script>

<section class="list-pane">
  <header>
    <div class="title-row">
      <div class="title">
        {#if subtitle && view.kind !== "project"}<span class="crumb">{subtitle} /</span>{/if}
        {title}
        <span class="count">{filtered.length}</span>
      </div>
      <button class="icon" on:click={() => openInPlane(links.view(view))} title="Open in Plane">↗</button>
      <button class="icon" on:click={() => dispatch("refresh")} title="Refresh" disabled={loading}>⟳</button>
    </div>

    <div class="filters">
      <input
        class="search"
        bind:this={searchEl}
        bind:value={search}
        placeholder="Search  (Ctrl+K)"
        autocomplete="off"
        on:keydown={(e) => e.key === "Escape" && ((search = ""), searchEl.blur())}
      />
      <select bind:value={filters.state} class:on={filters.state !== DEFAULT_FILTERS.state}>
        <option value="open">Open</option>
        <option value="done">Done</option>
        <option value="all">All states</option>
      </select>
      {#if view.kind !== "mine"}
        <select bind:value={filters.assignee} class:on={filters.assignee !== "any"}>
          <option value="any">Anyone</option>
          <option value="me">Me</option>
          <option value="unassigned">Unassigned</option>
          {#each $members as m (m.id)}
            <option value={m.id}>{m.display_name || m.email}</option>
          {/each}
        </select>
      {/if}
      <select bind:value={filters.priority} class:on={filters.priority !== "any"}>
        <option value="any">Any priority</option>
        {#each PRIORITIES as p}
          <option value={p}>{PRIORITY_GLYPH[p]} {p[0].toUpperCase() + p.slice(1)}</option>
        {/each}
      </select>
      {#if viewLabels.length}
        <select bind:value={filters.label} class:on={filters.label !== "any"}>
          <option value="any">Any label</option>
          {#each viewLabels as l (l.id)}
            <option value={l.name}>{l.name}</option>
          {/each}
        </select>
      {/if}
      {#if filtersActive(filters)}
        <button class="clear" on:click={() => (filters = { ...DEFAULT_FILTERS })}>Clear</button>
      {/if}
    </div>
  </header>

  <div class="rows">
    {#if loading && !issues.length}
      <div class="empty">Loading issues…</div>
    {:else if !groups.length}
      <div class="empty">
        {issues.length ? "Nothing matches these filters." : "No issues here yet."}
      </div>
    {/if}

    {#each groups as g (g.group)}
      <button class="group-head" on:click={() => toggleGroup(g.group)}>
        <span class="caret">{collapsedGroups.has(g.group) ? "▸" : "▾"}</span>
        <span class="dot {g.group}"></span>
        {STATE_GROUP_LABEL[g.group]}
        <span class="count">{g.items.length}</span>
      </button>
      {#if !collapsedGroups.has(g.group)}
        {#each g.items as issue (issue.id)}
          {@const kids = childrenByParent.get(issue.id) ?? []}
          {@render row(issue, kids, false)}
          {#if kids.length && expandedParents.has(issue.id)}
            {#each kids as kid (kid.id)}
              {@render row(kid, [], true)}
            {/each}
          {/if}
        {/each}
      {/if}
    {/each}
  </div>

  <footer class="quick">
    {#if view.kind === "mine"}
      <select bind:value={quickProjectId} class="quick-proj" title="Project for the new issue">
        {#each $projects as p (p.id)}
          <option value={p.id}>{p.identifier}</option>
        {/each}
      </select>
    {/if}
    <input
      bind:this={quickEl}
      bind:value={quickName}
      placeholder={view.kind === "module"
        ? "+ New issue in this module…"
        : view.kind === "cycle"
          ? "+ New issue in this cycle…"
          : "+ New issue…  (N)"}
      autocomplete="off"
      disabled={creating}
      on:keydown={(e) => {
        if (e.key === "Enter") quickAdd();
        if (e.key === "Escape") quickEl.blur();
      }}
    />
    {#if quickName.trim()}
      <button class="add" on:click={quickAdd} disabled={creating}>{creating ? "…" : "Add"}</button>
    {/if}
  </footer>
</section>

{#snippet row(issue: PlaneIssue, kids: PlaneIssue[], child: boolean)}
  <div
    class="row"
    class:child
    class:selected={issue.id === selectedId}
    class:tracking={issue.id === activeIssueId}
    data-issue={issue.id}
    role="button"
    tabindex="-1"
    on:click={() => dispatch("select", issue.id)}
    on:dblclick={() => dispatch("start", issue)}
    on:keydown={() => {}}
  >
    {#if kids.length}
      <button
        class="caret sub"
        on:click|stopPropagation={() => toggleParent(issue.id)}
        title="{kids.length} sub-issue{kids.length === 1 ? '' : 's'}"
      >
        {expandedParents.has(issue.id) ? "▾" : "▸"}
      </button>
    {:else}
      <span class="caret-space"></span>
    {/if}
    <span class="prio {issue.priority ?? 'none'}" title="Priority: {issue.priority ?? 'none'}">
      {PRIORITY_GLYPH[issue.priority ?? "none"]}
    </span>
    <span class="key">{issueKey(issue, $projects)}</span>
    <span class="name" title={stateNameOf(issue)}>{issue.name}</span>
    {#if kids.length}<span class="kids">{kids.length}</span>{/if}
    {#if issue.target_date}
      <span class="due" class:overdue={new Date(issue.target_date) < new Date() && !["completed", "cancelled"].includes(stateGroupOf(issue))}>
        {new Date(issue.target_date).toLocaleDateString(undefined, { month: "short", day: "numeric" })}
      </span>
    {/if}
    {#if $taskTotals[issue.id]}
      <span class="tracked" title="Time you've tracked">{formatTracked($taskTotals[issue.id])}</span>
    {/if}
    <span class="avatars">
      {#each issue.assignees.slice(0, 2) as a}
        <span class="av" title={memberName(a)}>{initials(a)}</span>
      {/each}
      {#if issue.assignees.length > 2}<span class="av more">+{issue.assignees.length - 2}</span>{/if}
    </span>
    {#if issue.id === activeIssueId}
      <span class="playing" title="Tracking">●</span>
    {:else}
      <button class="play" on:click|stopPropagation={() => dispatch("start", issue)} title="Start tracking">▶</button>
    {/if}
  </div>
{/snippet}

<style>
  .list-pane { display: flex; flex-direction: column; min-height: 0; min-width: 0; border-right: 1px solid #1a1a24; }
  header { padding: 12px 14px 8px; border-bottom: 1px solid #16161e; display: flex; flex-direction: column; gap: 10px; }
  .title-row { display: flex; align-items: center; gap: 6px; }
  .title { flex: 1; font-size: 15px; font-weight: 700; color: #e8e8ec; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .crumb { color: #5a5a72; font-weight: 500; margin-right: 4px; }
  .count { font-size: 11px; color: #5a5a72; font-weight: 500; margin-left: 6px; }
  .icon {
    background: none;
    border: 1px solid #22222e;
    color: #9ca3af;
    border-radius: 6px;
    width: 28px;
    height: 26px;
    cursor: pointer;
  }
  .icon:hover:not(:disabled) { color: #e0e0ec; background: #1a1a24; }
  .icon:disabled { opacity: 0.4; }

  .filters { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  .search, select, .quick input {
    background: #13131a;
    border: 1px solid #22222e;
    border-radius: 6px;
    padding: 5px 8px;
    color: #e0e0ec;
    font-size: 12px;
    outline: none;
  }
  .search { flex: 1 1 160px; min-width: 120px; }
  select { color-scheme: dark; cursor: pointer; color: #9ca3af; }
  select option { background: #13131a; color: #e0e0ec; }
  select.on { border-color: #4f46e5; color: #c7d2fe; background: #1e1b4b; }
  .search:focus, select:focus, .quick input:focus { border-color: #6366f1; }
  .clear { background: none; border: none; color: #818cf8; font-size: 12px; cursor: pointer; }

  .rows { flex: 1; overflow-y: auto; min-height: 0; padding: 4px 0 8px; }
  .empty { padding: 40px 20px; text-align: center; color: #5a5a72; font-size: 13px; }
  .group-head {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    background: #0f0f14;
    border: none;
    border-bottom: 1px solid #16161e;
    color: #9ca3af;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.4px;
    text-transform: uppercase;
    padding: 7px 14px;
    cursor: pointer;
    position: sticky;
    top: 0;
    z-index: 1;
  }
  .dot { width: 8px; height: 8px; border-radius: 50%; }
  .dot.started { background: #f59e0b; }
  .dot.unstarted { background: #9ca3af; }
  .dot.backlog { background: transparent; border: 1.5px dashed #6b7280; }
  .dot.completed { background: #22c55e; }
  .dot.cancelled { background: #ef4444; }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 14px 6px 8px;
    font-size: 13px;
    cursor: default;
    border-left: 2px solid transparent;
  }
  .row:hover { background: #13131a; }
  .row.selected { background: #1a1833; border-left-color: #6366f1; }
  .row.tracking .name { color: #86efac; }
  .row.child { padding-left: 34px; }
  .caret, .caret-space { width: 16px; flex: 0 0 16px; }
  .caret { background: none; border: none; color: #5a5a72; cursor: pointer; font-size: 10px; }
  .caret.sub:hover { color: #e0e0ec; }
  .prio { width: 14px; text-align: center; font-size: 11px; flex: 0 0 14px; }
  .prio.urgent { color: #ef4444; }
  .prio.high { color: #f97316; }
  .prio.medium { color: #eab308; }
  .prio.low { color: #60a5fa; }
  .prio.none { color: #3f3f50; }
  .key { color: #5a5a72; font-size: 11px; font-variant-numeric: tabular-nums; flex: 0 0 auto; min-width: 54px; }
  .name { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: #d4d4e0; }
  .kids { font-size: 10px; background: #1f2937; color: #93c5fd; border-radius: 4px; padding: 0 5px; }
  .due { font-size: 11px; color: #6b7280; }
  .due.overdue { color: #f87171; }
  .tracked { font-size: 11px; color: #93c5fd; font-variant-numeric: tabular-nums; }
  .avatars { display: flex; gap: 2px; }
  .av {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: #252532;
    color: #c0c0d0;
    font-size: 9px;
    font-weight: 700;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .av.more { background: #1a1a24; color: #6b7280; }
  .play {
    background: none;
    border: none;
    color: #6366f1;
    cursor: pointer;
    font-size: 11px;
    width: 18px;
    visibility: hidden;
  }
  .row:hover .play, .row.selected .play { visibility: visible; }
  .play:hover { color: #a5b4fc; }
  .playing { color: #22c55e; width: 18px; text-align: center; font-size: 10px; }

  .quick { display: flex; gap: 6px; padding: 8px 14px; border-top: 1px solid #16161e; }
  .quick input { flex: 1; }
  .quick-proj { flex: 0 0 auto; }
  .add {
    background: #6366f1;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0 12px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .add:disabled { opacity: 0.5; }
</style>
