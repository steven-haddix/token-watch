pub mod api;
pub mod credentials;
pub mod usage_manager;

use async_trait::async_trait;
use std::time::Duration;

use crate::api::{
    fetch_claude_usage, fetch_codex_usage, refresh_claude_token, refresh_codex_token,
    ClaudeTokenRefreshResponse, ClaudeUsageResponse, CodexTokenRefreshResponse, CodexUsageResponse,
};
use crate::credentials::{
    read_claude_credentials, read_codex_credentials, ClaudeCredentials, CodexCredentials,
};
use crate::usage_manager::{UsageData, UsageManager, UsageOps};

const CLAUDE_TTL: Duration = Duration::from_secs(120);
const CODEX_TTL: Duration = Duration::from_secs(60);

pub struct AppState {
    claude: UsageManager<ClaudeOps>,
    codex: UsageManager<CodexOps>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/124.0.0.0 Safari/537.36",
            )
            .build()
            .expect("Failed to create reqwest client");

        Self {
            claude: UsageManager::new(ClaudeOps {
                client: client.clone(),
            }),
            codex: UsageManager::new(CodexOps { client }),
        }
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

struct ClaudeOps {
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

struct CodexOps {
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
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_claude_usage,
            get_codex_usage,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
