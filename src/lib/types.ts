export type SessionStatus = "idle" | "active" | "on_break";

export interface SessionState {
  status: SessionStatus;
  session_id: string | null;
  clock_in: string | null;
  break_start: string | null;
  total_break_seconds: number;
  break_count: number;
}

export interface ActivitySnapshot {
  timestamp: string;
  keystrokes: number;
  mouse_clicks: number;
  mouse_distance_px: number;
  active_app: string;
  active_window: string;
  idle_seconds: number;
}

export interface NetworkConnection {
  timestamp: string;
  process_name: string;
  remote_host: string;
  remote_ip: string;
  remote_port: number;
  local_port: number;
}

export interface TodayStats {
  session_count: number;
  total_work_seconds: number;
  break_count: number;
  total_break_seconds: number;
}

export interface TeamMember {
  session_id: string;
  user_id: string;
  user_name: string;
  user_email: string;
  status: SessionStatus;
  clock_in: string;
  total_break_seconds: number;
  break_count: number;
  active_app: string;
}

export interface AppSettings {
  pb_url: string;
  pb_email: string;
  pb_password: string;
}

export interface BreakConfig {
  id: string;
  name: string;
  type_key: string;
  duration_minutes: number;
  sort_order: number;
  auto_start_enabled: boolean;
  auto_start_time: string | null;
  auto_end_time: string | null;
}

export interface LiveCounters {
  keystrokes: number;
  mouse_clicks: number;
  mouse_distance_px: number;
  idle_seconds: number;
  active_app: string;
  active_window: string;
  input_monitoring_active: boolean;
}
