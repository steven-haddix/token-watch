import { useState, useEffect } from "react";
import {
  Card,
  CardContent,
  ProgressBar,
  Chip,
  Spinner,
} from "@heroui/react";
import { useClaudeUsage, useCodexUsage, formatTimeUntil } from "./hooks/useUsage";
import type { WindowUsage, CodexWindowUsage, StaleReason } from "./types";

function progressColor(remaining: number): "success" | "warning" | "danger" {
  if (remaining > 50) return "success";
  if (remaining >= 20) return "warning";
  return "danger";
}

const progressTextClass = {
  success: "text-success",
  warning: "text-warning",
  danger: "text-danger",
} as const;

function isRateLimitedError(error: string | null): boolean {
  return !!error?.startsWith("RATE_LIMITED");
}

function isAuthError(error: string | null): boolean {
  return error === "AUTH_REQUIRED";
}

function staleChipColor(reason: StaleReason | null): "default" | "warning" | "danger" {
  if (reason === "auth_error") return "danger";
  if (reason === "network_error") return "default";
  return "warning";
}

function staleChipLabel(reason: StaleReason | null, retryAfter: string | null | undefined): string {
  const countdown =
    retryAfter && new Date(retryAfter).getTime() > Date.now() ? formatTimeUntil(retryAfter) : null;
  if (reason === "auth_error") return "Auth required";
  if (reason === "network_error") return "Cached";
  return countdown ? `Retry in ${countdown}` : "Rate limited";
}

interface WindowRowProps {
  label: string;
  window: WindowUsage;
}

function WindowRow({ label, window: w }: WindowRowProps) {
  const color = progressColor(w.remaining);
  const resetIn = formatTimeUntil(w.resets_at);
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex justify-between items-baseline">
        <span className="text-sm text-muted">{label}</span>
        <span className={`text-sm font-semibold ${progressTextClass[color]}`}>
          {Math.round(w.remaining)}%
        </span>
      </div>
      <ProgressBar value={w.remaining} color={color} size="sm" aria-label={label}>
        <ProgressBar.Track>
          <ProgressBar.Fill />
        </ProgressBar.Track>
      </ProgressBar>
      <span className="text-xs text-muted">~{resetIn} remaining</span>
    </div>
  );
}

interface CodexWindowRowProps {
  label: string;
  window: CodexWindowUsage;
}

function CodexWindowRow({ label, window: w }: CodexWindowRowProps) {
  const color = progressColor(w.remaining_percent);
  const resetIn = formatTimeUntil(w.resets_at);
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex justify-between items-baseline">
        <span className="text-sm text-muted">{label}</span>
        <span className={`text-sm font-semibold ${progressTextClass[color]}`}>
          {Math.round(w.remaining_percent)}%
        </span>
      </div>
      <ProgressBar value={w.remaining_percent} color={color} size="sm" aria-label={label}>
        <ProgressBar.Track>
          <ProgressBar.Fill />
        </ProgressBar.Track>
      </ProgressBar>
      <span className="text-xs text-muted">~{resetIn} remaining</span>
    </div>
  );
}

interface UsageViewProps {
  claudeUsage: ReturnType<typeof useClaudeUsage>;
  codexUsage: ReturnType<typeof useCodexUsage>;
}

export default function UsageView({ claudeUsage, codexUsage }: UsageViewProps) {
  const [, setTick] = useState(0);
  const isAnyStale = !!(claudeUsage.data?.stale || codexUsage.data?.stale);
  
  useEffect(() => {
    if (!isAnyStale) return;
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, [isAnyStale]);

  return (
    <div className="flex flex-col gap-3">
      {/* Claude Code Card */}
      <Card className="w-full">
        <CardContent className="flex flex-col gap-4">
          <div className="flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-orange-400 shrink-0" />
            <span className="font-semibold text-foreground">Claude</span>
            {claudeUsage.data && (
              <Chip size="sm" variant="soft" color="warning">
                {claudeUsage.data.subscription_type}
              </Chip>
            )}
            {claudeUsage.data?.stale && (
              <Chip
                size="sm"
                variant="soft"
                color={staleChipColor(claudeUsage.data.stale_reason)}
              >
                {staleChipLabel(claudeUsage.data.stale_reason, claudeUsage.data.retry_after)}
              </Chip>
            )}
            {claudeUsage.loading && !claudeUsage.data && (
              <Spinner size="sm" className="ml-auto" />
            )}
          </div>
          {claudeUsage.error && !claudeUsage.data ? (
            <div className="flex flex-col gap-1 py-2 text-center">
              <p className="text-sm text-danger">
                {isRateLimitedError(claudeUsage.error)
                  ? "Rate limited — no cached data yet."
                  : isAuthError(claudeUsage.error)
                    ? "Authentication expired. Reauthenticate Claude Code."
                    : "Credentials not found. Install and log in to Claude Code."}
              </p>
              {claudeUsage.error?.startsWith("RATE_LIMITED_UNTIL:") && (
                <p className="text-xs text-warning">
                  Retry in {formatTimeUntil(claudeUsage.error.slice("RATE_LIMITED_UNTIL:".length))}
                </p>
              )}
              {!isRateLimitedError(claudeUsage.error) && !isAuthError(claudeUsage.error) && (
                <p className="text-xs text-muted break-words">{claudeUsage.error}</p>
              )}
            </div>
          ) : claudeUsage.loading && !claudeUsage.data ? (
            <div className="flex justify-center py-2">
              <Spinner size="sm" />
            </div>
          ) : claudeUsage.data ? (
            <>
              {claudeUsage.data.stale_reason === "auth_error" && (
                <p className="text-xs text-danger text-center">
                  Cached usage shown. Reauthenticate Claude Code to resume live updates.
                </p>
              )}
              {claudeUsage.data.stale_reason === "network_error" && (
                <p className="text-xs text-muted text-center">
                  Showing cached usage while the latest request failed.
                </p>
              )}
              <WindowRow label="5-Hour Window" window={claudeUsage.data.five_hour} />
              <WindowRow label="7-Day Window" window={claudeUsage.data.seven_day} />

              {(claudeUsage.data.seven_day_opus || claudeUsage.data.seven_day_sonnet) && (
                <div className="border-t border-separator pt-3 flex flex-col gap-3">
                  {claudeUsage.data.seven_day_opus && (
                    <WindowRow label="Opus (7-Day)" window={claudeUsage.data.seven_day_opus} />
                  )}
                  {claudeUsage.data.seven_day_sonnet && (
                    <WindowRow label="Sonnet (7-Day)" window={claudeUsage.data.seven_day_sonnet} />
                  )}
                </div>
              )}

              {claudeUsage.data.extra_usage.is_enabled && (
                <p className="text-xs text-warning text-center">
                  Extra usage is enabled
                  {claudeUsage.data.extra_usage.used_credits != null &&
                    ` · ${claudeUsage.data.extra_usage.used_credits} credits used`}
                </p>
              )}
            </>
          ) : null}
        </CardContent>
      </Card>

      {/* Codex CLI Card */}
      <Card className="w-full">
        <CardContent className="flex flex-col gap-4">
          <div className="flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-blue-400 shrink-0" />
            <span className="font-semibold text-foreground">Codex</span>
            {codexUsage.data && (
              <Chip size="sm" variant="soft" color="accent">
                {codexUsage.data.plan_type}
              </Chip>
            )}
            {codexUsage.data?.has_credits && (
              <Chip size="sm" variant="soft" color="success">
                Credits
              </Chip>
            )}
            {codexUsage.data?.stale && (
              <Chip
                size="sm"
                variant="soft"
                color={staleChipColor(codexUsage.data.stale_reason)}
              >
                {staleChipLabel(codexUsage.data.stale_reason, codexUsage.data.retry_after)}
              </Chip>
            )}
            {codexUsage.loading && !codexUsage.data && (
              <Spinner size="sm" className="ml-auto" />
            )}
          </div>
          {codexUsage.error && !codexUsage.data ? (
            <div className="flex flex-col gap-1 py-2 text-center">
              <p className="text-sm text-danger">
                {isRateLimitedError(codexUsage.error)
                  ? "Rate limited — no cached data yet."
                  : isAuthError(codexUsage.error)
                    ? "Authentication expired. Reauthenticate Codex CLI."
                    : "Credentials not found. Install and log in to Codex CLI."}
              </p>
              {codexUsage.error?.startsWith("RATE_LIMITED_UNTIL:") && (
                <p className="text-xs text-warning">
                  Retry in {formatTimeUntil(codexUsage.error.slice("RATE_LIMITED_UNTIL:".length))}
                </p>
              )}
              {!isRateLimitedError(codexUsage.error) && !isAuthError(codexUsage.error) && (
                <p className="text-xs text-muted break-words">{codexUsage.error}</p>
              )}
            </div>
          ) : codexUsage.loading && !codexUsage.data ? (
            <div className="flex justify-center py-2">
              <Spinner size="sm" />
            </div>
          ) : codexUsage.data ? (
            <>
              {codexUsage.data.stale_reason === "auth_error" && (
                <p className="text-xs text-danger text-center">
                  Cached usage shown. Reauthenticate Codex CLI to resume live updates.
                </p>
              )}
              {codexUsage.data.stale_reason === "network_error" && (
                <p className="text-xs text-muted text-center">
                  Showing cached usage while the latest request failed.
                </p>
              )}
              <CodexWindowRow label="5-Hour Window" window={codexUsage.data.primary_window} />
              <CodexWindowRow label="7-Day Window" window={codexUsage.data.secondary_window} />

              {codexUsage.data.limit_reached && (
                <p className="text-xs text-danger text-center">Rate limit reached</p>
              )}
            </>
          ) : null}
        </CardContent>
      </Card>
    </div>
  );
}
