pub mod api;
pub mod credentials;
pub mod dispatch;
pub mod usage_manager;

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

use crate::api::{
    fetch_claude_usage, fetch_codex_usage, refresh_claude_token, refresh_codex_token,
    ClaudeTokenRefreshResponse, ClaudeUsageResponse, CodexTokenRefreshResponse, CodexUsageResponse,
};
use crate::credentials::{
    read_claude_credentials, read_codex_credentials, ClaudeCredentials, CodexCredentials,
};
use crate::dispatch::{
    DispatchCoordinator, DispatchJob, DispatchJobEnabledInput, DispatchJobUpsertInput,
    DispatchState,
};
use crate::usage_manager::{UsageData, UsageManager, UsageOps};

const CLAUDE_TTL: Duration = Duration::from_secs(120);
const CODEX_TTL: Duration = Duration::from_secs(60);

pub struct AppState {
    claude: Arc<UsageManager<ClaudeOps>>,
    codex: Arc<UsageManager<CodexOps>>,
    dispatch: Arc<DispatchCoordinator>,
}

impl AppState {
    pub async fn new(app: &tauri::AppHandle) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/124.0.0.0 Safari/537.36",
            )
            .build()
            .expect("Failed to create reqwest client");

        let claude = Arc::new(UsageManager::new(ClaudeOps {
            client: client.clone(),
        }));
        let codex = Arc::new(UsageManager::new(CodexOps { client }));
        let dispatch = Arc::new(DispatchCoordinator::new(app, claude.clone(), codex.clone()).await?);
        dispatch.start();

        Ok(Self {
            claude,
            codex,
            dispatch,
        })
    }
}

impl UsageData for ClaudeUsageResponse {
    fn mark_stale(&self, reason: &str, retry_after: Option<String>) -> Self {
        let mut clone = self.clone();
        clone.stale = true;
        clone.stale_reason = Some(reason.to_string());
        clone.retry_after = retry_after;
        clone
    }
}

impl UsageData for CodexUsageResponse {
    fn mark_stale(&self, reason: &str, retry_after: Option<String>) -> Self {
        let mut clone = self.clone();
        clone.stale = true;
        clone.stale_reason = Some(reason.to_string());
        clone.retry_after = retry_after;
        clone
    }
}

pub(crate) struct ClaudeOps {
    client: reqwest::Client,
}

#[async_trait]
impl UsageOps for ClaudeOps {
    type Data = ClaudeUsageResponse;
    type Credentials = ClaudeCredentials;
    type Refresh = ClaudeTokenRefreshResponse;

    fn ttl(&self) -> Duration {
        CLAUDE_TTL
    }

    fn credentials_refresh_token<'a>(&self, creds: &'a Self::Credentials) -> &'a str {
        &creds.refresh_token
    }

    fn merge_refresh(
        &self,
        creds: &Self::Credentials,
        refreshed: Self::Refresh,
    ) -> Self::Credentials {
        Self::Credentials {
            access_token: refreshed.access_token,
            refresh_token: refreshed
                .refresh_token
                .unwrap_or_else(|| creds.refresh_token.clone()),
            expires_at: creds.expires_at,
            subscription_type: creds.subscription_type.clone(),
        }
    }

    async fn read_credentials(&self) -> Result<Self::Credentials, String> {
        read_claude_credentials()
    }

    async fn fetch_usage(&self, creds: &Self::Credentials) -> Result<Self::Data, String> {
        fetch_claude_usage(&self.client, creds).await
    }

    async fn refresh_credentials(&self, refresh_token: &str) -> Result<Self::Refresh, String> {
        refresh_claude_token(&self.client, refresh_token).await
    }
}

pub(crate) struct CodexOps {
    client: reqwest::Client,
}

#[async_trait]
impl UsageOps for CodexOps {
    type Data = CodexUsageResponse;
    type Credentials = CodexCredentials;
    type Refresh = CodexTokenRefreshResponse;

    fn ttl(&self) -> Duration {
        CODEX_TTL
    }

    fn credentials_refresh_token<'a>(&self, creds: &'a Self::Credentials) -> &'a str {
        &creds.refresh_token
    }

    fn merge_refresh(
        &self,
        creds: &Self::Credentials,
        refreshed: Self::Refresh,
    ) -> Self::Credentials {
        Self::Credentials {
            access_token: refreshed.access_token,
            refresh_token: refreshed
                .refresh_token
                .unwrap_or_else(|| creds.refresh_token.clone()),
            account_id: creds.account_id.clone(),
        }
    }

    async fn read_credentials(&self) -> Result<Self::Credentials, String> {
        read_codex_credentials()
    }

    async fn fetch_usage(&self, creds: &Self::Credentials) -> Result<Self::Data, String> {
        fetch_codex_usage(&self.client, creds).await
    }

    async fn refresh_credentials(&self, refresh_token: &str) -> Result<Self::Refresh, String> {
        refresh_codex_token(&self.client, refresh_token).await
    }
}

#[tauri::command]
async fn get_claude_usage(
    state: tauri::State<'_, AppState>,
    force: Option<bool>,
) -> Result<ClaudeUsageResponse, String> {
    state.claude.get_usage(force.unwrap_or(false)).await
}

#[tauri::command]
async fn get_codex_usage(
    state: tauri::State<'_, AppState>,
    force: Option<bool>,
) -> Result<CodexUsageResponse, String> {
    state.codex.get_usage(force.unwrap_or(false)).await
}

#[tauri::command]
async fn get_dispatch_state(state: tauri::State<'_, AppState>) -> Result<DispatchState, String> {
    Ok(state.dispatch.get_state().await)
}

#[tauri::command]
async fn upsert_dispatch_job(
    state: tauri::State<'_, AppState>,
    input: DispatchJobUpsertInput,
) -> Result<DispatchJob, String> {
    state.dispatch.upsert_job(input).await
}

#[tauri::command]
async fn delete_dispatch_job(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    state.dispatch.delete_job(&id).await
}

#[tauri::command]
async fn set_dispatch_job_enabled(
    state: tauri::State<'_, AppState>,
    input: DispatchJobEnabledInput,
) -> Result<DispatchJob, String> {
    state.dispatch.set_job_enabled(input).await
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = tauri::async_runtime::block_on(AppState::new(app.handle()))
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_claude_usage,
            get_codex_usage,
            get_dispatch_state,
            upsert_dispatch_job,
            delete_dispatch_job,
            set_dispatch_job_enabled,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
