use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Result, anyhow};
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    config::{self, AppConfig},
    insights, key_store,
    models::AppSettings,
    models::{
        AccountConfig, AccountInput, AccountView, DashboardState, LocalUsageReport, LoginComplete,
        LoginFailed, ProviderKind, SecretStorageMode, UsageSnapshot,
    },
    providers::{
        self, ProviderClient,
        login::{self, LoginManager, LoginOutcome},
    },
    storage::ConfigStore,
    tray,
};

pub(crate) struct AppState {
    config_store: ConfigStore,
    config: Mutex<AppConfig>,
    provider_client: ProviderClient,
    login_manager: LoginManager,
    /// Last good claudex insights report. The async mutex also serializes
    /// collections, so concurrent callers (two webview windows plus the
    /// background refresh) trigger one claudex sync, mirroring the
    /// per-account provider fetch locks.
    local_usage: tokio::sync::Mutex<Option<LocalUsageReport>>,
}

impl AppState {
    pub(crate) fn load() -> Result<Self> {
        let config_store = ConfigStore::open()?;
        let mut config = config_store.load_config()?;
        let detected_changed = config.merge_detected(providers::detect_accounts());
        // Skip orphan GC when the database was just created (first launch, a
        // deleted `burnrate.sqlite`, a partial backup restore, or a recovered
        // malformed legacy config): an empty/fresh account list would classify
        // every managed dir — including live secondary-account credentials — as
        // orphaned. Collection resumes on the next launch with an established DB.
        if !config_store.created_database() {
            gc_orphaned_cli_dirs(&config);
        }
        if detected_changed {
            config_store.save_config(&config)?;
        }

        Ok(Self {
            config_store,
            config: Mutex::new(config),
            provider_client: ProviderClient::new(),
            login_manager: LoginManager::new(),
            local_usage: tokio::sync::Mutex::new(None),
        })
    }

    /// claudex-backed local usage for the enabled accounts' providers.
    /// Returns the last good report when a collection fails transiently, and
    /// a disabled-state report when the setting is off.
    pub(crate) async fn local_usage(&self) -> LocalUsageReport {
        if !self.settings().local_insights {
            return LocalUsageReport {
                available: false,
                message: Some("Local insights are disabled in Preferences.".to_string()),
                providers: Vec::new(),
                generated_at: Utc::now(),
            };
        }

        let providers: Vec<ProviderKind> = self
            .config
            .lock()
            .expect("config lock")
            .enabled_accounts_ordered()
            .iter()
            .map(|account| account.provider)
            .collect();

        let mut cached = self.local_usage.lock().await;
        let report = insights::collect_local_usage(providers).await;
        if report.available {
            *cached = Some(report.clone());
            report
        } else if let Some(previous) = cached.clone() {
            previous
        } else {
            report
        }
    }

    pub(crate) fn list_accounts(&self) -> Result<Vec<AccountView>> {
        Ok(self.config.lock().expect("config lock").views())
    }

    pub(crate) fn settings(&self) -> AppSettings {
        self.config.lock().expect("config lock").settings.clone()
    }

    pub(crate) fn save_settings(&self, settings: AppSettings) -> Result<AppSettings> {
        self.update_config(|config| {
            config.settings = settings;
            Ok(config.settings.clone())
        })
    }

    fn update_config<R>(&self, update: impl FnOnce(&mut AppConfig) -> Result<R>) -> Result<R> {
        let mut current = self.config.lock().expect("config lock");
        let mut next = current.clone();
        let result = update(&mut next)?;
        self.config_store.save_config(&next)?;
        *current = next;
        Ok(result)
    }

    fn update_config_if_changed(
        &self,
        update: impl FnOnce(&mut AppConfig) -> Result<bool>,
    ) -> Result<()> {
        let mut current = self.config.lock().expect("config lock");
        let mut next = current.clone();
        if update(&mut next)? {
            self.config_store.save_config(&next)?;
            *current = next;
        }
        Ok(())
    }

    pub(crate) fn save_account(&self, input: AccountInput) -> Result<Vec<AccountView>> {
        self.update_config(|config| {
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

            if account.provider == ProviderKind::Aws {
                key_store::clear_secret(account)?;
            } else if let Some(secret) = input.secret {
                key_store::set_secret(account, Some(secret))?;
                key_store::validate_plaintext_mode(account)?;
            } else if let Some(previous) = previous {
                key_store::migrate_secret(&previous, account)?;
            }

            Ok(config.views())
        })
    }

    pub(crate) fn detect_accounts(&self) -> Result<Vec<AccountView>> {
        let mut current = self.config.lock().expect("config lock");
        let mut next = current.clone();
        if next.merge_detected(providers::detect_accounts()) {
            let views = next.views();
            self.config_store.save_config(&next)?;
            *current = next;
            Ok(views)
        } else {
            Ok(current.views())
        }
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
        let _ = self.update_config_if_changed(|config| {
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
            Ok(changed)
        });
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
        self.update_config(|config| {
            config.reorder(ids);
            Ok(config.views())
        })
    }

    /// Begin an interactive sign-in. When `reauth_id` is `Some`, re-authenticate
    /// that existing account *in place* — refreshing its real credential location
    /// (the system default for an auto-detected account, or its managed dir) —
    /// rather than minting a throwaway one. Otherwise create a disabled
    /// placeholder account in an isolated CLI config dir for a brand-new account.
    /// Progress streams via `burnrate-login-progress`; completion/failure arrive
    /// via `burnrate-login-complete` / `burnrate-login-failed`.
    pub(crate) async fn start_account_login(
        &self,
        app: AppHandle,
        provider: ProviderKind,
        label: String,
        reauth_id: Option<String>,
    ) -> Result<AccountView> {
        if !provider_supports_login(provider) {
            return Err(anyhow!(
                "Interactive sign-in is only available for Claude Code and Codex."
            ));
        }
        login::ensure_provider_login_supported(provider).await?;

        let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel();
        let is_reauth = reauth_id.is_some();
        let (id, config_dir, email_hint, view) = match reauth_id {
            Some(reauth_id) => {
                let account = {
                    let config = self.config.lock().expect("config lock");
                    config
                        .accounts
                        .iter()
                        .find(|account| account.id == reauth_id)
                        .cloned()
                        .ok_or_else(|| anyhow!("account no longer exists"))?
                };
                // Fail closed: a browser re-auth without an isolated CLI dir would
                // run `claude auth login` / `codex login` against the system default
                // (`~/.claude` / `~/.codex`) and clobber the user's terminal session.
                // The auto-detected account is the *intended* system-default target;
                // any other account (e.g. one added via manual token entry) must have
                // its own managed dir first.
                if account.config_dir.is_none() && !account.auto_detected {
                    return Err(anyhow!(
                        "This account has no isolated CLI home, so a browser sign-in would overwrite your system-default {} session. Remove it and add it again via browser sign-in to get an isolated home.",
                        provider.display_name()
                    ));
                }
                self.login_manager
                    .reserve(&account.id, true, input_tx.clone())?;
                let view = match self.account_view(&account.id) {
                    Some(view) => view,
                    None => {
                        self.login_manager.finish(&account.id);
                        return Err(anyhow!("account no longer exists"));
                    }
                };
                (account.id, account.config_dir, account.email, view)
            }
            None => {
                let id = format!("{}-{}", provider.as_str(), uuid::Uuid::new_v4().simple());
                self.login_manager.reserve(&id, false, input_tx.clone())?;
                let view = match self.create_pending_login(provider, &id, label) {
                    Ok(view) => view,
                    Err(error) => {
                        self.login_manager.finish(&id);
                        return Err(error);
                    }
                };
                let config_dir = view.config_dir.clone();
                (id, config_dir, None, view)
            }
        };

        let app_task = app.clone();
        let id_task = id.clone();
        let handle = tokio::spawn(async move {
            let result = login::run_login(
                app_task.clone(),
                provider,
                id_task.clone(),
                config_dir,
                email_hint,
                input_rx,
            )
            .await;
            let state = app_task.state::<AppState>();
            state
                .finalize_login(&app_task, provider, &id_task, is_reauth, result)
                .await;
        });
        self.login_manager.attach(&id, handle.abort_handle());

        Ok(view)
    }

    pub(crate) fn submit_account_login_code(&self, id: &str, code: String) -> Result<()> {
        let code = code.trim();
        if code.is_empty() {
            return Err(anyhow!("Paste the authentication code first."));
        }
        self.login_manager.submit_input(id, code.to_string())
    }

    fn account_view(&self, id: &str) -> Option<AccountView> {
        self.config
            .lock()
            .expect("config lock")
            .views()
            .into_iter()
            .find(|view| view.id == id)
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

        let result = self.update_config(|config| {
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
                config_dir: Some(dir_string.clone()),
                aws_profile: None,
                aws_region: None,
                aws_monthly_budget_usd: None,
                aws_categories: Vec::new(),
                copilot_plan: None,
                copilot_custom_limit: None,
                order_index: None,
                created_at: now,
                updated_at: now,
            });
            config
                .views()
                .into_iter()
                .find(|view| view.id == id)
                .ok_or_else(|| anyhow!("pending login account vanished"))
        });
        if result.is_err() {
            cleanup_managed_dir(Some(&dir_string));
        }
        result
    }

    async fn finalize_login(
        &self,
        app: &AppHandle,
        provider: ProviderKind,
        id: &str,
        is_reauth: bool,
        result: Result<LoginOutcome>,
    ) {
        self.login_manager.finish(id);
        let promoted = result.and_then(|outcome| {
            if is_reauth {
                self.refresh_signed_in_account(id, outcome)
            } else {
                self.finalize_pending_account(provider, id, outcome)
            }
        });
        match promoted {
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
                // A failed re-auth keeps the existing account untouched; only a
                // failed brand-new sign-in discards its throwaway placeholder.
                if !is_reauth {
                    self.discard_pending_login(id);
                }
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

    /// Refresh an existing account after a successful in-place re-auth: its
    /// credentials were rewritten at its own location, so only metadata changes.
    fn refresh_signed_in_account(&self, id: &str, outcome: LoginOutcome) -> Result<AccountView> {
        self.update_config(|config| {
            let account = config
                .accounts
                .iter_mut()
                .find(|account| account.id == id)
                .ok_or_else(|| anyhow!("account no longer exists"))?;
            if let Some(email) = outcome.email {
                account.email = Some(email);
            }
            account.enabled = true;
            account.updated_at = Utc::now();
            config
                .views()
                .into_iter()
                .find(|view| view.id == id)
                .ok_or_else(|| anyhow!("account no longer exists"))
        })
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
        let mut cleanup_dirs: Vec<Option<String>> = Vec::new();
        #[cfg(target_os = "macos")]
        let mut delete_keychain_dirs: Vec<Option<String>> = Vec::new();
        let view = self.update_config(|config| {
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
                // The placeholder vanishing mid-flight (the user removed it while
                // the browser flow was open) aborts the merge: adopting or
                // discarding state for an account that no longer exists would at
                // best corrupt the existing account's dir binding — and a `None`
                // pending dir would key the *system-default* Keychain credential.
                // `update_config` rolls back on error; any dir the CLI re-created
                // on disk is collected by the next startup GC.
                let pending = config.remove(id).ok_or_else(|| {
                    anyhow!("sign-in discarded because the account was removed during login")
                })?;
                let pending_dir = pending.config_dir;
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
                        // Adopt the freshly authenticated dir; drop the stale one
                        // (including its macOS Keychain credential, which is keyed by
                        // the dir and would otherwise be orphaned).
                        let stale = existing.config_dir.take();
                        existing.config_dir = pending_dir.clone();
                        existing.credential_path = pending_dir;
                        #[cfg(target_os = "macos")]
                        if provider == ProviderKind::ClaudeCode {
                            delete_keychain_dirs.push(stale.clone());
                        }
                        cleanup_dirs.push(stale);
                    } else {
                        // The match is the system-default account: keep its creds and
                        // fully discard the throwaway login dir AND its macOS Keychain
                        // credential (keyed by the dir), which would otherwise be
                        // orphaned and accumulate on every same-account sign-in.
                        #[cfg(target_os = "macos")]
                        if provider == ProviderKind::ClaudeCode {
                            delete_keychain_dirs.push(pending_dir.clone());
                        }
                        cleanup_dirs.push(pending_dir);
                    }
                    existing.email = email;
                    existing.enabled = true;
                    existing.updated_at = Utc::now();
                } else {
                    #[cfg(target_os = "macos")]
                    if provider == ProviderKind::ClaudeCode {
                        delete_keychain_dirs.push(pending_dir.clone());
                    }
                    cleanup_dirs.push(pending_dir);
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

            config
                .views()
                .into_iter()
                .find(|view| view.id == final_id)
                .ok_or_else(|| anyhow!("signed-in account vanished"))
        })?;
        #[cfg(target_os = "macos")]
        for dir in delete_keychain_dirs {
            login::delete_claude_keychain_for_dir(dir.as_deref());
        }
        for dir in cleanup_dirs {
            cleanup_managed_dir(dir.as_deref());
        }
        Ok(view)
    }

    fn discard_pending_login(&self, id: &str) {
        let mut removed = None;
        let result = self.update_config(|config| {
            removed = config.remove(id);
            Ok(())
        });
        if result.is_ok()
            && let Some(removed) = removed
        {
            // A failed Claude sign-in may have written a Keychain entry (keyed by
            // the now-discarded dir) before erroring; drop it so it is not orphaned.
            #[cfg(target_os = "macos")]
            if removed.provider == ProviderKind::ClaudeCode {
                login::delete_claude_keychain(&removed);
            }
            cleanup_managed_dir(removed.config_dir.as_deref());
            let _ = key_store::remove_secret(&removed);
        }
    }

    /// Cancel an in-progress sign-in. Returns `(canceled, accounts)` where
    /// `canceled` is true only when an active login was actually aborted — a late
    /// cancel that races a completing login returns false so the caller does not
    /// emit a spurious failure over the success.
    pub(crate) fn cancel_account_login(&self, id: &str) -> Result<(bool, Vec<AccountView>)> {
        let outcome = self.login_manager.cancel(id);
        // Only tear down the placeholder when we canceled an *active* brand-new
        // sign-in; a re-auth (`Some(true)`) or a raced cancel (`None`) leaves the
        // account in place.
        if outcome == Some(false) {
            self.discard_pending_login(id);
        }
        let accounts = self.config.lock().expect("config lock").views();
        Ok((outcome.is_some(), accounts))
    }

    /// Sign out and remove an account. Browser-login (managed) Claude/Codex
    /// accounts also have their CLI credential store cleared; OpenRouter/Runpod
    /// and the auto-detected system-default account only drop their Burnrate
    /// record (never the user's `~/.claude` / `~/.codex` session).
    pub(crate) async fn logout_account(&self, id: &str) -> Result<Vec<AccountView>> {
        self.teardown_and_remove(id).await
    }

    pub(crate) async fn remove_account(&self, id: &str) -> Result<Vec<AccountView>> {
        self.teardown_and_remove(id).await
    }

    async fn teardown_and_remove(&self, id: &str) -> Result<Vec<AccountView>> {
        let account = self
            .config
            .lock()
            .expect("config lock")
            .accounts
            .iter()
            .find(|account| account.id == id)
            .cloned();

        let views = self.update_config(|config| {
            config.remove(id);
            Ok(config.views())
        })?;

        if let Some(account) = account {
            self.teardown_managed_credentials(&account).await;
        }

        Ok(views)
    }

    /// Clear an account's credentials. For a managed Claude/Codex dir this runs
    /// the CLI sign-out, deletes the orphan-prone macOS Keychain entry as a
    /// fallback, and removes the dir; for every account it drops any Burnrate
    /// keyring secret. The user's system-default session is never touched.
    async fn teardown_managed_credentials(&self, account: &AccountConfig) {
        let managed_dir = account
            .config_dir
            .as_deref()
            .filter(|dir| config::is_managed_cli_dir(Path::new(dir)));
        if let Some(dir) = managed_dir
            && provider_supports_login(account.provider)
        {
            if let Err(error) = login::run_logout(account.provider, Some(dir)).await {
                eprintln!(
                    "Burnrate: CLI sign-out for account {} did not complete: {error}",
                    account.id
                );
            }
            #[cfg(target_os = "macos")]
            if account.provider == ProviderKind::ClaudeCode {
                login::delete_claude_keychain(account);
            }
            cleanup_managed_dir(Some(dir));
        }
        let _ = key_store::remove_secret(account);
    }
}

fn provider_supports_login(provider: ProviderKind) -> bool {
    matches!(provider, ProviderKind::ClaudeCode | ProviderKind::Codex)
}

/// Remove managed CLI dirs under `<config_dir>/cli/<provider>/*` that no account
/// references — leftovers from interrupted, discarded, or merged sign-ins (these
/// would otherwise accumulate, and a stale Claude dir keeps an orphaned Keychain
/// credential). Guarded by `is_managed_cli_dir`, so the system defaults
/// (`~/.claude` / `~/.codex`) can never be touched.
fn gc_orphaned_cli_dirs(config: &AppConfig) {
    // Compare both the recorded spelling and the canonicalized path: the
    // persisted `config_dir` strings may differ from this run's scan through a
    // symlink (`/var` vs `/private/var`) or a respelled `BURNRATE_CONFIG_DIR`,
    // and a false "unreferenced" verdict here deletes live credentials.
    let mut referenced: HashSet<PathBuf> = HashSet::new();
    for dir in config
        .accounts
        .iter()
        .filter_map(|account| account.config_dir.as_deref())
    {
        if let Ok(canonical) = fs::canonicalize(dir) {
            referenced.insert(canonical);
        }
        referenced.insert(PathBuf::from(dir));
    }
    for dir in config::existing_managed_cli_dirs() {
        let canonical = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if referenced.contains(&dir)
            || referenced.contains(&canonical)
            || !config::is_managed_cli_dir(&dir)
        {
            continue;
        }
        #[cfg(target_os = "macos")]
        if dir
            .parent()
            .and_then(|parent| parent.file_name())
            .is_some_and(|name| name == ProviderKind::ClaudeCode.as_str())
        {
            login::delete_claude_keychain_for_dir(Some(dir.to_string_lossy().as_ref()));
        }
        let _ = fs::remove_dir_all(&dir);
    }
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
