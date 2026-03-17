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

export interface ClaudeUsageResponse {
  five_hour: WindowUsage;
  seven_day: WindowUsage;
  seven_day_opus: WindowUsage | null;
  seven_day_sonnet: WindowUsage | null;
  subscription_type: string;
  extra_usage: ExtraUsage;
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
}
