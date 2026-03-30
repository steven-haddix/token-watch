import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ClaudeUsageResponse, CodexUsageResponse, DispatchState } from "../types";

export interface UsageState<T> {
  data: T | null;
  loading: boolean;
  error: string | null;
  lastUpdated: Date | null;
  refresh: () => void;
}

export function formatTimeUntil(isoString: string): string {
  const target = new Date(isoString).getTime();
  const now = Date.now();
  const diffMs = target - now;
  if (diffMs <= 0) return "now";
  const diffSecs = Math.floor(diffMs / 1000);
  const days = Math.floor(diffSecs / 86400);
  const hours = Math.floor((diffSecs % 86400) / 3600);
  const minutes = Math.floor((diffSecs % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m`;
  return "< 1m";
}

function useUsagePoll<T>(
  command: string,
  intervalMs: number,
): UsageState<T> {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchUsage = useCallback(async (force = false) => {
    try {
      const result = await invoke<T>(command, force ? { force: true } : undefined);
      setData(result);
      setError(null);
      setLastUpdated(new Date());
    } catch (e) {
      setData(null);
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setLoading(false);
    }
  }, [command]);

  const refresh = useCallback(() => {
    setLoading(true);
    if (intervalRef.current) clearInterval(intervalRef.current);
    fetchUsage(true);
    if (document.visibilityState === "visible") {
      intervalRef.current = setInterval(fetchUsage, intervalMs);
    } else {
      intervalRef.current = null;
    }
  }, [fetchUsage, intervalMs]);

  useEffect(() => {
    const stopPolling = () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };

    const startPolling = () => {
      stopPolling();
      setLoading(true);
      fetchUsage();
      intervalRef.current = setInterval(fetchUsage, intervalMs);
    };

    const onVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        startPolling();
      } else {
        stopPolling();
      }
    };

    onVisibilityChange();
    document.addEventListener("visibilitychange", onVisibilityChange);

    return () => {
      stopPolling();
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [fetchUsage, intervalMs]);

  return { data, loading, error, lastUpdated, refresh };
}

export function useClaudeUsage(): UsageState<ClaudeUsageResponse> {
  return useUsagePoll<ClaudeUsageResponse>("get_claude_usage", 120_000);
}

export function useCodexUsage(): UsageState<CodexUsageResponse> {
  return useUsagePoll<CodexUsageResponse>("get_codex_usage", 60_000);
}

export function useDispatchState(): UsageState<DispatchState> {
  return useUsagePoll<DispatchState>("get_dispatch_state", 60_000);
}
