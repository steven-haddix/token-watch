import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ClaudeUsageResponse, CodexUsageResponse } from "../types";

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
  const hours = Math.floor(diffSecs / 3600);
  const minutes = Math.floor((diffSecs % 3600) / 60);
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

  const fetch = useCallback(async () => {
    try {
      const result = await invoke<T>(command);
      setData(result);
      setError(null);
      setLastUpdated(new Date());
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setLoading(false);
    }
  }, [command]);

  const refresh = useCallback(() => {
    setLoading(true);
    if (intervalRef.current) clearInterval(intervalRef.current);
    fetch();
    intervalRef.current = setInterval(fetch, intervalMs);
  }, [fetch, intervalMs]);

  useEffect(() => {
    fetch();
    intervalRef.current = setInterval(fetch, intervalMs);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [fetch, intervalMs]);

  return { data, loading, error, lastUpdated, refresh };
}

export function useClaudeUsage(): UsageState<ClaudeUsageResponse> {
  return useUsagePoll<ClaudeUsageResponse>("get_claude_usage", 120_000);
}

export function useCodexUsage(): UsageState<CodexUsageResponse> {
  return useUsagePoll<CodexUsageResponse>("get_codex_usage", 60_000);
}
