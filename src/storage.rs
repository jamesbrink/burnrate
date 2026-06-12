use std::{
    ffi::OsString,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    config::{self, AppConfig},
    models::{AccountConfig, AppSettings, AwsCategoryConfig, AwsCostFilter, AwsGroupBy},
};

const LATEST_SCHEMA_VERSION: i64 = 2;

pub(crate) struct ConfigStore {
    conn: Mutex<Connection>,
    db_path: PathBuf,
    created_database: bool,
}

impl ConfigStore {
    pub(crate) fn open() -> Result<Self> {
        Self::open_at(config::database_path()?, config::config_path()?)
    }

    pub(crate) fn open_at(db_path: PathBuf, legacy_json_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            config::create_private_dir(parent)?;
        }

        let created_database = if db_path.exists() {
            false
        } else {
            create_private_database_file(&db_path)?
        };
        let result = Self::open_at_inner(&db_path, &legacy_json_path, created_database);
        if result.is_err() && created_database {
            cleanup_database_files(&db_path);
        }
        result
    }

    fn open_at_inner(
        db_path: &Path,
        legacy_json_path: &Path,
        is_new_database: bool,
    ) -> Result<Self> {
        let mut conn = Connection::open(db_path)
            .with_context(|| format!("failed to open {}", db_path.display()))?;
        harden_database_files(db_path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // A pre-existing but schema-less file (0-byte or truncated restore)
        // is just as "new" as a created one: migrations will build the schema
        // from scratch, so legacy import must run and destructive startup
        // reconciliation must be skipped, exactly as for a fresh file.
        let schema_version: i64 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let is_new_database = is_new_database || schema_version < 1;
        run_migrations(&mut conn)?;

        if is_new_database && legacy_json_path.exists() {
            import_legacy_json(&mut conn, legacy_json_path)?;
        }
        harden_database_files(db_path)?;

        Ok(Self {
            conn: Mutex::new(conn),
            db_path: db_path.to_path_buf(),
            created_database: is_new_database,
        })
    }

    /// True when this open created the database file (first launch, a deleted
    /// `burnrate.sqlite`, or a partial backup restore). Callers use it to skip
    /// destructive reconciliation that assumes an established account list.
    pub(crate) fn created_database(&self) -> bool {
        self.created_database
    }

    pub(crate) fn load_config(&self) -> Result<AppConfig> {
        let conn = self.conn.lock().expect("config store lock");
        load_config_from_conn(&conn)
    }

    pub(crate) fn save_config(&self, config: &AppConfig) -> Result<()> {
        let mut conn = self.conn.lock().expect("config store lock");
        let tx = conn.transaction()?;
        save_config_tx(&tx, config)?;
        tx.commit()?;
        harden_database_files(&self.db_path)?;
        Ok(())
    }
}

#[cfg(unix)]
fn create_private_database_file(path: &Path) -> Result<bool> {
    use std::os::unix::fs::OpenOptionsExt;

    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to create {}", path.display())),
    }
}

#[cfg(not(unix))]
fn create_private_database_file(path: &Path) -> Result<bool> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to create {}", path.display())),
    }
}

fn database_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut file_name = OsString::from(
        path.file_name()
            .expect("database path should include a file name"),
    );
    file_name.push(suffix);
    path.with_file_name(file_name)
}

fn database_file_paths(path: &Path) -> [PathBuf; 3] {
    [
        path.to_path_buf(),
        database_sidecar_path(path, "-wal"),
        database_sidecar_path(path, "-shm"),
    ]
}

fn harden_database_files(path: &Path) -> Result<()> {
    for file_path in database_file_paths(path) {
        if file_path.exists() {
            config::set_private_file_permissions(&file_path)?;
        }
    }
    Ok(())
}

fn cleanup_database_files(path: &Path) {
    for file_path in database_file_paths(path) {
        let _ = fs::remove_file(file_path);
    }
}

fn run_migrations(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > LATEST_SCHEMA_VERSION {
        bail!(
            "config database schema version {version} is newer than this Burnrate build supports ({LATEST_SCHEMA_VERSION}); a newer Burnrate (or a dev build) wrote this database — update Burnrate to the latest version"
        );
    }
    if version < 1 {
        let tx = conn.transaction()?;
        tx.execute_batch(
            r#"
            CREATE TABLE app_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                hide_from_dock INTEGER NOT NULL CHECK (hide_from_dock IN (0, 1)),
                update_channel TEXT NOT NULL,
                tray_scale REAL NOT NULL
            );

            CREATE TABLE accounts (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                auto_detected INTEGER NOT NULL CHECK (auto_detected IN (0, 1)),
                credential_path TEXT,
                endpoint_override TEXT,
                secret_storage TEXT NOT NULL,
                keyring_account TEXT,
                plaintext_secret TEXT,
                email TEXT,
                config_dir TEXT,
                aws_profile TEXT,
                aws_region TEXT,
                aws_monthly_budget_usd REAL,
                order_index INTEGER,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE aws_categories (
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                position INTEGER NOT NULL,
                id TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                filter_json TEXT NOT NULL,
                group_by_json TEXT,
                PRIMARY KEY (account_id, id)
            );

            PRAGMA user_version = 1;
            "#,
        )?;
        tx.commit()?;
    }
    if version < 2 {
        let tx = conn.transaction()?;
        tx.execute_batch(
            r#"
            ALTER TABLE accounts ADD COLUMN copilot_plan TEXT;
            ALTER TABLE accounts ADD COLUMN copilot_custom_limit REAL;
            ALTER TABLE app_settings ADD COLUMN local_insights INTEGER NOT NULL DEFAULT 1
                CHECK (local_insights IN (0, 1));

            PRAGMA user_version = 2;
            "#,
        )?;
        tx.commit()?;
    }
    Ok(())
}

fn import_legacy_json(conn: &mut Connection, legacy_json_path: &Path) -> Result<()> {
    let (config, _) = config::load_or_recover_from_path(legacy_json_path)?;
    let tx = conn.transaction()?;
    save_config_tx(&tx, &config)?;
    tx.commit()?;

    if legacy_json_path.exists() {
        let migrated_path = migrated_config_path(legacy_json_path)?;
        fs::rename(legacy_json_path, &migrated_path).with_context(|| {
            format!(
                "failed to move imported config {} to {}",
                legacy_json_path.display(),
                migrated_path.display()
            )
        })?;
    }

    Ok(())
}

fn migrated_config_path(path: &Path) -> Result<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX epoch")?
        .as_nanos();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(config::CONFIG_FILE);
    Ok(path.with_file_name(format!("{name}.migrated-{nonce}")))
}

fn load_config_from_conn(conn: &Connection) -> Result<AppConfig> {
    let settings = load_settings(conn)?;
    let accounts = load_accounts(conn)?;
    Ok(AppConfig { settings, accounts })
}

fn load_settings(conn: &Connection) -> Result<AppSettings> {
    let row = conn
        .query_row(
            "SELECT hide_from_dock, update_channel, tray_scale, local_insights FROM app_settings WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;

    let Some((hide_from_dock, update_channel, tray_scale, local_insights)) = row else {
        return Ok(AppSettings::default());
    };

    Ok(AppSettings {
        hide_from_dock: int_to_bool(hide_from_dock),
        update_channel: from_wire(&update_channel)?,
        tray_scale,
        local_insights: int_to_bool(local_insights),
    })
}

fn load_accounts(conn: &Connection) -> Result<Vec<AccountConfig>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            id,
            provider,
            label,
            enabled,
            auto_detected,
            credential_path,
            endpoint_override,
            secret_storage,
            keyring_account,
            plaintext_secret,
            email,
            config_dir,
            aws_profile,
            aws_region,
            aws_monthly_budget_usd,
            copilot_plan,
            copilot_custom_limit,
            order_index,
            created_at,
            updated_at
        FROM accounts
        ORDER BY rowid
        "#,
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(AccountRow {
                id: row.get(0)?,
                provider: row.get(1)?,
                label: row.get(2)?,
                enabled: row.get(3)?,
                auto_detected: row.get(4)?,
                credential_path: row.get(5)?,
                endpoint_override: row.get(6)?,
                secret_storage: row.get(7)?,
                keyring_account: row.get(8)?,
                plaintext_secret: row.get(9)?,
                email: row.get(10)?,
                config_dir: row.get(11)?,
                aws_profile: row.get(12)?,
                aws_region: row.get(13)?,
                aws_monthly_budget_usd: row.get(14)?,
                copilot_plan: row.get(15)?,
                copilot_custom_limit: row.get(16)?,
                order_index: row.get(17)?,
                created_at: row.get(18)?,
                updated_at: row.get(19)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    rows.into_iter().map(|row| row.into_account(conn)).collect()
}

struct AccountRow {
    id: String,
    provider: String,
    label: String,
    enabled: i64,
    auto_detected: i64,
    credential_path: Option<String>,
    endpoint_override: Option<String>,
    secret_storage: String,
    keyring_account: Option<String>,
    plaintext_secret: Option<String>,
    email: Option<String>,
    config_dir: Option<String>,
    aws_profile: Option<String>,
    aws_region: Option<String>,
    aws_monthly_budget_usd: Option<f64>,
    copilot_plan: Option<String>,
    copilot_custom_limit: Option<f64>,
    order_index: Option<i64>,
    created_at: String,
    updated_at: String,
}

impl AccountRow {
    fn into_account(self, conn: &Connection) -> Result<AccountConfig> {
        let aws_categories = load_aws_categories(conn, &self.id)?;
        Ok(AccountConfig {
            id: self.id,
            provider: from_wire(&self.provider)?,
            label: self.label,
            enabled: int_to_bool(self.enabled),
            auto_detected: int_to_bool(self.auto_detected),
            credential_path: self.credential_path,
            endpoint_override: self.endpoint_override,
            secret_storage: from_wire(&self.secret_storage)?,
            keyring_account: self.keyring_account,
            plaintext_secret: self.plaintext_secret,
            email: self.email,
            config_dir: self.config_dir,
            aws_profile: self.aws_profile,
            aws_region: self.aws_region,
            aws_monthly_budget_usd: self.aws_monthly_budget_usd,
            aws_categories,
            copilot_plan: self.copilot_plan.as_deref().map(from_wire).transpose()?,
            copilot_custom_limit: self.copilot_custom_limit,
            order_index: self.order_index,
            created_at: parse_timestamp(&self.created_at)?,
            updated_at: parse_timestamp(&self.updated_at)?,
        })
    }
}

fn load_aws_categories(conn: &Connection, account_id: &str) -> Result<Vec<AwsCategoryConfig>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, label, enabled, filter_json, group_by_json
        FROM aws_categories
        WHERE account_id = ?1
        ORDER BY position, rowid
        "#,
    )?;
    let rows = stmt
        .query_map([account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    rows.into_iter()
        .map(|(id, label, enabled, filter_json, group_by_json)| {
            Ok(AwsCategoryConfig {
                id,
                label,
                enabled: int_to_bool(enabled),
                filter: serde_json::from_str::<AwsCostFilter>(&filter_json)?,
                group_by: group_by_json
                    .map(|json| serde_json::from_str::<AwsGroupBy>(&json))
                    .transpose()?,
            })
        })
        .collect()
}

fn save_config_tx(tx: &Transaction<'_>, config: &AppConfig) -> Result<()> {
    tx.execute("DELETE FROM aws_categories", [])?;
    tx.execute("DELETE FROM accounts", [])?;
    tx.execute("DELETE FROM app_settings", [])?;

    tx.execute(
        r#"
        INSERT INTO app_settings (id, hide_from_dock, update_channel, tray_scale, local_insights)
        VALUES (1, ?1, ?2, ?3, ?4)
        "#,
        params![
            bool_to_int(config.settings.hide_from_dock),
            to_wire(&config.settings.update_channel)?,
            config.settings.tray_scale,
            bool_to_int(config.settings.local_insights),
        ],
    )?;

    for account in &config.accounts {
        tx.execute(
            r#"
            INSERT INTO accounts (
                id,
                provider,
                label,
                enabled,
                auto_detected,
                credential_path,
                endpoint_override,
                secret_storage,
                keyring_account,
                plaintext_secret,
                email,
                config_dir,
                aws_profile,
                aws_region,
                aws_monthly_budget_usd,
                copilot_plan,
                copilot_custom_limit,
                order_index,
                created_at,
                updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
            "#,
            params![
                account.id,
                to_wire(&account.provider)?,
                account.label,
                bool_to_int(account.enabled),
                bool_to_int(account.auto_detected),
                account.credential_path,
                account.endpoint_override,
                to_wire(&account.secret_storage)?,
                account.keyring_account,
                account.plaintext_secret,
                account.email,
                account.config_dir,
                account.aws_profile,
                account.aws_region,
                account.aws_monthly_budget_usd,
                account.copilot_plan.as_ref().map(to_wire).transpose()?,
                account.copilot_custom_limit,
                account.order_index,
                format_timestamp(&account.created_at),
                format_timestamp(&account.updated_at),
            ],
        )?;

        for (position, category) in account.aws_categories.iter().enumerate() {
            tx.execute(
                r#"
                INSERT INTO aws_categories (
                    account_id,
                    position,
                    id,
                    label,
                    enabled,
                    filter_json,
                    group_by_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    account.id,
                    position as i64,
                    category.id,
                    category.label,
                    bool_to_int(category.enabled),
                    serde_json::to_string(&category.filter)?,
                    category
                        .group_by
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()?,
                ],
            )?;
        }
    }

    Ok(())
}

fn bool_to_int(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn int_to_bool(value: i64) -> bool {
    value != 0
}

fn to_wire<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(value) => Ok(value),
        other => Err(anyhow!(
            "expected wire value to serialize as string, got {other}"
        )),
    }
}

fn from_wire<T: DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .with_context(|| format!("failed to parse wire value {value:?}"))
}

fn format_timestamp(value: &DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("failed to parse timestamp {value:?}"))?
        .with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::tempdir;

    use super::*;
    use crate::models::{
        AccountInput, AwsCostFilter, AwsGroupByKind, CopilotPlan, ProviderKind, SecretStorageMode,
        UpdateChannel,
    };

    #[test]
    fn creates_empty_database_at_latest_schema_version() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join(config::DATABASE_FILE);
        let json_path = dir.path().join(config::CONFIG_FILE);

        let store = ConfigStore::open_at(db_path.clone(), json_path).unwrap();

        assert!(store.load_config().unwrap().accounts.is_empty());
        let conn = Connection::open(db_path).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn rejects_future_schema_version_without_mutating_database() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join(config::DATABASE_FILE);
        let json_path = dir.path().join(config::CONFIG_FILE);
        let conn = Connection::open(&db_path).unwrap();
        conn.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION + 1)
            .unwrap();
        drop(conn);

        let error = match ConfigStore::open_at(db_path.clone(), json_path) {
            Ok(_) => panic!("future schema version should be rejected"),
            Err(error) => error,
        };

        let message = error.to_string();
        assert!(message.contains("newer than this Burnrate build"));
        // The message reaches the fatal startup dialog, so it must tell the
        // user how to get unstuck, not just what went wrong.
        assert!(message.contains("update Burnrate"));
        let conn = Connection::open(db_path).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION + 1);
    }

    #[test]
    fn saves_and_loads_full_config_snapshot() {
        let dir = tempdir().unwrap();
        let store = ConfigStore::open_at(
            dir.path().join(config::DATABASE_FILE),
            dir.path().join(config::CONFIG_FILE),
        )
        .unwrap();
        let mut config = AppConfig {
            settings: AppSettings {
                hide_from_dock: false,
                update_channel: UpdateChannel::Nightly,
                tray_scale: 0.75,
                local_insights: false,
            },
            accounts: Vec::new(),
        };
        let account = config.upsert_manual(AccountInput {
            id: Some("aws-main".to_string()),
            provider: ProviderKind::Aws,
            label: "AWS".to_string(),
            enabled: true,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Keyring,
            secret: None,
            aws_profile: Some("dev.urandom.io".to_string()),
            aws_region: Some("us-west-1".to_string()),
            aws_monthly_budget_usd: Some(150.0),
            aws_categories: vec![AwsCategoryConfig {
                id: "bedrock".to_string(),
                label: "Bedrock".to_string(),
                enabled: true,
                filter: AwsCostFilter::Dimension {
                    key: "SERVICE".to_string(),
                    values: vec!["Amazon Bedrock".to_string()],
                },
                group_by: Some(AwsGroupBy {
                    kind: AwsGroupByKind::Dimension,
                    key: "LINKED_ACCOUNT".to_string(),
                }),
            }],
            copilot_plan: None,
            copilot_custom_limit: None,
        });
        config.accounts[0].email = Some("aws@example.test".to_string());
        config.accounts[0].order_index = Some(2);
        assert_eq!(account.provider, ProviderKind::Aws);

        store.save_config(&config).unwrap();
        let loaded = store.load_config().unwrap();

        assert!(!loaded.settings.hide_from_dock);
        assert_eq!(loaded.settings.update_channel, UpdateChannel::Nightly);
        assert_eq!(loaded.settings.tray_scale, 0.75);
        assert!(!loaded.settings.local_insights);
        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].provider, ProviderKind::Aws);
        assert_eq!(
            loaded.accounts[0].aws_profile.as_deref(),
            Some("dev.urandom.io")
        );
        assert_eq!(loaded.accounts[0].aws_categories.len(), 1);
        assert_eq!(
            loaded.accounts[0].aws_categories[0]
                .group_by
                .as_ref()
                .unwrap()
                .key,
            "LINKED_ACCOUNT"
        );
    }

    #[test]
    fn copilot_account_round_trips_plan_and_custom_limit() {
        let dir = tempdir().unwrap();
        let store = ConfigStore::open_at(
            dir.path().join(config::DATABASE_FILE),
            dir.path().join(config::CONFIG_FILE),
        )
        .unwrap();
        let mut config = AppConfig::default();
        config.upsert_manual(AccountInput {
            id: Some("copilot-local".to_string()),
            provider: ProviderKind::Copilot,
            label: "GitHub Copilot".to_string(),
            enabled: true,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Keyring,
            secret: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
            copilot_plan: Some(CopilotPlan::Custom),
            copilot_custom_limit: Some(2500.0),
        });

        store.save_config(&config).unwrap();
        let loaded = store.load_config().unwrap();

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].provider, ProviderKind::Copilot);
        assert_eq!(loaded.accounts[0].copilot_plan, Some(CopilotPlan::Custom));
        assert_eq!(loaded.accounts[0].copilot_custom_limit, Some(2500.0));
    }

    /// Build a database exactly as schema v1 wrote it (pre-Copilot, no
    /// `local_insights`), with one settings row and one account row.
    fn create_v1_database(db_path: &Path) {
        let conn = Connection::open(db_path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE app_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                hide_from_dock INTEGER NOT NULL CHECK (hide_from_dock IN (0, 1)),
                update_channel TEXT NOT NULL,
                tray_scale REAL NOT NULL
            );

            CREATE TABLE accounts (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                auto_detected INTEGER NOT NULL CHECK (auto_detected IN (0, 1)),
                credential_path TEXT,
                endpoint_override TEXT,
                secret_storage TEXT NOT NULL,
                keyring_account TEXT,
                plaintext_secret TEXT,
                email TEXT,
                config_dir TEXT,
                aws_profile TEXT,
                aws_region TEXT,
                aws_monthly_budget_usd REAL,
                order_index INTEGER,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE aws_categories (
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                position INTEGER NOT NULL,
                id TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                filter_json TEXT NOT NULL,
                group_by_json TEXT,
                PRIMARY KEY (account_id, id)
            );

            INSERT INTO app_settings (id, hide_from_dock, update_channel, tray_scale)
            VALUES (1, 0, 'nightly', 0.75);

            INSERT INTO accounts (
                id, provider, label, enabled, auto_detected, secret_storage,
                created_at, updated_at
            )
            VALUES (
                'claude-code-local', 'claude-code', 'Claude Code', 1, 1, 'keyring',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
            );

            PRAGMA user_version = 1;
            "#,
        )
        .unwrap();
    }

    #[test]
    fn migrates_v1_database_preserving_rows_and_defaulting_new_fields() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join(config::DATABASE_FILE);
        create_v1_database(&db_path);

        let store =
            ConfigStore::open_at(db_path.clone(), dir.path().join(config::CONFIG_FILE)).unwrap();

        // An established v1 database is not "new": destructive startup
        // reconciliation must still run for it.
        assert!(!store.created_database());

        let loaded = store.load_config().unwrap();
        assert!(!loaded.settings.hide_from_dock);
        assert_eq!(loaded.settings.update_channel, UpdateChannel::Nightly);
        assert!(loaded.settings.local_insights, "new flag defaults on");
        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].id, "claude-code-local");
        assert_eq!(loaded.accounts[0].copilot_plan, None);
        assert_eq!(loaded.accounts[0].copilot_custom_limit, None);

        drop(store);
        let conn = Connection::open(db_path).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }

    #[test]
    fn imports_current_legacy_json_once_and_renames_it() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join(config::DATABASE_FILE);
        let json_path = dir.path().join(config::CONFIG_FILE);
        let mut legacy = AppConfig::default();
        legacy.upsert_manual(AccountInput {
            id: Some("openrouter-main".to_string()),
            provider: ProviderKind::OpenRouter,
            label: "OpenRouter".to_string(),
            enabled: true,
            endpoint_override: Some("https://example.test".to_string()),
            secret_storage: SecretStorageMode::Keyring,
            secret: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
            copilot_plan: None,
            copilot_custom_limit: None,
        });
        config::save_to_path(&json_path, &legacy).unwrap();

        let store = ConfigStore::open_at(db_path, json_path.clone()).unwrap();
        let loaded = store.load_config().unwrap();

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].provider, ProviderKind::OpenRouter);
        assert!(!json_path.exists());
        assert!(fs::read_dir(dir.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("accounts.json.migrated-")
        }));
    }

    #[test]
    fn treats_schemaless_existing_database_file_as_created() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join(config::DATABASE_FILE);
        // A 0-byte file (e.g. a truncated restore) exists but holds no schema;
        // migrations build it from scratch, so the store must report it as
        // created — callers skip destructive startup reconciliation on it.
        fs::write(&db_path, b"").unwrap();

        let store =
            ConfigStore::open_at(db_path.clone(), dir.path().join(config::CONFIG_FILE)).unwrap();
        assert!(store.created_database());

        drop(store);
        let reopened = ConfigStore::open_at(db_path, dir.path().join(config::CONFIG_FILE)).unwrap();
        assert!(!reopened.created_database());
    }

    #[test]
    fn does_not_auto_import_invalid_backups() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join(config::DATABASE_FILE);
        let json_path = dir.path().join(config::CONFIG_FILE);
        let backup_path = dir.path().join("accounts.json.invalid-123");
        let mut backup = AppConfig::default();
        backup.upsert_manual(AccountInput {
            id: Some("runpod-main".to_string()),
            provider: ProviderKind::Runpod,
            label: "Runpod".to_string(),
            enabled: true,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Keyring,
            secret: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
            copilot_plan: None,
            copilot_custom_limit: None,
        });
        config::save_to_path(&backup_path, &backup).unwrap();

        let store = ConfigStore::open_at(db_path, json_path).unwrap();

        assert!(store.load_config().unwrap().accounts.is_empty());
        assert!(backup_path.exists());
    }

    #[test]
    fn removes_new_database_when_legacy_import_fails() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join(config::DATABASE_FILE);
        let json_path = dir.path().join(config::CONFIG_FILE);
        fs::write(
            &json_path,
            r#"{"accounts":[{"id":"bad","provider":"__unknown__"}]}"#,
        )
        .unwrap();

        let error = match ConfigStore::open_at(db_path.clone(), json_path.clone()) {
            Ok(_) => panic!("incompatible legacy JSON should fail import"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("incompatible"));
        for path in database_file_paths(&db_path) {
            assert!(!path.exists(), "{} should be removed", path.display());
        }
        assert!(json_path.exists());
    }

    #[test]
    #[cfg(unix)]
    fn hardens_database_and_wal_sidecar_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let db_path = dir.path().join(config::DATABASE_FILE);
        let json_path = dir.path().join(config::CONFIG_FILE);
        let store = ConfigStore::open_at(db_path.clone(), json_path).unwrap();
        store.save_config(&AppConfig::default()).unwrap();

        for path in database_file_paths(&db_path) {
            if path.exists() {
                let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "{} mode", path.display());
            }
        }
    }

    #[test]
    fn save_replaces_snapshot_transactionally() {
        let dir = tempdir().unwrap();
        let store = ConfigStore::open_at(
            dir.path().join(config::DATABASE_FILE),
            dir.path().join(config::CONFIG_FILE),
        )
        .unwrap();
        let mut first = AppConfig::default();
        first.upsert_manual(AccountInput {
            id: Some("codex-main".to_string()),
            provider: ProviderKind::Codex,
            label: "Codex".to_string(),
            enabled: true,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Keyring,
            secret: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
            copilot_plan: None,
            copilot_custom_limit: None,
        });
        store.save_config(&first).unwrap();

        let second = AppConfig {
            settings: AppSettings {
                hide_from_dock: true,
                update_channel: UpdateChannel::Stable,
                tray_scale: 1.0,
                local_insights: true,
            },
            accounts: Vec::new(),
        };
        store.save_config(&second).unwrap();

        assert!(store.load_config().unwrap().accounts.is_empty());
    }

    #[test]
    fn timestamp_round_trips_with_nanos() {
        let now = Utc::now();
        let encoded = format_timestamp(&now);
        let decoded = parse_timestamp(&encoded).unwrap();

        assert_eq!(decoded, now);
    }
}
