use crate::api::{ClaudeUsageResponse, CodexUsageResponse};
use crate::usage_manager::{AUTH_REQUIRED, UsageManager};
use crate::{ClaudeOps, CodexOps};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::Manager;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{self, Duration};

const MAX_RECENT_RUNS: usize = 50;
static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispatchTarget {
    Codex,
    ClaudeGeneric,
    ClaudeSonnet,
    ClaudeOpus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispatchScheduleKind {
    OnceNextReset,
    EveryReset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispatchRunStatus {
    Running,
    Succeeded,
    Failed,
    SkippedNoBudget,
    SkippedOverlap,
    SkippedUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchJob {
    pub id: String,
    pub name: String,
    pub target: DispatchTarget,
    pub command: String,
    pub schedule_kind: DispatchScheduleKind,
    pub min_remaining_percent: u8,
    pub max_time_before_reset_minutes: u32,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchRun {
    pub id: String,
    pub job_id: String,
    pub cycle_key: String,
    pub status: DispatchRunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveDispatchRun {
    pub run_id: String,
    pub job_id: String,
    pub job_name: String,
    pub target: DispatchTarget,
    pub started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchState {
    pub jobs: Vec<DispatchJob>,
    pub recent_runs: Vec<DispatchRun>,
    pub active_runs: Vec<ActiveDispatchRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchJobUpsertInput {
    pub id: Option<String>,
    pub name: String,
    pub target: DispatchTarget,
    pub command: String,
    pub schedule_kind: DispatchScheduleKind,
    pub min_remaining_percent: u8,
    pub max_time_before_reset_minutes: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchJobEnabledInput {
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedDispatchState {
    #[serde(default)]
    jobs: Vec<DispatchJob>,
    #[serde(default)]
    recent_runs: Vec<DispatchRun>,
    #[serde(default)]
    cursors: HashMap<String, DispatchCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DispatchCursor {
    last_cycle_key: Option<String>,
}

#[derive(Default)]
struct DispatchStore {
    persisted: PersistedDispatchState,
    active_runs: HashMap<DispatchFamily, ActiveDispatchRun>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DispatchFamily {
    Claude,
    Codex,
}

impl DispatchTarget {
    fn family(&self) -> DispatchFamily {
        match self {
            DispatchTarget::Codex => DispatchFamily::Codex,
            DispatchTarget::ClaudeGeneric
            | DispatchTarget::ClaudeSonnet
            | DispatchTarget::ClaudeOpus => DispatchFamily::Claude,
        }
    }

    fn slug(&self) -> &'static str {
        match self {
            DispatchTarget::Codex => "codex",
            DispatchTarget::ClaudeGeneric => "claude-generic",
            DispatchTarget::ClaudeSonnet => "claude-sonnet",
            DispatchTarget::ClaudeOpus => "claude-opus",
        }
    }
}

#[derive(Clone)]
struct GateWindow {
    remaining_percent: f64,
    reset_at: DateTime<Utc>,
}

#[derive(Clone)]
struct GateSnapshot {
    family: DispatchFamily,
    cycle_key: String,
    watch_window_open: bool,
    passes_remaining: bool,
    live_data_ok: bool,
    unavailable_summary: String,
}

pub struct DispatchCoordinator {
    claude: Arc<UsageManager<ClaudeOps>>,
    codex: Arc<UsageManager<CodexOps>>,
    store: Mutex<DispatchStore>,
    data_path: PathBuf,
}

impl DispatchCoordinator {
    pub(crate) async fn new(
        app: &tauri::AppHandle,
        claude: Arc<UsageManager<ClaudeOps>>,
        codex: Arc<UsageManager<CodexOps>>,
    ) -> Result<Self, String> {
        let mut data_path = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;
        data_path.push("dispatch-state.json");

        let persisted = load_persisted_state(&data_path).await?;
        Ok(Self {
            claude,
            codex,
            store: Mutex::new(DispatchStore {
                persisted,
                active_runs: HashMap::new(),
            }),
            data_path,
        })
    }

    pub(crate) fn start(self: &Arc<Self>) {
        let coordinator = self.clone();
        tokio::spawn(async move {
            coordinator.tick().await;

            let mut interval = time::interval(Duration::from_secs(60));
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                coordinator.tick().await;
            }
        });
    }

    pub(crate) fn schedule_tick(self: &Arc<Self>) {
        let coordinator = self.clone();
        tokio::spawn(async move {
            coordinator.tick().await;
        });
    }

    pub(crate) async fn get_state(&self) -> DispatchState {
        let store = self.store.lock().await;
        DispatchState {
            jobs: sorted_jobs(&store.persisted.jobs),
            recent_runs: sorted_runs(&store.persisted.recent_runs),
            active_runs: store.active_runs.values().cloned().collect(),
        }
    }

    pub(crate) async fn upsert_job(
        self: &Arc<Self>,
        input: DispatchJobUpsertInput,
    ) -> Result<DispatchJob, String> {
        validate_job_input(&input)?;
        let now = Utc::now().to_rfc3339();
        let mut job = DispatchJob {
            id: input
                .id
                .clone()
                .unwrap_or_else(|| next_id("job", Utc::now())),
            name: input.name.trim().to_string(),
            target: input.target,
            command: input.command.trim().to_string(),
            schedule_kind: input.schedule_kind,
            min_remaining_percent: input.min_remaining_percent,
            max_time_before_reset_minutes: input.max_time_before_reset_minutes,
            enabled: input.enabled,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        let snapshot = {
            let mut store = self.store.lock().await;
            if let Some(existing) = store
                .persisted
                .jobs
                .iter_mut()
                .find(|candidate| candidate.id == job.id)
            {
                job.created_at = existing.created_at.clone();
                existing.name = job.name.clone();
                existing.target = job.target.clone();
                existing.command = job.command.clone();
                existing.schedule_kind = job.schedule_kind.clone();
                existing.min_remaining_percent = job.min_remaining_percent;
                existing.max_time_before_reset_minutes = job.max_time_before_reset_minutes;
                existing.enabled = job.enabled;
                existing.updated_at = now;
                job = existing.clone();
            } else {
                store.persisted.jobs.push(job.clone());
            }

            store.persisted.cursors.remove(&job.id);
            store.persisted.clone()
        };

        persist_state(&self.data_path, &snapshot).await?;
        self.schedule_tick();
        Ok(job)
    }

    pub(crate) async fn delete_job(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let snapshot = {
            let mut store = self.store.lock().await;
            let initial_len = store.persisted.jobs.len();
            store.persisted.jobs.retain(|job| job.id != id);
            if store.persisted.jobs.len() == initial_len {
                return Err("Dispatch job not found".to_string());
            }
            store.persisted.cursors.remove(id);
            store.persisted.clone()
        };

        persist_state(&self.data_path, &snapshot).await
    }

    pub(crate) async fn set_job_enabled(
        self: &Arc<Self>,
        input: DispatchJobEnabledInput,
    ) -> Result<DispatchJob, String> {
        let (job, snapshot) = {
            let mut store = self.store.lock().await;
            let target_id = input.id.clone();
            let job = store
                .persisted
                .jobs
                .iter_mut()
                .find(|job| job.id == target_id)
                .ok_or_else(|| "Dispatch job not found".to_string())?;
            job.enabled = input.enabled;
            job.updated_at = Utc::now().to_rfc3339();
            let job_id = job.id.clone();
            if input.enabled {
                let _ = job;
                store.persisted.cursors.remove(&job_id);
                let job = store
                    .persisted
                    .jobs
                    .iter()
                    .find(|job| job.id == target_id)
                    .expect("job must exist after enabling")
                    .clone();
                (job, store.persisted.clone())
            } else {
                (job.clone(), store.persisted.clone())
            }
        };

        persist_state(&self.data_path, &snapshot).await?;
        if job.enabled {
            self.schedule_tick();
        }
        Ok(job)
    }

    async fn tick(self: &Arc<Self>) {
        let jobs = {
            let store = self.store.lock().await;
            store.persisted.jobs.clone()
        };

        let mut claude_usage: Option<Result<ClaudeUsageResponse, String>> = None;
        let mut codex_usage: Option<Result<CodexUsageResponse, String>> = None;

        for job in jobs.into_iter().filter(|job| job.enabled) {
            let snapshot = match job.target.family() {
                DispatchFamily::Claude => {
                    if claude_usage.is_none() {
                        claude_usage = Some(self.claude.get_usage(false).await);
                    }

                    match claude_usage.clone().unwrap() {
                        Ok(usage) => Some(snapshot_for_claude(&job, &usage)),
                        Err(error) => {
                            eprintln!("dispatch: failed to fetch Claude usage: {error}");
                            None
                        }
                    }
                }
                DispatchFamily::Codex => {
                    if codex_usage.is_none() {
                        codex_usage = Some(self.codex.get_usage(false).await);
                    }

                    match codex_usage.clone().unwrap() {
                        Ok(usage) => Some(snapshot_for_codex(&job, &usage)),
                        Err(error) => {
                            eprintln!("dispatch: failed to fetch Codex usage: {error}");
                            None
                        }
                    }
                }
            };

            let Some(snapshot) = snapshot else {
                continue;
            };

            if let Err(error) = self.advance_cycle_if_needed(&job, &snapshot).await {
                eprintln!("dispatch: failed to advance cycle: {error}");
                continue;
            }

            if self
                .has_recorded_cycle(&job.id, &snapshot.cycle_key)
                .await
            {
                continue;
            }

            if !snapshot.live_data_ok {
                if snapshot.watch_window_open {
                    let _ = self
                        .record_cycle_result(
                            &job,
                            &snapshot.cycle_key,
                            DispatchRunStatus::SkippedUnavailable,
                            None,
                            snapshot.unavailable_summary.clone(),
                        )
                        .await;
                }
                continue;
            }

            if !snapshot.watch_window_open || !snapshot.passes_remaining {
                continue;
            }

            if self.family_is_active(snapshot.family).await {
                let _ = self
                    .record_cycle_result(
                        &job,
                        &snapshot.cycle_key,
                        DispatchRunStatus::SkippedOverlap,
                        None,
                        format!("Skipped because another {} dispatch is already running.", family_label(snapshot.family)),
                    )
                    .await;
                continue;
            }

            if let Err(error) = self.launch_job(job.clone(), snapshot).await {
                eprintln!("dispatch: failed to launch job {}: {}", job.id, error);
            }
        }
    }

    async fn advance_cycle_if_needed(
        &self,
        job: &DispatchJob,
        snapshot: &GateSnapshot,
    ) -> Result<(), String> {
        let maybe_snapshot = {
            let mut store = self.store.lock().await;
            let cursor = store.persisted.cursors.entry(job.id.clone()).or_default();
            let previous_cycle = cursor.last_cycle_key.clone();
            cursor.last_cycle_key = Some(snapshot.cycle_key.clone());

            if let Some(previous_cycle) = previous_cycle {
                if previous_cycle != snapshot.cycle_key
                    && !store.persisted.recent_runs.iter().any(|run| {
                        run.job_id == job.id && run.cycle_key == previous_cycle
                    })
                {
                    let mut next_state = store.persisted.clone();
                    next_state.recent_runs.insert(
                        0,
                        DispatchRun {
                            id: next_id("run", Utc::now()),
                            job_id: job.id.clone(),
                            cycle_key: previous_cycle,
                            status: DispatchRunStatus::SkippedNoBudget,
                            started_at: Utc::now().to_rfc3339(),
                            finished_at: Some(Utc::now().to_rfc3339()),
                            exit_code: None,
                            summary: "Skipped because the reset window closed before the job met its budget gate.".to_string(),
                        },
                    );
                    trim_runs(&mut next_state.recent_runs);
                    store.persisted = next_state.clone();
                    Some(next_state)
                } else {
                    Some(store.persisted.clone())
                }
            } else {
                Some(store.persisted.clone())
            }
        };

        if let Some(snapshot) = maybe_snapshot {
            persist_state(&self.data_path, &snapshot).await?;
        }

        Ok(())
    }

    async fn has_recorded_cycle(&self, job_id: &str, cycle_key: &str) -> bool {
        let store = self.store.lock().await;
        store
            .persisted
            .recent_runs
            .iter()
            .any(|run| run.job_id == job_id && run.cycle_key == cycle_key)
    }

    async fn family_is_active(&self, family: DispatchFamily) -> bool {
        let store = self.store.lock().await;
        store.active_runs.contains_key(&family)
    }

    async fn record_cycle_result(
        &self,
        job: &DispatchJob,
        cycle_key: &str,
        status: DispatchRunStatus,
        exit_code: Option<i32>,
        summary: String,
    ) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        let snapshot = {
            let mut store = self.store.lock().await;
            store.persisted.recent_runs.insert(
                0,
                DispatchRun {
                    id: next_id("run", Utc::now()),
                    job_id: job.id.clone(),
                    cycle_key: cycle_key.to_string(),
                    status,
                    started_at: now.clone(),
                    finished_at: Some(now),
                    exit_code,
                    summary,
                },
            );
            trim_runs(&mut store.persisted.recent_runs);
            store.persisted.clone()
        };

        persist_state(&self.data_path, &snapshot).await
    }

    async fn launch_job(
        self: &Arc<Self>,
        job: DispatchJob,
        snapshot: GateSnapshot,
    ) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        let run_id = next_id("run", Utc::now());
        let run = DispatchRun {
            id: run_id.clone(),
            job_id: job.id.clone(),
            cycle_key: snapshot.cycle_key.clone(),
            status: DispatchRunStatus::Running,
            started_at: now.clone(),
            finished_at: None,
            exit_code: None,
            summary: format!("Started {}", job.name),
        };
        let active = ActiveDispatchRun {
            run_id: run_id.clone(),
            job_id: job.id.clone(),
            job_name: job.name.clone(),
            target: job.target.clone(),
            started_at: now,
        };

        let persisted_snapshot = {
            let mut store = self.store.lock().await;
            store.persisted.recent_runs.insert(0, run);
            trim_runs(&mut store.persisted.recent_runs);
            store.active_runs.insert(snapshot.family, active);
            store.persisted.clone()
        };
        persist_state(&self.data_path, &persisted_snapshot).await?;

        let coordinator = self.clone();
        tokio::spawn(async move {
            let status = execute_command(&job.command).await;
            if let Err(error) = coordinator.finish_run(job, run_id, snapshot.family, status).await {
                eprintln!("dispatch: failed to finish run: {error}");
            }
        });

        Ok(())
    }

    async fn finish_run(
        &self,
        job: DispatchJob,
        run_id: String,
        family: DispatchFamily,
        status: Result<std::process::ExitStatus, String>,
    ) -> Result<(), String> {
        let finished_at = Utc::now().to_rfc3339();
        let (exit_code, run_status, summary) = match status {
            Ok(exit_status) if exit_status.success() => (
                exit_status.code(),
                DispatchRunStatus::Succeeded,
                "Completed successfully.".to_string(),
            ),
            Ok(exit_status) => (
                exit_status.code(),
                DispatchRunStatus::Failed,
                match exit_status.code() {
                    Some(code) => format!("Exited with code {code}."),
                    None => "Process exited unsuccessfully.".to_string(),
                },
            ),
            Err(error) => (None, DispatchRunStatus::Failed, error),
        };

        let snapshot = {
            let mut store = self.store.lock().await;
            store.active_runs.remove(&family);

            if let Some(run) = store
                .persisted
                .recent_runs
                .iter_mut()
                .find(|run| run.id == run_id)
            {
                run.status = run_status;
                run.finished_at = Some(finished_at.clone());
                run.exit_code = exit_code;
                run.summary = summary.clone();
            }

            if job.schedule_kind == DispatchScheduleKind::OnceNextReset {
                if let Some(saved_job) = store
                    .persisted
                    .jobs
                    .iter_mut()
                    .find(|saved_job| saved_job.id == job.id)
                {
                    saved_job.enabled = false;
                    saved_job.updated_at = finished_at.clone();
                }
            }

            store.persisted.clone()
        };

        persist_state(&self.data_path, &snapshot).await
    }
}

async fn execute_command(command: &str) -> Result<std::process::ExitStatus, String> {
    let home = std::env::var("HOME").ok();
    let mut process = Command::new("/bin/zsh");
    process
        .arg("-lc")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(home) = home {
        process.current_dir(home);
    }

    process
        .status()
        .await
        .map_err(|e| format!("Failed to launch command: {e}"))
}

async fn load_persisted_state(path: &Path) -> Result<PersistedDispatchState, String> {
    if !path.exists() {
        return Ok(PersistedDispatchState::default());
    }

    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| format!("Failed to read dispatch state: {e}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| format!("Failed to parse dispatch state: {e}"))
}

async fn persist_state(path: &Path, snapshot: &PersistedDispatchState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create dispatch data directory: {e}"))?;
    }

    let tmp_path = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(snapshot)
        .map_err(|e| format!("Failed to serialize dispatch state: {e}"))?;
    tokio::fs::write(&tmp_path, bytes)
        .await
        .map_err(|e| format!("Failed to write dispatch state: {e}"))?;
    tokio::fs::rename(&tmp_path, path)
        .await
        .map_err(|e| format!("Failed to replace dispatch state: {e}"))
}

fn validate_job_input(input: &DispatchJobUpsertInput) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("Job name is required".to_string());
    }
    if input.command.trim().is_empty() {
        return Err("Job command is required".to_string());
    }
    if input.min_remaining_percent == 0 {
        return Err("Minimum remaining percentage must be at least 1".to_string());
    }
    if input.max_time_before_reset_minutes == 0 {
        return Err("Time before reset must be at least 1 minute".to_string());
    }
    Ok(())
}

fn sorted_jobs(jobs: &[DispatchJob]) -> Vec<DispatchJob> {
    let mut cloned = jobs.to_vec();
    cloned.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    cloned
}

fn sorted_runs(runs: &[DispatchRun]) -> Vec<DispatchRun> {
    let mut cloned = runs.to_vec();
    cloned.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    cloned
}

fn trim_runs(runs: &mut Vec<DispatchRun>) {
    if runs.len() > MAX_RECENT_RUNS {
        runs.truncate(MAX_RECENT_RUNS);
    }
}

fn next_id(prefix: &str, now: DateTime<Utc>) -> String {
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{counter}", now.timestamp_millis())
}

fn parse_iso(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn snapshot_for_claude(job: &DispatchJob, usage: &ClaudeUsageResponse) -> GateSnapshot {
    let mut windows = vec![
        GateWindow {
            remaining_percent: usage.five_hour.remaining,
            reset_at: parse_iso(&usage.five_hour.resets_at),
        },
        GateWindow {
            remaining_percent: usage.seven_day.remaining,
            reset_at: parse_iso(&usage.seven_day.resets_at),
        },
    ];

    match job.target {
        DispatchTarget::ClaudeSonnet => {
            if let Some(window) = &usage.seven_day_sonnet {
                windows.push(GateWindow {
                    remaining_percent: window.remaining,
                    reset_at: parse_iso(&window.resets_at),
                });
            }
        }
        DispatchTarget::ClaudeOpus => {
            if let Some(window) = &usage.seven_day_opus {
                windows.push(GateWindow {
                    remaining_percent: window.remaining,
                    reset_at: parse_iso(&window.resets_at),
                });
            }
        }
        DispatchTarget::Codex | DispatchTarget::ClaudeGeneric => {}
    }

    build_snapshot(job, usage.stale, usage.stale_reason.as_deref(), windows)
}

fn snapshot_for_codex(job: &DispatchJob, usage: &CodexUsageResponse) -> GateSnapshot {
    let windows = vec![
        GateWindow {
            remaining_percent: usage.primary_window.remaining_percent,
            reset_at: parse_iso(&usage.primary_window.resets_at),
        },
        GateWindow {
            remaining_percent: usage.secondary_window.remaining_percent,
            reset_at: parse_iso(&usage.secondary_window.resets_at),
        },
    ];

    build_snapshot(job, usage.stale, usage.stale_reason.as_deref(), windows)
}

fn build_snapshot(
    job: &DispatchJob,
    stale: bool,
    stale_reason: Option<&str>,
    windows: Vec<GateWindow>,
) -> GateSnapshot {
    let now = Utc::now();
    let anchor_reset_at = windows
        .iter()
        .map(|window| window.reset_at)
        .min()
        .unwrap_or(now);
    let seconds_until_reset = (anchor_reset_at - now).num_seconds();
    let watch_window_open = seconds_until_reset >= 0
        && seconds_until_reset <= (job.max_time_before_reset_minutes as i64 * 60);
    let passes_remaining = windows
        .iter()
        .all(|window| window.remaining_percent >= job.min_remaining_percent as f64);

    GateSnapshot {
        family: job.target.family(),
        cycle_key: format!("{}:{}", job.target.slug(), anchor_reset_at.timestamp()),
        watch_window_open,
        passes_remaining,
        live_data_ok: !stale,
        unavailable_summary: unavailable_summary(stale_reason),
    }
}

fn unavailable_summary(reason: Option<&str>) -> String {
    match reason {
        Some("auth_error") | Some(AUTH_REQUIRED) => {
            "Skipped because live usage data requires reauthentication.".to_string()
        }
        Some("network_error") => {
            "Skipped because live usage data was unavailable due to a network error.".to_string()
        }
        Some("rate_limited") => {
            "Skipped because the usage API was rate limited.".to_string()
        }
        _ => "Skipped because live usage data was unavailable.".to_string(),
    }
}

fn family_label(family: DispatchFamily) -> &'static str {
    match family {
        DispatchFamily::Claude => "Claude",
        DispatchFamily::Codex => "Codex",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{CodexWindowUsage, ExtraUsage, WindowUsage};
    use std::fs;

    fn claude_usage() -> ClaudeUsageResponse {
        ClaudeUsageResponse {
            five_hour: WindowUsage {
                utilization: 20.0,
                remaining: 80.0,
                resets_at: "2026-03-28T12:30:00Z".to_string(),
            },
            seven_day: WindowUsage {
                utilization: 30.0,
                remaining: 70.0,
                resets_at: "2026-04-01T00:00:00Z".to_string(),
            },
            seven_day_opus: Some(WindowUsage {
                utilization: 40.0,
                remaining: 60.0,
                resets_at: "2026-04-01T00:00:00Z".to_string(),
            }),
            seven_day_sonnet: Some(WindowUsage {
                utilization: 45.0,
                remaining: 55.0,
                resets_at: "2026-04-01T00:00:00Z".to_string(),
            }),
            subscription_type: "pro".to_string(),
            extra_usage: ExtraUsage {
                is_enabled: false,
                used_credits: None,
                utilization: None,
            },
            stale: false,
            stale_reason: None,
            retry_after: None,
        }
    }

    fn codex_usage() -> CodexUsageResponse {
        CodexUsageResponse {
            plan_type: "plus".to_string(),
            primary_window: CodexWindowUsage {
                used_percent: 15.0,
                remaining_percent: 85.0,
                reset_at_unix: 1773859200,
                resets_at: "2026-03-28T12:20:00Z".to_string(),
            },
            secondary_window: CodexWindowUsage {
                used_percent: 25.0,
                remaining_percent: 75.0,
                reset_at_unix: 1774464000,
                resets_at: "2026-04-01T00:00:00Z".to_string(),
            },
            has_credits: true,
            limit_reached: false,
            stale: false,
            stale_reason: None,
            retry_after: None,
        }
    }

    fn job(target: DispatchTarget) -> DispatchJob {
        DispatchJob {
            id: "job-1".to_string(),
            name: "Run report".to_string(),
            target,
            command: "echo hi".to_string(),
            schedule_kind: DispatchScheduleKind::EveryReset,
            min_remaining_percent: 50,
            max_time_before_reset_minutes: 60,
            enabled: true,
            created_at: "2026-03-28T12:00:00Z".to_string(),
            updated_at: "2026-03-28T12:00:00Z".to_string(),
        }
    }

    #[test]
    fn claude_model_jobs_include_specific_weekly_window() {
        let snapshot = snapshot_for_claude(&job(DispatchTarget::ClaudeOpus), &claude_usage());
        assert!(snapshot.passes_remaining);
        assert!(snapshot.cycle_key.starts_with("claude-opus:"));
    }

    #[test]
    fn all_relevant_windows_must_pass() {
        let mut usage = codex_usage();
        usage.secondary_window.remaining_percent = 40.0;
        let snapshot = snapshot_for_codex(&job(DispatchTarget::Codex), &usage);
        assert!(!snapshot.passes_remaining);
    }

    #[test]
    fn stale_usage_blocks_dispatch() {
        let mut usage = claude_usage();
        usage.stale = true;
        usage.stale_reason = Some("auth_error".to_string());
        let snapshot = snapshot_for_claude(&job(DispatchTarget::ClaudeGeneric), &usage);
        assert!(!snapshot.live_data_ok);
    }

    #[tokio::test]
    async fn persisted_state_round_trips() {
        let dir = std::env::temp_dir().join(next_id("dispatch-test", Utc::now()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dispatch-state.json");
        let mut state = PersistedDispatchState::default();
        state.jobs.push(job(DispatchTarget::Codex));
        persist_state(&path, &state).await.unwrap();
        let loaded = load_persisted_state(&path).await.unwrap();
        assert_eq!(loaded.jobs.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
