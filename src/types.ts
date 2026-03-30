export interface WindowUsage {
  utilization: number;    // 0-100, percentage used
  remaining: number;      // 0-100, percentage remaining
  resets_at: string;      // ISO 8601
}

export interface ExtraUsage {
  is_enabled: boolean;
  used_credits: number | null;
  utilization: number | null;
}

export type StaleReason = "rate_limited" | "network_error" | "auth_error";

export interface ClaudeUsageResponse {
  five_hour: WindowUsage;
  seven_day: WindowUsage;
  seven_day_opus: WindowUsage | null;
  seven_day_sonnet: WindowUsage | null;
  subscription_type: string;
  extra_usage: ExtraUsage;
  stale: boolean;
  stale_reason: StaleReason | null;
  retry_after: string | null;
}

export interface CodexWindowUsage {
  used_percent: number;
  remaining_percent: number;
  reset_at_unix: number;
  resets_at: string;
}

export interface CodexUsageResponse {
  plan_type: string;
  primary_window: CodexWindowUsage;
  secondary_window: CodexWindowUsage;
  has_credits: boolean;
  limit_reached: boolean;
  stale: boolean;
  stale_reason: StaleReason | null;
  retry_after: string | null;
}

export type DispatchTarget =
  | "codex"
  | "claude_generic"
  | "claude_sonnet"
  | "claude_opus";

export type DispatchScheduleKind = "once_next_reset" | "every_reset";

export type DispatchRunStatus =
  | "running"
  | "succeeded"
  | "failed"
  | "skipped_no_budget"
  | "skipped_overlap"
  | "skipped_unavailable";

export interface DispatchJob {
  id: string;
  name: string;
  target: DispatchTarget;
  command: string;
  schedule_kind: DispatchScheduleKind;
  min_remaining_percent: number;
  max_time_before_reset_minutes: number;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface DispatchRun {
  id: string;
  job_id: string;
  cycle_key: string;
  status: DispatchRunStatus;
  started_at: string;
  finished_at: string | null;
  exit_code: number | null;
  summary: string;
}

export interface ActiveDispatchRun {
  run_id: string;
  job_id: string;
  job_name: string;
  target: DispatchTarget;
  started_at: string;
}

export interface DispatchState {
  jobs: DispatchJob[];
  recent_runs: DispatchRun[];
  active_runs: ActiveDispatchRun[];
}

export interface DispatchJobUpsertInput {
  id?: string | null;
  name: string;
  target: DispatchTarget;
  command: string;
  schedule_kind: DispatchScheduleKind;
  min_remaining_percent: number;
  max_time_before_reset_minutes: number;
  enabled: boolean;
}

export interface DispatchJobEnabledInput {
  id: string;
  enabled: boolean;
}
