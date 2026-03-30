import { fireEvent, render, screen } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FullView, mostRecentDate } from "../../src/App";
import type { ClaudeUsageResponse, CodexUsageResponse } from "../../src/types";

const useClaudeUsageMock = vi.fn();
const useCodexUsageMock = vi.fn();
const useDispatchStateMock = vi.fn();

vi.mock("../../src/hooks/useUsage", () => ({
  useClaudeUsage: () => useClaudeUsageMock(),
  useCodexUsage: () => useCodexUsageMock(),
  useDispatchState: () => useDispatchStateMock(),
  formatTimeUntil: () => "10m",
}));

vi.mock("@heroui/react", () => {
  const ProgressBar = Object.assign(
    ({ children, ...props }: PropsWithChildren<Record<string, unknown>>) => (
      <div data-testid={`progress-${String(props["aria-label"] ?? "unknown")}`}>{children}</div>
    ),
    {
      Track: ({ children }: PropsWithChildren) => <div>{children}</div>,
      Fill: () => <div />,
    },
  );

  return {
    Card: ({ children }: PropsWithChildren) => <section>{children}</section>,
    CardHeader: ({ children }: PropsWithChildren) => <div>{children}</div>,
    CardContent: ({ children }: PropsWithChildren) => <div>{children}</div>,
    Separator: () => <hr />,
    ProgressBar,
    Chip: ({ children }: PropsWithChildren) => <span>{children}</span>,
    Spinner: () => <span>Loading</span>,
    Button: ({
      children,
      onPress,
      isDisabled,
    }: PropsWithChildren<{
      onPress?: () => void;
      isDisabled?: boolean;
    }>) => (
      <button disabled={isDisabled} onClick={onPress}>
        {children}
      </button>
    ),
  };
});

function buildClaudeUsage(overrides: Partial<ClaudeUsageResponse> = {}): ClaudeUsageResponse {
  return {
    five_hour: { utilization: 25, remaining: 75, resets_at: "2026-03-18T13:00:00Z" },
    seven_day: { utilization: 60, remaining: 40, resets_at: "2026-03-24T13:00:00Z" },
    seven_day_opus: null,
    seven_day_sonnet: null,
    subscription_type: "pro",
    extra_usage: { is_enabled: false, used_credits: null, utilization: null },
    stale: false,
    stale_reason: null,
    retry_after: null,
    ...overrides,
  };
}

function buildCodexUsage(overrides: Partial<CodexUsageResponse> = {}): CodexUsageResponse {
  return {
    plan_type: "plus",
    primary_window: {
      used_percent: 15,
      remaining_percent: 85,
      reset_at_unix: 1773859200,
      resets_at: "2026-03-18T13:00:00Z",
    },
    secondary_window: {
      used_percent: 55,
      remaining_percent: 45,
      reset_at_unix: 1774464000,
      resets_at: "2026-03-24T13:00:00Z",
    },
    has_credits: true,
    limit_reached: false,
    stale: false,
    stale_reason: null,
    retry_after: null,
    ...overrides,
  };
}

function buildUsageState<T>(overrides: Partial<{
  data: T | null;
  loading: boolean;
  error: string | null;
  lastUpdated: Date | null;
  refresh: () => void;
}> = {}) {
  return {
    data: null,
    loading: false,
    error: null,
    lastUpdated: null,
    refresh: vi.fn(),
    ...overrides,
  };
}

function buildDispatchState(overrides: Partial<{
  data: {
    jobs: Array<{
      id: string;
      name: string;
      target: "codex";
      command: string;
      schedule_kind: "once_next_reset";
      min_remaining_percent: number;
      max_time_before_reset_minutes: number;
      enabled: boolean;
      created_at: string;
      updated_at: string;
    }>;
    recent_runs: Array<{
      id: string;
      job_id: string;
      cycle_key: string;
      status: "succeeded";
      started_at: string;
      finished_at: string;
      exit_code: number;
      summary: string;
    }>;
    active_runs: Array<{
      run_id: string;
      job_id: string;
      job_name: string;
      target: "codex";
      started_at: string;
    }>;
  } | null;
  loading: boolean;
  error: string | null;
  lastUpdated: Date | null;
  refresh: () => void;
}> = {}) {
  return {
    data: {
      jobs: [],
      recent_runs: [],
      active_runs: [],
    },
    loading: false,
    error: null,
    lastUpdated: null,
    refresh: vi.fn(),
    ...overrides,
  };
}

describe("FullView", () => {
  beforeEach(() => {
    useClaudeUsageMock.mockReset();
    useCodexUsageMock.mockReset();
    useDispatchStateMock.mockReset();
    useDispatchStateMock.mockReturnValue(buildDispatchState());
  });

  it("renders stale auth messaging and auth-required errors", () => {
    useClaudeUsageMock.mockReturnValue(
      buildUsageState({
        data: buildClaudeUsage({ stale: true, stale_reason: "auth_error" }),
      }),
    );
    useCodexUsageMock.mockReturnValue(
      buildUsageState({
        error: "AUTH_REQUIRED",
      }),
    );

    render(<FullView />);

    expect(screen.getAllByText("Auth required").length).toBeGreaterThan(0);
    expect(
      screen.getByText("Cached usage shown. Reauthenticate Claude Code to resume live updates."),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Authentication expired. Reauthenticate Codex CLI."),
    ).toBeInTheDocument();
  });

  it("renders retry messaging when a service is rate limited without cache", () => {
    useClaudeUsageMock.mockReturnValue(
      buildUsageState({
        error: "RATE_LIMITED_UNTIL:2026-03-18T13:00:00Z",
      }),
    );
    useCodexUsageMock.mockReturnValue(
      buildUsageState({
        data: buildCodexUsage(),
      }),
    );

    render(<FullView />);

    expect(screen.getByText("Rate limited — no cached data yet.")).toBeInTheDocument();
    expect(screen.getByText("Retry in 10m")).toBeInTheDocument();
  });

  it("refreshes both services from the footer action", () => {
    const refreshClaude = vi.fn();
    const refreshCodex = vi.fn();

    useClaudeUsageMock.mockReturnValue(
      buildUsageState({
        data: buildClaudeUsage(),
        refresh: refreshClaude,
      }),
    );
    useCodexUsageMock.mockReturnValue(
      buildUsageState({
        data: buildCodexUsage(),
        refresh: refreshCodex,
      }),
    );

    render(<FullView />);
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(refreshClaude).toHaveBeenCalledTimes(1);
    expect(refreshCodex).toHaveBeenCalledTimes(1);
  });
});

describe("mostRecentDate", () => {
  it("returns the latest non-null date", () => {
    const older = new Date("2026-03-18T12:00:00Z");
    const newer = new Date("2026-03-18T12:05:00Z");

    expect(mostRecentDate(null, older, newer)).toBe(newer);
    expect(mostRecentDate(null, null)).toBeNull();
  });
});
