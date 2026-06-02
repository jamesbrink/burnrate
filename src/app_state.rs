use std::{path::PathBuf, sync::Mutex};

use anyhow::Result;

use crate::{
    config::{self, AppConfig},
    key_store,
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

    pub(crate) fn save_account(&self, input: AccountInput) -> Result<Vec<AccountView>> {
        let mut config = self.config.lock().expect("config lock");
        let account = config.upsert_manual(input.clone());

        if let Some(secret) = input.secret {
            let account = config
                .accounts
                .iter_mut()
                .find(|item| item.id == account.id)
                .expect("upserted account exists");
            key_store::set_secret(account, Some(secret))?;
            key_store::validate_plaintext_mode(account)?;
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

        let mut snapshots = Vec::with_capacity(accounts.len());
        for account in accounts {
            snapshots.push(self.provider_client.refresh_account(&account).await);
        }
        snapshots
    }

    pub(crate) async fn dashboard(&self) -> Result<DashboardState> {
        let accounts = self.list_accounts()?;
        let snapshots = self.snapshots().await;
        let tray_summary = tray::summarize(&snapshots);

        Ok(DashboardState {
            accounts,
            snapshots,
            tray_summary,
        })
    }
}
