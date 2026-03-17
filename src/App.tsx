import {
  Card,
  CardHeader,
  CardContent,
  Separator,
  ProgressBar,
  Chip,
  Spinner,
  Button,
} from "@heroui/react";
import { useClaudeUsage, useCodexUsage, formatTimeUntil } from "./hooks/useUsage";
import type { WindowUsage, CodexWindowUsage } from "./types";

function progressColor(remaining: number): "success" | "warning" | "danger" {
  if (remaining > 50) return "success";
  if (remaining >= 20) return "warning";
  return "danger";
}

function formatLastUpdated(date: Date): string {
  return date.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

interface WindowRowProps {
  label: string;
  window: WindowUsage;
}

function WindowRow({ label, window: w }: WindowRowProps) {
  const color = progressColor(w.remaining);
  const resetIn = formatTimeUntil(w.resets_at);
  return (
    <div className="flex flex-col gap-1">
      <div className="flex justify-between items-center">
        <span className="text-xs font-medium text-default-600">{label}</span>
        <span className="text-xs text-default-400">
          {Math.round(w.remaining)}% remaining · resets in {resetIn}
        </span>
      </div>
      <ProgressBar value={w.utilization} color={color} size="sm" aria-label={label}>
        <ProgressBar.Track>
          <ProgressBar.Fill />
        </ProgressBar.Track>
      </ProgressBar>
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
    <div className="flex flex-col gap-1">
      <div className="flex justify-between items-center">
        <span className="text-xs font-medium text-default-600">{label}</span>
        <span className="text-xs text-default-400">
          {Math.round(w.remaining_percent)}% remaining · resets in {resetIn}
        </span>
      </div>
      <ProgressBar value={w.used_percent} color={color} size="sm" aria-label={label}>
        <ProgressBar.Track>
          <ProgressBar.Fill />
        </ProgressBar.Track>
      </ProgressBar>
    </div>
  );
}

function App() {
  const claudeUsage = useClaudeUsage();
  const codexUsage = useCodexUsage();

  const lastUpdated = claudeUsage.lastUpdated ?? codexUsage.lastUpdated;

  function handleRefresh() {
    claudeUsage.refresh();
    codexUsage.refresh();
  }

  return (
    <div className="flex flex-col gap-3 p-3 min-h-screen bg-background">
      {/* Claude Code Card */}
      <Card className="w-full">
        <CardHeader className="flex items-center justify-between pb-1 gap-2">
          <div className="flex items-center gap-2">
            <span className="text-sm font-semibold">Claude Code</span>
            {claudeUsage.data && (
              <Chip size="sm" variant="soft" color="accent">
                {claudeUsage.data.subscription_type}
              </Chip>
            )}
          </div>
          {claudeUsage.loading && !claudeUsage.data && (
            <Spinner size="sm" />
          )}
        </CardHeader>
        <Separator />
        <CardContent className="flex flex-col gap-3 pt-3">
          {claudeUsage.error && !claudeUsage.data ? (
            <p className="text-sm text-danger text-center py-2">
              Credentials not found. Install and log in to Claude Code.
            </p>
          ) : claudeUsage.loading && !claudeUsage.data ? (
            <div className="flex justify-center py-2">
              <Spinner size="sm" />
            </div>
          ) : claudeUsage.data ? (
            <>
              <WindowRow label="5-Hour Window" window={claudeUsage.data.five_hour} />
              <WindowRow label="7-Day Window" window={claudeUsage.data.seven_day} />

              {(claudeUsage.data.seven_day_opus || claudeUsage.data.seven_day_sonnet) && (
                <>
                  <Separator />
                  <div className="flex flex-col gap-2">
                    {claudeUsage.data.seven_day_opus && (
                      <WindowRow label="Opus (7-Day)" window={claudeUsage.data.seven_day_opus} />
                    )}
                    {claudeUsage.data.seven_day_sonnet && (
                      <WindowRow label="Sonnet (7-Day)" window={claudeUsage.data.seven_day_sonnet} />
                    )}
                  </div>
                </>
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
        <CardHeader className="flex items-center justify-between pb-1 gap-2">
          <div className="flex items-center gap-2">
            <span className="text-sm font-semibold">Codex CLI</span>
            {codexUsage.data && (
              <Chip size="sm" variant="soft" color="default">
                {codexUsage.data.plan_type}
              </Chip>
            )}
            {codexUsage.data?.has_credits && (
              <Chip size="sm" variant="soft" color="success">
                Credits
              </Chip>
            )}
          </div>
          {codexUsage.loading && !codexUsage.data && (
            <Spinner size="sm" />
          )}
        </CardHeader>
        <Separator />
        <CardContent className="flex flex-col gap-3 pt-3">
          {codexUsage.error && !codexUsage.data ? (
            <p className="text-sm text-danger text-center py-2">
              Credentials not found. Install and log in to Codex CLI.
            </p>
          ) : codexUsage.loading && !codexUsage.data ? (
            <div className="flex justify-center py-2">
              <Spinner size="sm" />
            </div>
          ) : codexUsage.data ? (
            <>
              <CodexWindowRow label="5-Hour Window" window={codexUsage.data.primary_window} />
              <CodexWindowRow label="7-Day Window" window={codexUsage.data.secondary_window} />

              {codexUsage.data.limit_reached && (
                <p className="text-xs text-danger text-center">
                  Rate limit reached
                </p>
              )}
            </>
          ) : null}
        </CardContent>
      </Card>

      {/* Footer */}
      <div className="flex items-center justify-between mt-auto px-1">
        <span className="text-xs text-default-400">
          {lastUpdated
            ? `Last updated ${formatLastUpdated(lastUpdated)}`
            : "Not yet updated"}
        </span>
        <Button
          size="sm"
          variant="secondary"
          onPress={handleRefresh}
          isDisabled={claudeUsage.loading || codexUsage.loading}
        >
          Refresh
        </Button>
      </div>
    </div>
  );
}

export default App;
