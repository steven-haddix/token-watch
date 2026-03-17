use reqwest::Client;
use serde::Deserialize;
use crate::credentials::{ClaudeCredentials, CodexCredentials};

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

#[derive(Deserialize)]
pub struct ClaudeTokenRefreshResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Deserialize)]
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
    let resp = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await
        .map_err(|e| format!("Network error fetching Claude usage: {}", e))?;

    let status = resp.status();
    if status == 429 {
        return Err("RATE_LIMITED".to_string());
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
    })
}

pub async fn refresh_claude_token(
    client: &Client,
    refresh_token: &str,
) -> Result<ClaudeTokenRefreshResponse, String> {
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", "9d1c250a-e61b-44d9-88ed-5944d1962f5e"),
    ];

    let resp = client
        .post("https://console.anthropic.com/v1/oauth/token")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Network error refreshing Claude token: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Claude token refresh failed: {}", resp.status()));
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
    let resp = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("ChatGPT-Account-Id", &creds.account_id)
        .send()
        .await
        .map_err(|e| format!("Network error fetching Codex usage: {}", e))?;

    let status = resp.status();
    if status == 429 {
        return Err("RATE_LIMITED".to_string());
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
    })
}

pub async fn refresh_codex_token(
    client: &Client,
    refresh_token: &str,
) -> Result<CodexTokenRefreshResponse, String> {
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": "app_EMoamEEZ73f0CkXaXp7hrann"
    });

    let resp = client
        .post("https://auth.openai.com/oauth/token")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error refreshing Codex token: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Codex token refresh failed: {}", resp.status()));
    }

    resp.json::<CodexTokenRefreshResponse>()
        .await
        .map_err(|e| format!("Failed to parse Codex token refresh response: {}", e))
}
