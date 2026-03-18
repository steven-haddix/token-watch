use crate::credentials::{ClaudeCredentials, CodexCredentials};
use reqwest::Client;
use serde::Deserialize;

const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CLAUDE_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";

macro_rules! dbg_log {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) {
            eprintln!("[token-watch {}] {}", chrono::Utc::now().format("%H:%M:%S"), format!($($arg)*));
        }
    };
}

// ── Public response types ────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WindowUsage {
    pub utilization: f64,
    pub remaining: f64,
    pub resets_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtraUsage {
    pub is_enabled: bool,
    pub used_credits: Option<f64>,
    pub utilization: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClaudeUsageResponse {
    pub five_hour: WindowUsage,
    pub seven_day: WindowUsage,
    pub seven_day_opus: Option<WindowUsage>,
    pub seven_day_sonnet: Option<WindowUsage>,
    pub subscription_type: String,
    pub extra_usage: ExtraUsage,
    pub stale: bool,
    pub stale_reason: Option<String>,
    pub retry_after: Option<String>, // ISO timestamp when rate limit lifts
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodexWindowUsage {
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub reset_at_unix: i64,
    pub resets_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodexUsageResponse {
    pub plan_type: String,
    pub primary_window: CodexWindowUsage,
    pub secondary_window: CodexWindowUsage,
    pub has_credits: bool,
    pub limit_reached: bool,
    pub stale: bool,
    pub stale_reason: Option<String>,
    pub retry_after: Option<String>, // ISO timestamp when rate limit lifts
}

// ── Internal API response shapes ─────────────────────────────────────────────

#[derive(Deserialize)]
struct ApiWindowUsage {
    utilization: f64,
    resets_at: String,
}

#[derive(Deserialize)]
struct ApiExtraUsage {
    is_enabled: bool,
    used_credits: Option<f64>,
    utilization: Option<f64>,
}

#[derive(Deserialize)]
struct ClaudeApiResponse {
    five_hour: ApiWindowUsage,
    seven_day: ApiWindowUsage,
    seven_day_opus: Option<ApiWindowUsage>,
    seven_day_sonnet: Option<ApiWindowUsage>,
    extra_usage: ApiExtraUsage,
}

#[derive(Deserialize)]
struct CodexRateWindow {
    used_percent: f64,
    reset_at: i64,
}

#[derive(Deserialize)]
struct CodexRateLimit {
    limit_reached: bool,
    primary_window: CodexRateWindow,
    secondary_window: CodexRateWindow,
}

#[derive(Deserialize)]
struct CodexCreditsBlock {
    has_credits: bool,
}

#[derive(Deserialize)]
struct CodexApiResponse {
    plan_type: String,
    rate_limit: CodexRateLimit,
    credits: CodexCreditsBlock,
}

// ── Refresh token response shapes ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ClaudeTokenRefreshResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CodexTokenRefreshResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn unix_to_iso(unix: i64) -> String {
    use chrono::{TimeZone, Utc};
    let dt = match Utc.timestamp_opt(unix, 0) {
        chrono::LocalResult::Single(dt) => dt,
        _ => Utc::now(),
    };
    dt.to_rfc3339()
}

fn to_window_usage(w: ApiWindowUsage) -> WindowUsage {
    let remaining = (100.0 - w.utilization).max(0.0);
    WindowUsage {
        utilization: w.utilization,
        remaining,
        resets_at: w.resets_at,
    }
}

// ── Claude functions ─────────────────────────────────────────────────────────

pub async fn fetch_claude_usage(
    client: &Client,
    creds: &ClaudeCredentials,
) -> Result<ClaudeUsageResponse, String> {
    fetch_claude_usage_from_url(client, CLAUDE_USAGE_URL, creds).await
}

async fn fetch_claude_usage_from_url(
    client: &Client,
    url: &str,
    creds: &ClaudeCredentials,
) -> Result<ClaudeUsageResponse, String> {
    dbg_log!("claude: GET /api/oauth/usage");
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await
        .map_err(|e| format!("Network error fetching Claude usage: {}", e))?;

    let status = resp.status();
    dbg_log!("claude: HTTP {}", status);
    if status == 429 {
        let retry_secs: u64 = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(60)
            .max(30); // minimum 30s even if the header says less
        dbg_log!("claude: 429 rate limited, retry after {}s", retry_secs);
        return Err(format!("RATE_LIMITED:{}", retry_secs));
    }
    if status == 401 {
        return Err("UNAUTHORIZED".to_string());
    }
    if !status.is_success() {
        return Err(format!("Claude usage API returned status {}", status));
    }

    let raw: ClaudeApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Claude usage response: {}", e))?;

    dbg_log!(
        "claude: ok — 5h util={:.1}% 7d util={:.1}%",
        raw.five_hour.utilization,
        raw.seven_day.utilization
    );
    Ok(ClaudeUsageResponse {
        five_hour: to_window_usage(raw.five_hour),
        seven_day: to_window_usage(raw.seven_day),
        seven_day_opus: raw.seven_day_opus.map(to_window_usage),
        seven_day_sonnet: raw.seven_day_sonnet.map(to_window_usage),
        subscription_type: creds.subscription_type.clone(),
        extra_usage: ExtraUsage {
            is_enabled: raw.extra_usage.is_enabled,
            used_credits: raw.extra_usage.used_credits,
            utilization: raw.extra_usage.utilization,
        },
        stale: false,
        stale_reason: None,
        retry_after: None,
    })
}

pub async fn refresh_claude_token(
    client: &Client,
    refresh_token: &str,
) -> Result<ClaudeTokenRefreshResponse, String> {
    refresh_claude_token_from_url(client, CLAUDE_TOKEN_URL, refresh_token).await
}

async fn refresh_claude_token_from_url(
    client: &Client,
    url: &str,
    refresh_token: &str,
) -> Result<ClaudeTokenRefreshResponse, String> {
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", "9d1c250a-e61b-44d9-88ed-5944d1962f5e"),
    ];

    let resp = client
        .post(url)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Network error refreshing Claude token: {}", e))?;

    let status = resp.status();
    if matches!(
        status,
        reqwest::StatusCode::BAD_REQUEST
            | reqwest::StatusCode::UNAUTHORIZED
            | reqwest::StatusCode::FORBIDDEN
    ) {
        return Err("UNAUTHORIZED".to_string());
    }
    if !status.is_success() {
        return Err(format!("Claude token refresh failed: {}", status));
    }

    resp.json::<ClaudeTokenRefreshResponse>()
        .await
        .map_err(|e| format!("Failed to parse Claude token refresh response: {}", e))
}

// ── Codex functions ──────────────────────────────────────────────────────────

pub async fn fetch_codex_usage(
    client: &Client,
    creds: &CodexCredentials,
) -> Result<CodexUsageResponse, String> {
    fetch_codex_usage_from_url(client, CODEX_USAGE_URL, creds).await
}

async fn fetch_codex_usage_from_url(
    client: &Client,
    url: &str,
    creds: &CodexCredentials,
) -> Result<CodexUsageResponse, String> {
    dbg_log!("codex: GET /backend-api/wham/usage");
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("ChatGPT-Account-Id", &creds.account_id)
        .send()
        .await
        .map_err(|e| format!("Network error fetching Codex usage: {}", e))?;

    let status = resp.status();
    dbg_log!("codex: HTTP {}", status);
    if status == 429 {
        let retry_secs: u64 = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);
        dbg_log!("codex: 429 rate limited, retry after {}s", retry_secs);
        return Err(format!("RATE_LIMITED:{}", retry_secs));
    }
    if status == 401 {
        return Err("UNAUTHORIZED".to_string());
    }
    if !status.is_success() {
        return Err(format!("Codex usage API returned status {}", status));
    }

    let raw: CodexApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Codex usage response: {}", e))?;

    dbg_log!(
        "codex: ok — primary={:.1}% used 7d={:.1}% used",
        raw.rate_limit.primary_window.used_percent,
        raw.rate_limit.secondary_window.used_percent
    );
    Ok(CodexUsageResponse {
        plan_type: raw.plan_type,
        primary_window: CodexWindowUsage {
            used_percent: raw.rate_limit.primary_window.used_percent,
            remaining_percent: (100.0 - raw.rate_limit.primary_window.used_percent).max(0.0),
            reset_at_unix: raw.rate_limit.primary_window.reset_at,
            resets_at: unix_to_iso(raw.rate_limit.primary_window.reset_at),
        },
        secondary_window: CodexWindowUsage {
            used_percent: raw.rate_limit.secondary_window.used_percent,
            remaining_percent: (100.0 - raw.rate_limit.secondary_window.used_percent).max(0.0),
            reset_at_unix: raw.rate_limit.secondary_window.reset_at,
            resets_at: unix_to_iso(raw.rate_limit.secondary_window.reset_at),
        },
        has_credits: raw.credits.has_credits,
        limit_reached: raw.rate_limit.limit_reached,
        stale: false,
        stale_reason: None,
        retry_after: None,
    })
}

pub async fn refresh_codex_token(
    client: &Client,
    refresh_token: &str,
) -> Result<CodexTokenRefreshResponse, String> {
    refresh_codex_token_from_url(client, CODEX_TOKEN_URL, refresh_token).await
}

async fn refresh_codex_token_from_url(
    client: &Client,
    url: &str,
    refresh_token: &str,
) -> Result<CodexTokenRefreshResponse, String> {
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": "app_EMoamEEZ73f0CkXaXp7hrann"
    });

    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error refreshing Codex token: {}", e))?;

    let status = resp.status();
    if matches!(
        status,
        reqwest::StatusCode::BAD_REQUEST
            | reqwest::StatusCode::UNAUTHORIZED
            | reqwest::StatusCode::FORBIDDEN
    ) {
        return Err("UNAUTHORIZED".to_string());
    }
    if !status.is_success() {
        return Err(format!("Codex token refresh failed: {}", status));
    }

    resp.json::<CodexTokenRefreshResponse>()
        .await
        .map_err(|e| format!("Failed to parse Codex token refresh response: {}", e))
}

#[cfg(test)]
mod tests {
    use super::{
        fetch_claude_usage_from_url, fetch_codex_usage_from_url, refresh_claude_token_from_url,
        refresh_codex_token_from_url,
    };
    use crate::credentials::{ClaudeCredentials, CodexCredentials};
    use reqwest::Client;
    use wiremock::matchers::{body_partial_json, body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_client() -> Client {
        Client::builder().build().unwrap()
    }

    fn claude_creds() -> ClaudeCredentials {
        ClaudeCredentials {
            access_token: "claude-access".to_string(),
            refresh_token: "claude-refresh".to_string(),
            expires_at: 0,
            subscription_type: "pro".to_string(),
        }
    }

    fn codex_creds() -> CodexCredentials {
        CodexCredentials {
            access_token: "codex-access".to_string(),
            refresh_token: "codex-refresh".to_string(),
            account_id: "acct_123".to_string(),
        }
    }

    #[tokio::test]
    async fn fetch_claude_usage_sends_required_headers_and_maps_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/claude/usage"))
            .and(header("authorization", "Bearer claude-access"))
            .and(header("anthropic-beta", "oauth-2025-04-20"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "five_hour": { "utilization": 42.5, "resets_at": "2026-03-18T16:00:00Z" },
                "seven_day": { "utilization": 80.0, "resets_at": "2026-03-24T16:00:00Z" },
                "seven_day_opus": null,
                "seven_day_sonnet": {
                    "utilization": 10.0,
                    "resets_at": "2026-03-24T16:00:00Z"
                },
                "extra_usage": {
                    "is_enabled": true,
                    "used_credits": 12.0,
                    "utilization": 5.0
                }
            })))
            .mount(&server)
            .await;

        let response = fetch_claude_usage_from_url(
            &test_client(),
            &format!("{}/claude/usage", server.uri()),
            &claude_creds(),
        )
        .await
        .unwrap();

        assert_eq!(response.subscription_type, "pro");
        assert_eq!(response.five_hour.remaining, 57.5);
        assert_eq!(response.seven_day.remaining, 20.0);
        assert_eq!(response.seven_day_sonnet.unwrap().remaining, 90.0);
        assert!(response.extra_usage.is_enabled);
        assert!(!response.stale);
    }

    #[tokio::test]
    async fn fetch_claude_usage_enforces_minimum_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/claude/usage"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "5"))
            .mount(&server)
            .await;

        let error = fetch_claude_usage_from_url(
            &test_client(),
            &format!("{}/claude/usage", server.uri()),
            &claude_creds(),
        )
        .await
        .unwrap_err();

        assert_eq!(error, "RATE_LIMITED:30");
    }

    #[tokio::test]
    async fn fetch_codex_usage_maps_headers_and_windows() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/codex/usage"))
            .and(header("authorization", "Bearer codex-access"))
            .and(header("chatgpt-account-id", "acct_123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "plan_type": "plus",
                "rate_limit": {
                    "limit_reached": false,
                    "primary_window": { "used_percent": 25.0, "reset_at": 1773859200 },
                    "secondary_window": { "used_percent": 90.0, "reset_at": 1774464000 }
                },
                "credits": { "has_credits": true }
            })))
            .mount(&server)
            .await;

        let response = fetch_codex_usage_from_url(
            &test_client(),
            &format!("{}/codex/usage", server.uri()),
            &codex_creds(),
        )
        .await
        .unwrap();

        assert_eq!(response.plan_type, "plus");
        assert_eq!(response.primary_window.remaining_percent, 75.0);
        assert_eq!(response.secondary_window.remaining_percent, 10.0);
        assert!(response.has_credits);
        assert!(!response.limit_reached);
    }

    #[tokio::test]
    async fn fetch_usage_returns_unauthorized_and_parse_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/claude/usage"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let unauthorized = fetch_claude_usage_from_url(
            &test_client(),
            &format!("{}/claude/usage", server.uri()),
            &claude_creds(),
        )
        .await
        .unwrap_err();
        assert_eq!(unauthorized, "UNAUTHORIZED");

        server.reset().await;

        Mock::given(method("GET"))
            .and(path("/codex/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{not-json"))
            .mount(&server)
            .await;

        let parse_error = fetch_codex_usage_from_url(
            &test_client(),
            &format!("{}/codex/usage", server.uri()),
            &codex_creds(),
        )
        .await
        .unwrap_err();
        assert!(parse_error.starts_with("Failed to parse Codex usage response:"));
    }

    #[tokio::test]
    async fn refresh_endpoints_map_success_and_auth_failures() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/claude/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=claude-refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "new-claude-access",
                "refresh_token": "new-claude-refresh"
            })))
            .mount(&server)
            .await;

        let refreshed = refresh_claude_token_from_url(
            &test_client(),
            &format!("{}/claude/token", server.uri()),
            "claude-refresh",
        )
        .await
        .unwrap();
        assert_eq!(refreshed.access_token, "new-claude-access");
        assert_eq!(
            refreshed.refresh_token.as_deref(),
            Some("new-claude-refresh")
        );

        server.reset().await;

        Mock::given(method("POST"))
            .and(path("/codex/token"))
            .and(body_partial_json(serde_json::json!({
                "grant_type": "refresh_token",
                "refresh_token": "codex-refresh"
            })))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let error = refresh_codex_token_from_url(
            &test_client(),
            &format!("{}/codex/token", server.uri()),
            "codex-refresh",
        )
        .await
        .unwrap_err();
        assert_eq!(error, "UNAUTHORIZED");
    }
}
