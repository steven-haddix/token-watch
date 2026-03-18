pub mod credentials;
pub mod api;

use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};

use crate::api::{
    ClaudeUsageResponse, CodexUsageResponse,
    fetch_claude_usage, fetch_codex_usage,
    refresh_claude_token, refresh_codex_token,
};
use crate::credentials::{
    ClaudeCredentials, CodexCredentials,
    read_claude_credentials, read_codex_credentials,
};

macro_rules! dbg_log {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) {
            eprintln!("[token-watch {}] {}", chrono::Utc::now().format("%H:%M:%S"), format!($($arg)*));
        }
    };
}

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
    /// Wall-clock time after which Claude API calls are allowed again.
    claude_retry_after: RwLock<Option<DateTime<Utc>>>,
    /// Wall-clock time after which Codex API calls are allowed again.
    codex_retry_after: RwLock<Option<DateTime<Utc>>>,
    /// Serializes concurrent fetches — callers that arrive while a fetch is
    /// in flight will wait, then get the freshly-cached result on re-check.
    claude_fetch_lock: tokio::sync::Mutex<()>,
    codex_fetch_lock: tokio::sync::Mutex<()>,
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
            client,
            claude_cache: RwLock::new(CachedData::new()),
            codex_cache: RwLock::new(CachedData::new()),
            claude_creds: RwLock::new(None),
            codex_creds: RwLock::new(None),
            claude_retry_after: RwLock::new(None),
            codex_retry_after: RwLock::new(None),
            claude_fetch_lock: tokio::sync::Mutex::new(()),
            codex_fetch_lock: tokio::sync::Mutex::new(()),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse retry-after seconds from a `RATE_LIMITED:<secs>` error string.
fn parse_retry_secs(e: &str) -> u64 {
    e.split(':').nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60)
}

const AUTH_REQUIRED: &str = "AUTH_REQUIRED";
const STALE_REASON_RATE_LIMITED: &str = "rate_limited";
const STALE_REASON_NETWORK_ERROR: &str = "network_error";
const STALE_REASON_AUTH_ERROR: &str = "auth_error";

// ── Tauri commands ────────────────────────────────────────────────────────────

const CLAUDE_TTL: Duration = Duration::from_secs(120);
const CODEX_TTL: Duration = Duration::from_secs(60);

#[tauri::command]
async fn get_claude_usage(
    state: tauri::State<'_, AppState>,
) -> Result<ClaudeUsageResponse, String> {
    // 0. Honour rate-limit cooldown — return stale data without hitting the API.
    {
        let retry_after = state.claude_retry_after.read().await;
        if let Some(retry_at) = *retry_after {
            if retry_at > Utc::now() {
                dbg_log!("claude: cooldown active, retry at {}", retry_at.format("%H:%M:%S"));
                let retry_iso = retry_at.to_rfc3339();
                let cache = state.claude_cache.read().await;
                if let Some(mut data) = cache.data.clone() {
                    data.stale = true;
                    data.stale_reason = Some(STALE_REASON_RATE_LIMITED.to_string());
                    data.retry_after = Some(retry_iso);
                    return Ok(data);
                }
                return Err(format!("RATE_LIMITED_UNTIL:{}", retry_iso));
            }
        }
    }

    // 1. Return fresh cached data if available.
    {
        let cache = state.claude_cache.read().await;
        if let Some(data) = cache.get_if_fresh(CLAUDE_TTL) {
            dbg_log!("claude: cache hit");
            return Ok(data.clone());
        }
    }

    // 1b. Serialize concurrent fetches: acquire lock then re-check both the
    //     cooldown and the cache. Callers queued while a prior fetch was
    //     in flight (including ones that ended in a 429) exit here without
    //     hitting the network.
    let _fetch_guard = state.claude_fetch_lock.lock().await;
    {
        let retry_after = state.claude_retry_after.read().await;
        if let Some(retry_at) = *retry_after {
            if retry_at > Utc::now() {
                dbg_log!("claude: cooldown active (after fetch-lock)");
                let retry_iso = retry_at.to_rfc3339();
                let cache = state.claude_cache.read().await;
                if let Some(mut data) = cache.data.clone() {
                    data.stale = true;
                    data.stale_reason = Some(STALE_REASON_RATE_LIMITED.to_string());
                    data.retry_after = Some(retry_iso);
                    return Ok(data);
                }
                return Err(format!("RATE_LIMITED_UNTIL:{}", retry_iso));
            }
        }
    }
    {
        let cache = state.claude_cache.read().await;
        if let Some(data) = cache.get_if_fresh(CLAUDE_TTL) {
            dbg_log!("claude: cache hit (after fetch-lock)");
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
    dbg_log!("claude: cache miss, fetching...");
    match fetch_claude_usage(&state.client, &creds).await {
        Ok(data) => {
            dbg_log!("claude: fetch succeeded");
            // Clear any lingering rate-limit cooldown on success.
            *state.claude_retry_after.write().await = None;
            let mut cache = state.claude_cache.write().await;
            *cache = CachedData {
                data: Some(data.clone()),
                fetched_at: Some(Instant::now()),
                stale: false,
            };
            Ok(data)
        }

        Err(e) if e == "UNAUTHORIZED" => {
            dbg_log!("claude: 401, refreshing token...");
            // 4. Refresh token and retry once.
            match refresh_claude_token(&state.client, &creds.refresh_token).await {
                Ok(refreshed) => {
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
                    match fetch_claude_usage(&state.client, &new_creds).await {
                        Ok(data) => {
                            dbg_log!("claude: fetch succeeded after refresh");
                            *state.claude_retry_after.write().await = None;
                            let mut cache = state.claude_cache.write().await;
                            *cache = CachedData {
                                data: Some(data.clone()),
                                fetched_at: Some(Instant::now()),
                                stale: false,
                            };
                            Ok(data)
                        }
                        Err(e) if e.starts_with("RATE_LIMITED") => {
                            let retry_secs = parse_retry_secs(&e);
                            let retry_at = Utc::now() + chrono::Duration::seconds(retry_secs as i64);
                            dbg_log!("claude: rate limited after refresh, cooldown set for {}s", retry_secs);
                            *state.claude_retry_after.write().await = Some(retry_at);

                            let mut cache = state.claude_cache.write().await;
                            cache.stale = true;
                            let retry_iso = retry_at.to_rfc3339();
                            if let Some(mut stale_data) = cache.data.clone() {
                                stale_data.stale = true;
                                stale_data.stale_reason = Some(STALE_REASON_RATE_LIMITED.to_string());
                                stale_data.retry_after = Some(retry_iso);
                                Ok(stale_data)
                            } else {
                                Err(format!("RATE_LIMITED_UNTIL:{}", retry_iso))
                            }
                        }
                        Err(e) if e.contains("Network error") => {
                            dbg_log!("claude: network error after refresh — {}", e);
                            let mut cache = state.claude_cache.write().await;
                            cache.stale = true;
                            if let Some(mut stale_data) = cache.data.clone() {
                                stale_data.stale = true;
                                stale_data.stale_reason = Some(STALE_REASON_NETWORK_ERROR.to_string());
                                stale_data.retry_after = None;
                                Ok(stale_data)
                            } else {
                                Err(e)
                            }
                        }
                        Err(e) if e == "UNAUTHORIZED" => {
                            dbg_log!("claude: refreshed token still unauthorized");
                            *state.claude_retry_after.write().await = None;
                            let mut creds = state.claude_creds.write().await;
                            *creds = None;
                            drop(creds);

                            let mut cache = state.claude_cache.write().await;
                            cache.stale = true;
                            if let Some(mut stale_data) = cache.data.clone() {
                                stale_data.stale = true;
                                stale_data.stale_reason = Some(STALE_REASON_AUTH_ERROR.to_string());
                                stale_data.retry_after = None;
                                Ok(stale_data)
                            } else {
                                Err(AUTH_REQUIRED.to_string())
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) if e == "UNAUTHORIZED" => {
                    dbg_log!("claude: refresh token rejected, clearing cached credentials");
                    *state.claude_retry_after.write().await = None;
                    let mut creds = state.claude_creds.write().await;
                    *creds = None;
                    drop(creds);

                    let mut cache = state.claude_cache.write().await;
                    cache.stale = true;
                    if let Some(mut stale_data) = cache.data.clone() {
                        stale_data.stale = true;
                        stale_data.stale_reason = Some(STALE_REASON_AUTH_ERROR.to_string());
                        stale_data.retry_after = None;
                        Ok(stale_data)
                    } else {
                        Err(AUTH_REQUIRED.to_string())
                    }
                }
                Err(e) if e.contains("Network error") => {
                    dbg_log!("claude: token refresh network error — {}", e);
                    let mut cache = state.claude_cache.write().await;
                    cache.stale = true;
                    if let Some(mut stale_data) = cache.data.clone() {
                        stale_data.stale = true;
                        stale_data.stale_reason = Some(STALE_REASON_NETWORK_ERROR.to_string());
                        stale_data.retry_after = None;
                        Ok(stale_data)
                    } else {
                        Err(e)
                    }
                }
                Err(e) => Err(e),
            }
        }

        Err(e) if e.starts_with("RATE_LIMITED") => {
            // 5. Set cooldown, return stale data if available.
            let retry_secs = parse_retry_secs(&e);
            let retry_at = Utc::now() + chrono::Duration::seconds(retry_secs as i64);
            dbg_log!("claude: rate limited, cooldown set for {}s", retry_secs);
            *state.claude_retry_after.write().await = Some(retry_at);

            let mut cache = state.claude_cache.write().await;
            cache.stale = true;
            let retry_iso = retry_at.to_rfc3339();
            if let Some(mut stale_data) = cache.data.clone() {
                stale_data.stale = true;
                stale_data.stale_reason = Some(STALE_REASON_RATE_LIMITED.to_string());
                stale_data.retry_after = Some(retry_iso);
                Ok(stale_data)
            } else {
                Err(format!("RATE_LIMITED_UNTIL:{}", retry_iso))
            }
        }

        Err(e) if e.contains("Network error") => {
            dbg_log!("claude: network error — {}", e);
            // 6. On network error, return stale data silently.
            let mut cache = state.claude_cache.write().await;
            cache.stale = true;
            if let Some(mut stale_data) = cache.data.clone() {
                stale_data.stale = true;
                stale_data.stale_reason = Some(STALE_REASON_NETWORK_ERROR.to_string());
                stale_data.retry_after = None;
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
    // 0. Honour rate-limit cooldown — return stale data without hitting the API.
    {
        let retry_after = state.codex_retry_after.read().await;
        if let Some(retry_at) = *retry_after {
            if retry_at > Utc::now() {
                dbg_log!("codex: cooldown active, retry at {}", retry_at.format("%H:%M:%S"));
                let retry_iso = retry_at.to_rfc3339();
                let cache = state.codex_cache.read().await;
                if let Some(mut data) = cache.data.clone() {
                    data.stale = true;
                    data.stale_reason = Some(STALE_REASON_RATE_LIMITED.to_string());
                    data.retry_after = Some(retry_iso);
                    return Ok(data);
                }
                return Err(format!("RATE_LIMITED_UNTIL:{}", retry_iso));
            }
        }
    }

    // 1. Return fresh cached data if available.
    {
        let cache = state.codex_cache.read().await;
        if let Some(data) = cache.get_if_fresh(CODEX_TTL) {
            dbg_log!("codex: cache hit");
            return Ok(data.clone());
        }
    }

    // 1b. Serialize concurrent fetches: acquire lock then re-check both the
    //     cooldown and the cache.
    let _fetch_guard = state.codex_fetch_lock.lock().await;
    {
        let retry_after = state.codex_retry_after.read().await;
        if let Some(retry_at) = *retry_after {
            if retry_at > Utc::now() {
                dbg_log!("codex: cooldown active (after fetch-lock)");
                let retry_iso = retry_at.to_rfc3339();
                let cache = state.codex_cache.read().await;
                if let Some(mut data) = cache.data.clone() {
                    data.stale = true;
                    data.stale_reason = Some(STALE_REASON_RATE_LIMITED.to_string());
                    data.retry_after = Some(retry_iso);
                    return Ok(data);
                }
                return Err(format!("RATE_LIMITED_UNTIL:{}", retry_iso));
            }
        }
    }
    {
        let cache = state.codex_cache.read().await;
        if let Some(data) = cache.get_if_fresh(CODEX_TTL) {
            dbg_log!("codex: cache hit (after fetch-lock)");
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
    dbg_log!("codex: cache miss, fetching...");
    match fetch_codex_usage(&state.client, &creds).await {
        Ok(data) => {
            dbg_log!("codex: fetch succeeded");
            // Clear any lingering rate-limit cooldown on success.
            *state.codex_retry_after.write().await = None;
            let mut cache = state.codex_cache.write().await;
            *cache = CachedData {
                data: Some(data.clone()),
                fetched_at: Some(Instant::now()),
                stale: false,
            };
            Ok(data)
        }

        Err(e) if e == "UNAUTHORIZED" => {
            dbg_log!("codex: 401, refreshing token...");
            // 4. Refresh token and retry once.
            match refresh_codex_token(&state.client, &creds.refresh_token).await {
                Ok(refreshed) => {
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
                    match fetch_codex_usage(&state.client, &new_creds).await {
                        Ok(data) => {
                            dbg_log!("codex: fetch succeeded after refresh");
                            *state.codex_retry_after.write().await = None;
                            let mut cache = state.codex_cache.write().await;
                            *cache = CachedData {
                                data: Some(data.clone()),
                                fetched_at: Some(Instant::now()),
                                stale: false,
                            };
                            Ok(data)
                        }
                        Err(e) if e.starts_with("RATE_LIMITED") => {
                            let retry_secs = parse_retry_secs(&e);
                            let retry_at = Utc::now() + chrono::Duration::seconds(retry_secs as i64);
                            dbg_log!("codex: rate limited after refresh, cooldown set for {}s", retry_secs);
                            *state.codex_retry_after.write().await = Some(retry_at);

                            let mut cache = state.codex_cache.write().await;
                            cache.stale = true;
                            let retry_iso = retry_at.to_rfc3339();
                            if let Some(mut stale_data) = cache.data.clone() {
                                stale_data.stale = true;
                                stale_data.stale_reason = Some(STALE_REASON_RATE_LIMITED.to_string());
                                stale_data.retry_after = Some(retry_iso);
                                Ok(stale_data)
                            } else {
                                Err(format!("RATE_LIMITED_UNTIL:{}", retry_iso))
                            }
                        }
                        Err(e) if e.contains("Network error") => {
                            dbg_log!("codex: network error after refresh — {}", e);
                            let mut cache = state.codex_cache.write().await;
                            cache.stale = true;
                            if let Some(mut stale_data) = cache.data.clone() {
                                stale_data.stale = true;
                                stale_data.stale_reason = Some(STALE_REASON_NETWORK_ERROR.to_string());
                                stale_data.retry_after = None;
                                Ok(stale_data)
                            } else {
                                Err(e)
                            }
                        }
                        Err(e) if e == "UNAUTHORIZED" => {
                            dbg_log!("codex: refreshed token still unauthorized");
                            *state.codex_retry_after.write().await = None;
                            let mut creds = state.codex_creds.write().await;
                            *creds = None;
                            drop(creds);

                            let mut cache = state.codex_cache.write().await;
                            cache.stale = true;
                            if let Some(mut stale_data) = cache.data.clone() {
                                stale_data.stale = true;
                                stale_data.stale_reason = Some(STALE_REASON_AUTH_ERROR.to_string());
                                stale_data.retry_after = None;
                                Ok(stale_data)
                            } else {
                                Err(AUTH_REQUIRED.to_string())
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) if e == "UNAUTHORIZED" => {
                    dbg_log!("codex: refresh token rejected, clearing cached credentials");
                    *state.codex_retry_after.write().await = None;
                    let mut creds = state.codex_creds.write().await;
                    *creds = None;
                    drop(creds);

                    let mut cache = state.codex_cache.write().await;
                    cache.stale = true;
                    if let Some(mut stale_data) = cache.data.clone() {
                        stale_data.stale = true;
                        stale_data.stale_reason = Some(STALE_REASON_AUTH_ERROR.to_string());
                        stale_data.retry_after = None;
                        Ok(stale_data)
                    } else {
                        Err(AUTH_REQUIRED.to_string())
                    }
                }
                Err(e) if e.contains("Network error") => {
                    dbg_log!("codex: token refresh network error — {}", e);
                    let mut cache = state.codex_cache.write().await;
                    cache.stale = true;
                    if let Some(mut stale_data) = cache.data.clone() {
                        stale_data.stale = true;
                        stale_data.stale_reason = Some(STALE_REASON_NETWORK_ERROR.to_string());
                        stale_data.retry_after = None;
                        Ok(stale_data)
                    } else {
                        Err(e)
                    }
                }
                Err(e) => Err(e),
            }
        }

        Err(e) if e.starts_with("RATE_LIMITED") => {
            // 5. Set cooldown, return stale data if available.
            let retry_secs = parse_retry_secs(&e);
            let retry_at = Utc::now() + chrono::Duration::seconds(retry_secs as i64);
            dbg_log!("codex: rate limited, cooldown set for {}s", retry_secs);
            *state.codex_retry_after.write().await = Some(retry_at);

            let mut cache = state.codex_cache.write().await;
            cache.stale = true;
            let retry_iso = retry_at.to_rfc3339();
            if let Some(mut stale_data) = cache.data.clone() {
                stale_data.stale = true;
                stale_data.stale_reason = Some(STALE_REASON_RATE_LIMITED.to_string());
                stale_data.retry_after = Some(retry_iso);
                Ok(stale_data)
            } else {
                Err(format!("RATE_LIMITED_UNTIL:{}", retry_iso))
            }
        }

        Err(e) if e.contains("Network error") => {
            dbg_log!("codex: network error — {}", e);
            // 6. On network error, return stale data silently.
            let mut cache = state.codex_cache.write().await;
            cache.stale = true;
            if let Some(mut stale_data) = cache.data.clone() {
                stale_data.stale = true;
                stale_data.stale_reason = Some(STALE_REASON_NETWORK_ERROR.to_string());
                stale_data.retry_after = None;
                Ok(stale_data)
            } else {
                Err(e)
            }
        }

        Err(e) => Err(e),
    }
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![get_claude_usage, get_codex_usage, quit_app])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
