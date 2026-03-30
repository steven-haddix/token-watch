import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Card, CardContent, Chip, Spinner } from "@heroui/react";

import {
  dispatchAnchorReset,
  dispatchRuleSummary,
  dispatchScheduleLabel,
  dispatchStatusLabel,
  dispatchTargetLabel,
  mostRecentRunForJob,
  nextDispatchSummary,
} from "../dispatch";
import { formatTimeUntil, type UsageState, useDispatchState } from "../hooks/useUsage";
import type {
  ClaudeUsageResponse,
  CodexUsageResponse,
  DispatchJob,
  DispatchJobEnabledInput,
  DispatchJobUpsertInput,
} from "../types";

interface DispatchSectionProps {
  claudeUsage: UsageState<ClaudeUsageResponse>;
  codexUsage: UsageState<CodexUsageResponse>;
  compact?: boolean;
}

const defaultForm: DispatchJobUpsertInput = {
  id: null,
  name: "",
  target: "codex",
  command: "",
  schedule_kind: "once_next_reset",
  min_remaining_percent: 20,
  max_time_before_reset_minutes: 45,
  enabled: true,
};

export default function DispatchSection({
  claudeUsage,
  codexUsage,
  compact = false,
}: DispatchSectionProps) {
  const dispatchState = useDispatchState();
  const [form, setForm] = useState<DispatchJobUpsertInput>(defaultForm);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [showForm, setShowForm] = useState(false);

  const activeRuns = dispatchState.data?.active_runs ?? [];
  const nextSummary = useMemo(
    () =>
      nextDispatchSummary(
        dispatchState.data?.jobs ?? [],
        claudeUsage.data,
        codexUsage.data,
      ),
    [dispatchState.data?.jobs, claudeUsage.data, codexUsage.data],
  );

  const hasJobs = (dispatchState.data?.jobs.length ?? 0) > 0;

  async function refreshAll() {
    dispatchState.refresh();
    claudeUsage.refresh();
    codexUsage.refresh();
  }

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    setSaving(true);
    try {
      await invoke("upsert_dispatch_job", {
        input: {
          ...form,
          min_remaining_percent: Number(form.min_remaining_percent),
          max_time_before_reset_minutes: Number(form.max_time_before_reset_minutes),
        },
      });
      setForm(defaultForm);
      setShowForm(false);
      await refreshAll();
    } catch (submitError) {
      setError(typeof submitError === "string" ? submitError : String(submitError));
    } finally {
      setSaving(false);
    }
  }

  function handleCancel() {
    setForm(defaultForm);
    setShowForm(false);
    setError(null);
  }

  async function handleToggle(job: DispatchJob) {
    setBusyId(job.id);
    setError(null);
    try {
      const input: DispatchJobEnabledInput = { id: job.id, enabled: !job.enabled };
      await invoke("set_dispatch_job_enabled", { input });
      await refreshAll();
    } catch (toggleError) {
      setError(typeof toggleError === "string" ? toggleError : String(toggleError));
    } finally {
      setBusyId(null);
    }
  }

  async function handleDelete(jobId: string) {
    setBusyId(jobId);
    setError(null);
    try {
      await invoke("delete_dispatch_job", { id: jobId });
      if (form.id === jobId) setForm(defaultForm);
      await refreshAll();
    } catch (deleteError) {
      setError(typeof deleteError === "string" ? deleteError : String(deleteError));
    } finally {
      setBusyId(null);
    }
  }

  function startEdit(job: DispatchJob) {
    setError(null);
    setForm({
      id: job.id,
      name: job.name,
      target: job.target,
      command: job.command,
      schedule_kind: job.schedule_kind,
      min_remaining_percent: job.min_remaining_percent,
      max_time_before_reset_minutes: job.max_time_before_reset_minutes,
      enabled: job.enabled,
    });
    setShowForm(true);
  }

  if (compact) {
    return (
      <Card className="p-0">
        <CardContent className="p-3 flex flex-col gap-2">
          <div className="flex items-center gap-1.5">
            <span className="text-sm font-semibold text-foreground">Dispatch</span>
            {dispatchState.loading && !dispatchState.data && <Spinner size="sm" className="ml-auto" />}
            {activeRuns.length > 0 && (
              <Chip size="sm" variant="soft" color="success">
                {activeRuns.length} running
              </Chip>
            )}
          </div>
          {dispatchState.error && !dispatchState.data ? (
            <p className="text-xs text-danger break-words">{dispatchState.error}</p>
          ) : (
            <>
              <p className="text-xs text-muted">
                {dispatchState.data?.jobs.length
                  ? `${dispatchState.data.jobs.filter((job) => job.enabled).length} armed jobs`
                  : "No jobs configured"}
              </p>
              {nextSummary && <p className="text-xs text-foreground">{nextSummary}</p>}
            </>
          )}
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      {/* Header Section */}
      <div className="flex items-center justify-between">
        <div className="flex flex-col">
          <h2 className="text-xl font-bold text-foreground">Dispatch Jobs</h2>
          <p className="text-sm text-muted">
            Launch saved Claude or Codex commands near reset windows.
          </p>
        </div>
        {!showForm && (
          <Button
            color="primary"
            size="md"
            className="font-medium"
            onPress={() => setShowForm(true)}
          >
            <PlusIcon className="w-4 h-4 mr-2" />
            Create Job
          </Button>
        )}
      </div>

      {showForm ? (
        <Card className="w-full border-primary/20 shadow-lg shadow-primary/5">
          <CardContent className="p-6 flex flex-col gap-6">
            <div className="flex items-center justify-between">
              <h3 className="font-semibold text-foreground text-lg">
                {form.id ? "Edit Job" : "New Dispatch Job"}
              </h3>
              <Button isIconOnly variant="light" size="sm" onPress={handleCancel}>
                <CloseIcon className="w-4 h-4" />
              </Button>
            </div>

            <form className="grid grid-cols-1 md:grid-cols-2 gap-5" onSubmit={handleSubmit}>
              <label className="flex flex-col gap-2 text-sm font-medium text-foreground">
                Job Name
                <input
                  className="rounded-xl border border-separator bg-content1 px-4 py-2.5 text-sm text-foreground focus:ring-2 focus:ring-primary/20 outline-none transition-all"
                  value={form.name}
                  onChange={(event) => setForm((current) => ({ ...current, name: event.target.value }))}
                  placeholder="e.g., Nightly cleanup"
                  required
                />
              </label>
              <label className="flex flex-col gap-2 text-sm font-medium text-foreground">
                Target CLI
                <select
                  className="rounded-xl border border-separator bg-content1 px-4 py-2.5 text-sm text-foreground focus:ring-2 focus:ring-primary/20 outline-none transition-all appearance-none"
                  value={form.target}
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      target: event.target.value as DispatchJob["target"],
                    }))
                  }
                >
                  <option value="codex">Codex</option>
                  <option value="claude_generic">Claude (Default)</option>
                  <option value="claude_sonnet">Claude Sonnet</option>
                  <option value="claude_opus">Claude Opus</option>
                </select>
              </label>
              <label className="flex flex-col gap-2 text-sm font-medium text-foreground md:col-span-2">
                Shell Command
                <input
                  className="rounded-xl border border-separator bg-content1 px-4 py-2.5 text-sm font-mono text-foreground focus:ring-2 focus:ring-primary/20 outline-none transition-all"
                  value={form.command}
                  onChange={(event) =>
                    setForm((current) => ({ ...current, command: event.target.value }))
                  }
                  placeholder="cd ~/Code/project && codex run ..."
                  required
                />
              </label>
              <label className="flex flex-col gap-2 text-sm font-medium text-foreground">
                Schedule Type
                <select
                  className="rounded-xl border border-separator bg-content1 px-4 py-2.5 text-sm text-foreground focus:ring-2 focus:ring-primary/20 outline-none transition-all appearance-none"
                  value={form.schedule_kind}
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      schedule_kind: event.target.value as DispatchJob["schedule_kind"],
                    }))
                  }
                >
                  <option value="once_next_reset">Once (Next Reset)</option>
                  <option value="every_reset">Recurring (Every Reset)</option>
                </select>
              </label>
              <div className="grid grid-cols-2 gap-4">
                <label className="flex flex-col gap-2 text-sm font-medium text-foreground">
                  Min Budget %
                  <input
                    type="number"
                    min={1}
                    max={100}
                    className="rounded-xl border border-separator bg-content1 px-4 py-2.5 text-sm text-foreground focus:ring-2 focus:ring-primary/20 outline-none transition-all"
                    value={form.min_remaining_percent}
                    onChange={(event) =>
                      setForm((current) => ({
                        ...current,
                        min_remaining_percent: Number(event.target.value),
                      }))
                    }
                  />
                </label>
                <label className="flex flex-col gap-2 text-sm font-medium text-foreground">
                  Buffer (Mins)
                  <input
                    type="number"
                    min={1}
                    className="rounded-xl border border-separator bg-content1 px-4 py-2.5 text-sm text-foreground focus:ring-2 focus:ring-primary/20 outline-none transition-all"
                    value={form.max_time_before_reset_minutes}
                    onChange={(event) =>
                      setForm((current) => ({
                        ...current,
                        max_time_before_reset_minutes: Number(event.target.value),
                      }))
                    }
                  />
                </label>
              </div>
              
              <div className="flex items-center justify-between md:col-span-2 pt-2">
                <label className="flex items-center gap-3 text-sm font-medium text-foreground cursor-pointer group">
                  <div className={`
                    w-10 h-6 rounded-full p-1 transition-colors
                    ${form.enabled ? 'bg-primary' : 'bg-content3'}
                  `}>
                    <div className={`
                      w-4 h-4 bg-white rounded-full transition-transform
                      ${form.enabled ? 'translate-x-4' : 'translate-x-0'}
                    `} />
                  </div>
                  <input
                    type="checkbox"
                    className="hidden"
                    checked={form.enabled}
                    onChange={(event) =>
                      setForm((current) => ({ ...current, enabled: event.target.checked }))
                    }
                  />
                  Initially Enabled
                </label>

                <div className="flex items-center gap-3">
                  <Button variant="light" size="md" onPress={handleCancel}>
                    Cancel
                  </Button>
                  <Button type="submit" color="primary" size="md" className="font-semibold px-8" isDisabled={saving}>
                    {saving ? "Saving..." : form.id ? "Update Job" : "Create Job"}
                  </Button>
                </div>
              </div>
            </form>

            {error && (
              <div className="p-3 rounded-xl bg-danger/10 border border-danger/20 text-danger text-xs">
                {error}
              </div>
            )}
          </CardContent>
        </Card>
      ) : !hasJobs ? (
        <Card className="w-full border-dashed border-2 border-separator bg-transparent">
          <CardContent className="flex flex-col items-center justify-center py-16 px-6 text-center gap-4">
            <div className="w-16 h-16 rounded-full bg-content2 flex items-center justify-center text-muted">
              <SchedulingIcon size={32} />
            </div>
            <div className="flex flex-col gap-1 max-w-sm">
              <h3 className="text-lg font-semibold text-foreground">No dispatch jobs yet</h3>
              <p className="text-sm text-muted">
                Create a job to automatically run commands when you have remaining budget right before it resets.
              </p>
            </div>
            <Button
              color="primary"
              variant="flat"
              onPress={() => setShowForm(true)}
              className="mt-2"
            >
              <PlusIcon className="w-4 h-4 mr-2" />
              Add your first job
            </Button>
          </CardContent>
        </Card>
      ) : (
        <div className="flex flex-col gap-4">
          {activeRuns.length > 0 && (
            <div className="flex flex-col gap-2 p-4 rounded-2xl bg-success/10 border border-success/20">
              <div className="flex items-center gap-2">
                <Spinner size="sm" color="success" />
                <span className="text-sm font-semibold text-success-700">Currently Running</span>
              </div>
              {activeRuns.map(run => (
                <div key={run.run_id} className="flex items-center justify-between text-xs text-success-800">
                  <span>{run.job_name} ({dispatchTargetLabel(run.target)})</span>
                  <span>Started {new Date(run.started_at).toLocaleTimeString()}</span>
                </div>
              ))}
            </div>
          )}

          {nextSummary && (
            <div className="px-4 py-2 rounded-xl bg-content1 border border-separator flex items-center gap-3">
              <div className="w-2 h-2 rounded-full bg-primary animate-pulse" />
              <span className="text-xs font-medium text-foreground">Next Trigger: {nextSummary}</span>
            </div>
          )}

          <div className="grid grid-cols-1 gap-4">
            {dispatchState.data?.jobs.map((job) => {
              const lastRun = mostRecentRunForJob(job.id, dispatchState.data?.recent_runs ?? []);
              const anchorReset = dispatchAnchorReset(job, claudeUsage.data, codexUsage.data);

              return (
                <Card key={job.id} className="overflow-hidden border-separator/50 hover:border-primary/30 transition-colors">
                  <CardContent className="p-4 flex flex-col gap-4">
                    <div className="flex items-start justify-between gap-4">
                      <div className="flex flex-col gap-1">
                        <div className="flex items-center gap-2">
                          <span className="font-bold text-foreground">{job.name}</span>
                          <Chip size="sm" variant="flat" color={job.enabled ? "success" : "default"}>
                            {job.enabled ? "Active" : "Paused"}
                          </Chip>
                        </div>
                        <span className="text-xs text-muted-foreground">{dispatchRuleSummary(job)}</span>
                      </div>
                      <div className="flex items-center gap-1">
                        <Button isIconOnly variant="light" size="sm" onPress={() => startEdit(job)}>
                          <EditIcon className="w-4 h-4" />
                        </Button>
                        <Button isIconOnly variant="light" color="danger" size="sm" onPress={() => handleDelete(job.id)}>
                          <TrashIcon className="w-4 h-4" />
                        </Button>
                      </div>
                    </div>

                    <div className="bg-content2/50 p-3 rounded-xl border border-separator/50">
                      <code className="text-[11px] font-mono text-foreground break-all leading-relaxed">
                        $ {job.command}
                      </code>
                    </div>

                    <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                      <div className="flex items-center gap-1.5 text-xs text-muted">
                        <TargetIcon className="w-3.5 h-3.5" />
                        <span>{dispatchTargetLabel(job.target)}</span>
                      </div>
                      <div className="flex items-center gap-1.5 text-xs text-muted">
                        <CalendarIcon className="w-3.5 h-3.5" />
                        <span>{dispatchScheduleLabel(job.schedule_kind)}</span>
                      </div>
                      {anchorReset && (
                        <div className="flex items-center gap-1.5 text-xs text-primary font-medium">
                          <TimerIcon className="w-3.5 h-3.5" />
                          <span>Reset in {formatTimeUntil(anchorReset)}</span>
                        </div>
                      )}
                    </div>

                    {lastRun && (
                      <div className="pt-3 border-t border-separator/50 flex flex-col gap-2">
                         <div className="flex items-center justify-between">
                            <span className="text-[10px] uppercase font-bold text-muted-foreground/50 tracking-wider">Latest Run</span>
                            <Chip 
                              size="sm" 
                              variant="soft" 
                              color={lastRun.status === "succeeded" ? "success" : lastRun.status === "failed" ? "danger" : "default"}
                            >
                              {dispatchStatusLabel(lastRun.status)}
                            </Chip>
                         </div>
                         <p className="text-xs text-muted leading-relaxed">
                           {lastRun.summary}
                         </p>
                         <span className="text-[10px] text-muted-foreground/70">
                           {new Date(lastRun.started_at).toLocaleString()}
                         </span>
                      </div>
                    )}

                    <div className="flex items-center gap-2 pt-1">
                      <Button
                        size="sm"
                        variant={job.enabled ? "flat" : "solid"}
                        color={job.enabled ? "default" : "primary"}
                        className="w-full"
                        onPress={() => handleToggle(job)}
                        isDisabled={busyId === job.id}
                      >
                        {job.enabled ? "Pause Dispatch" : "Resume Dispatch"}
                      </Button>
                    </div>
                  </CardContent>
                </Card>
              );
            })}
          </div>
        </div>
      )}

      {dispatchState.loading && !dispatchState.data && (
        <div className="flex justify-center py-8">
          <Spinner size="md" />
        </div>
      )}
    </div>
  );
}

function PlusIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  );
}

function CloseIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

function EditIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
      <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
    </svg>
  );
}

function TrashIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
      <line x1="10" y1="11" x2="10" y2="17" />
      <line x1="14" y1="11" x2="14" y2="17" />
    </svg>
  );
}

function TargetIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="10" />
      <circle cx="12" cy="12" r="6" />
      <circle cx="12" cy="12" r="2" />
    </svg>
  );
}

function CalendarIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="4" width="18" height="18" rx="2" ry="2" />
      <line x1="16" y1="2" x2="16" y2="6" />
      <line x1="8" y1="2" x2="8" y2="6" />
      <line x1="3" y1="10" x2="21" y2="10" />
    </svg>
  );
}

function TimerIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="10" y1="2" x2="14" y2="2" />
      <line x1="12" y1="14" x2="15" y2="11" />
      <circle cx="12" cy="14" r="8" />
    </svg>
  );
}

function SchedulingIcon({ size = 24 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="10" />
      <polyline points="12 6 12 12 16 14" />
    </svg>
  );
}
