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
    Aws,
    Copilot,
}

impl ProviderKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ProviderKind::ClaudeCode => "claude-code",
            ProviderKind::Codex => "codex",
            ProviderKind::OpenRouter => "openrouter",
            ProviderKind::Runpod => "runpod",
            ProviderKind::Aws => "aws",
            ProviderKind::Copilot => "copilot",
        }
    }

    /// Human-facing provider name for user-visible messages (the wire id from
    /// [`Self::as_str`] mirrors `src-ui/constants.ts` `providerLabels`).
    pub(crate) fn display_name(self) -> &'static str {
        match self {
            ProviderKind::ClaudeCode => "Claude Code",
            ProviderKind::Codex => "Codex",
            ProviderKind::OpenRouter => "OpenRouter",
            ProviderKind::Runpod => "Runpod",
            ProviderKind::Aws => "AWS",
            ProviderKind::Copilot => "GitHub Copilot",
        }
    }
}

/// GitHub Copilot subscription tier, which determines the monthly premium
/// request allowance. `Custom` defers to the account's
/// `copilot_custom_limit` for plans with negotiated or unknown quotas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CopilotPlan {
    Free,
    Pro,
    ProPlus,
    Business,
    Enterprise,
    Custom,
}

impl CopilotPlan {
    /// Monthly premium-request allowance per GitHub's published plan quotas.
    /// `Custom` has no built-in allowance — the account's
    /// `copilot_custom_limit` applies instead.
    pub(crate) fn monthly_limit(self) -> Option<f64> {
        match self {
            CopilotPlan::Free => Some(50.0),
            CopilotPlan::Pro | CopilotPlan::Business => Some(300.0),
            CopilotPlan::ProPlus => Some(1500.0),
            CopilotPlan::Enterprise => Some(1000.0),
            CopilotPlan::Custom => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            CopilotPlan::Free => "Free",
            CopilotPlan::Pro => "Pro",
            CopilotPlan::ProPlus => "Pro+",
            CopilotPlan::Business => "Business",
            CopilotPlan::Enterprise => "Enterprise",
            CopilotPlan::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SecretStorageMode {
    Keyring,
    Plaintext,
}

/// Release channel the in-app auto-updater follows. `Stable` tracks the
/// `releases/latest` manifest; `Nightly` follows the rolling `nightly` tag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum UpdateChannel {
    #[default]
    Stable,
    Nightly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppSettings {
    pub hide_from_dock: bool,
    /// Release channel for automatic updates. Defaults to `Stable`; older
    /// config files without this field deserialize to the default.
    #[serde(default)]
    pub update_channel: UpdateChannel,
    /// Manual tray content scale. `1.0` is native size (disabled); users can
    /// lower it to `0.5` to fit dense popovers before scrolling.
    #[serde(default = "default_tray_scale")]
    pub tray_scale: f64,
    /// Whether claudex-backed local usage insights are collected and shown.
    /// On by default; the opt-out exists because indexing builds (and keeps)
    /// `~/.claudex/index.db` from local CLI session logs.
    #[serde(default = "default_true")]
    pub local_insights: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hide_from_dock: true,
            update_channel: UpdateChannel::default(),
            tray_scale: default_tray_scale(),
            local_insights: true,
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
    /// AWS profile name. `None` uses the SDK default credential chain and
    /// current `AWS_PROFILE` environment, without storing static keys.
    #[serde(default)]
    pub aws_profile: Option<String>,
    /// AWS region for SDK configuration. Cost Explorer itself is global; when
    /// unset Burnrate defaults to `us-east-1` so the SDK has a region.
    #[serde(default)]
    pub aws_region: Option<String>,
    /// Optional monthly budget in USD used to calculate remaining/warning state.
    #[serde(default)]
    pub aws_monthly_budget_usd: Option<f64>,
    /// User-configurable Cost Explorer categories shown as sub-buckets.
    #[serde(default)]
    pub aws_categories: Vec<AwsCategoryConfig>,
    /// GitHub Copilot plan, which sets the monthly premium request allowance.
    /// `None` shows usage without a limit.
    #[serde(default)]
    pub copilot_plan: Option<CopilotPlan>,
    /// Monthly premium request allowance when `copilot_plan` is `Custom`.
    #[serde(default)]
    pub copilot_custom_limit: Option<f64>,
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
    #[serde(default)]
    pub aws_profile: Option<String>,
    #[serde(default)]
    pub aws_region: Option<String>,
    #[serde(default)]
    pub aws_monthly_budget_usd: Option<f64>,
    #[serde(default)]
    pub aws_categories: Vec<AwsCategoryConfig>,
    #[serde(default)]
    pub copilot_plan: Option<CopilotPlan>,
    #[serde(default)]
    pub copilot_custom_limit: Option<f64>,
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
    #[serde(default)]
    pub aws_profile: Option<String>,
    #[serde(default)]
    pub aws_region: Option<String>,
    #[serde(default)]
    pub aws_monthly_budget_usd: Option<f64>,
    #[serde(default)]
    pub aws_categories: Vec<AwsCategoryConfig>,
    #[serde(default)]
    pub copilot_plan: Option<CopilotPlan>,
    #[serde(default)]
    pub copilot_custom_limit: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AwsCategoryConfig {
    pub id: String,
    pub label: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub filter: AwsCostFilter,
    #[serde(default)]
    pub group_by: Option<AwsGroupBy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum AwsCostFilter {
    Dimension { key: String, values: Vec<String> },
    Tag { key: String, values: Vec<String> },
    CostCategory { key: String, values: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AwsGroupBy {
    pub kind: AwsGroupByKind,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AwsGroupByKind {
    Dimension,
    Tag,
    CostCategory,
}

fn default_true() -> bool {
    true
}

fn default_tray_scale() -> f64 {
    1.0
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

/// claudex-backed local usage metrics, aggregated per provider. Local session
/// history cannot be split between multiple accounts of one provider, so these
/// are provider-level — the UI says so rather than implying per-account
/// precision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalUsageReport {
    /// False when insights are disabled, claudex has no data, or collection
    /// failed; `message` carries the human-readable reason.
    pub available: bool,
    pub message: Option<String>,
    pub providers: Vec<ProviderLocalUsage>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderLocalUsage {
    pub provider: ProviderKind,
    pub today_cost_usd: f64,
    pub today_sessions: i64,
    pub week_cost_usd: f64,
    pub month_cost_usd: f64,
    /// Linear month-end extrapolation of `month_cost_usd`; `None` when there
    /// is no spend yet.
    pub projected_month_cost_usd: Option<f64>,
    pub month_input_tokens: i64,
    pub month_output_tokens: i64,
    pub top_model: Option<String>,
    pub model_distribution: Vec<LocalModelUsage>,
    pub top_projects: Vec<LocalProjectCost>,
    /// Daily cost buckets, ascending by date (sparkline source).
    pub daily: Vec<LocalDailyUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDailyUsage {
    /// ISO date (`YYYY-MM-DD`).
    pub date: String,
    pub cost_usd: f64,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalModelUsage {
    pub model: String,
    pub sessions: i64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalProjectCost {
    pub project: String,
    pub sessions: i64,
    pub cost_usd: f64,
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
        assert_eq!(ProviderKind::Aws.as_str(), "aws");
        assert_eq!(ProviderKind::Copilot.as_str(), "copilot");
        assert_eq!(
            serde_json::to_string(&ProviderKind::OpenRouter).unwrap(),
            "\"openrouter\""
        );
        assert_eq!(
            serde_json::from_str::<ProviderKind>("\"open-router\"").unwrap(),
            ProviderKind::OpenRouter
        );
        assert_eq!(
            serde_json::to_string(&ProviderKind::Copilot).unwrap(),
            "\"copilot\""
        );
    }

    #[test]
    fn copilot_plans_use_stable_wire_names() {
        assert_eq!(
            serde_json::to_string(&CopilotPlan::ProPlus).unwrap(),
            "\"pro-plus\""
        );
        assert_eq!(
            serde_json::from_str::<CopilotPlan>("\"enterprise\"").unwrap(),
            CopilotPlan::Enterprise
        );
    }

    #[test]
    fn settings_without_local_insights_field_default_to_enabled() {
        let settings: AppSettings = serde_json::from_str(r#"{"hideFromDock":false}"#).unwrap();
        assert!(settings.local_insights);
        assert!(AppSettings::default().local_insights);
    }

    #[test]
    fn default_settings_hide_dock_for_tray_first_launch() {
        assert!(AppSettings::default().hide_from_dock);
    }

    #[test]
    fn default_update_channel_is_stable() {
        assert_eq!(AppSettings::default().update_channel, UpdateChannel::Stable);
        assert_eq!(
            serde_json::to_string(&UpdateChannel::Nightly).unwrap(),
            "\"nightly\""
        );
    }

    #[test]
    fn default_tray_scale_is_native_size() {
        assert_eq!(AppSettings::default().tray_scale, 1.0);
    }

    #[test]
    fn settings_without_channel_field_default_to_stable() {
        // Config files written before the updater shipped have no
        // `updateChannel`; they must still deserialize.
        let settings: AppSettings = serde_json::from_str(r#"{"hideFromDock":false}"#).unwrap();
        assert!(!settings.hide_from_dock);
        assert_eq!(settings.update_channel, UpdateChannel::Stable);
        assert_eq!(settings.tray_scale, 1.0);
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
