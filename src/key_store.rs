use anyhow::{Context, Result, bail};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use crate::models::{AccountConfig, SecretStorageMode};

const SERVICE: &str = "burnrate";
static KEYRING_SECRET_CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

pub(crate) fn set_secret(account: &mut AccountConfig, secret: Option<String>) -> Result<()> {
    let Some(secret) = secret else {
        return Ok(());
    };

    match account.secret_storage {
        SecretStorageMode::Keyring => {
            let keyring_account = format!("{}:{}", account.provider.as_str(), account.id);
            let entry = keyring::Entry::new(SERVICE, &keyring_account)
                .context("failed to open OS keyring entry")?;
            entry
                .set_password(&secret)
                .context("failed to save secret in OS keyring")?;
            remember_keyring_secret(&keyring_account, Some(secret));
            account.keyring_account = Some(keyring_account);
            account.plaintext_secret = None;
        }
        SecretStorageMode::Plaintext => {
            if let Some(keyring_account) = &account.keyring_account {
                forget_keyring_secret(keyring_account);
            }
            account.plaintext_secret = Some(secret);
            account.keyring_account = None;
        }
    }

    Ok(())
}

pub(crate) fn migrate_secret(previous: &AccountConfig, account: &mut AccountConfig) -> Result<()> {
    if previous.secret_storage == account.secret_storage {
        return Ok(());
    }

    let Some(secret) = get_secret(previous)? else {
        clear_secret_refs(account);
        return Ok(());
    };

    set_secret(account, Some(secret))?;
    remove_secret(previous)?;
    Ok(())
}

fn clear_secret_refs(account: &mut AccountConfig) {
    if let Some(keyring_account) = &account.keyring_account {
        forget_keyring_secret(keyring_account);
    }
    account.keyring_account = None;
    account.plaintext_secret = None;
}

pub(crate) fn get_secret(account: &AccountConfig) -> Result<Option<String>> {
    match account.secret_storage {
        SecretStorageMode::Keyring => {
            let Some(keyring_account) = &account.keyring_account else {
                return Ok(None);
            };
            if let Some(secret) = cached_keyring_secret(keyring_account) {
                return Ok(secret);
            }
            let entry = keyring::Entry::new(SERVICE, keyring_account)
                .context("failed to open OS keyring entry")?;
            match entry.get_password() {
                Ok(secret) => {
                    remember_keyring_secret(keyring_account, Some(secret.clone()));
                    Ok(Some(secret))
                }
                Err(keyring::Error::NoEntry) => {
                    remember_keyring_secret(keyring_account, None);
                    Ok(None)
                }
                Err(error) => Err(error).context("failed to read secret from OS keyring"),
            }
        }
        SecretStorageMode::Plaintext => Ok(account.plaintext_secret.clone()),
    }
}

pub(crate) fn remove_secret(account: &AccountConfig) -> Result<()> {
    if account.secret_storage == SecretStorageMode::Plaintext {
        return Ok(());
    }

    let Some(keyring_account) = &account.keyring_account else {
        return Ok(());
    };
    let entry =
        keyring::Entry::new(SERVICE, keyring_account).context("failed to open OS keyring entry")?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            forget_keyring_secret(keyring_account);
            Ok(())
        }
        Err(error) => Err(error).context("failed to remove secret from OS keyring"),
    }
}

pub(crate) fn validate_plaintext_mode(account: &AccountConfig) -> Result<()> {
    if account.secret_storage == SecretStorageMode::Plaintext && account.plaintext_secret.is_none()
    {
        bail!("plaintext mode is enabled but no plaintext secret is stored");
    }
    Ok(())
}

fn secret_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    KEYRING_SECRET_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_keyring_secret(keyring_account: &str) -> Option<Option<String>> {
    secret_cache()
        .lock()
        .expect("keyring secret cache poisoned")
        .get(keyring_account)
        .cloned()
}

fn remember_keyring_secret(keyring_account: &str, secret: Option<String>) {
    secret_cache()
        .lock()
        .expect("keyring secret cache poisoned")
        .insert(keyring_account.to_string(), secret);
}

fn forget_keyring_secret(keyring_account: &str) {
    secret_cache()
        .lock()
        .expect("keyring secret cache poisoned")
        .remove(keyring_account);
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::models::{ProviderKind, SecretStorageMode};

    fn account(mode: SecretStorageMode) -> AccountConfig {
        AccountConfig {
            id: "openrouter-main".to_string(),
            provider: ProviderKind::OpenRouter,
            label: "OpenRouter".to_string(),
            enabled: true,
            auto_detected: false,
            credential_path: None,
            endpoint_override: None,
            secret_storage: mode,
            keyring_account: None,
            plaintext_secret: None,
            email: None,
            config_dir: None,
            order_index: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn plaintext_mode_stores_secret_in_config() {
        let mut account = account(SecretStorageMode::Plaintext);
        set_secret(&mut account, Some("sk-test".to_string())).unwrap();

        assert_eq!(get_secret(&account).unwrap(), Some("sk-test".to_string()));
        assert!(account.keyring_account.is_none());
    }

    #[test]
    fn plaintext_validation_requires_explicit_secret() {
        let account = account(SecretStorageMode::Plaintext);
        assert!(validate_plaintext_mode(&account).is_err());
    }

    #[test]
    fn migration_clears_stale_refs_when_previous_secret_is_missing() {
        let previous = account(SecretStorageMode::Plaintext);
        let mut next = account(SecretStorageMode::Keyring);
        next.keyring_account = Some("stale".to_string());

        migrate_secret(&previous, &mut next).unwrap();

        assert!(next.keyring_account.is_none());
        assert!(next.plaintext_secret.is_none());
    }

    #[test]
    fn keyring_secret_cache_tracks_hits_misses_and_forgets() {
        let keyring_account = "test-cache-entry";
        forget_keyring_secret(keyring_account);

        assert_eq!(cached_keyring_secret(keyring_account), None);

        remember_keyring_secret(keyring_account, Some("sk-test".to_string()));
        assert_eq!(
            cached_keyring_secret(keyring_account),
            Some(Some("sk-test".to_string()))
        );

        remember_keyring_secret(keyring_account, None);
        assert_eq!(cached_keyring_secret(keyring_account), Some(None));

        forget_keyring_secret(keyring_account);
        assert_eq!(cached_keyring_secret(keyring_account), None);
    }
}
