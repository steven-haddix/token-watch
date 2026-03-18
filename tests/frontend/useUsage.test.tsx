import { act, cleanup, renderHook } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { useClaudeUsage } from "../../src/hooks/useUsage";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

let visibilityState: DocumentVisibilityState = "visible";

function setVisibility(nextState: DocumentVisibilityState) {
  visibilityState = nextState;
  document.dispatchEvent(new Event("visibilitychange"));
}

async function flushEffects() {
  await act(async () => {
    await Promise.resolve();
  });
}

beforeAll(() => {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => visibilityState,
  });
});

afterAll(() => {
  cleanup();
});

describe("useClaudeUsage", () => {
  beforeEach(() => {
    visibilityState = "visible";
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({
      five_hour: { utilization: 25, remaining: 75, resets_at: "2026-03-18T13:00:00Z" },
      seven_day: { utilization: 40, remaining: 60, resets_at: "2026-03-24T13:00:00Z" },
      seven_day_opus: null,
      seven_day_sonnet: null,
      subscription_type: "pro",
      extra_usage: { is_enabled: false, used_credits: null, utilization: null },
      stale: false,
      stale_reason: null,
      retry_after: null,
    });
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-18T12:00:00Z"));
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("polls immediately and on the configured interval while visible", async () => {
    renderHook(() => useClaudeUsage());
    await flushEffects();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_claude_usage", undefined);

    await act(async () => {
      vi.advanceTimersByTime(120_000);
      await Promise.resolve();
    });

    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(invokeMock).toHaveBeenNthCalledWith(2, "get_claude_usage", undefined);
  });

  it("does not poll while hidden and resumes when visible again", async () => {
    visibilityState = "hidden";
    renderHook(() => useClaudeUsage());

    await act(async () => {
      vi.advanceTimersByTime(240_000);
    });
    expect(invokeMock).not.toHaveBeenCalled();

    act(() => {
      setVisibility("visible");
    });
    await flushEffects();

    expect(invokeMock).toHaveBeenCalledTimes(1);

    act(() => {
      setVisibility("hidden");
    });

    await act(async () => {
      vi.advanceTimersByTime(240_000);
    });
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("forces a live fetch on manual refresh and resets the interval schedule", async () => {
    const { result } = renderHook(() => useClaudeUsage());
    await flushEffects();

    expect(invokeMock).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.refresh();
    });
    await flushEffects();

    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(invokeMock).toHaveBeenNthCalledWith(2, "get_claude_usage", { force: true });

    await act(async () => {
      vi.advanceTimersByTime(119_999);
    });
    expect(invokeMock).toHaveBeenCalledTimes(2);

    await act(async () => {
      vi.advanceTimersByTime(1);
      await Promise.resolve();
    });

    expect(invokeMock).toHaveBeenCalledTimes(3);
    expect(invokeMock).toHaveBeenNthCalledWith(3, "get_claude_usage", undefined);
  });
});
