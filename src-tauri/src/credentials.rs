use serde::Deserialize;
use std::process::Command;

// ── Claude Code credentials ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ClaudeCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub subscription_type: String,
}

#[derive(Deserialize)]
struct KeychainJson {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: ClaudeAiOauth,
}

#[derive(Deserialize)]
struct ClaudeAiOauth {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: i64,
    #[serde(rename = "subscriptionType")]
    subscription_type: String,
}

pub fn read_claude_credentials() -> Result<ClaudeCredentials, String> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .map_err(|e| format!("Failed to run security command: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "security command failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    let raw = String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 in keychain output: {}", e))?;
    parse_claude_credentials(&raw)
}

// ── Codex CLI credentials ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CodexCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub account_id: String,
}

#[derive(Deserialize)]
struct CodexAuthJson {
    tokens: CodexTokens,
}

#[derive(Deserialize)]
struct CodexTokens {
    access_token: String,
    refresh_token: String,
    account_id: String,
}

pub fn read_codex_credentials() -> Result<CodexCredentials, String> {
    let home =
        std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
    let path = format!("{}/.codex/auth.json", home);

    let contents = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "Could not read ~/.codex/auth.json: {}. Is Codex CLI installed and logged in?",
            e
        )
    })?;

    parse_codex_credentials(&contents)
}

fn parse_claude_credentials(raw: &str) -> Result<ClaudeCredentials, String> {
    let parsed: KeychainJson = serde_json::from_str(raw.trim())
        .map_err(|e| format!("Failed to parse Claude Code credentials: {}", e))?;

    Ok(ClaudeCredentials {
        access_token: parsed.claude_ai_oauth.access_token,
        refresh_token: parsed.claude_ai_oauth.refresh_token,
        expires_at: parsed.claude_ai_oauth.expires_at,
        subscription_type: parsed.claude_ai_oauth.subscription_type,
    })
}

fn parse_codex_credentials(contents: &str) -> Result<CodexCredentials, String> {
    let parsed: CodexAuthJson = serde_json::from_str(contents)
        .map_err(|e| format!("Failed to parse Codex auth.json: {}", e))?;

    Ok(CodexCredentials {
        access_token: parsed.tokens.access_token,
        refresh_token: parsed.tokens.refresh_token,
        account_id: parsed.tokens.account_id,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_claude_credentials, parse_codex_credentials};

    #[test]
    fn parses_claude_credentials_from_keychain_json() {
        let credentials = parse_claude_credentials(
            r#"
            {
              "claudeAiOauth": {
                "accessToken": "access-a",
                "refreshToken": "refresh-a",
                "expiresAt": 123456,
                "subscriptionType": "pro"
              }
            }
            "#,
        )
        .unwrap();

        assert_eq!(credentials.access_token, "access-a");
        assert_eq!(credentials.refresh_token, "refresh-a");
        assert_eq!(credentials.expires_at, 123456);
        assert_eq!(credentials.subscription_type, "pro");
    }

    #[test]
    fn parses_codex_credentials_from_auth_json() {
        let credentials = parse_codex_credentials(
            r#"
            {
              "tokens": {
                "access_token": "access-b",
                "refresh_token": "refresh-b",
                "account_id": "acct_123"
              }
            }
            "#,
        )
        .unwrap();

        assert_eq!(credentials.access_token, "access-b");
        assert_eq!(credentials.refresh_token, "refresh-b");
        assert_eq!(credentials.account_id, "acct_123");
    }

    #[test]
    fn surfaces_parse_errors_with_context() {
        let error = parse_codex_credentials("{oops").unwrap_err();
        assert!(error.starts_with("Failed to parse Codex auth.json:"));
    }
}
