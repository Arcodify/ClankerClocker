<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import {
    projects,
    modulesByProject,
    cyclesByProject,
    ensureProjectGroups,
    viewKey,
    links,
    openInPlane,
    cyclePhase,
    type TaskView,
  } from "../../lib/plane";

  export let view: TaskView;

  const dispatch = createEventDispatcher<{ select: TaskView; error: string }>();

  let expanded = new Set<string>();
  let loadingGroups = new Set<string>();
  let showDoneCycles = new Set<string>();
  let filter = "";

  $: activeKey = viewKey(view);
  $: visibleProjects = $projects.filter(
    (p) => !filter.trim() || `${p.identifier} ${p.name}`.toLowerCase().includes(filter.trim().toLowerCase()),
  );

  async function toggle(projectId: string) {
    if (expanded.has(projectId)) {
      expanded.delete(projectId);
      expanded = new Set(expanded);
      return;
    }
    expanded = new Set(expanded.add(projectId));
    loadingGroups = new Set(loadingGroups.add(projectId));
    try {
      await ensureProjectGroups(projectId);
    } catch (e) {
      dispatch("error", String(e));
    } finally {
      loadingGroups.delete(projectId);
      loadingGroups = new Set(loadingGroups);
    }
  }

  function selectProject(projectId: string) {
    dispatch("select", { kind: "project", projectId });
    if (!expanded.has(projectId)) toggle(projectId);
  }

  function sortedCycles(projectId: string, showDone: boolean) {
    const order = { current: 0, upcoming: 1, draft: 2, completed: 3 };
    return ($cyclesByProject[projectId] ?? [])
      .map((c) => ({ c, phase: cyclePhase(c) }))
      .filter((x) => showDone || x.phase !== "completed")
      .sort((a, b) => order[a.phase] - order[b.phase]);
  }

  function hiddenDoneCount(projectId: string) {
    return ($cyclesByProject[projectId] ?? []).filter((c) => cyclePhase(c) === "completed").length;
  }

  function toggleDone(projectId: string) {
    if (showDoneCycles.has(projectId)) showDoneCycles.delete(projectId);
    else showDoneCycles.add(projectId);
    showDoneCycles = new Set(showDoneCycles);
  }
</script>

<aside class="sidebar">
  <button class="nav" class:active={activeKey === "mine"} on:click={() => dispatch("select", { kind: "mine" })}>
    <span class="ico">★</span> My issues
  </button>

  <div class="section">
    <span>Projects</span>
    {#if $projects.length > 6}
      <input class="filter" bind:value={filter} placeholder="Filter…" autocomplete="off" />
    {/if}
  </div>

  <div class="tree">
    {#each visibleProjects as p (p.id)}
      {@const open = expanded.has(p.id)}
      <div class="row" class:active={activeKey === `p:${p.id}`}>
        <button class="caret" on:click={() => toggle(p.id)} aria-label={open ? "Collapse" : "Expand"}>
          {open ? "▾" : "▸"}
        </button>
        <button class="label" on:click={() => selectProject(p.id)} title={p.name}>
          <span class="ident">{p.identifier}</span>{p.name}
        </button>
        <button class="ext" on:click={() => openInPlane(links.project(p.id))} title="Open in Plane">↗</button>
      </div>

      {#if open}
        {#if loadingGroups.has(p.id)}
          <div class="hint">Loading…</div>
        {:else}
          {@const mods = $modulesByProject[p.id] ?? []}
          {@const showDone = showDoneCycles.has(p.id)}
          {@const cycles = sortedCycles(p.id, showDone)}
          {@const doneCount = hiddenDoneCount(p.id)}

          {#if mods.length}
            <div class="group">Modules</div>
            {#each mods as m (m.id)}
              <div class="row child" class:active={activeKey === `m:${m.id}`}>
                <button
                  class="label"
                  on:click={() => dispatch("select", { kind: "module", projectId: p.id, moduleId: m.id })}
                  title={m.name}
                >
                  <span class="ico">◇</span>{m.name}
                </button>
                <button class="ext" on:click={() => openInPlane(links.module(p.id, m.id))} title="Open in Plane">↗</button>
              </div>
            {/each}
          {/if}

          {#if cycles.length || doneCount}
            <div class="group">Cycles</div>
            {#each cycles as { c, phase } (c.id)}
              <div class="row child" class:active={activeKey === `c:${c.id}`} class:dim={phase === "completed"}>
                <button
                  class="label"
                  on:click={() => dispatch("select", { kind: "cycle", projectId: p.id, cycleId: c.id })}
                  title={c.name}
                >
                  <span class="ico">↻</span>{c.name}
                  {#if phase === "current"}<span class="tag">now</span>{/if}
                </button>
                <button class="ext" on:click={() => openInPlane(links.cycle(p.id, c.id))} title="Open in Plane">↗</button>
              </div>
            {/each}
            {#if doneCount}
              <button class="more" on:click={() => toggleDone(p.id)}>
                {showDone ? "Hide completed" : `Show ${doneCount} completed`}
              </button>
            {/if}
          {/if}

          {#if !mods.length && !cycles.length && !doneCount}
            <div class="hint">No modules or cycles</div>
          {/if}
        {/if}
      {/if}
    {:else}
      <div class="hint">No projects</div>
    {/each}
  </div>
</aside>

<style>
  .sidebar {
    background: #0b0b0e;
    border-right: 1px solid #1a1a24;
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 10px 8px;
    gap: 2px;
    font-size: 13px;
  }
  .nav, .label {
    background: none;
    border: none;
    color: #c0c0d0;
    text-align: left;
    cursor: pointer;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .nav { padding: 7px 8px; border-radius: 6px; }
  .nav:hover, .row:hover { background: #15151d; }
  .nav.active, .row.active { background: #1e1b4b; color: #e0e0ec; }
  .row.active .label { color: #e0e0ec; }
  .ico { color: #6b7280; margin-right: 6px; font-size: 11px; }
  .section {
    margin: 12px 8px 4px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: #4a4a62;
  }
  .filter {
    width: 100px;
    background: #13131a;
    border: 1px solid #22222e;
    border-radius: 5px;
    color: #e0e0ec;
    font-size: 11px;
    padding: 3px 6px;
    outline: none;
    text-transform: none;
    letter-spacing: 0;
  }
  .tree { overflow-y: auto; flex: 1; min-height: 0; display: flex; flex-direction: column; gap: 1px; }
  .row { display: flex; align-items: center; border-radius: 6px; padding-right: 4px; }
  .row.child { padding-left: 22px; }
  .row.dim .label { color: #5a5a72; }
  .label { flex: 1; min-width: 0; padding: 6px 4px; }
  .caret {
    background: none;
    border: none;
    color: #5a5a72;
    width: 20px;
    flex: 0 0 20px;
    cursor: pointer;
    font-size: 11px;
  }
  .ident {
    display: inline-block;
    background: #1f2937;
    color: #93c5fd;
    border-radius: 4px;
    padding: 0 5px;
    font-size: 10px;
    font-weight: 700;
    margin-right: 6px;
  }
  .ext {
    background: none;
    border: none;
    color: #6b7280;
    cursor: pointer;
    padding: 2px 5px;
    border-radius: 4px;
    visibility: hidden;
  }
  .row:hover .ext { visibility: visible; }
  .ext:hover { color: #e0e0ec; background: #252532; }
  .group {
    padding: 6px 0 2px 30px;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: #4a4a62;
  }
  .tag {
    margin-left: 6px;
    font-size: 9px;
    background: #052e16;
    color: #4ade80;
    border-radius: 4px;
    padding: 0 4px;
  }
  .more {
    background: none;
    border: none;
    color: #6366f1;
    font-size: 11px;
    text-align: left;
    padding: 3px 0 3px 30px;
    cursor: pointer;
  }
  .hint { padding: 4px 0 4px 30px; color: #4a4a62; font-size: 11px; }
</style>
