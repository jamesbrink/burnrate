use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Client;

use crate::{
    config::default_auto_account,
    models::{AccountConfig, BurnRateSnapshot, ProviderKind, QuotaSnapshot, UsageSnapshot},
};

use super::{
    bucket_from_parts, datetime, endpoint, number, overall_status, parse_usage_buckets,
    primary_quota, require_token, subscription_from_json,
};

const DEFAULT_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/rate_limits";

pub(crate) fn detect() -> Option<AccountConfig> {
    let base = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))?;

    if !base.exists() {
        return None;
    }

    let credential_path = if base.join("auth.json").exists() {
        base.join("auth.json")
    } else {
        base
    };

    Some(default_auto_account(
        "codex-local",
        ProviderKind::Codex,
        "Codex",
        credential_path,
    ))
}

pub(crate) async fn fetch(http: &Client, account: &AccountConfig) -> Result<UsageSnapshot> {
    let token = require_token(account)?;
    let value: serde_json::Value = http
        .get(endpoint(
            account,
            "BURNRATE_CODEX_RATE_LIMITS_URL",
            DEFAULT_ENDPOINT,
        )?)
        .bearer_auth(token)
        .send()
        .await
        .context("failed to fetch Codex rate limits")?
        .error_for_status()
        .context("Codex rate limit request failed")?
        .json()
        .await
        .context("failed to decode Codex rate limits")?;

    Ok(parse_codex_rate_limits(account, &value))
}

pub(crate) fn parse_codex_rate_limits(
    account: &AccountConfig,
    value: &serde_json::Value,
) -> UsageSnapshot {
    let result = value.get("result").unwrap_or(value);
    let mut buckets = parse_usage_buckets(value, "requests");
    if buckets.is_empty() {
        let limit = number(result, &["/limit", "/quota/limit", "/rate_limits/0/limit"]);
        let remaining = number(
            result,
            &["/remaining", "/quota/remaining", "/rate_limits/0/remaining"],
        );
        let used = number(result, &["/used", "/quota/used", "/rate_limits/0/used"])
            .or_else(|| {
                limit
                    .zip(remaining)
                    .map(|(limit, remaining)| limit - remaining)
            })
            .unwrap_or(0.0);
        let reset_at = datetime(
            result,
            &["/reset_at", "/quota/reset_at", "/rate_limits/0/reset_at"],
        );
        buckets.push(bucket_from_parts(
            "requests",
            "Requests",
            None,
            QuotaSnapshot {
                used,
                limit,
                remaining,
                unit: "requests".to_string(),
                reset_at,
            },
        ));
    }

    let subscription = subscription_from_json(
        value,
        "codex-app-server",
        &[
            "/result/plan",
            "/result/plan_type",
            "/result/subscription/plan",
            "/result/account/plan",
            "/plan",
            "/plan_type",
            "/subscription/plan",
            "/account/plan",
        ],
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
            id: "codex-local".to_string(),
            provider: ProviderKind::Codex,
            label: "Codex".to_string(),
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
    fn maps_json_rpc_rate_limits() {
        let snapshot = parse_codex_rate_limits(
            &account(),
            &json!({
                "jsonrpc": "2.0",
                "result": {
                    "rate_limits": [
                        {
                            "limit": 100,
                            "remaining": 4,
                            "reset_at": "2026-06-01T12:00:00Z"
                        }
                    ]
                }
            }),
        );

        assert_eq!(snapshot.status, SnapshotStatus::Exhausted);
        assert_eq!(snapshot.quota.unwrap().used, 96.0);
        assert_eq!(snapshot.usage_buckets[0].label, "Requests");
    }

    #[test]
    fn maps_all_codex_subscription_buckets() {
        let snapshot = parse_codex_rate_limits(
            &account(),
            &json!({
                "result": {
                    "plan": "pro",
                    "rate_limit_tier": "chatgpt_pro",
                    "rate_limits": [
                        {
                            "id": "5_hour",
                            "limit": 300,
                            "remaining": 42,
                            "reset_at": "2026-06-01T17:00:00Z"
                        },
                        {
                            "id": "weekly",
                            "limit": 1000,
                            "used": 150
                        }
                    ]
                }
            }),
        );

        let subscription = snapshot.subscription.unwrap();
        assert_eq!(subscription.plan, SubscriptionPlan::Pro);
        assert_eq!(snapshot.status, SnapshotStatus::Warning);
        assert_eq!(snapshot.usage_buckets.len(), 2);
        assert_eq!(snapshot.usage_buckets[0].label, "5-hour");
        assert_eq!(snapshot.usage_buckets[1].label, "Weekly");
    }

    #[tokio::test]
    async fn fetches_json_rpc_rate_limits_with_local_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .and(header("authorization", "Bearer token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "result": {
                    "rate_limits": [
                        { "limit": 120, "remaining": 24 }
                    ]
                }
            })))
            .mount(&server)
            .await;

        let mut account = account();
        account.endpoint_override = Some(server.uri());
        let snapshot = fetch(&Client::new(), &account).await.unwrap();

        assert_eq!(snapshot.status, SnapshotStatus::Warning);
        assert_eq!(snapshot.quota.unwrap().used, 96.0);
    }
}
