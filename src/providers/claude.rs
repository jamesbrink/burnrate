use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use reqwest::Client;

use crate::{
    config::default_auto_account,
    models::{
        AccountConfig, BurnRateSnapshot, ProviderKind, QuotaSnapshot, SnapshotStatus, UsageSnapshot,
    },
};

use super::{endpoint, number, require_token, text};

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

    Ok(parse_claude_usage(account, &value))
}

pub(crate) fn parse_claude_usage(
    account: &AccountConfig,
    value: &serde_json::Value,
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

    let status = status_from_remaining(limit, remaining);
    UsageSnapshot {
        account_id: account.id.clone(),
        provider: account.provider,
        label: account.label.clone(),
        status,
        quota: Some(QuotaSnapshot {
            used,
            limit,
            remaining,
            unit: "tokens".to_string(),
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
