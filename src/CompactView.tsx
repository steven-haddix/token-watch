import { useState, useEffect } from "react";
import { Card, CardContent, ProgressBar, Chip, Spinner, Button } from "@heroui/react";

import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useClaudeUsage, useCodexUsage, formatTimeUntil } from "./hooks/useUsage";
import type { StaleReason } from "./types";

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

interface CompactRowProps {
  label: string;
  remaining: number;
}

function CompactRow({ label, remaining }: CompactRowProps) {
  const color = progressColor(remaining);
  return (
    <div className="flex flex-col gap-1">
      <div className="flex justify-between items-baseline">
        <span className="text-xs text-muted">{label}</span>
        <span className={`text-xs font-semibold ${progressTextClass[color]}`}>
          {Math.round(remaining)}%
        </span>
      </div>
      <ProgressBar value={remaining} color={color} size="sm" aria-label={label}>
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

function staleChipColor(reason: StaleReason | null): "default" | "warning" | "danger" {
  if (reason === "auth_error") return "danger";
  if (reason === "network_error") return "default";
  return "warning";
}

function staleLabel(reason: StaleReason | null, retryAfter: string | null | undefined): string {
  const countdown =
    retryAfter && new Date(retryAfter).getTime() > Date.now() ? formatTimeUntil(retryAfter) : null;
  if (reason === "auth_error") return "Auth required";
  if (reason === "network_error") return "Cached";
  return countdown ? countdown : "Rate limited";
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

function formatUpdated(date: Date): string {
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export default function CompactView() {
  const claudeUsage = useClaudeUsage();
  const codexUsage = useCodexUsage();

  const [, setTick] = useState(0);
  const isAnyStale = !!(claudeUsage.data?.stale || codexUsage.data?.stale);
  useEffect(() => {
    if (!isAnyStale) return;
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, [isAnyStale]);

  const claudeErrorRetry = rateLimitRetryTime(claudeUsage.error);
  const codexErrorRetry = rateLimitRetryTime(codexUsage.error);

  const lastUpdated = [claudeUsage.lastUpdated, codexUsage.lastUpdated]
    .filter((d): d is Date => d != null)
    .reduce<Date | null>((latest, d) => (!latest || d > latest ? d : latest), null);

  return (
    <div className="flex flex-col h-screen bg-background rounded-xl overflow-hidden p-3 gap-2 select-none">
      {/* Header */}
      <div className="flex items-center justify-between px-0.5 pb-0.5">
        <span className="text-sm font-bold text-foreground">AI Usage</span>
        <span className="text-xs text-muted">
          {lastUpdated ? formatUpdated(lastUpdated) : "—"}
        </span>
      </div>

      {/* Claude Card */}
      <Card className="p-0">
        <CardContent className="p-3 flex flex-col gap-2">
          <div className="flex items-center gap-1.5">
            <span className="w-1.5 h-1.5 rounded-full bg-orange-400 shrink-0" />
            <span className="text-sm font-semibold text-foreground">Claude</span>
            {claudeUsage.data && (
              <Chip size="sm" variant="soft" color="warning">
                {claudeUsage.data.subscription_type}
              </Chip>
            )}
            {claudeUsage.data?.stale && (
              <Chip size="sm" variant="soft" color={staleChipColor(claudeUsage.data.stale_reason)}>
                {staleLabel(claudeUsage.data.stale_reason, claudeUsage.data.retry_after)}
              </Chip>
            )}
            {claudeUsage.loading && !claudeUsage.data && (
              <Spinner size="sm" className="ml-auto" />
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
                <p className="text-xs text-warning">Retry in {formatTimeUntil(claudeErrorRetry)}</p>
              )}
            </div>
          ) : claudeUsage.loading && !claudeUsage.data ? (
            <div className="flex justify-center py-1">
              <Spinner size="sm" />
            </div>
          ) : claudeUsage.data ? (
            <>
              <CompactRow label="5-Hour" remaining={claudeUsage.data.five_hour.remaining} />
              <CompactRow label="7-Day" remaining={claudeUsage.data.seven_day.remaining} />
            </>
          ) : null}
        </CardContent>
      </Card>

      {/* Codex Card */}
      <Card className="p-0">
        <CardContent className="p-3 flex flex-col gap-2">
          <div className="flex items-center gap-1.5">
            <span className="w-1.5 h-1.5 rounded-full bg-blue-400 shrink-0" />
            <span className="text-sm font-semibold text-foreground">Codex</span>
            {codexUsage.data && (
              <Chip size="sm" variant="soft" color="accent">
                {codexUsage.data.plan_type}
              </Chip>
            )}
            {codexUsage.data?.stale && (
              <Chip size="sm" variant="soft" color={staleChipColor(codexUsage.data.stale_reason)}>
                {staleLabel(codexUsage.data.stale_reason, codexUsage.data.retry_after)}
              </Chip>
            )}
            {codexUsage.loading && !codexUsage.data && (
              <Spinner size="sm" className="ml-auto" />
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
                <p className="text-xs text-warning">Retry in {formatTimeUntil(codexErrorRetry)}</p>
              )}
            </div>
          ) : codexUsage.loading && !codexUsage.data ? (
            <div className="flex justify-center py-1">
              <Spinner size="sm" />
            </div>
          ) : codexUsage.data ? (
            <>
              <CompactRow label="5-Hour" remaining={codexUsage.data.primary_window.remaining_percent} />
              <CompactRow label="7-Day" remaining={codexUsage.data.secondary_window.remaining_percent} />
            </>
          ) : null}
        </CardContent>
      </Card>

      {/* Spacer */}
      <div className="flex-1" />

      {/* Open Full App */}
      <div className="flex justify-end">
        <Button onPress={openFullApp} variant="secondary" size="sm">
          Dashboard →
        </Button>
      </div>
    </div>
  );
}
