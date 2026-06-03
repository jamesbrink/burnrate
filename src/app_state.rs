use std::{fs, path::Path, path::PathBuf, sync::Mutex};

use anyhow::{Result, anyhow};
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    config::{self, AppConfig},
    key_store,
    models::AppSettings,
    models::{
        AccountConfig, AccountInput, AccountView, DashboardState, LoginComplete, LoginFailed,
        ProviderKind, SecretStorageMode, UsageSnapshot,
    },
    providers::{
        self, ProviderClient,
        login::{self, LoginManager, LoginOutcome},
    },
    tray,
};

pub(crate) struct AppState {
    config_path: PathBuf,
    config: Mutex<AppConfig>,
    provider_client: ProviderClient,
    login_manager: LoginManager,
}

impl AppState {
    pub(crate) fn load() -> Result<Self> {
        let config_path = config::config_path()?;
        let (mut config, should_save) = config::load_or_recover_from_path(&config_path)?;
        let detected_changed = config.merge_detected(providers::detect_accounts());
        if should_save || detected_changed {
            config::save_to_path(&config_path, &config)?;
        }

        Ok(Self {
            config_path,
            config: Mutex::new(config),
            provider_client: ProviderClient::new(),
            login_manager: LoginManager::new(),
        })
    }

    pub(crate) fn list_accounts(&self) -> Result<Vec<AccountView>> {
        Ok(self.config.lock().expect("config lock").views())
    }

    pub(crate) fn settings(&self) -> AppSettings {
        self.config.lock().expect("config lock").settings.clone()
    }

    pub(crate) fn save_settings(&self, settings: AppSettings) -> Result<AppSettings> {
        let mut config = self.config.lock().expect("config lock");
        config.settings = settings;
        config::save_to_path(&self.config_path, &config)?;
        Ok(config.settings.clone())
    }

    pub(crate) fn save_account(&self, input: AccountInput) -> Result<Vec<AccountView>> {
        let mut config = self.config.lock().expect("config lock");
        let previous = input.id.as_ref().and_then(|id| {
            config
                .accounts
                .iter()
                .find(|account| &account.id == id)
                .cloned()
        });
        let account = config.upsert_manual(input.clone());

        let account = config
            .accounts
            .iter_mut()
            .find(|item| item.id == account.id)
            .expect("upserted account exists");

        if let Some(secret) = input.secret {
            key_store::set_secret(account, Some(secret))?;
            key_store::validate_plaintext_mode(account)?;
        } else if let Some(previous) = previous {
            key_store::migrate_secret(&previous, account)?;
        }

        config::save_to_path(&self.config_path, &config)?;
        Ok(config.views())
    }

    pub(crate) fn remove_account(&self, id: &str) -> Result<Vec<AccountView>> {
        let mut config = self.config.lock().expect("config lock");
        if let Some(account) = config.remove(id) {
            key_store::remove_secret(&account)?;
        }
        config::save_to_path(&self.config_path, &config)?;
        Ok(config.views())
    }

    pub(crate) fn detect_accounts(&self) -> Result<Vec<AccountView>> {
        let mut config = self.config.lock().expect("config lock");
        if config.merge_detected(providers::detect_accounts()) {
            config::save_to_path(&self.config_path, &config)?;
        }
        Ok(config.views())
    }

    pub(crate) async fn snapshots(&self) -> Vec<UsageSnapshot> {
        let accounts = self
            .config
            .lock()
            .expect("config lock")
            .enabled_accounts_ordered();

        let mut tasks = Vec::with_capacity(accounts.len());
        for (index, account) in accounts.into_iter().enumerate() {
            let provider_client = self.provider_client.clone();
            let task_account = account.clone();
            let handle =
                tokio::spawn(async move { provider_client.refresh_account(&task_account).await });
            tasks.push((index, account, handle));
        }

        let mut snapshots = Vec::with_capacity(tasks.len());
        for (index, account, handle) in tasks {
            match handle.await {
                Ok(snapshot) => snapshots.push((index, snapshot)),
                Err(error) => snapshots.push((
                    index,
                    providers::error_snapshot(
                        &account,
                        anyhow::anyhow!("provider task panicked: {error}"),
                    ),
                )),
            }
        }
        snapshots.sort_by_key(|(index, _)| *index);
        let snapshots: Vec<UsageSnapshot> = snapshots
            .into_iter()
            .map(|(_, snapshot)| snapshot)
            .collect();
        self.persist_discovered_emails(&snapshots);
        snapshots
    }

    /// Persist account emails discovered during a fetch (claude auth status /
    /// codex id_token) so the account list shows them on the next load. Runs once,
    /// only when something changed, to avoid disk churn. Holds no lock across an
    /// await (called after the fan-out completes).
    fn persist_discovered_emails(&self, snapshots: &[UsageSnapshot]) {
        let mut config = self.config.lock().expect("config lock");
        let mut changed = false;
        for snapshot in snapshots {
            if let Some(email) = snapshot.email.as_deref()
                && let Some(account) = config
                    .accounts
                    .iter_mut()
                    .find(|account| account.id == snapshot.account_id)
                && account.email.as_deref() != Some(email)
            {
                account.email = Some(email.to_string());
                changed = true;
            }
        }
        if changed {
            let _ = config::save_to_path(&self.config_path, &config);
        }
    }

    pub(crate) async fn dashboard(&self) -> Result<DashboardState> {
        // Fetch snapshots first so any discovered emails are persisted before we
        // read the account views.
        let snapshots = self.snapshots().await;
        let accounts = self.list_accounts()?;
        let tray_summary = tray::summarize(&snapshots);

        Ok(DashboardState {
            accounts,
            snapshots,
            tray_summary,
            settings: self.settings(),
        })
    }

    pub(crate) fn reorder_accounts(&self, ids: &[String]) -> Result<Vec<AccountView>> {
        let mut config = self.config.lock().expect("config lock");
        config.reorder(ids);
        config::save_to_path(&self.config_path, &config)?;
        Ok(config.views())
    }

    /// Begin an interactive sign-in. Creates a disabled placeholder account in an
    /// isolated CLI config dir, then spawns the CLI login in the background.
    /// Progress streams via `burnrate-login-progress`; completion/failure arrive
    /// via `burnrate-login-complete` / `burnrate-login-failed`.
    pub(crate) fn start_account_login(
        &self,
        app: AppHandle,
        provider: ProviderKind,
        label: String,
    ) -> Result<AccountView> {
        if !matches!(provider, ProviderKind::ClaudeCode | ProviderKind::Codex) {
            return Err(anyhow!(
                "Interactive sign-in is only available for Claude Code and Codex."
            ));
        }

        let id = format!("{}-{}", provider.as_str(), uuid::Uuid::new_v4().simple());
        self.login_manager.reserve(&id)?;

        let view = match self.create_pending_login(provider, &id, label) {
            Ok(view) => view,
            Err(error) => {
                self.login_manager.finish(&id);
                return Err(error);
            }
        };
        let config_dir = view
            .config_dir
            .clone()
            .expect("pending login account has a config dir");

        let app_task = app.clone();
        let id_task = id.clone();
        let handle = tokio::spawn(async move {
            let result = login::run_login(
                app_task.clone(),
                provider,
                id_task.clone(),
                config_dir,
                None,
            )
            .await;
            let state = app_task.state::<AppState>();
            state
                .finalize_login(&app_task, provider, &id_task, result)
                .await;
        });
        self.login_manager.attach(&id, handle.abort_handle());

        Ok(view)
    }

    fn create_pending_login(
        &self,
        provider: ProviderKind,
        id: &str,
        label: String,
    ) -> Result<AccountView> {
        let dir = config::account_cli_dir(provider, id)?;
        config::create_private_dir(&dir)?;
        let dir_string = dir.display().to_string();

        let mut config = self.config.lock().expect("config lock");
        let now = Utc::now();
        config.accounts.push(AccountConfig {
            id: id.to_string(),
            provider,
            label,
            enabled: false,
            auto_detected: false,
            credential_path: Some(dir_string.clone()),
            endpoint_override: None,
            secret_storage: SecretStorageMode::Keyring,
            keyring_account: None,
            plaintext_secret: None,
            email: None,
            config_dir: Some(dir_string),
            order_index: None,
            created_at: now,
            updated_at: now,
        });
        config::save_to_path(&self.config_path, &config)?;
        config
            .views()
            .into_iter()
            .find(|view| view.id == id)
            .ok_or_else(|| anyhow!("pending login account vanished"))
    }

    async fn finalize_login(
        &self,
        app: &AppHandle,
        provider: ProviderKind,
        id: &str,
        result: Result<LoginOutcome>,
    ) {
        self.login_manager.finish(id);
        match result.and_then(|outcome| self.finalize_pending_account(provider, id, outcome)) {
            Ok(view) => {
                // Emit the originating (pending) id so the UI matches even when
                // the reuse policy resolved to a different existing account.
                let _ = app.emit(
                    "burnrate-login-complete",
                    LoginComplete {
                        id: id.to_string(),
                        account: view,
                    },
                );
                if let Ok(dashboard) = self.dashboard().await {
                    tray::update_summary(app, &dashboard.tray_summary);
                    let _ = app.emit("burnrate-dashboard-updated", &dashboard);
                }
            }
            Err(error) => {
                self.discard_pending_login(id);
                let _ = app.emit(
                    "burnrate-login-failed",
                    LoginFailed {
                        id: id.to_string(),
                        error: error.to_string(),
                    },
                );
            }
        }
    }

    /// Promote a completed login to an enabled account, or — when the email
    /// already belongs to another account — refresh that one instead of creating
    /// a duplicate (reuse policy).
    fn finalize_pending_account(
        &self,
        provider: ProviderKind,
        id: &str,
        outcome: LoginOutcome,
    ) -> Result<AccountView> {
        let email = outcome.email;
        let mut config = self.config.lock().expect("config lock");

        let existing_id = email.as_deref().and_then(|email| {
            config
                .accounts
                .iter()
                .find(|account| {
                    account.provider == provider
                        && account.id != id
                        && account.email.as_deref() == Some(email)
                })
                .map(|account| account.id.clone())
        });

        let final_id = if let Some(existing_id) = existing_id {
            let pending_dir = config.remove(id).and_then(|account| account.config_dir);
            if let Some(existing) = config
                .accounts
                .iter_mut()
                .find(|account| account.id == existing_id)
            {
                let existing_managed = existing
                    .config_dir
                    .as_deref()
                    .is_some_and(|dir| config::is_managed_cli_dir(Path::new(dir)));
                if existing_managed {
                    // Adopt the freshly authenticated dir; drop the stale one.
                    let stale = existing.config_dir.take();
                    existing.config_dir = pending_dir.clone();
                    existing.credential_path = pending_dir;
                    cleanup_managed_dir(stale.as_deref());
                } else {
                    // The match is the system-default account; keep its creds and
                    // discard the throwaway login dir.
                    cleanup_managed_dir(pending_dir.as_deref());
                }
                existing.email = email;
                existing.enabled = true;
                existing.updated_at = Utc::now();
            } else {
                cleanup_managed_dir(pending_dir.as_deref());
            }
            existing_id
        } else {
            if let Some(pending) = config.accounts.iter_mut().find(|account| account.id == id) {
                pending.email = email;
                pending.enabled = true;
                pending.updated_at = Utc::now();
            }
            id.to_string()
        };

        config::save_to_path(&self.config_path, &config)?;
        config
            .views()
            .into_iter()
            .find(|view| view.id == final_id)
            .ok_or_else(|| anyhow!("signed-in account vanished"))
    }

    fn discard_pending_login(&self, id: &str) {
        let mut config = self.config.lock().expect("config lock");
        if let Some(removed) = config.remove(id) {
            cleanup_managed_dir(removed.config_dir.as_deref());
            let _ = key_store::remove_secret(&removed);
        }
        let _ = config::save_to_path(&self.config_path, &config);
    }

    pub(crate) fn cancel_account_login(&self, id: &str) -> Result<Vec<AccountView>> {
        self.login_manager.cancel(id);
        self.discard_pending_login(id);
        Ok(self.config.lock().expect("config lock").views())
    }

    pub(crate) async fn logout_account(&self, id: &str) -> Result<Vec<AccountView>> {
        let account = self
            .config
            .lock()
            .expect("config lock")
            .accounts
            .iter()
            .find(|account| account.id == id)
            .cloned();

        if let Some(account) = account {
            // Only run the CLI sign-out for dirs we manage; never touch the user's
            // system-default session (`~/.claude` / `~/.codex`).
            let managed_dir = account
                .config_dir
                .as_deref()
                .filter(|dir| config::is_managed_cli_dir(Path::new(dir)));
            if let Some(dir) = managed_dir
                && provider_supports_login(account.provider)
            {
                let _ = login::run_logout(account.provider, Some(dir)).await;
            }
            let _ = key_store::remove_secret(&account);
        }

        let mut config = self.config.lock().expect("config lock");
        if let Some(removed) = config.remove(id) {
            cleanup_managed_dir(removed.config_dir.as_deref());
        }
        config::save_to_path(&self.config_path, &config)?;
        Ok(config.views())
    }
}

fn provider_supports_login(provider: ProviderKind) -> bool {
    matches!(provider, ProviderKind::ClaudeCode | ProviderKind::Codex)
}

/// Remove a per-account CLI dir, but only if it is one Burnrate manages — never a
/// system default such as `~/.claude` or `~/.codex`.
fn cleanup_managed_dir(dir: Option<&str>) {
    if let Some(dir) = dir {
        let path = Path::new(dir);
        if config::is_managed_cli_dir(path) {
            let _ = fs::remove_dir_all(path);
        }
    }
}
