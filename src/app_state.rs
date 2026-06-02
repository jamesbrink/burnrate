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
        let mut config = config::load_from_path(&config_path)?;
        config.merge_detected(providers::detect_accounts());
        config::save_to_path(&config_path, &config)?;

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
        config.merge_detected(providers::detect_accounts());
        config::save_to_path(&self.config_path, &config)?;
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

        let mut tasks = tokio::task::JoinSet::new();
        for (index, account) in accounts.into_iter().enumerate() {
            let provider_client = self.provider_client.clone();
            tasks.spawn(async move { (index, provider_client.refresh_account(&account).await) });
        }

        let mut snapshots = Vec::with_capacity(tasks.len());
        while let Some(result) = tasks.join_next().await {
            if let Ok(snapshot) = result {
                snapshots.push(snapshot);
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
