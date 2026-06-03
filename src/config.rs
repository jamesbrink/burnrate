use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{
    AccountConfig, AccountInput, AccountView, AppSettings, ProviderKind, SecretStorageMode,
};

const CONFIG_FILE: &str = "accounts.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppConfig {
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
}

impl AppConfig {
    pub(crate) fn views(&self) -> Vec<AccountView> {
        self.sorted_accounts()
            .into_iter()
            .map(|account| AccountView {
                id: account.id.clone(),
                provider: account.provider,
                label: account.label.clone(),
                enabled: account.enabled,
                auto_detected: account.auto_detected,
                credential_path: account.credential_path.clone(),
                endpoint_override: account.endpoint_override.clone(),
                secret_storage: account.secret_storage,
                has_secret: match account.secret_storage {
                    SecretStorageMode::Keyring => account.keyring_account.is_some(),
                    SecretStorageMode::Plaintext => account.plaintext_secret.is_some(),
                },
                email: account.email.clone(),
                config_dir: account.config_dir.clone(),
                aws_profile: account.aws_profile.clone(),
                aws_region: account.aws_region.clone(),
                aws_monthly_budget_usd: account.aws_monthly_budget_usd,
                aws_categories: account.aws_categories.clone(),
                created_at: account.created_at,
                updated_at: account.updated_at,
            })
            .collect()
    }

    /// Accounts in display order: explicit `order_index` first (ascending),
    /// then legacy/unset accounts in their stored insertion order.
    fn sorted_accounts(&self) -> Vec<&AccountConfig> {
        let mut refs: Vec<(usize, &AccountConfig)> = self.accounts.iter().enumerate().collect();
        refs.sort_by(|(ai, a), (bi, b)| match (a.order_index, b.order_index) {
            (Some(x), Some(y)) => x.cmp(&y).then(ai.cmp(bi)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => ai.cmp(bi),
        });
        refs.into_iter().map(|(_, account)| account).collect()
    }

    /// Enabled accounts (clones) in display order, for snapshot fan-out.
    pub(crate) fn enabled_accounts_ordered(&self) -> Vec<AccountConfig> {
        self.sorted_accounts()
            .into_iter()
            .filter(|account| account.enabled)
            .cloned()
            .collect()
    }

    /// Apply a user-defined global order. `ids` lists accounts in the desired
    /// order; any account not present is appended after, preserving its prior
    /// relative order. `order_index` is re-normalized to a dense `0..n`. Does not
    /// touch `updated_at` (which keys the provider success cache).
    pub(crate) fn reorder(&mut self, ids: &[String]) {
        let mut ordered: Vec<usize> = Vec::with_capacity(self.accounts.len());
        for id in ids {
            if let Some(index) = self.accounts.iter().position(|account| &account.id == id)
                && !ordered.contains(&index)
            {
                ordered.push(index);
            }
        }
        // Append any accounts not named in `ids`, in their current display order.
        let remaining: Vec<usize> = self
            .sorted_accounts()
            .into_iter()
            .filter_map(|account| self.accounts.iter().position(|a| a.id == account.id))
            .filter(|index| !ordered.contains(index))
            .collect();
        ordered.extend(remaining);
        for (rank, &index) in ordered.iter().enumerate() {
            self.accounts[index].order_index = Some(rank as i64);
        }
    }

    pub(crate) fn upsert_manual(&mut self, input: AccountInput) -> AccountConfig {
        let now = Utc::now();
        let id = input
            .id
            .unwrap_or_else(|| format!("{}-{}", input.provider.as_str(), Uuid::new_v4().simple()));

        let existing = self.accounts.iter_mut().find(|account| account.id == id);
        if let Some(account) = existing {
            account.provider = input.provider;
            account.label = input.label;
            account.enabled = input.enabled;
            account.endpoint_override = input.endpoint_override;
            account.secret_storage = input.secret_storage;
            account.aws_profile = input.aws_profile;
            account.aws_region = input.aws_region;
            account.aws_monthly_budget_usd = input.aws_monthly_budget_usd;
            account.aws_categories = input.aws_categories;
            account.auto_detected = false;
            account.updated_at = now;
            return account.clone();
        }

        let account = AccountConfig {
            id,
            provider: input.provider,
            label: input.label,
            enabled: input.enabled,
            auto_detected: false,
            credential_path: None,
            endpoint_override: input.endpoint_override,
            secret_storage: input.secret_storage,
            keyring_account: None,
            plaintext_secret: None,
            email: None,
            config_dir: None,
            aws_profile: input.aws_profile,
            aws_region: input.aws_region,
            aws_monthly_budget_usd: input.aws_monthly_budget_usd,
            aws_categories: input.aws_categories,
            order_index: None,
            created_at: now,
            updated_at: now,
        };
        self.accounts.push(account.clone());
        account
    }

    pub(crate) fn remove(&mut self, id: &str) -> Option<AccountConfig> {
        let index = self.accounts.iter().position(|account| account.id == id)?;
        Some(self.accounts.remove(index))
    }

    pub(crate) fn merge_detected(&mut self, detected: Vec<AccountConfig>) -> bool {
        let mut changed = false;
        for account in detected {
            if let Some(existing) = self.accounts.iter_mut().find(|item| item.id == account.id) {
                let account_changed =
                    !existing.auto_detected || existing.credential_path != account.credential_path;
                if account_changed {
                    existing.auto_detected = true;
                    existing.credential_path = account.credential_path.clone();
                    existing.updated_at = Utc::now();
                    changed = true;
                }
            } else {
                self.accounts.push(account);
                changed = true;
            }
        }
        changed
    }
}

/// Create a directory (and parents) with private `0700` permissions on its leaf,
/// matching the hardening used for the config dir. Used for per-account CLI homes.
pub(crate) fn create_private_dir(path: &Path) -> Result<()> {
    let existed = path.exists();
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", path.display()))?;
    set_private_dir_permissions(path, existed)
}

pub(crate) fn config_dir() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("BURNRATE_CONFIG_DIR") {
        return Ok(PathBuf::from(path));
    }

    let base = dirs::data_local_dir()
        .or_else(dirs::config_local_dir)
        .context("could not find a local data directory")?;
    Ok(base.join("burnrate"))
}

pub(crate) fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(CONFIG_FILE))
}

#[cfg(test)]
pub(crate) fn load_from_path(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&contents).with_context(|| format!("failed to parse {}", path.display()))
}

pub(crate) fn load_or_recover_from_path(path: &Path) -> Result<(AppConfig, bool)> {
    if !path.exists() {
        return Ok((AppConfig::default(), true));
    }

    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    match serde_json::from_str(&contents) {
        Ok(config) => Ok((config, false)),
        Err(error) => {
            let backup = recovered_config_path(path)?;
            fs::rename(path, &backup).with_context(|| {
                format!(
                    "failed to move malformed config {} to {}",
                    path.display(),
                    backup.display()
                )
            })?;
            eprintln!(
                "Burnrate moved malformed config {} to {}: {error}",
                path.display(),
                backup.display()
            );
            Ok((AppConfig::default(), true))
        }
    }
}

pub(crate) fn save_to_path(path: &Path, config: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        let parent_existed = parent.exists();
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        set_private_dir_permissions(parent, parent_existed)?;
    }

    let contents = serde_json::to_string_pretty(config)?;
    write_private_file(path, &contents)
}

fn recovered_config_path(path: &Path) -> Result<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX epoch")?
        .as_secs();
    Ok(path.with_extension(format!("json.invalid-{nonce}")))
}

fn write_private_file(path: &Path, contents: &str) -> Result<()> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX epoch")?
        .as_nanos();
    let tmp_path = path.with_file_name(format!(
        ".{}.tmp-{}-{nonce}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("accounts.json"),
        std::process::id()
    ));

    let result = (|| -> Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        set_private_file_mode(&mut options);

        let mut file = options
            .open(&tmp_path)
            .with_context(|| format!("failed to create {}", tmp_path.display()))?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", tmp_path.display()))?;
        fs::rename(&tmp_path, path)
            .with_context(|| format!("failed to replace {}", path.display()))?;
        set_private_file_permissions(path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }

    result
}

#[cfg(unix)]
fn set_private_file_mode(options: &mut fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_private_file_mode(_options: &mut fs::OpenOptions) {}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set private permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path, existed: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if !existed {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to set private permissions on {}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path, _existed: bool) -> Result<()> {
    Ok(())
}

pub(crate) fn default_auto_account(
    id: &str,
    provider: ProviderKind,
    label: &str,
    credential_path: PathBuf,
) -> AccountConfig {
    let now = Utc::now();
    AccountConfig {
        id: id.to_string(),
        provider,
        label: label.to_string(),
        enabled: true,
        auto_detected: true,
        credential_path: Some(credential_path.display().to_string()),
        endpoint_override: None,
        secret_storage: SecretStorageMode::Keyring,
        keyring_account: None,
        plaintext_secret: None,
        email: None,
        config_dir: None,
        aws_profile: None,
        aws_region: None,
        aws_monthly_budget_usd: None,
        aws_categories: Vec::new(),
        order_index: None,
        created_at: now,
        updated_at: now,
    }
}

/// The isolated CLI home Burnrate manages for a non-default account, e.g.
/// `<config_dir>/cli/claude-code/<account-id>` used as `CLAUDE_CONFIG_DIR` /
/// `CODEX_HOME`. The auto-detected primary account does not use this (it shares
/// the system default location).
pub(crate) fn account_cli_dir(provider: ProviderKind, account_id: &str) -> Result<PathBuf> {
    Ok(config_dir()?
        .join("cli")
        .join(provider.as_str())
        .join(account_id))
}

/// True only when `path` lives under Burnrate's managed `<config_dir>/cli/` tree.
/// Guards destructive cleanup from ever touching a system default such as
/// `~/.claude` or `~/.codex`.
pub(crate) fn is_managed_cli_dir(path: &Path) -> bool {
    let Ok(root) = config_dir() else {
        return false;
    };
    let cli_root = root.join("cli");
    path.starts_with(&cli_root) && path != cli_root
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn saves_and_loads_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        let mut config = AppConfig::default();
        config.upsert_manual(AccountInput {
            id: Some("openrouter-main".to_string()),
            provider: ProviderKind::OpenRouter,
            label: "OpenRouter".to_string(),
            enabled: true,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Plaintext,
            secret: Some("secret".to_string()),
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
        });

        save_to_path(&path, &config).unwrap();
        let loaded = load_from_path(&path).unwrap();

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].provider, ProviderKind::OpenRouter);
        assert!(loaded.settings.hide_from_dock);
    }

    #[test]
    fn loads_legacy_config_without_settings() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        fs::write(&path, r#"{"accounts":[]}"#).unwrap();

        let loaded = load_from_path(&path).unwrap();

        assert!(loaded.accounts.is_empty());
        assert!(loaded.settings.hide_from_dock);
    }

    #[test]
    fn recovers_malformed_config_by_moving_it_aside() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        fs::write(&path, "{not-json").unwrap();

        let (loaded, should_save) = load_or_recover_from_path(&path).unwrap();

        assert!(loaded.accounts.is_empty());
        assert!(should_save);
        assert!(!path.exists());
        assert!(fs::read_dir(dir.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("invalid")
        }));
    }

    #[test]
    fn views_report_secret_state_without_exposing_secret() {
        let mut config = AppConfig::default();
        let account = config.upsert_manual(AccountInput {
            id: Some("openrouter-main".to_string()),
            provider: ProviderKind::OpenRouter,
            label: "OpenRouter".to_string(),
            enabled: true,
            endpoint_override: Some("https://example.test".to_string()),
            secret_storage: SecretStorageMode::Plaintext,
            secret: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
        });
        config.accounts[0].plaintext_secret = Some("sk-test".to_string());

        let view = config.views().pop().unwrap();

        assert_eq!(view.id, account.id);
        assert!(view.has_secret);
        assert_eq!(
            view.endpoint_override.as_deref(),
            Some("https://example.test")
        );
    }

    #[test]
    fn views_only_report_secret_for_selected_storage() {
        let mut config = AppConfig::default();
        config.upsert_manual(AccountInput {
            id: Some("openrouter-main".to_string()),
            provider: ProviderKind::OpenRouter,
            label: "OpenRouter".to_string(),
            enabled: true,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Plaintext,
            secret: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
        });
        config.accounts[0].keyring_account = Some("stale-keyring-entry".to_string());

        assert!(!config.views()[0].has_secret);

        config.accounts[0].plaintext_secret = Some("sk-test".to_string());
        assert!(config.views()[0].has_secret);
    }

    #[cfg(unix)]
    #[test]
    fn saves_config_with_private_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        save_to_path(&path, &AppConfig::default()).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn upsert_updates_existing_manual_account() {
        let mut config = AppConfig::default();
        config.upsert_manual(AccountInput {
            id: Some("openrouter-main".to_string()),
            provider: ProviderKind::OpenRouter,
            label: "OpenRouter".to_string(),
            enabled: true,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Keyring,
            secret: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
        });

        config.upsert_manual(AccountInput {
            id: Some("openrouter-main".to_string()),
            provider: ProviderKind::Codex,
            label: "Codex Manual".to_string(),
            enabled: false,
            endpoint_override: Some("http://localhost".to_string()),
            secret_storage: SecretStorageMode::Plaintext,
            secret: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
        });

        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].provider, ProviderKind::Codex);
        assert!(!config.accounts[0].enabled);
        assert!(!config.accounts[0].auto_detected);
    }

    #[test]
    fn remove_returns_removed_account() {
        let mut config = AppConfig::default();
        config.upsert_manual(AccountInput {
            id: Some("openrouter-main".to_string()),
            provider: ProviderKind::OpenRouter,
            label: "OpenRouter".to_string(),
            enabled: true,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Keyring,
            secret: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
        });

        let removed = config.remove("openrouter-main").unwrap();

        assert_eq!(removed.id, "openrouter-main");
        assert!(config.accounts.is_empty());
        assert!(config.remove("missing").is_none());
    }

    #[test]
    fn merges_detected_accounts_without_duplicates() {
        let mut config = AppConfig::default();
        let detected = default_auto_account(
            "codex-local",
            ProviderKind::Codex,
            "Codex",
            PathBuf::from("/tmp/codex"),
        );

        assert!(config.merge_detected(vec![detected.clone()]));
        let updated_at = config.accounts[0].updated_at;
        assert!(!config.merge_detected(vec![detected]));

        assert_eq!(config.accounts.len(), 1);
        assert!(config.accounts[0].auto_detected);
        assert_eq!(config.accounts[0].updated_at, updated_at);
    }

    fn add_account(config: &mut AppConfig, id: &str, provider: ProviderKind) {
        config.upsert_manual(AccountInput {
            id: Some(id.to_string()),
            provider,
            label: id.to_string(),
            enabled: true,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Keyring,
            secret: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
        });
    }

    fn view_ids(config: &AppConfig) -> Vec<String> {
        config.views().into_iter().map(|view| view.id).collect()
    }

    #[test]
    fn reorder_assigns_dense_indices_and_appends_unspecified() {
        let mut config = AppConfig::default();
        add_account(&mut config, "a", ProviderKind::ClaudeCode);
        add_account(&mut config, "b", ProviderKind::Codex);
        add_account(&mut config, "c", ProviderKind::OpenRouter);

        config.reorder(&["c".to_string(), "a".to_string(), "b".to_string()]);
        assert_eq!(view_ids(&config), vec!["c", "a", "b"]);
        for account in &config.accounts {
            assert!(account.order_index.is_some());
        }

        // A partial list reorders the named account and appends the rest in
        // their current display order.
        config.reorder(&["b".to_string()]);
        assert_eq!(view_ids(&config), vec!["b", "c", "a"]);
    }

    #[test]
    fn reorder_ignores_unknown_ids() {
        let mut config = AppConfig::default();
        add_account(&mut config, "a", ProviderKind::ClaudeCode);
        add_account(&mut config, "b", ProviderKind::Codex);

        config.reorder(&["ghost".to_string(), "b".to_string(), "a".to_string()]);

        assert_eq!(view_ids(&config), vec!["b", "a"]);
    }

    #[test]
    fn views_keep_insertion_order_until_reordered() {
        let mut config = AppConfig::default();
        add_account(&mut config, "a", ProviderKind::ClaudeCode);
        add_account(&mut config, "b", ProviderKind::Codex);
        add_account(&mut config, "c", ProviderKind::OpenRouter);

        // Legacy/unset order_index renders in insertion order.
        assert_eq!(view_ids(&config), vec!["a", "b", "c"]);

        // An explicitly ordered account sorts ahead of unset ones.
        config.accounts[2].order_index = Some(0);
        assert_eq!(view_ids(&config), vec!["c", "a", "b"]);
    }

    #[test]
    fn enabled_accounts_ordered_filters_and_sorts() {
        let mut config = AppConfig::default();
        add_account(&mut config, "a", ProviderKind::ClaudeCode);
        add_account(&mut config, "b", ProviderKind::Codex);
        add_account(&mut config, "c", ProviderKind::OpenRouter);
        config.accounts[1].enabled = false;
        config.reorder(&["c".to_string(), "a".to_string(), "b".to_string()]);

        let enabled: Vec<String> = config
            .enabled_accounts_ordered()
            .into_iter()
            .map(|account| account.id)
            .collect();

        assert_eq!(enabled, vec!["c", "a"]);
    }

    #[test]
    fn loads_legacy_account_without_new_fields() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        fs::write(
            &path,
            r#"{
                "accounts": [{
                    "id": "claude-code-local",
                    "provider": "claude-code",
                    "label": "Claude Code",
                    "enabled": true,
                    "autoDetected": true,
                    "credentialPath": "/home/user/.claude",
                    "endpointOverride": null,
                    "secretStorage": "keyring",
                    "keyringAccount": null,
                    "plaintextSecret": null,
                    "createdAt": "2026-01-01T00:00:00Z",
                    "updatedAt": "2026-01-01T00:00:00Z"
                }]
            }"#,
        )
        .unwrap();

        let loaded = load_from_path(&path).unwrap();

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].email, None);
        assert_eq!(loaded.accounts[0].config_dir, None);
        assert_eq!(loaded.accounts[0].order_index, None);
        assert_eq!(loaded.accounts[0].cli_config_dir(), None);
    }

    #[test]
    fn account_cli_dir_is_under_config_dir() {
        let expected = config_dir()
            .unwrap()
            .join("cli")
            .join("codex")
            .join("codex-123");
        let actual = account_cli_dir(ProviderKind::Codex, "codex-123").unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn is_managed_cli_dir_guards_system_default() {
        let root = config_dir().unwrap();
        let managed = root.join("cli").join("claude-code").join("acct-1");

        assert!(is_managed_cli_dir(&managed));
        // The `cli` root itself is not a deletable per-account dir.
        assert!(!is_managed_cli_dir(&root.join("cli")));
        // A system default location must never be considered managed.
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        assert!(!is_managed_cli_dir(&home.join(".claude")));
        assert!(!is_managed_cli_dir(&home.join(".codex")));
    }
}
