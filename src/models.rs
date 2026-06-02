use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProviderKind {
    ClaudeCode,
    Codex,
    OpenRouter,
}

impl ProviderKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ProviderKind::ClaudeCode => "claude-code",
            ProviderKind::Codex => "codex",
            ProviderKind::OpenRouter => "openrouter",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SecretStorageMode {
    Keyring,
    Plaintext,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppSettings {
    pub hide_from_dock: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountConfig {
    pub id: String,
    pub provider: ProviderKind,
    pub label: String,
    pub enabled: bool,
    pub auto_detected: bool,
    pub credential_path: Option<String>,
    pub endpoint_override: Option<String>,
    pub secret_storage: SecretStorageMode,
    pub keyring_account: Option<String>,
    pub plaintext_secret: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountInput {
    pub id: Option<String>,
    pub provider: ProviderKind,
    pub label: String,
    pub enabled: bool,
    pub endpoint_override: Option<String>,
    pub secret_storage: SecretStorageMode,
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountView {
    pub id: String,
    pub provider: ProviderKind,
    pub label: String,
    pub enabled: bool,
    pub auto_detected: bool,
    pub credential_path: Option<String>,
    pub endpoint_override: Option<String>,
    pub secret_storage: SecretStorageMode,
    pub has_secret: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageSnapshot {
    pub account_id: String,
    pub provider: ProviderKind,
    pub label: String,
    pub status: SnapshotStatus,
    pub quota: Option<QuotaSnapshot>,
    pub burn_rate: Option<BurnRateSnapshot>,
    pub message: Option<String>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SnapshotStatus {
    Healthy,
    Warning,
    Exhausted,
    Error,
    Stale,
    NotConfigured,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuotaSnapshot {
    pub used: f64,
    pub limit: Option<f64>,
    pub remaining: Option<f64>,
    pub unit: String,
    pub reset_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BurnRateSnapshot {
    pub per_hour: f64,
    pub projected_depletion_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardState {
    pub accounts: Vec<AccountView>,
    pub snapshots: Vec<UsageSnapshot>,
    pub tray_summary: TraySummary,
    pub settings: AppSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TraySummary {
    pub label: String,
    pub status: SnapshotStatus,
    pub critical_count: usize,
    pub warning_count: usize,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_uses_stable_config_names() {
        assert_eq!(ProviderKind::ClaudeCode.as_str(), "claude-code");
        assert_eq!(ProviderKind::Codex.as_str(), "codex");
        assert_eq!(ProviderKind::OpenRouter.as_str(), "openrouter");
    }

    #[test]
    fn default_settings_keep_dock_visible() {
        assert!(!AppSettings::default().hide_from_dock);
    }
}
