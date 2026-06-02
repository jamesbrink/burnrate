use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use reqwest::Client;

use crate::{
    config::default_auto_account,
    models::{
        AccountConfig, BurnRateSnapshot, ProviderKind, QuotaSnapshot, SubscriptionSnapshot,
        UsageSnapshot,
    },
};

use super::{
    bucket_from_parts, endpoint, number, overall_status, parse_usage_buckets, primary_quota,
    require_token, subscription_from_json, text,
};

const DEFAULT_ENDPOINT: &str = "https://api.anthropic.com/v1/organizations/usage_report/messages";

pub(crate) fn detect() -> Option<AccountConfig> {
    let base = std::env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))?;

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

pub(crate) async fn fetch(http: &Client, account: &AccountConfig) -> Result<UsageSnapshot> {
    let token = require_token(account)?;
    let mut url = endpoint(account, "BURNRATE_CLAUDE_USAGE_URL", DEFAULT_ENDPOINT)?;
    if !url.contains('?') {
        let end = Utc::now();
        let start = end - Duration::days(1);
        url = format!(
            "{url}?starting_at={}&ending_at={}",
            start.format("%Y-%m-%d"),
            end.format("%Y-%m-%d")
        );
    }

    let value: serde_json::Value = http
        .get(url)
        .bearer_auth(token)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .context("failed to fetch Anthropic usage")?
        .error_for_status()
        .context("Anthropic usage request failed")?
        .json()
        .await
        .context("failed to decode Anthropic usage")?;

    Ok(parse_claude_usage_with_subscription(
        account,
        &value,
        local_subscription_metadata().ok().flatten(),
    ))
}

#[cfg(test)]
pub(crate) fn parse_claude_usage(
    account: &AccountConfig,
    value: &serde_json::Value,
) -> UsageSnapshot {
    parse_claude_usage_with_subscription(account, value, None)
}

fn parse_claude_usage_with_subscription(
    account: &AccountConfig,
    value: &serde_json::Value,
    local_subscription: Option<SubscriptionSnapshot>,
) -> UsageSnapshot {
    let used = number(
        value,
        &[
            "/usage/input_tokens",
            "/usage/total_tokens",
            "/total_tokens",
            "/tokens/used",
        ],
    )
    .unwrap_or_else(|| {
        aggregate_number(value, "input_tokens") + aggregate_number(value, "output_tokens")
    });
    let limit = number(value, &["/limit", "/quota/limit", "/tokens/limit"]);
    let remaining = number(
        value,
        &["/remaining", "/quota/remaining", "/tokens/remaining"],
    )
    .or_else(|| limit.map(|limit| (limit - used).max(0.0)));
    let reset_at = text(value, &["/reset_at", "/quota/reset_at"])
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.with_timezone(&Utc));
    let mut buckets = parse_usage_buckets(value, "tokens");
    if buckets.is_empty() {
        buckets.push(bucket_from_parts(
            "tokens",
            "Tokens",
            None,
            QuotaSnapshot {
                used,
                limit,
                remaining,
                unit: "tokens".to_string(),
                reset_at,
            },
        ));
    }
    let subscription = local_subscription.or_else(|| {
        subscription_from_json(
            value,
            "anthropic-usage",
            &[
                "/plan",
                "/plan_type",
                "/subscription/plan",
                "/account/plan",
                "/organization/type",
                "/organizationType",
                "/oauthAccount/organizationType",
            ],
        )
    });
    let quota = primary_quota(&buckets);
    let burn_rate_used = quota.as_ref().map(|quota| quota.used).unwrap_or(used);
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

pub(crate) fn local_subscription_metadata() -> Result<Option<SubscriptionSnapshot>> {
    let path = claude_metadata_path()?;
    if !path.exists() {
        return Ok(None);
    }
    parse_claude_subscription_metadata(&path)
}

fn claude_metadata_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("BURNRATE_CLAUDE_METADATA_FILE") {
        return Ok(PathBuf::from(path));
    }
    Ok(dirs::home_dir()
        .context("could not find home directory")?
        .join(".claude.json"))
}

fn parse_claude_subscription_metadata(path: &Path) -> Result<Option<SubscriptionSnapshot>> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(subscription_from_json(
        &value,
        "claude-local-metadata",
        &[
            "/oauthAccount/organizationType",
            "/oauthAccount/billingType",
            "/organizationType",
            "/billingType",
        ],
    ))
}

fn aggregate_number(value: &serde_json::Value, key: &str) -> f64 {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(item_key, item)| {
                let own = if item_key == key {
                    item.as_f64().unwrap_or(0.0)
                } else {
                    0.0
                };
                own + aggregate_number(item, key)
            })
            .sum(),
        serde_json::Value::Array(values) => {
            values.iter().map(|item| aggregate_number(item, key)).sum()
        }
        _ => 0.0,
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
            plaintext_secret: Some("token".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn aggregates_anthropic_usage_rows() {
        let snapshot = parse_claude_usage(
            &account(),
            &json!({
                "data": [
                    { "usage": { "input_tokens": 10, "output_tokens": 5 } },
                    { "usage": { "input_tokens": 7, "output_tokens": 3 } }
                ],
                "quota": { "limit": 100, "remaining": 75 }
            }),
        );

        assert_eq!(snapshot.quota.unwrap().used, 25.0);
        assert_eq!(snapshot.usage_buckets[0].label, "Tokens");
    }

    #[test]
    fn maps_claude_max_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".claude.json");
        std::fs::write(
            &path,
            serde_json::to_string(&json!({
                "oauthAccount": {
                    "billingType": "stripe_subscription",
                    "organizationType": "claude_max",
                    "organizationRateLimitTier": "default_claude_max_20x",
                    "hasExtraUsageEnabled": true
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let subscription = parse_claude_subscription_metadata(&path).unwrap().unwrap();

        assert_eq!(subscription.plan, SubscriptionPlan::Max);
        assert_eq!(subscription.plan_label, "Max 20x");
        assert_eq!(subscription.extra_usage_enabled, Some(true));
    }

    #[test]
    fn maps_claude_usage_buckets() {
        let snapshot = parse_claude_usage(
            &account(),
            &json!({
                "usage_buckets": [
                    {
                        "name": "5_hour",
                        "used": 96,
                        "limit": 100,
                        "remaining": 4,
                        "reset_at": "2026-06-01T17:00:00Z"
                    },
                    {
                        "name": "weekly",
                        "used": 400,
                        "limit": 1000,
                        "remaining": 600
                    }
                ]
            }),
        );

        assert_eq!(snapshot.status, SnapshotStatus::Exhausted);
        assert_eq!(snapshot.usage_buckets.len(), 2);
        assert_eq!(snapshot.usage_buckets[0].label, "5-hour");
        assert_eq!(snapshot.usage_buckets[1].label, "Weekly");
    }

    #[tokio::test]
    async fn fetches_anthropic_usage_with_local_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .and(header("authorization", "Bearer token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    { "usage": { "input_tokens": 12, "output_tokens": 8 } }
                ],
                "quota": { "limit": 100, "remaining": 80 }
            })))
            .mount(&server)
            .await;

        let mut account = account();
        account.endpoint_override = Some(server.uri());
        let snapshot = fetch(&Client::new(), &account).await.unwrap();

        assert_eq!(snapshot.status, SnapshotStatus::Healthy);
        assert_eq!(snapshot.quota.unwrap().used, 20.0);
    }
}
