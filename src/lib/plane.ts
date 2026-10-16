import { writable, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import type {
  PlaneProject,
  PlaneIssue,
  PlaneState,
  PlaneMember,
  PlaneModule,
  PlaneCycle,
  PlaneLabel,
  PlaneUser,
  PlanePriority,
  AppSettings,
} from "./types";

// ── Views ────────────────────────────────────────────────────────────────

export type TaskView =
  | { kind: "mine" }
  | { kind: "project"; projectId: string }
  | { kind: "module"; projectId: string; moduleId: string }
  | { kind: "cycle"; projectId: string; cycleId: string };

export function viewKey(v: TaskView): string {
  switch (v.kind) {
    case "mine":
      return "mine";
    case "project":
      return `p:${v.projectId}`;
    case "module":
      return `m:${v.moduleId}`;
    case "cycle":
      return `c:${v.cycleId}`;
  }
}

export function viewProjectId(v: TaskView): string | null {
  return v.kind === "mine" ? null : v.projectId;
}

// ── Caches (shared by every component in the Tasks window) ───────────────

export const planeMe = writable<PlaneUser | null>(null);
export const projects = writable<PlaneProject[]>([]);
export const members = writable<PlaneMember[]>([]);
export const statesByProject = writable<Record<string, PlaneState[]>>({});
export const labelsByProject = writable<Record<string, PlaneLabel[]>>({});
export const modulesByProject = writable<Record<string, PlaneModule[]>>({});
export const cyclesByProject = writable<Record<string, PlaneCycle[]>>({});
export const issuesByView = writable<Record<string, PlaneIssue[]>>({});
/** Seconds tracked per Plane issue id (all-time, current user). */
export const taskTotals = writable<Record<string, number>>({});

let webBase = "";

export async function loadBasics(): Promise<void> {
  const s = await invoke<AppSettings>("get_settings");
  webBase = planeWebBase(s.pm_base_url, s.pm_workspace_slug);
  const [me, ps, ms] = await Promise.all([
    invoke<PlaneUser>("get_plane_me"),
    invoke<PlaneProject[]>("list_plane_projects"),
    invoke<PlaneMember[]>("list_plane_members").catch(() => [] as PlaneMember[]),
  ]);
  planeMe.set(me);
  projects.set(ps);
  members.set(ms);
  refreshTotals();
}

export function refreshTotals(): void {
  invoke<Record<string, number>>("get_task_totals")
    .then((t) => taskTotals.set(t))
    .catch(() => {});
}

/** States + labels for a project — needed to group and filter its issues. */
export async function ensureProjectMeta(projectId: string): Promise<void> {
  const jobs: Promise<unknown>[] = [];
  if (!get(statesByProject)[projectId]) {
    jobs.push(
      invoke<PlaneState[]>("list_plane_project_states", { projectId }).then((s) =>
        statesByProject.update((m) => ({ ...m, [projectId]: s })),
      ),
    );
  }
  if (!get(labelsByProject)[projectId]) {
    jobs.push(
      invoke<PlaneLabel[]>("list_plane_labels", { projectId })
        .catch(() => [] as PlaneLabel[])
        .then((l) => labelsByProject.update((m) => ({ ...m, [projectId]: l }))),
    );
  }
  await Promise.all(jobs);
}

/** Modules + cycles, loaded when a project is expanded in the sidebar. */
export async function ensureProjectGroups(projectId: string): Promise<void> {
  const jobs: Promise<unknown>[] = [];
  if (!get(modulesByProject)[projectId]) {
    jobs.push(
      invoke<PlaneModule[]>("list_plane_modules", { projectId })
        .catch(() => [] as PlaneModule[])
        .then((x) => modulesByProject.update((m) => ({ ...m, [projectId]: x }))),
    );
  }
  if (!get(cyclesByProject)[projectId]) {
    jobs.push(
      invoke<PlaneCycle[]>("list_plane_cycles", { projectId })
        .catch(() => [] as PlaneCycle[])
        .then((x) => cyclesByProject.update((m) => ({ ...m, [projectId]: x }))),
    );
  }
  await Promise.all(jobs);
}

export async function loadView(v: TaskView, force = false): Promise<PlaneIssue[]> {
  const key = viewKey(v);
  const cached = get(issuesByView)[key];
  if (cached && !force) return cached;
  let issues: PlaneIssue[];
  switch (v.kind) {
    case "mine":
      issues = await invoke<PlaneIssue[]>("list_my_plane_issues");
      break;
    case "project":
      issues = await invoke<PlaneIssue[]>("list_plane_project_issues", { projectId: v.projectId });
      break;
    case "module":
      issues = await invoke<PlaneIssue[]>("list_plane_module_issues", {
        projectId: v.projectId,
        moduleId: v.moduleId,
      });
      break;
    case "cycle":
      issues = await invoke<PlaneIssue[]>("list_plane_cycle_issues", {
        projectId: v.projectId,
        cycleId: v.cycleId,
      });
      break;
  }
  await Promise.all([...new Set(issues.map((i) => i.project))].map(ensureProjectMeta));
  issuesByView.update((m) => ({ ...m, [key]: issues }));
  return issues;
}

/** Applies a local edit to every cached copy of an issue so all views agree. */
export function patchCachedIssue(issueId: string, patch: Partial<PlaneIssue>): void {
  issuesByView.update((m) => {
    const out: Record<string, PlaneIssue[]> = {};
    for (const [k, list] of Object.entries(m)) {
      out[k] = list.map((i) => (i.id === issueId ? { ...i, ...patch } : i));
    }
    return out;
  });
}

export async function updateIssue(issue: PlaneIssue, patch: Partial<PlaneIssue>): Promise<void> {
  const before: Partial<PlaneIssue> = {};
  for (const k of Object.keys(patch) as (keyof PlaneIssue)[]) (before as any)[k] = issue[k];
  patchCachedIssue(issue.id, patch); // optimistic
  try {
    await invoke("update_plane_issue", { projectId: issue.project, issueId: issue.id, patch });
  } catch (e) {
    patchCachedIssue(issue.id, before);
    throw e;
  }
}

export interface NewIssueInput {
  name: string;
  priority?: PlanePriority;
  assignees?: string[];
  module_id?: string;
  cycle_id?: string;
}

export async function createIssue(projectId: string, input: NewIssueInput): Promise<PlaneIssue> {
  const issue = await invoke<PlaneIssue>("create_plane_issue", { projectId, issue: input });
  // Drop cached lists that should now contain it so they refetch.
  issuesByView.update((m) => {
    const out = { ...m };
    delete out["mine"];
    delete out[`p:${projectId}`];
    if (input.module_id) delete out[`m:${input.module_id}`];
    if (input.cycle_id) delete out[`c:${input.cycle_id}`];
    return out;
  });
  return issue;
}

// ── Links into Plane's web app ───────────────────────────────────────────

/**
 * Plane's web UI lives on the same origin as its API for self-hosted
 * installs; Plane Cloud splits them (api.plane.so vs app.plane.so).
 */
export function planeWebBase(apiBase: string, slug: string): string {
  if (!apiBase || !slug) return "";
  const origin = apiBase.replace(/\/+$/, "").replace(/\/api(\/v1)?$/, "").replace("://api.plane.so", "://app.plane.so");
  return `${origin}/${slug}`;
}

export const links = {
  project: (projectId: string) => (webBase ? `${webBase}/projects/${projectId}/issues/` : ""),
  issue: (i: PlaneIssue) => (webBase ? `${webBase}/projects/${i.project}/issues/${i.id}` : ""),
  module: (projectId: string, moduleId: string) =>
    webBase ? `${webBase}/projects/${projectId}/modules/${moduleId}` : "",
  cycle: (projectId: string, cycleId: string) =>
    webBase ? `${webBase}/projects/${projectId}/cycles/${cycleId}` : "",
  view: (v: TaskView) => {
    switch (v.kind) {
      case "mine":
        return webBase ? `${webBase}/profile/${get(planeMe)?.id ?? ""}/assigned` : "";
      case "project":
        return links.project(v.projectId);
      case "module":
        return links.module(v.projectId, v.moduleId);
      case "cycle":
        return links.cycle(v.projectId, v.cycleId);
    }
  },
};

export function openInPlane(url: string): void {
  if (url) open(url).catch(() => {});
}

// ── Display helpers ──────────────────────────────────────────────────────

export const STATE_GROUP_ORDER = ["started", "unstarted", "backlog", "completed", "cancelled"] as const;
export const STATE_GROUP_LABEL: Record<string, string> = {
  started: "In progress",
  unstarted: "Todo",
  backlog: "Backlog",
  completed: "Done",
  cancelled: "Cancelled",
};
export const PRIORITIES: PlanePriority[] = ["urgent", "high", "medium", "low", "none"];
export const PRIORITY_GLYPH: Record<string, string> = {
  urgent: "‼",
  high: "▲",
  medium: "■",
  low: "▼",
  none: "–",
};

export function issueKey(i: PlaneIssue, ps: PlaneProject[]): string {
  const p = ps.find((x) => x.id === i.project);
  return p && i.sequence_id != null ? `${p.identifier}-${i.sequence_id}` : "";
}

export function formatTracked(seconds: number | undefined): string {
  if (!seconds) return "";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h === 0) return `${m}m`;
  return m ? `${h}h ${m}m` : `${h}h`;
}

/** Plane cycles have no status field — derive it from the dates. */
export function cyclePhase(c: PlaneCycle): "current" | "upcoming" | "completed" | "draft" {
  if (!c.start_date || !c.end_date) return "draft";
  const now = Date.now();
  if (new Date(c.end_date).getTime() + 86_400_000 < now) return "completed";
  if (new Date(c.start_date).getTime() > now) return "upcoming";
  return "current";
}

// ── Filters (persisted per view) ─────────────────────────────────────────

export interface IssueFilters {
  state: "open" | "all" | "done";
  assignee: "any" | "me" | "unassigned" | string;
  priority: "any" | PlanePriority;
  label: "any" | string;
}

export const DEFAULT_FILTERS: IssueFilters = { state: "open", assignee: "any", priority: "any", label: "any" };

export function loadFilters(v: TaskView): IssueFilters {
  try {
    const raw = localStorage.getItem(`taskFilters:${viewKey(v)}`);
    if (raw) return { ...DEFAULT_FILTERS, ...JSON.parse(raw) };
  } catch (_) {}
  return { ...DEFAULT_FILTERS };
}

export function saveFilters(v: TaskView, f: IssueFilters): void {
  try {
    localStorage.setItem(`taskFilters:${viewKey(v)}`, JSON.stringify(f));
  } catch (_) {}
}

export function filtersActive(f: IssueFilters): boolean {
  return (Object.keys(DEFAULT_FILTERS) as (keyof IssueFilters)[]).some((k) => f[k] !== DEFAULT_FILTERS[k]);
}
