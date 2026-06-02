use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Client;

use crate::{
    config::default_auto_account,
    models::{
        AccountConfig, BurnRateSnapshot, ProviderKind, QuotaSnapshot, SnapshotStatus, UsageSnapshot,
    },
};

use super::{endpoint, number, require_token, text};

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
        ))
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
    let reset_at = text(
        result,
        &["/reset_at", "/quota/reset_at", "/rate_limits/0/reset_at"],
    )
    .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
    .map(|value| value.with_timezone(&Utc));

    let status = match (limit, remaining) {
        (Some(limit), Some(remaining)) if limit > 0.0 && remaining / limit <= 0.05 => {
            SnapshotStatus::Exhausted
        }
        (Some(limit), Some(remaining)) if limit > 0.0 && remaining / limit <= 0.2 => {
            SnapshotStatus::Warning
        }
        _ => SnapshotStatus::Healthy,
    };

    UsageSnapshot {
        account_id: account.id.clone(),
        provider: account.provider,
        label: account.label.clone(),
        status,
        quota: Some(QuotaSnapshot {
            used,
            limit,
            remaining,
            unit: "requests".to_string(),
            reset_at,
        }),
        burn_rate: Some(BurnRateSnapshot {
            per_hour: used / 24.0,
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
    use crate::models::SecretStorageMode;

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
