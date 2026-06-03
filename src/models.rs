use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProviderKind {
    ClaudeCode,
    Codex,
    #[serde(rename = "openrouter", alias = "open-router")]
    OpenRouter,
    Runpod,
}

impl ProviderKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ProviderKind::ClaudeCode => "claude-code",
            ProviderKind::Codex => "codex",
            ProviderKind::OpenRouter => "openrouter",
            ProviderKind::Runpod => "runpod",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SecretStorageMode {
    Keyring,
    Plaintext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppSettings {
    pub hide_from_dock: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hide_from_dock: true,
        }
    }
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
    /// Email address associated with the signed-in account, when known.
    #[serde(default)]
    pub email: Option<String>,
    /// Per-account CLI home (`CLAUDE_CONFIG_DIR` / `CODEX_HOME`). `None` means the
    /// system default location (`~/.claude` / `~/.codex`).
    #[serde(default)]
    pub config_dir: Option<String>,
    /// Global display order; lower sorts first. `None` is legacy/unset and sorts
    /// after explicitly ordered accounts.
    #[serde(default)]
    pub order_index: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AccountConfig {
    /// The per-account CLI home, or `None` for the system-default account.
    pub(crate) fn cli_config_dir(&self) -> Option<&str> {
        self.config_dir.as_deref()
    }
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
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub config_dir: Option<String>,
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
    #[serde(default)]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription: Option<SubscriptionSnapshot>,
    #[serde(default)]
    pub usage_buckets: Vec<UsageBucketSnapshot>,
    pub quota: Option<QuotaSnapshot>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SubscriptionPlan {
    Free,
    Pro,
    Max,
    Team,
    Enterprise,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubscriptionSnapshot {
    pub plan: SubscriptionPlan,
    pub plan_label: String,
    pub rate_limit_tier: Option<String>,
    pub extra_usage_enabled: Option<bool>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageBucketSnapshot {
    pub id: String,
    pub label: String,
    pub window: Option<String>,
    pub used: f64,
    pub limit: Option<f64>,
    pub remaining: Option<f64>,
    pub unit: String,
    pub reset_at: Option<DateTime<Utc>>,
    pub status: SnapshotStatus,
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

/// Emitted as `burnrate-login-complete` when an interactive sign-in succeeds.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginComplete {
    pub id: String,
    pub account: AccountView,
}

/// Emitted as `burnrate-login-failed` when a sign-in errors or is cancelled.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginFailed {
    pub id: String,
    pub error: String,
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
        assert_eq!(ProviderKind::Runpod.as_str(), "runpod");
        assert_eq!(
            serde_json::to_string(&ProviderKind::OpenRouter).unwrap(),
            "\"openrouter\""
        );
        assert_eq!(
            serde_json::from_str::<ProviderKind>("\"open-router\"").unwrap(),
            ProviderKind::OpenRouter
        );
    }

    #[test]
    fn default_settings_hide_dock_for_tray_first_launch() {
        assert!(AppSettings::default().hide_from_dock);
    }

    #[test]
    fn subscription_plans_use_stable_wire_names() {
        assert_eq!(
            serde_json::to_string(&SubscriptionPlan::Max).unwrap(),
            "\"max\""
        );
        assert_eq!(
            serde_json::to_string(&SubscriptionPlan::Unknown).unwrap(),
            "\"unknown\""
        );
    }
}
