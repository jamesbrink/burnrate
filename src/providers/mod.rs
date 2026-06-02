mod claude;
mod codex;
mod openrouter;

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};

use crate::{
    key_store,
    models::{
        AccountConfig, ProviderKind, QuotaSnapshot, SnapshotStatus, SubscriptionPlan,
        SubscriptionSnapshot, UsageBucketSnapshot, UsageSnapshot,
    },
};

const PROVIDER_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub(crate) struct ProviderClient {
    http: Client,
}

impl ProviderClient {
    pub(crate) fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(PROVIDER_TIMEOUT)
                .build()
                .expect("provider HTTP client should build"),
        }
    }

    pub(crate) async fn refresh_account(&self, account: &AccountConfig) -> UsageSnapshot {
        let result = match account.provider {
            ProviderKind::ClaudeCode => claude::fetch(&self.http, account).await,
            ProviderKind::Codex => codex::fetch(&self.http, account).await,
            ProviderKind::OpenRouter => openrouter::fetch(&self.http, account).await,
        };

        match result {
            Ok(snapshot) => snapshot,
            Err(error) => error_snapshot(account, error),
        }
    }
}

pub(crate) fn detect_accounts() -> Vec<AccountConfig> {
    let mut accounts = Vec::new();
    if let Some(account) = claude::detect() {
        accounts.push(account);
    }
    if let Some(account) = codex::detect() {
        accounts.push(account);
    }
    accounts
}

fn error_snapshot(account: &AccountConfig, error: anyhow::Error) -> UsageSnapshot {
    UsageSnapshot {
        account_id: account.id.clone(),
        provider: account.provider,
        label: account.label.clone(),
        status: SnapshotStatus::Error,
        subscription: None,
        usage_buckets: Vec::new(),
        quota: None,
        burn_rate: None,
        message: Some(error.to_string()),
        fetched_at: Utc::now(),
    }
}

fn endpoint(account: &AccountConfig, env_key: &str, default: &str) -> Result<String> {
    let value = account
        .endpoint_override
        .clone()
        .or_else(|| std::env::var(env_key).ok())
        .unwrap_or_else(|| default.to_string());

    validate_endpoint(&value)?;
    Ok(value)
}

fn validate_endpoint(value: &str) -> Result<()> {
    let url = Url::parse(value).with_context(|| format!("invalid endpoint URL: {value}"))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if is_localhost(&url) => Ok(()),
        "http" => Err(anyhow!(
            "endpoint overrides must use HTTPS unless targeting localhost"
        )),
        scheme => Err(anyhow!("unsupported endpoint URL scheme: {scheme}")),
    }
}

fn is_localhost(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("::1")
    )
}

fn number(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = value.pointer(key)?;
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|item| item.parse::<f64>().ok()))
    })
}

fn bool_value(value: &serde_json::Value, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| {
        let value = value.pointer(key)?;
        value.as_bool().or_else(|| {
            value
                .as_str()
                .and_then(|item| match item.to_ascii_lowercase().as_str() {
                    "true" | "yes" | "1" => Some(true),
                    "false" | "no" | "0" => Some(false),
                    _ => None,
                })
        })
    })
}

fn text(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .pointer(key)
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
    })
}

fn datetime(value: &serde_json::Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    text(value, keys)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn status_from_remaining(limit: Option<f64>, remaining: Option<f64>) -> SnapshotStatus {
    match (limit, remaining) {
        (Some(limit), Some(remaining)) if limit > 0.0 && remaining / limit <= 0.05 => {
            SnapshotStatus::Exhausted
        }
        (Some(limit), Some(remaining)) if limit > 0.0 && remaining / limit <= 0.2 => {
            SnapshotStatus::Warning
        }
        _ => SnapshotStatus::Healthy,
    }
}

fn quota_from_bucket(bucket: &UsageBucketSnapshot) -> QuotaSnapshot {
    QuotaSnapshot {
        used: bucket.used,
        limit: bucket.limit,
        remaining: bucket.remaining,
        unit: bucket.unit.clone(),
        reset_at: bucket.reset_at,
    }
}

fn primary_quota(buckets: &[UsageBucketSnapshot]) -> Option<QuotaSnapshot> {
    buckets.first().map(quota_from_bucket)
}

fn overall_status(buckets: &[UsageBucketSnapshot]) -> SnapshotStatus {
    if buckets.iter().any(|bucket| {
        matches!(
            bucket.status,
            SnapshotStatus::Exhausted | SnapshotStatus::Error
        )
    }) {
        SnapshotStatus::Exhausted
    } else if buckets
        .iter()
        .any(|bucket| bucket.status == SnapshotStatus::Warning)
    {
        SnapshotStatus::Warning
    } else {
        SnapshotStatus::Healthy
    }
}

fn bucket_from_parts(
    id: impl Into<String>,
    label: impl Into<String>,
    window: Option<String>,
    quota: QuotaSnapshot,
) -> UsageBucketSnapshot {
    let status = status_from_remaining(quota.limit, quota.remaining);
    UsageBucketSnapshot {
        id: id.into(),
        label: label.into(),
        window,
        used: quota.used,
        limit: quota.limit,
        remaining: quota.remaining,
        unit: quota.unit,
        reset_at: quota.reset_at,
        status,
    }
}

fn parse_usage_buckets(value: &serde_json::Value, default_unit: &str) -> Vec<UsageBucketSnapshot> {
    let mut buckets = Vec::new();
    for pointer in [
        "/result/rate_limits",
        "/result/usage_buckets",
        "/result/buckets",
        "/data/rate_limits",
        "/data/usage_buckets",
        "/data/buckets",
        "/usage/rate_limits",
        "/usage/buckets",
        "/rate_limits",
        "/usage_buckets",
        "/buckets",
        "/limits",
    ] {
        let Some(items) = value.pointer(pointer).and_then(|value| value.as_array()) else {
            continue;
        };
        for (index, item) in items.iter().enumerate() {
            if let Some(bucket) = parse_usage_bucket(item, index, default_unit) {
                buckets.push(bucket);
            }
        }
        if !buckets.is_empty() {
            return buckets;
        }
    }

    Vec::new()
}

fn parse_usage_bucket(
    value: &serde_json::Value,
    index: usize,
    default_unit: &str,
) -> Option<UsageBucketSnapshot> {
    let limit = number(
        value,
        &[
            "/limit",
            "/quota/limit",
            "/max",
            "/maximum",
            "/cap",
            "/total",
            "/total_allowed",
        ],
    );
    let remaining = number(
        value,
        &[
            "/remaining",
            "/quota/remaining",
            "/available",
            "/remaining_quota",
            "/remaining_requests",
            "/remaining_tokens",
        ],
    );
    let used = number(
        value,
        &[
            "/used",
            "/quota/used",
            "/consumed",
            "/usage",
            "/used_quota",
            "/used_requests",
            "/used_tokens",
        ],
    )
    .or_else(|| {
        limit
            .zip(remaining)
            .map(|(limit, remaining)| limit - remaining)
    })
    .unwrap_or(0.0);

    if limit.is_none() && remaining.is_none() && used == 0.0 {
        return None;
    }

    let explicit_id = text(
        value,
        &[
            "/id",
            "/name",
            "/key",
            "/type",
            "/bucket",
            "/window",
            "/period",
            "/limit_type",
        ],
    );
    let raw_id = explicit_id
        .clone()
        .unwrap_or_else(|| format!("{default_unit}-{index}"));
    let label = explicit_id
        .as_deref()
        .map(bucket_label)
        .unwrap_or_else(|| title_case(default_unit));
    let window = bucket_window(&raw_id);
    let reset_at = datetime(
        value,
        &[
            "/reset_at",
            "/resetAt",
            "/resets_at",
            "/resetsAt",
            "/reset_time",
            "/resetTime",
            "/expires_at",
            "/expiresAt",
        ],
    );
    let unit = text(value, &["/unit", "/quota/unit"]).unwrap_or_else(|| default_unit.to_string());

    Some(bucket_from_parts(
        slug(&raw_id),
        label,
        window,
        QuotaSnapshot {
            used: used.max(0.0),
            limit,
            remaining,
            unit,
            reset_at,
        },
    ))
}

fn bucket_label(value: &str) -> String {
    let normalized = value.replace(['_', '-'], " ").to_ascii_lowercase();
    if normalized.contains("5") && normalized.contains("hour") {
        "5-hour".to_string()
    } else if normalized.contains("week") {
        "Weekly".to_string()
    } else if normalized.contains("day") || normalized.contains("24") {
        "Daily".to_string()
    } else if normalized.contains("month") {
        "Monthly".to_string()
    } else if normalized.trim().is_empty() {
        "Quota".to_string()
    } else {
        title_case(&normalized)
    }
}

fn bucket_window(value: &str) -> Option<String> {
    let normalized = value.replace(['_', '-'], " ").to_ascii_lowercase();
    if normalized.contains("5") && normalized.contains("hour") {
        Some("5-hour".to_string())
    } else if normalized.contains("week") {
        Some("weekly".to_string())
    } else if normalized.contains("day") || normalized.contains("24") {
        Some("daily".to_string())
    } else if normalized.contains("month") {
        Some("monthly".to_string())
    } else {
        None
    }
}

fn title_case(value: &str) -> String {
    value
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "quota".to_string()
    } else {
        out
    }
}

fn plan_from_text(value: Option<&str>) -> SubscriptionPlan {
    let Some(value) = value else {
        return SubscriptionPlan::Unknown;
    };
    let value = value.to_ascii_lowercase();
    if value.contains("claude_max") || value.contains("max") {
        SubscriptionPlan::Max
    } else if value.contains("team") {
        SubscriptionPlan::Team
    } else if value.contains("enterprise") {
        SubscriptionPlan::Enterprise
    } else if value.contains("pro")
        || value.contains("plus")
        || value.contains("stripe_subscription")
    {
        SubscriptionPlan::Pro
    } else if value.contains("free") {
        SubscriptionPlan::Free
    } else {
        SubscriptionPlan::Unknown
    }
}

fn plan_label(plan: SubscriptionPlan, raw: Option<&str>) -> String {
    match plan {
        SubscriptionPlan::Free => "Free".to_string(),
        SubscriptionPlan::Pro => "Pro".to_string(),
        SubscriptionPlan::Max => raw
            .and_then(|value| {
                value.split('_').rev().find_map(|part| {
                    part.strip_suffix('x')
                        .and_then(|count| count.parse::<u32>().ok())
                        .map(|count| format!("Max {count}x"))
                })
            })
            .unwrap_or_else(|| "Max".to_string()),
        SubscriptionPlan::Team => "Team".to_string(),
        SubscriptionPlan::Enterprise => "Enterprise".to_string(),
        SubscriptionPlan::Unknown => "Unknown plan".to_string(),
    }
}

fn subscription_from_json(
    value: &serde_json::Value,
    source: &str,
    plan_keys: &[&str],
) -> Option<SubscriptionSnapshot> {
    let raw_plan = text(value, plan_keys);
    let rate_limit_tier = text(
        value,
        &[
            "/rate_limit_tier",
            "/rateLimitTier",
            "/organizationRateLimitTier",
            "/oauthAccount/organizationRateLimitTier",
            "/account/rate_limit_tier",
            "/account/rateLimitTier",
        ],
    );
    let plan = plan_from_text(raw_plan.as_deref().or(rate_limit_tier.as_deref()));
    if plan == SubscriptionPlan::Unknown && raw_plan.is_none() && rate_limit_tier.is_none() {
        return None;
    }
    let extra_usage_enabled = bool_value(
        value,
        &[
            "/extra_usage_enabled",
            "/extraUsageEnabled",
            "/hasExtraUsageEnabled",
            "/oauthAccount/hasExtraUsageEnabled",
            "/account/hasExtraUsageEnabled",
        ],
    );

    Some(SubscriptionSnapshot {
        plan,
        plan_label: plan_label(plan, rate_limit_tier.as_deref().or(raw_plan.as_deref())),
        rate_limit_tier,
        extra_usage_enabled,
        source: source.to_string(),
    })
}

fn token_from_config(account: &AccountConfig) -> Result<Option<String>> {
    if let Some(secret) = key_store::get_secret(account)? {
        return Ok(Some(secret));
    }

    let Some(path) = &account.credential_path else {
        return Ok(None);
    };
    let path = std::path::Path::new(path);
    if path.is_file() {
        return read_token_file(path, account.provider);
    }

    for candidate in ["auth.json", ".credentials.json", "credentials.json"] {
        let candidate = path.join(candidate);
        if candidate.exists() {
            return read_token_file(&candidate, account.provider);
        }
    }

    Ok(None)
}

fn read_token_file(path: &std::path::Path, provider: ProviderKind) -> Result<Option<String>> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(find_token(&json, token_pointers(provider)))
}

fn token_pointers(provider: ProviderKind) -> &'static [&'static str] {
    match provider {
        ProviderKind::ClaudeCode => &[
            "/claudeAiOauth/accessToken",
            "/oauth/accessToken",
            "/oauth/access_token",
            "/accessToken",
            "/access_token",
            "/oauthAccessToken",
            "/oauth_access_token",
        ],
        ProviderKind::Codex => &[
            "/tokens/access_token",
            "/tokens/accessToken",
            "/auth/access_token",
            "/auth/accessToken",
            "/access_token",
            "/accessToken",
            "/api_key",
            "/apiKey",
        ],
        ProviderKind::OpenRouter => &["/api_key", "/apiKey", "/key"],
    }
}

fn find_token(value: &serde_json::Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(|value| value.as_str())
            .filter(|token| !token.is_empty())
            .map(ToString::to_string)
    })
}

fn require_token(account: &AccountConfig) -> Result<String> {
    token_from_config(account)?.ok_or_else(|| anyhow!("no credential found for {}", account.label))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::models::SecretStorageMode;

    fn account() -> AccountConfig {
        AccountConfig {
            id: "openrouter-main".to_string(),
            provider: ProviderKind::OpenRouter,
            label: "OpenRouter".to_string(),
            enabled: true,
            auto_detected: false,
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
    fn finds_provider_specific_tokens() {
        let token = find_token(
            &json!({
                "tokens": {
                    "access_token": "tok_123"
                }
            }),
            token_pointers(ProviderKind::Codex),
        );

        assert_eq!(token, Some("tok_123".to_string()));
    }

    #[test]
    fn ignores_unrecognized_nested_token_shapes() {
        let token = find_token(
            &json!({
                "unrelated": {
                    "token": "tok_wrong"
                }
            }),
            token_pointers(ProviderKind::Codex),
        );

        assert_eq!(token, None);
    }

    #[test]
    fn endpoint_allows_https_and_localhost_http() {
        let mut account = account();
        account.endpoint_override = Some("https://example.test".to_string());

        assert_eq!(
            endpoint(&account, "BURNRATE_TEST_ENDPOINT", "https://default.test").unwrap(),
            "https://example.test"
        );

        account.endpoint_override = Some("http://127.0.0.1:9000".to_string());
        assert!(endpoint(&account, "BURNRATE_TEST_ENDPOINT", "https://default.test").is_ok());
    }

    #[test]
    fn endpoint_rejects_untrusted_http_overrides() {
        let mut account = account();
        account.endpoint_override = Some("http://example.test".to_string());

        let error = endpoint(&account, "BURNRATE_TEST_ENDPOINT", "https://default.test")
            .unwrap_err()
            .to_string();

        assert!(error.contains("HTTPS"));
    }

    #[test]
    fn provider_timeout_is_bounded() {
        assert_eq!(PROVIDER_TIMEOUT, Duration::from_secs(15));
    }

    #[test]
    fn reads_token_from_file_and_directory_candidates() {
        let dir = tempdir().unwrap();
        let token_path = dir.path().join("auth.json");
        std::fs::write(
            &token_path,
            r#"{"tokens":{"access_token":"tok_file"},"unrelated":{"token":"wrong"}}"#,
        )
        .unwrap();

        let mut account = account();
        account.provider = ProviderKind::Codex;
        account.credential_path = Some(dir.path().display().to_string());

        assert_eq!(
            token_from_config(&account).unwrap(),
            Some("tok_file".to_string())
        );
    }

    #[test]
    fn token_reader_rejects_unrecognized_file_shapes() {
        let dir = tempdir().unwrap();
        let token_path = dir.path().join("auth.json");
        std::fs::write(&token_path, r#"{"unrelated":{"token":"wrong"}}"#).unwrap();

        let mut account = account();
        account.provider = ProviderKind::Codex;
        account.credential_path = Some(dir.path().display().to_string());

        assert_eq!(token_from_config(&account).unwrap(), None);
    }

    #[test]
    fn endpoint_prefers_account_override() {
        let mut account = account();
        account.endpoint_override = Some("https://example.test".to_string());

        assert_eq!(
            endpoint(&account, "BURNRATE_TEST_ENDPOINT", "https://default.test").unwrap(),
            "https://example.test"
        );
    }

    #[test]
    fn json_helpers_read_numbers_and_text() {
        let value = json!({
            "quota": {
                "remaining": "42.5",
                "reset_at": "2026-06-01T12:00:00Z"
            }
        });

        assert_eq!(number(&value, &["/quota/remaining"]), Some(42.5));
        assert_eq!(
            text(&value, &["/quota/reset_at"]),
            Some("2026-06-01T12:00:00Z".to_string())
        );
        assert_eq!(number(&value, &["/missing"]), None);
    }

    #[test]
    fn requires_token_for_accounts_without_credentials() {
        let error = require_token(&account()).unwrap_err();

        assert!(error.to_string().contains("no credential found"));
    }

    #[tokio::test]
    async fn refresh_account_maps_provider_errors_to_snapshots() {
        let snapshot = ProviderClient::new().refresh_account(&account()).await;

        assert_eq!(snapshot.status, SnapshotStatus::Error);
        assert!(snapshot.message.unwrap().contains("no credential found"));
    }
}
