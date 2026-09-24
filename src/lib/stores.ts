import { writable, derived } from "svelte/store";
import type { SessionState, ActivitySnapshot, NetworkConnection, AppSettings, TodayStats, TeamMember, PlaneProject, PlaneIssue, PlaneMember, PlaneState, TaskRecord } from "./types";

export const session = writable<SessionState>({
  status: "idle",
  session_id: null,
  clock_in: null,
  break_start: null,
  break_name: null,
  total_break_seconds: 0,
  break_count: 0,
});

export const latestActivity = writable<ActivitySnapshot | null>(null);
export const networkFeed = writable<NetworkConnection[]>([]);
export const settings = writable<AppSettings>({
  pb_url: "",
  pb_email: "",
  pb_password: "",
  is_admin: false,
  clock_in_time: "09:00",
  clock_out_time: "18:00",
  auto_clock_out_enabled: true,
  pm_enabled: false,
  pm_workspace_slug: "",
  pm_base_url: "",
});
export const authToken = writable<string>("");
export const userId = writable<string>("");
export const isAdmin = writable<boolean>(false);
export const userName = writable<string>("");
export const errorMessage = writable<string>("");
export const view = writable<"login" | "dashboard" | "settings" | "about" | "admin" | "apiKeys">("login");
export const todayStats = writable<TodayStats | null>(null);
export const teamStatus = writable<TeamMember[]>([]);
export const elapsedSeconds = writable<number>(0);

export const activeTask = writable<TaskRecord | null>(null);
export const taskElapsedSeconds = writable<number>(0);
export const planeProjects = writable<PlaneProject[]>([]);
export const planeIssuesByProject = writable<Record<string, PlaneIssue[]>>({});
export const planeStatesByProject = writable<Record<string, PlaneState[]>>({});
export const planeMembers = writable<PlaneMember[]>([]);
export const recentPlaneIssues = writable<PlaneIssue[]>([]);
export const taskPanelView = writable<
  "closed" | "menu" | "pickProject" | "pickIssue" | "custom" | "recent"
>("closed");

export const formattedElapsed = derived(elapsedSeconds, ($s) => {
  const h = Math.floor($s / 3600).toString().padStart(2, "0");
  const m = Math.floor(($s % 3600) / 60).toString().padStart(2, "0");
  const s = ($s % 60).toString().padStart(2, "0");
  return `${h}:${m}:${s}`;
});

export function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}
