use std::{path::PathBuf, sync::Mutex};

use anyhow::Result;

use crate::{
    config::{self, AppConfig},
    key_store,
    models::AppSettings,
    models::{AccountInput, AccountView, DashboardState, UsageSnapshot},
    providers::{self, ProviderClient},
    tray,
};

pub(crate) struct AppState {
    config_path: PathBuf,
    config: Mutex<AppConfig>,
    provider_client: ProviderClient,
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
            .accounts
            .iter()
            .filter(|account| account.enabled)
            .cloned()
            .collect::<Vec<_>>();

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
        snapshots
            .into_iter()
            .map(|(_, snapshot)| snapshot)
            .collect()
    }

    pub(crate) async fn dashboard(&self) -> Result<DashboardState> {
        let accounts = self.list_accounts()?;
        let snapshots = self.snapshots().await;
        let tray_summary = tray::summarize(&snapshots);

        Ok(DashboardState {
            accounts,
            snapshots,
            tray_summary,
            settings: self.settings(),
        })
    }
}
