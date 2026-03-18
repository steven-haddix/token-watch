import { useState, useEffect } from "react";
import { ProgressBar, Spinner } from "@heroui/react";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useClaudeUsage, useCodexUsage, formatTimeUntil } from "./hooks/useUsage";
import type { StaleReason } from "./types";

function progressColor(remaining: number): "success" | "warning" | "danger" {
  if (remaining > 50) return "success";
  if (remaining >= 20) return "warning";
  return "danger";
}

interface CompactRowProps {
  label: string;
  remaining: number;
  resetsAt: string;
}

function CompactRow({ label, remaining, resetsAt }: CompactRowProps) {
  return (
    <div className="flex flex-col gap-1">
      <div className="flex justify-between items-baseline">
        <span className="text-xs font-medium text-default-700">{label}</span>
        <span className="text-xs text-default-400">
          <span className="font-medium text-default-600">{Math.round(remaining)}%</span>
          {" · "}{formatTimeUntil(resetsAt)}
        </span>
      </div>
      <ProgressBar value={remaining} color={progressColor(remaining)} size="sm" aria-label={label}>
        <ProgressBar.Track>
          <ProgressBar.Fill />
        </ProgressBar.Track>
      </ProgressBar>
    </div>
  );
}

async function openFullApp() {
  const main = await WebviewWindow.getByLabel("main");
  if (main) {
    await main.show();
    await main.setFocus();
  }
}

function retryLabel(retryAfter: string | null | undefined): string | null {
  if (!retryAfter) return null;
  const ms = new Date(retryAfter).getTime() - Date.now();
  return ms > 0 ? formatTimeUntil(retryAfter) : null;
}

function isRateLimited(error: string | null): boolean {
  return !!error?.startsWith("RATE_LIMITED");
}

function isAuthError(error: string | null): boolean {
  return error === "AUTH_REQUIRED";
}

function rateLimitRetryTime(error: string | null): string | null {
  if (!error?.startsWith("RATE_LIMITED_UNTIL:")) return null;
  return error.slice("RATE_LIMITED_UNTIL:".length);
}

function staleLabel(reason: StaleReason | null, retryAfter: string | null | undefined): string {
  const countdown = retryLabel(retryAfter);
  if (reason === "auth_error") return "Auth required";
  if (reason === "network_error") return "Cached";
  return countdown ? countdown : "Rate limited";
}

export default function CompactView() {
  const claudeUsage = useClaudeUsage();
  const codexUsage = useCodexUsage();

  // Tick every second while any data is stale so the countdown stays live.
  const [, setTick] = useState(0);
  const isAnyStale = !!(claudeUsage.data?.stale || codexUsage.data?.stale);
  useEffect(() => {
    if (!isAnyStale) return;
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, [isAnyStale]);

  // Retry time from error string when no cached data exists
  const claudeErrorRetry = rateLimitRetryTime(claudeUsage.error);
  const codexErrorRetry = rateLimitRetryTime(codexUsage.error);

  return (
    <div className="flex flex-col h-screen bg-background rounded-xl overflow-hidden p-3 gap-2 select-none">
      {/* Header */}
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold text-default-500 tracking-wide uppercase">
          Token Watch
        </span>
        {((claudeUsage.loading && !claudeUsage.data) || (codexUsage.loading && !codexUsage.data)) ? (
          <Spinner size="sm" />
        ) : null}
      </div>

      {/* Claude Code */}
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-1.5">
          <span className="text-xs font-semibold text-default-600">Claude Code</span>
          {claudeUsage.data?.stale && (
            <span className={`text-xs ${claudeUsage.data.stale_reason === "auth_error" ? "text-danger-600" : "text-warning-600"}`}>
              {staleLabel(claudeUsage.data.stale_reason, claudeUsage.data.retry_after)}
            </span>
          )}
        </div>
        {claudeUsage.error && !claudeUsage.data ? (
          <div className="flex flex-col gap-0.5">
            <p className="text-xs text-danger">
              {isRateLimited(claudeUsage.error)
                ? "Rate limited"
                : isAuthError(claudeUsage.error)
                  ? "Auth required"
                  : "Not logged in"}
            </p>
            {claudeErrorRetry && (
              <p className="text-xs text-warning-600">Retry in {retryLabel(claudeErrorRetry) ?? "< 1m"}</p>
            )}
          </div>
        ) : claudeUsage.loading && !claudeUsage.data ? (
          <div className="flex justify-center py-1"><Spinner size="sm" /></div>
        ) : claudeUsage.data ? (
          <>
            <CompactRow label="5-Hour" remaining={claudeUsage.data.five_hour.remaining} resetsAt={claudeUsage.data.five_hour.resets_at} />
            <CompactRow label="7-Day" remaining={claudeUsage.data.seven_day.remaining} resetsAt={claudeUsage.data.seven_day.resets_at} />
          </>
        ) : null}
      </div>

      {/* Divider */}
      <div className="h-px bg-default-200" />

      {/* Codex CLI */}
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-1.5">
          <span className="text-xs font-semibold text-default-600">Codex CLI</span>
          {codexUsage.data?.stale && (
            <span className={`text-xs ${codexUsage.data.stale_reason === "auth_error" ? "text-danger-600" : "text-warning-600"}`}>
              {staleLabel(codexUsage.data.stale_reason, codexUsage.data.retry_after)}
            </span>
          )}
        </div>
        {codexUsage.error && !codexUsage.data ? (
          <div className="flex flex-col gap-0.5">
            <p className="text-xs text-danger">
              {isRateLimited(codexUsage.error)
                ? "Rate limited"
                : isAuthError(codexUsage.error)
                  ? "Auth required"
                  : "Not logged in"}
            </p>
            {codexErrorRetry && (
              <p className="text-xs text-warning-600">Retry in {retryLabel(codexErrorRetry) ?? "< 1m"}</p>
            )}
          </div>
        ) : codexUsage.loading && !codexUsage.data ? (
          <div className="flex justify-center py-1"><Spinner size="sm" /></div>
        ) : codexUsage.data ? (
          <>
            <CompactRow label="5-Hour" remaining={codexUsage.data.primary_window.remaining_percent} resetsAt={codexUsage.data.primary_window.resets_at} />
            <CompactRow label="7-Day" remaining={codexUsage.data.secondary_window.remaining_percent} resetsAt={codexUsage.data.secondary_window.resets_at} />
          </>
        ) : null}
      </div>

      {/* Spacer */}
      <div className="flex-1" />

      {/* Open Full App */}
      <button
        onClick={openFullApp}
        className="w-full text-xs font-medium text-default-600 hover:text-default-900 bg-default-100 hover:bg-default-200 rounded-lg py-2 px-3 transition-colors cursor-default"
      >
        Open Full App →
      </button>
    </div>
  );
}
