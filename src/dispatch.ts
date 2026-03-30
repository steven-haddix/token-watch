import type {
  ClaudeUsageResponse,
  CodexUsageResponse,
  DispatchJob,
  DispatchRun,
  DispatchRunStatus,
  DispatchTarget,
} from "./types";
import { formatTimeUntil } from "./hooks/useUsage";

export function dispatchTargetLabel(target: DispatchTarget): string {
  switch (target) {
    case "codex":
      return "Codex";
    case "claude_generic":
      return "Claude";
    case "claude_sonnet":
      return "Claude Sonnet";
    case "claude_opus":
      return "Claude Opus";
  }
}

export function dispatchScheduleLabel(schedule: DispatchJob["schedule_kind"]): string {
  return schedule === "every_reset" ? "Every reset" : "Run once";
}

export function dispatchStatusLabel(status: DispatchRunStatus): string {
  switch (status) {
    case "running":
      return "Running";
    case "succeeded":
      return "Succeeded";
    case "failed":
      return "Failed";
    case "skipped_no_budget":
      return "No budget";
    case "skipped_overlap":
      return "Overlap";
    case "skipped_unavailable":
      return "Unavailable";
  }
}

function claudeAnchor(job: DispatchJob, usage: ClaudeUsageResponse | null): string | null {
  if (!usage) return null;
  const resets = [usage.five_hour.resets_at, usage.seven_day.resets_at];
  if (job.target === "claude_sonnet" && usage.seven_day_sonnet) {
    resets.push(usage.seven_day_sonnet.resets_at);
  }
  if (job.target === "claude_opus" && usage.seven_day_opus) {
    resets.push(usage.seven_day_opus.resets_at);
  }
  return resets.sort((left, right) => new Date(left).getTime() - new Date(right).getTime())[0] ?? null;
}

function codexAnchor(usage: CodexUsageResponse | null): string | null {
  if (!usage) return null;
  return [usage.primary_window.resets_at, usage.secondary_window.resets_at]
    .sort((left, right) => new Date(left).getTime() - new Date(right).getTime())[0] ?? null;
}

export function dispatchAnchorReset(
  job: DispatchJob,
  claudeUsage: ClaudeUsageResponse | null,
  codexUsage: CodexUsageResponse | null,
): string | null {
  if (job.target === "codex") {
    return codexAnchor(codexUsage);
  }
  return claudeAnchor(job, claudeUsage);
}

export function dispatchRuleSummary(job: DispatchJob): string {
  return `${dispatchTargetLabel(job.target)} · >=${job.min_remaining_percent}% left · within ${job.max_time_before_reset_minutes}m of reset`;
}

export function nextDispatchSummary(
  jobs: DispatchJob[],
  claudeUsage: ClaudeUsageResponse | null,
  codexUsage: CodexUsageResponse | null,
): string | null {
  const enabledJobs = jobs.filter((job) => job.enabled);
  if (enabledJobs.length === 0) return null;

  const next = enabledJobs
    .map((job) => ({
      job,
      anchor: dispatchAnchorReset(job, claudeUsage, codexUsage),
    }))
    .filter((item): item is { job: DispatchJob; anchor: string } => !!item.anchor)
    .sort((left, right) => new Date(left.anchor).getTime() - new Date(right.anchor).getTime())[0];

  if (!next) return null;
  return `${next.job.name} in ${formatTimeUntil(next.anchor)}`;
}

export function mostRecentRunForJob(jobId: string, runs: DispatchRun[]): DispatchRun | null {
  return runs.find((run) => run.job_id === jobId) ?? null;
}
