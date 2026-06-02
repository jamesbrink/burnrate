use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Client;

use crate::models::{AccountConfig, QuotaSnapshot, UsageSnapshot};

use super::{
    bucket_from_parts, endpoint, number, primary_quota, require_token, status_from_remaining,
};

const DEFAULT_ENDPOINT: &str = "https://openrouter.ai/api/v1/credits";

pub(crate) async fn fetch(http: &Client, account: &AccountConfig) -> Result<UsageSnapshot> {
    let token = require_token(account)?;
    let url = endpoint(account, "BURNRATE_OPENROUTER_CREDITS_URL", DEFAULT_ENDPOINT)?;
    let value: serde_json::Value = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .context("failed to fetch OpenRouter credits")?
        .error_for_status()
        .context("OpenRouter credits request failed")?
        .json()
        .await
        .context("failed to decode OpenRouter credits")?;

    Ok(parse_openrouter(account, &value))
}

pub(crate) fn parse_openrouter(
    account: &AccountConfig,
    value: &serde_json::Value,
) -> UsageSnapshot {
    let total = number(
        value,
        &["/data/total_credits", "/total_credits", "/credits/total"],
    );
    let used = number(
        value,
        &["/data/total_usage", "/total_usage", "/credits/used"],
    )
    .unwrap_or(0.0);
    let remaining = total.map(|total| (total - used).max(0.0)).or_else(|| {
        number(
            value,
            &[
                "/data/remaining_credits",
                "/remaining_credits",
                "/credits/remaining",
            ],
        )
    });

    let status = status_from_remaining(total, remaining);

    let bucket = bucket_from_parts(
        "credits",
        "Credits",
        None,
        QuotaSnapshot {
            used,
            limit: total,
            remaining,
            unit: "USD".to_string(),
            reset_at: None,
        },
    );
    let quota = primary_quota(std::slice::from_ref(&bucket));

    UsageSnapshot {
        account_id: account.id.clone(),
        provider: account.provider,
        label: account.label.clone(),
        status,
        subscription: None,
        usage_buckets: vec![bucket],
        quota,
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
    use crate::models::{ProviderKind, SecretStorageMode, SnapshotStatus};

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
            plaintext_secret: Some("sk-test".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn maps_openrouter_credit_response() {
        let snapshot = parse_openrouter(
            &account(),
            &json!({
                "data": {
                    "total_credits": 100.0,
                    "total_usage": 85.0
                }
            }),
        );

        assert_eq!(snapshot.status, SnapshotStatus::Warning);
        let quota = snapshot.quota.unwrap();
        assert_eq!(quota.remaining, Some(15.0));
        assert_eq!(quota.unit, "USD");
    }

    #[tokio::test]
    async fn fetches_openrouter_credits_with_manual_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "total_credits": 25.0,
                    "total_usage": 4.0
                }
            })))
            .mount(&server)
            .await;

        let mut account = account();
        account.endpoint_override = Some(server.uri());
        let snapshot = fetch(&Client::new(), &account).await.unwrap();

        assert_eq!(snapshot.status, SnapshotStatus::Healthy);
        let quota = snapshot.quota.unwrap();
        assert_eq!(quota.remaining, Some(21.0));
        assert_eq!(quota.unit, "USD");
    }
}
