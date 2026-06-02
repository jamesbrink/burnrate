mod claude;
mod codex;
mod openrouter;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use reqwest::Client;

use crate::{
    key_store,
    models::{AccountConfig, ProviderKind, SnapshotStatus, UsageSnapshot},
};

#[derive(Clone)]
pub(crate) struct ProviderClient {
    http: Client,
}

impl ProviderClient {
    pub(crate) fn new() -> Self {
        Self {
            http: Client::new(),
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
        quota: None,
        burn_rate: None,
        message: Some(error.to_string()),
        fetched_at: Utc::now(),
    }
}

fn endpoint(account: &AccountConfig, env_key: &str, default: &str) -> String {
    account
        .endpoint_override
        .clone()
        .or_else(|| std::env::var(env_key).ok())
        .unwrap_or_else(|| default.to_string())
}

fn number(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = value.pointer(key)?;
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|item| item.parse::<f64>().ok()))
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

fn token_from_config(account: &AccountConfig) -> Result<Option<String>> {
    if let Some(secret) = key_store::get_secret(account)? {
        return Ok(Some(secret));
    }

    let Some(path) = &account.credential_path else {
        return Ok(None);
    };
    let path = std::path::Path::new(path);
    if path.is_file() {
        return read_token_file(path);
    }

    for candidate in ["auth.json", ".credentials.json", "credentials.json"] {
        let candidate = path.join(candidate);
        if candidate.exists() {
            return read_token_file(&candidate);
        }
    }

    Ok(None)
}

fn read_token_file(path: &std::path::Path) -> Result<Option<String>> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(find_token(&json))
}

fn find_token(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for key in [
                "access_token",
                "accessToken",
                "oauth_access_token",
                "oauthAccessToken",
                "token",
                "api_key",
                "apiKey",
            ] {
                if let Some(token) = map.get(key).and_then(|value| value.as_str())
                    && !token.is_empty()
                {
                    return Some(token.to_string());
                }
            }
            map.values().find_map(find_token)
        }
        serde_json::Value::Array(values) => values.iter().find_map(find_token),
        _ => None,
    }
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
    fn finds_nested_tokens() {
        let token = find_token(&json!({
            "auth": {
                "oauthAccessToken": "tok_123"
            }
        }));

        assert_eq!(token, Some("tok_123".to_string()));
    }

    #[test]
    fn endpoint_prefers_account_override() {
        let mut account = account();
        account.endpoint_override = Some("https://example.test".to_string());

        assert_eq!(
            endpoint(&account, "BURNRATE_TEST_ENDPOINT", "https://default.test"),
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
    fn reads_token_from_file_and_directory_candidates() {
        let dir = tempdir().unwrap();
        let token_path = dir.path().join("auth.json");
        std::fs::write(&token_path, r#"{"auth":{"accessToken":"tok_file"}}"#).unwrap();

        let mut account = account();
        account.credential_path = Some(dir.path().display().to_string());

        assert_eq!(
            token_from_config(&account).unwrap(),
            Some("tok_file".to_string())
        );
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
