use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use crate::{
    config::default_auto_account,
    models::{
        AccountConfig, BurnRateSnapshot, ProviderKind, QuotaSnapshot, SubscriptionSnapshot,
        UsageBucketSnapshot, UsageSnapshot,
    },
};

use super::{bucket_from_parts, endpoint, overall_status, primary_quota, subscription_from_json};

const DEFAULT_USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const DEFAULT_TOKEN_ENDPOINT: &str = "https://platform.claude.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const OAUTH_SCOPES: &str = "user:inference user:profile user:sessions:claude_code";
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialFile {
    claude_ai_oauth: OAuthCredentials,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthCredentials {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenRefreshResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeUsageLimit {
    utilization: f64,
    resets_at: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeExtraUsage {
    is_enabled: bool,
    #[serde(default)]
    monthly_limit: Option<f64>,
    #[serde(default)]
    used_credits: Option<f64>,
    #[serde(default)]
    utilization: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ClaudeUsageData {
    #[serde(default)]
    five_hour: Option<ClaudeUsageLimit>,
    #[serde(default)]
    seven_day: Option<ClaudeUsageLimit>,
    #[serde(default)]
    seven_day_sonnet: Option<ClaudeUsageLimit>,
    #[serde(default)]
    seven_day_opus: Option<ClaudeUsageLimit>,
    #[serde(default)]
    extra_usage: Option<ClaudeExtraUsage>,
}

#[derive(Debug, Clone)]
struct ClaudeOAuthUsage {
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
    usage: ClaudeUsageData,
}

pub(crate) fn detect() -> Option<AccountConfig> {
    let base = std::env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))?;

    #[cfg(target_os = "macos")]
    {
        if base.exists() || std::env::var("CLAUDE_CODE_OAUTH_TOKEN").is_ok() {
            return Some(default_auto_account(
                "claude-code-local",
                ProviderKind::ClaudeCode,
                "Claude Code",
                base,
            ));
        }
        None
    }

    #[cfg(not(target_os = "macos"))]
    {
        let credential_path = if base.join(".credentials.json").exists() {
            base.join(".credentials.json")
        } else if base.join("credentials.json").exists() {
            base.join("credentials.json")
        } else if base.exists() {
            base
        } else {
            return None;
        };

        Some(default_auto_account(
            "claude-code-local",
            ProviderKind::ClaudeCode,
            "Claude Code",
            credential_path,
        ))
    }
}

pub(crate) async fn fetch(http: &Client, account: &AccountConfig) -> Result<UsageSnapshot> {
    let usage = fetch_oauth_usage(http, account).await?;
    Ok(parse_claude_oauth_usage(account, usage))
}

async fn fetch_oauth_usage(http: &Client, account: &AccountConfig) -> Result<ClaudeOAuthUsage> {
    let credentials = read_credentials(account).await?;
    let oauth = credentials.claude_ai_oauth;
    let now = now_millis();
    let (access_token, refresh_token, expires_at) = if oauth.expires_at <= now + 60_000 {
        let refreshed = refresh_token(http, &oauth.refresh_token).await?;
        (
            refreshed.access_token,
            refreshed.refresh_token.unwrap_or(oauth.refresh_token),
            now + refreshed.expires_in.unwrap_or(3600) * 1000,
        )
    } else {
        (
            oauth.access_token.clone(),
            oauth.refresh_token.clone(),
            oauth.expires_at,
        )
    };

    let value: serde_json::Value = http
        .get(endpoint(
            account,
            "BURNRATE_CLAUDE_USAGE_URL",
            DEFAULT_USAGE_ENDPOINT,
        )?)
        .bearer_auth(&access_token)
        .header("x-api-key", CLIENT_ID)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", ANTHROPIC_BETA)
        .header("User-Agent", claude_code_user_agent())
        .send()
        .await
        .context("failed to fetch Claude Code usage")?
        .error_for_status()
        .context("Claude Code usage request failed")?
        .json()
        .await
        .context("failed to decode Claude Code usage")?;

    let usage = parse_usage_data(&value)?;

    let _ = refresh_token;
    let _ = expires_at;

    Ok(ClaudeOAuthUsage {
        subscription_type: oauth.subscription_type,
        rate_limit_tier: oauth.rate_limit_tier,
        usage,
    })
}

async fn refresh_token(http: &Client, refresh_token: &str) -> Result<TokenRefreshResponse> {
    let resp = http
        .post(endpoint_from_env(
            "BURNRATE_CLAUDE_TOKEN_URL",
            DEFAULT_TOKEN_ENDPOINT,
        )?)
        .json(&json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": CLIENT_ID,
            "scope": OAUTH_SCOPES,
        }))
        .header("anthropic-beta", ANTHROPIC_BETA)
        .header("User-Agent", claude_code_user_agent())
        .send()
        .await
        .context("failed to refresh Claude Code token")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let hint = if body.contains("invalid_grant") || body.contains("not found or invalid") {
            " Run `claude auth login` to re-authenticate."
        } else {
            ""
        };
        return Err(anyhow!(
            "Claude Code token refresh failed ({status}): {body}{hint}"
        ));
    }

    resp.json()
        .await
        .context("failed to decode Claude Code token refresh")
}

fn endpoint_from_env(env_key: &str, default: &str) -> Result<String> {
    let value = std::env::var(env_key).unwrap_or_else(|_| default.to_string());
    let account = AccountConfig {
        id: "endpoint-validation".to_string(),
        provider: ProviderKind::ClaudeCode,
        label: "Claude Code".to_string(),
        enabled: true,
        auto_detected: true,
        credential_path: None,
        endpoint_override: Some(value),
        secret_storage: crate::models::SecretStorageMode::Keyring,
        keyring_account: None,
        plaintext_secret: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    endpoint(&account, env_key, default)
}

async fn read_credentials(account: &AccountConfig) -> Result<CredentialFile> {
    #[cfg(target_os = "macos")]
    {
        if let Some(creds) = read_credentials_file(account)? {
            return Ok(creds);
        }
        tokio::task::spawn_blocking(read_macos_keychain_credentials)
            .await
            .context("Claude Code credential reader panicked")?
    }

    #[cfg(not(target_os = "macos"))]
    {
        read_credentials_file(account)?.ok_or_else(|| {
            if std::env::var("CLAUDE_CODE_OAUTH_TOKEN").is_ok() {
                anyhow!(
                    "Claude Code is using environment variable authentication. Usage tracking requires a standard login. Run `claude auth login` to enable."
                )
            } else {
                anyhow!("Claude Code credentials not found. Sign in with `claude auth login`.")
            }
        })
    }
}

#[cfg(target_os = "macos")]
fn read_macos_keychain_credentials() -> Result<CredentialFile> {
    let user = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-a",
            &user,
            "-w",
        ])
        .output()
        .context("failed to run macOS security command")?;

    if !output.status.success() {
        if std::env::var("CLAUDE_CODE_OAUTH_TOKEN").is_ok() {
            return Err(anyhow!(
                "Claude Code is using environment variable authentication. Usage tracking requires a standard login. Run `claude auth login` to enable."
            ));
        }
        return Err(anyhow!(
            "Claude Code credentials not found in Keychain. Sign in with `claude auth login`."
        ));
    }

    let json = String::from_utf8(output.stdout).context("invalid UTF-8 in Claude credentials")?;
    parse_credential_json(&json)
}

fn read_credentials_file(account: &AccountConfig) -> Result<Option<CredentialFile>> {
    let Some(path) = account.credential_path.as_ref().map(PathBuf::from) else {
        return Ok(None);
    };
    let path = if path.is_dir() {
        if path.join(".credentials.json").exists() {
            path.join(".credentials.json")
        } else if path.join("credentials.json").exists() {
            path.join("credentials.json")
        } else {
            return Ok(None);
        }
    } else if path.exists() {
        path
    } else {
        return Ok(None);
    };

    parse_credential_path(&path).map(Some)
}

fn parse_credential_path(path: &Path) -> Result<CredentialFile> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    parse_credential_json(&contents)
}

fn parse_credential_json(contents: &str) -> Result<CredentialFile> {
    serde_json::from_str(contents).context("failed to parse Claude Code credentials")
}

fn parse_usage_data(value: &serde_json::Value) -> Result<ClaudeUsageData> {
    if has_oauth_usage_keys(value) {
        return serde_json::from_value(value.clone()).context("failed to parse Claude Code usage");
    }
    if let Some(usage) = value.get("usage") {
        return serde_json::from_value(usage.clone()).context("failed to parse Claude Code usage");
    }
    serde_json::from_value(value.clone()).context("failed to parse Claude Code usage")
}

fn has_oauth_usage_keys(value: &serde_json::Value) -> bool {
    [
        "five_hour",
        "seven_day",
        "seven_day_sonnet",
        "seven_day_opus",
        "extra_usage",
    ]
    .iter()
    .any(|key| value.get(key).is_some())
}

fn parse_claude_oauth_usage(account: &AccountConfig, usage: ClaudeOAuthUsage) -> UsageSnapshot {
    let mut buckets = Vec::new();
    if let Some(limit) = usage.usage.five_hour.as_ref() {
        buckets.push(bucket_from_oauth_limit("session-5h", "5-hour", limit));
    }
    if let Some(limit) = usage.usage.seven_day.as_ref() {
        buckets.push(bucket_from_oauth_limit("weekly", "Weekly", limit));
    }
    if let Some(limit) = usage.usage.seven_day_sonnet.as_ref() {
        buckets.push(bucket_from_oauth_limit(
            "weekly-sonnet",
            "Weekly Sonnet",
            limit,
        ));
    }
    if let Some(limit) = usage.usage.seven_day_opus.as_ref() {
        buckets.push(bucket_from_oauth_limit("weekly-opus", "Weekly Opus", limit));
    }
    if let Some(extra) = usage
        .usage
        .extra_usage
        .as_ref()
        .filter(|extra| extra.is_enabled)
        && let Some(bucket) = bucket_from_extra_usage(extra)
    {
        buckets.push(bucket);
    }

    let subscription = subscription_from_oauth(
        usage.subscription_type.as_deref(),
        usage.rate_limit_tier.as_deref(),
        usage
            .usage
            .extra_usage
            .as_ref()
            .map(|extra| extra.is_enabled),
    );
    let quota = primary_quota(&buckets);
    let burn_rate_used = quota.as_ref().map(|quota| quota.used).unwrap_or(0.0);
    let status = overall_status(&buckets);

    UsageSnapshot {
        account_id: account.id.clone(),
        provider: account.provider,
        label: account.label.clone(),
        status,
        subscription,
        usage_buckets: buckets,
        quota,
        burn_rate: Some(BurnRateSnapshot {
            per_hour: burn_rate_used / 24.0,
            projected_depletion_at: None,
        }),
        message: None,
        fetched_at: Utc::now(),
    }
}

fn bucket_from_oauth_limit(
    id: impl Into<String>,
    label: impl Into<String>,
    limit: &ClaudeUsageLimit,
) -> UsageBucketSnapshot {
    let used = limit.utilization.clamp(0.0, 100.0);
    bucket_from_parts(
        id,
        label,
        None,
        QuotaSnapshot {
            used,
            limit: Some(100.0),
            remaining: Some((100.0 - used).max(0.0)),
            unit: "%".to_string(),
            reset_at: reset_from_value(&limit.resets_at),
        },
    )
}

fn bucket_from_extra_usage(extra: &ClaudeExtraUsage) -> Option<UsageBucketSnapshot> {
    let used = extra
        .used_credits
        .or(extra.utilization)
        .or_else(|| extra.monthly_limit.map(|_| 0.0))?;
    Some(bucket_from_parts(
        "extra-usage",
        "Extra usage",
        None,
        QuotaSnapshot {
            used,
            limit: extra.monthly_limit,
            remaining: extra.monthly_limit.map(|limit| (limit - used).max(0.0)),
            unit: if extra.used_credits.is_some() {
                "credits".to_string()
            } else {
                "%".to_string()
            },
            reset_at: None,
        },
    ))
}

fn subscription_from_oauth(
    subscription_type: Option<&str>,
    rate_limit_tier: Option<&str>,
    extra_usage_enabled: Option<bool>,
) -> Option<SubscriptionSnapshot> {
    subscription_from_json(
        &json!({
            "subscriptionType": subscription_type,
            "rateLimitTier": rate_limit_tier,
            "extraUsageEnabled": extra_usage_enabled,
        }),
        "claude-code-oauth",
        &["/subscriptionType", "/plan"],
    )
}

fn reset_from_value(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    if let Some(text) = value.as_str() {
        return chrono::DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|value| value.with_timezone(&Utc));
    }
    let millis = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))?;
    let millis = if millis < 10_000_000_000 {
        millis * 1000
    } else {
        millis
    };
    Utc.timestamp_millis_opt(millis).single()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn claude_code_user_agent() -> String {
    let output = Command::new("claude").arg("--version").output();
    match output {
        Ok(output) if output.status.success() => {
            let raw = String::from_utf8_lossy(&output.stdout);
            let version = raw.split_whitespace().next().unwrap_or("0.0.0");
            format!("claude-code/{version}")
        }
        _ => "claude-code/0.0.0".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    use super::*;
    use crate::models::{SecretStorageMode, SnapshotStatus, SubscriptionPlan};

    fn account() -> AccountConfig {
        AccountConfig {
            id: "claude-code-local".to_string(),
            provider: ProviderKind::ClaudeCode,
            label: "Claude Code".to_string(),
            enabled: true,
            auto_detected: true,
            credential_path: None,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Plaintext,
            keyring_account: None,
            plaintext_secret: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn parses_claude_code_credentials() {
        let creds = parse_credential_json(
            r#"{
                "claudeAiOauth": {
                    "accessToken": "sk-ant-oat01-test",
                    "refreshToken": "sk-ant-ort01-test",
                    "expiresAt": 1769163729172,
                    "subscriptionType": "max",
                    "rateLimitTier": "default_claude_max_20x"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(creds.claude_ai_oauth.access_token, "sk-ant-oat01-test");
        assert_eq!(creds.claude_ai_oauth.refresh_token, "sk-ant-ort01-test");
        assert_eq!(
            creds.claude_ai_oauth.subscription_type.as_deref(),
            Some("max")
        );
        assert_eq!(
            creds.claude_ai_oauth.rate_limit_tier.as_deref(),
            Some("default_claude_max_20x")
        );
    }

    #[test]
    fn maps_claude_oauth_usage_buckets_and_max_plan() {
        let snapshot = parse_claude_oauth_usage(
            &account(),
            ClaudeOAuthUsage {
                subscription_type: Some("max".to_string()),
                rate_limit_tier: Some("default_claude_max_20x".to_string()),
                usage: serde_json::from_value(json!({
                    "five_hour": {
                        "utilization": 96,
                        "resets_at": "2026-06-01T17:00:00Z"
                    },
                    "seven_day": {
                        "utilization": 40,
                        "resets_at": 1780000000000_i64
                    },
                    "seven_day_sonnet": {
                        "utilization": 12,
                        "resets_at": "2026-06-02T17:00:00Z"
                    },
                    "extra_usage": {
                        "is_enabled": true,
                        "monthly_limit": 100,
                        "used_credits": 25
                    }
                }))
                .unwrap(),
            },
        );

        let subscription = snapshot.subscription.unwrap();
        assert_eq!(subscription.plan, SubscriptionPlan::Max);
        assert_eq!(subscription.plan_label, "Max 20x");
        assert_eq!(subscription.extra_usage_enabled, Some(true));
        assert_eq!(snapshot.status, SnapshotStatus::Exhausted);
        assert_eq!(snapshot.usage_buckets[0].label, "5-hour");
        assert_eq!(snapshot.usage_buckets[1].label, "Weekly");
        assert_eq!(snapshot.usage_buckets[2].label, "Weekly Sonnet");
        assert_eq!(snapshot.usage_buckets[3].label, "Extra usage");
    }

    #[tokio::test]
    async fn fetches_oauth_usage_with_local_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let credentials = dir.path().join(".credentials.json");
        std::fs::write(
            &credentials,
            serde_json::to_string(&json!({
                "claudeAiOauth": {
                    "accessToken": "token",
                    "refreshToken": "refresh",
                    "expiresAt": now_millis() + 120_000,
                    "subscriptionType": "pro"
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .and(header("authorization", "Bearer token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "five_hour": {
                    "utilization": 20,
                    "resets_at": "2026-06-01T17:00:00Z"
                }
            })))
            .mount(&server)
            .await;

        let mut account = account();
        account.credential_path = Some(credentials.to_string_lossy().to_string());
        account.endpoint_override = Some(server.uri());
        let snapshot = fetch(&Client::new(), &account).await.unwrap();

        assert_eq!(snapshot.status, SnapshotStatus::Healthy);
        assert_eq!(snapshot.subscription.unwrap().plan, SubscriptionPlan::Pro);
        assert_eq!(snapshot.quota.unwrap().used, 20.0);
    }
}
