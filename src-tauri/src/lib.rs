pub mod credentials;
pub mod api;

use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::api::{
    ClaudeUsageResponse, CodexUsageResponse,
    fetch_claude_usage, fetch_codex_usage,
    refresh_claude_token, refresh_codex_token,
};
use crate::credentials::{
    ClaudeCredentials, CodexCredentials,
    read_claude_credentials, read_codex_credentials,
};

// ── Cache wrapper ─────────────────────────────────────────────────────────────

struct CachedData<T: Clone> {
    data: Option<T>,
    fetched_at: Option<Instant>,
    stale: bool, // true when last fetch failed (rate limit / network error)
}

impl<T: Clone> CachedData<T> {
    fn new() -> Self {
        Self {
            data: None,
            fetched_at: None,
            stale: false,
        }
    }

    /// Returns Some(data) if cached data exists and is younger than `ttl`.
    fn get_if_fresh(&self, ttl: Duration) -> Option<&T> {
        if let (Some(data), Some(fetched_at)) = (&self.data, &self.fetched_at) {
            if fetched_at.elapsed() < ttl {
                return Some(data);
            }
        }
        None
    }
}

// ── Application state ─────────────────────────────────────────────────────────

pub struct AppState {
    client: reqwest::Client,
    claude_cache: RwLock<CachedData<ClaudeUsageResponse>>,
    codex_cache: RwLock<CachedData<CodexUsageResponse>>,
    claude_creds: RwLock<Option<ClaudeCredentials>>,
    codex_creds: RwLock<Option<CodexCredentials>>,
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
            client,
            claude_cache: RwLock::new(CachedData::new()),
            codex_cache: RwLock::new(CachedData::new()),
            claude_creds: RwLock::new(None),
            codex_creds: RwLock::new(None),
        }
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

const CLAUDE_TTL: Duration = Duration::from_secs(120);
const CODEX_TTL: Duration = Duration::from_secs(60);

#[tauri::command]
async fn get_claude_usage(
    state: tauri::State<'_, AppState>,
) -> Result<ClaudeUsageResponse, String> {
    // 1. Return fresh cached data if available.
    {
        let cache = state.claude_cache.read().await;
        if let Some(data) = cache.get_if_fresh(CLAUDE_TTL) {
            return Ok(data.clone());
        }
    }

    // 2. Get credentials — use cached value or read from keychain.
    let creds: ClaudeCredentials = {
        let cached = state.claude_creds.read().await;
        match cached.clone() {
            Some(c) => c,
            None => {
                drop(cached);
                let fresh = read_claude_credentials()?;
                let mut w = state.claude_creds.write().await;
                *w = Some(fresh.clone());
                fresh
            }
        }
    };

    // 3. Attempt fetch.
    match fetch_claude_usage(&state.client, &creds).await {
        Ok(data) => {
            let mut cache = state.claude_cache.write().await;
            *cache = CachedData {
                data: Some(data.clone()),
                fetched_at: Some(Instant::now()),
                stale: false,
            };
            Ok(data)
        }

        Err(e) if e == "UNAUTHORIZED" => {
            // 4. Refresh token and retry once.
            let refreshed = refresh_claude_token(&state.client, &creds.refresh_token).await?;
            let new_creds = ClaudeCredentials {
                access_token: refreshed.access_token,
                refresh_token: refreshed
                    .refresh_token
                    .unwrap_or(creds.refresh_token.clone()),
                expires_at: creds.expires_at,
                subscription_type: creds.subscription_type.clone(),
            };
            {
                let mut w = state.claude_creds.write().await;
                *w = Some(new_creds.clone());
            }
            let data = fetch_claude_usage(&state.client, &new_creds).await?;
            let mut cache = state.claude_cache.write().await;
            *cache = CachedData {
                data: Some(data.clone()),
                fetched_at: Some(Instant::now()),
                stale: false,
            };
            Ok(data)
        }

        Err(e) if e == "RATE_LIMITED" || e.contains("Network error") => {
            // 5. On rate-limit or network error, return stale data if available.
            let mut cache = state.claude_cache.write().await;
            cache.stale = true;
            if let Some(stale_data) = cache.data.clone() {
                Ok(stale_data)
            } else {
                Err(e)
            }
        }

        Err(e) => Err(e),
    }
}

#[tauri::command]
async fn get_codex_usage(
    state: tauri::State<'_, AppState>,
) -> Result<CodexUsageResponse, String> {
    // 1. Return fresh cached data if available.
    {
        let cache = state.codex_cache.read().await;
        if let Some(data) = cache.get_if_fresh(CODEX_TTL) {
            return Ok(data.clone());
        }
    }

    // 2. Get credentials — use cached value or read from file.
    let creds: CodexCredentials = {
        let cached = state.codex_creds.read().await;
        match cached.clone() {
            Some(c) => c,
            None => {
                drop(cached);
                let fresh = read_codex_credentials()?;
                let mut w = state.codex_creds.write().await;
                *w = Some(fresh.clone());
                fresh
            }
        }
    };

    // 3. Attempt fetch.
    match fetch_codex_usage(&state.client, &creds).await {
        Ok(data) => {
            let mut cache = state.codex_cache.write().await;
            *cache = CachedData {
                data: Some(data.clone()),
                fetched_at: Some(Instant::now()),
                stale: false,
            };
            Ok(data)
        }

        Err(e) if e == "UNAUTHORIZED" => {
            // 4. Refresh token and retry once.
            let refreshed = refresh_codex_token(&state.client, &creds.refresh_token).await?;
            let new_creds = CodexCredentials {
                access_token: refreshed.access_token,
                refresh_token: refreshed
                    .refresh_token
                    .unwrap_or(creds.refresh_token.clone()),
                account_id: creds.account_id.clone(),
            };
            {
                let mut w = state.codex_creds.write().await;
                *w = Some(new_creds.clone());
            }
            let data = fetch_codex_usage(&state.client, &new_creds).await?;
            let mut cache = state.codex_cache.write().await;
            *cache = CachedData {
                data: Some(data.clone()),
                fetched_at: Some(Instant::now()),
                stale: false,
            };
            Ok(data)
        }

        Err(e) if e == "RATE_LIMITED" || e.contains("Network error") => {
            // 5. On rate-limit or network error, return stale data if available.
            let mut cache = state.codex_cache.write().await;
            cache.stale = true;
            if let Some(stale_data) = cache.data.clone() {
                Ok(stale_data)
            } else {
                Err(e)
            }
        }

        Err(e) => Err(e),
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![get_claude_usage, get_codex_usage])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
