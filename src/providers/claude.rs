use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, TimeZone, Utc};
use reqwest::Client;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};
use tokio::{process::Command as TokioCommand, time::timeout};

use crate::{
    config::default_auto_account,
    models::{
        AccountConfig, ProviderKind, QuotaSnapshot, SnapshotStatus, SubscriptionSnapshot,
        UsageBucketSnapshot, UsageSnapshot,
    },
};

use super::{bucket_from_parts, endpoint, overall_status, primary_quota, subscription_from_json};

const DEFAULT_USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";
const INFERENCE_SCOPE: &str = "user:inference";
const PROFILE_SCOPE: &str = "user:profile";
const USAGE_CACHE_TTL_MS: u64 = 5 * 60 * 1000;
const USAGE_ERROR_BACKOFF_MS: u64 = 2 * 60 * 1000;
const AUTH_STATUS_TIMEOUT_MS: u64 = 1_500;
const REAUTH_HINT: &str = "Run `claude auth login` to refresh Claude Code authentication.";

#[derive(Debug, Clone)]
struct ClaudeUsageCacheEntry {
    snapshot: Option<UsageSnapshot>,
    last_attempt_at: u64,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialFile {
    claude_ai_oauth: OAuthCredentials,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeAuthStatus {
    #[serde(default)]
    logged_in: bool,
    auth_method: Option<String>,
    api_provider: Option<String>,
    subscription_type: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthCredentials {
    access_token: String,
    #[serde(rename = "refreshToken")]
    _refresh_token: String,
    expires_at: u64,
    #[serde(default)]
    scopes: Vec<String>,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeUsageLimit {
    utilization: f64,
    resets_at: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeExtraUsage {
    is_enabled: bool,
    #[serde(default)]
    monthly_limit: Option<f64>,
    #[serde(default)]
    used_credits: Option<f64>,
    #[serde(default)]
    utilization: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ClaudeUsageData {
    #[serde(default)]
    five_hour: Option<ClaudeUsageLimit>,
    #[serde(default)]
    seven_day: Option<ClaudeUsageLimit>,
    #[serde(default)]
    seven_day_oauth_apps: Option<ClaudeUsageLimit>,
    #[serde(default)]
    seven_day_sonnet: Option<ClaudeUsageLimit>,
    #[serde(default)]
    seven_day_opus: Option<ClaudeUsageLimit>,
    #[serde(default)]
    extra_usage: Option<ClaudeExtraUsage>,
}

#[derive(Debug, Clone)]
struct ClaudeOAuthUsage {
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
    usage: ClaudeUsageData,
    email: Option<String>,
}

pub(crate) fn detect() -> Option<AccountConfig> {
    let base = std::env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))?;

    #[cfg(target_os = "macos")]
    {
        if base.exists() || std::env::var("CLAUDE_CODE_OAUTH_TOKEN").is_ok() {
            return Some(default_auto_account(
                "claude-code-local",
                ProviderKind::ClaudeCode,
                "Claude Code",
                base,
            ));
        }
        None
    }

    #[cfg(not(target_os = "macos"))]
    {
        let credential_path = if base.join(".credentials.json").exists() {
            base.join(".credentials.json")
        } else if base.join("credentials.json").exists() {
            base.join("credentials.json")
        } else if base.exists() {
            base
        } else {
            return None;
        };

        Some(default_auto_account(
            "claude-code-local",
            ProviderKind::ClaudeCode,
            "Claude Code",
            credential_path,
        ))
    }
}

pub(crate) async fn fetch(http: &Client, account: &AccountConfig) -> Result<UsageSnapshot> {
    let now = now_millis();
    if let Some(cached) = cached_before_fetch(account, now)? {
        return Ok(cached);
    }

    match fetch_oauth_usage(http, account).await {
        Ok(usage) => {
            let snapshot = parse_claude_oauth_usage(account, usage);
            remember_success(account, snapshot.clone(), now);
            Ok(snapshot)
        }
        Err(error) => {
            let message = error.to_string();
            if let Some(snapshot) = remember_failure_and_stale(account, now, &message) {
                return Ok(snapshot);
            }
            Err(anyhow!(message))
        }
    }
}

async fn fetch_oauth_usage(http: &Client, account: &AccountConfig) -> Result<ClaudeOAuthUsage> {
    let mut email = None;
    if should_check_claude_auth(account) {
        email = ensure_claude_code_auth(account.cli_config_dir()).await?;
    }
    let credentials = read_credentials(account).await?;
    let oauth = credentials.claude_ai_oauth;
    let now = now_millis();
    validate_oauth_credentials(&oauth)?;
    if oauth.expires_at <= now {
        return Err(anyhow!("Claude Code OAuth token is expired. {REAUTH_HINT}"));
    }

    let resp = http
        .get(endpoint(
            account,
            "BURNRATE_CLAUDE_USAGE_URL",
            DEFAULT_USAGE_ENDPOINT,
        )?)
        .bearer_auth(&oauth.access_token)
        .header("anthropic-beta", ANTHROPIC_BETA)
        .header("Accept", "application/json, text/plain, */*")
        .header("Content-Type", "application/json")
        .header("User-Agent", claude_code_user_agent())
        .send()
        .await
        .context("failed to fetch Claude Code usage")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status == StatusCode::UNAUTHORIZED {
            forget_cached_usage(account);
        }
        return Err(anyhow!(format_usage_http_error(
            "Claude Code usage API error",
            status,
            &body
        )));
    }

    let value: serde_json::Value = resp
        .json()
        .await
        .context("failed to decode Claude Code usage")?;

    let usage = parse_usage_data(&value)?;

    Ok(ClaudeOAuthUsage {
        subscription_type: oauth.subscription_type,
        rate_limit_tier: oauth.rate_limit_tier,
        usage,
        email,
    })
}

fn is_claude_ai_subscriber(oauth: &OAuthCredentials) -> bool {
    oauth.scopes.iter().any(|scope| scope == INFERENCE_SCOPE)
}

fn has_profile_scope(oauth: &OAuthCredentials) -> bool {
    oauth.scopes.iter().any(|scope| scope == PROFILE_SCOPE)
}

fn validate_oauth_credentials(oauth: &OAuthCredentials) -> Result<()> {
    if !is_claude_ai_subscriber(oauth) || !has_profile_scope(oauth) {
        return Err(anyhow!(
            "Claude Code is signed in with an inference-only token, not a full claude.ai OAuth login. {REAUTH_HINT}"
        ));
    }
    Ok(())
}

fn usage_cache() -> &'static Mutex<HashMap<String, ClaudeUsageCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, ClaudeUsageCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_before_fetch(account: &AccountConfig, now: u64) -> Result<Option<UsageSnapshot>> {
    let cache = usage_cache()
        .lock()
        .expect("Claude usage cache should not be poisoned");
    let Some(entry) = cache.get(&account.id) else {
        return Ok(None);
    };
    let age = now.saturating_sub(entry.last_attempt_at);
    if let Some(snapshot) = entry.snapshot.as_ref()
        && age < USAGE_CACHE_TTL_MS
    {
        return Ok(Some(snapshot.clone()));
    }
    if entry.snapshot.is_none() && age < USAGE_ERROR_BACKOFF_MS {
        return Err(anyhow!(entry.last_error.clone().unwrap_or_else(|| {
            "Claude Code usage data temporarily unavailable. Try again later.".to_string()
        })));
    }
    Ok(None)
}

fn remember_success(account: &AccountConfig, snapshot: UsageSnapshot, now: u64) {
    let mut cache = usage_cache()
        .lock()
        .expect("Claude usage cache should not be poisoned");
    cache.insert(
        account.id.clone(),
        ClaudeUsageCacheEntry {
            snapshot: Some(snapshot),
            last_attempt_at: now,
            last_error: None,
        },
    );
}

fn forget_cached_usage(account: &AccountConfig) {
    usage_cache()
        .lock()
        .expect("Claude usage cache should not be poisoned")
        .remove(&account.id);
}

fn remember_failure_and_stale(
    account: &AccountConfig,
    now: u64,
    message: &str,
) -> Option<UsageSnapshot> {
    let mut cache = usage_cache()
        .lock()
        .expect("Claude usage cache should not be poisoned");
    let entry = cache
        .entry(account.id.clone())
        .or_insert_with(|| ClaudeUsageCacheEntry {
            snapshot: None,
            last_attempt_at: 0,
            last_error: None,
        });
    entry.last_attempt_at = now;
    entry.last_error = Some(message.to_string());
    let mut snapshot = entry.snapshot.clone()?;
    snapshot.status = SnapshotStatus::Stale;
    snapshot.message = Some(format!("Using cached Claude Code usage; {message}"));
    entry.snapshot = Some(snapshot.clone());
    Some(snapshot)
}

fn format_http_error(prefix: &str, status: StatusCode, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.pointer("/message"))
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| body.trim().to_string());
    if detail.is_empty() {
        format!("{prefix} ({status})")
    } else {
        format!("{prefix} ({status}): {detail}")
    }
}

fn format_usage_http_error(prefix: &str, status: StatusCode, body: &str) -> String {
    let message = format_http_error(prefix, status, body);
    if matches!(
        status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    ) {
        format!("{message}. {REAUTH_HINT}")
    } else {
        message
    }
}

async fn read_credentials(account: &AccountConfig) -> Result<CredentialFile> {
    #[cfg(target_os = "macos")]
    {
        if account
            .credential_path
            .as_ref()
            .map(PathBuf::from)
            .is_some_and(|path| path.is_file())
            && let Some(creds) = read_credentials_file(account)?
        {
            return Ok(creds);
        }

        let account_for_keychain = account.clone();
        match tokio::task::spawn_blocking(move || {
            read_macos_keychain_credentials(&account_for_keychain)
        })
        .await
        .context("Claude Code credential reader panicked")?
        {
            Ok(creds) => Ok(creds),
            Err(keychain_error) => {
                if let Some(creds) = read_credentials_file(account)? {
                    return Ok(creds);
                }
                Err(keychain_error)
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        read_credentials_file(account)?.ok_or_else(|| {
            if env_present("CLAUDE_CODE_OAUTH_TOKEN") {
                anyhow!(
                    "Claude Code is using environment variable authentication. Usage tracking requires a standard login. Run `claude auth login` to enable."
                )
            } else {
                anyhow!("Claude Code credentials not found. Sign in with `claude auth login`.")
            }
        })
    }
}

/// Best-effort deletion of the macOS Keychain credential for an account. Used as
/// a fallback during sign-out/removal: Claude stores its OAuth token in the
/// Keychain (keyed by the per-account config dir), which a failed
/// `claude auth logout` would otherwise leave orphaned after the dir is deleted.
#[cfg(target_os = "macos")]
pub(crate) fn delete_keychain_credentials(account: &AccountConfig) {
    delete_keychain_credentials_for_dir(account.cli_config_dir());
}

/// Like [`delete_keychain_credentials`] but keyed directly by a config dir, for
/// clearing the credential of a *stale* dir that is about to be discarded (e.g.
/// when the reuse policy adopts a freshly authenticated dir into an account).
#[cfg(target_os = "macos")]
pub(crate) fn delete_keychain_credentials_for_dir(config_dir: Option<&str>) {
    let user = keychain_username();
    let service_name = keychain_service_name_for(config_dir, oauth_file_suffix());
    let _ = Command::new("security")
        .args(["delete-generic-password", "-s", &service_name, "-a", &user])
        .output();
}

#[cfg(target_os = "macos")]
fn read_macos_keychain_credentials(account: &AccountConfig) -> Result<CredentialFile> {
    let user = keychain_username();
    let service_name = keychain_service_name_for(account.cli_config_dir(), oauth_file_suffix());
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            &service_name,
            "-a",
            &user,
            "-w",
        ])
        .output()
        .context("failed to run macOS security command")?;

    if !output.status.success() {
        if env_present("CLAUDE_CODE_OAUTH_TOKEN") {
            return Err(anyhow!(
                "Claude Code is using environment variable authentication. Usage tracking requires a standard login. Run `claude auth login` to enable."
            ));
        }
        return Err(anyhow!(
            "Claude Code credentials not found in Keychain service `{service_name}` for account `{user}`. {REAUTH_HINT}"
        ));
    }

    let json = String::from_utf8(output.stdout).context("invalid UTF-8 in Claude credentials")?;
    parse_credential_json(&json)
}

#[cfg(target_os = "macos")]
fn keychain_username() -> String {
    std::env::var("USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("LOGNAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "claude-code-user".to_string())
}

#[cfg(target_os = "macos")]
fn keychain_service_name_for(config_dir: Option<&str>, oauth_suffix: &str) -> String {
    let dir_suffix = config_dir
        .filter(|value| !value.trim().is_empty())
        .map(|dir| {
            let digest = Sha256::digest(dir.as_bytes());
            let hex = format!("{digest:x}");
            format!("-{}", &hex[..8])
        })
        .unwrap_or_default();
    format!("Claude Code{oauth_suffix}-credentials{dir_suffix}")
}

#[cfg(target_os = "macos")]
fn oauth_file_suffix() -> &'static str {
    if env_present("CLAUDE_CODE_CUSTOM_OAUTH_URL") {
        "-custom-oauth"
    } else if std::env::var("USER_TYPE").ok().as_deref() == Some("ant")
        && env_present("USE_LOCAL_OAUTH")
    {
        "-local-oauth"
    } else if std::env::var("USER_TYPE").ok().as_deref() == Some("ant")
        && env_present("USE_STAGING_OAUTH")
    {
        "-staging-oauth"
    } else {
        ""
    }
}

fn read_credentials_file(account: &AccountConfig) -> Result<Option<CredentialFile>> {
    let Some(path) = account.credential_path.as_ref().map(PathBuf::from) else {
        return Ok(None);
    };
    let path = if path.is_dir() {
        if path.join(".credentials.json").exists() {
            path.join(".credentials.json")
        } else if path.join("credentials.json").exists() {
            path.join("credentials.json")
        } else {
            return Ok(None);
        }
    } else if path.exists() {
        path
    } else {
        return Ok(None);
    };

    parse_credential_path(&path).map(Some)
}

fn parse_credential_path(path: &Path) -> Result<CredentialFile> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    parse_credential_json(&contents)
}

fn parse_credential_json(contents: &str) -> Result<CredentialFile> {
    serde_json::from_str(contents).context("failed to parse Claude Code credentials")
}

/// Best-effort auth check used during `fetch`. Returns the signed-in email when
/// available. A failed `claude auth status` call does not block usage fetching
/// (returns `Ok(None)`), but an invalid login (wrong provider, no subscription)
/// propagates as an error.
async fn ensure_claude_code_auth(config_dir: Option<&str>) -> Result<Option<String>> {
    let status = match claude_auth_status(config_dir).await {
        Ok(status) => status,
        Err(_) => return Ok(None),
    };
    validate_auth_status(&status)?;
    Ok(status.email.clone())
}

/// Verify a freshly completed login for `config_dir`, returning the account
/// email. Unlike [`ensure_claude_code_auth`], a failed status call is an error
/// here because we are confirming a just-finished sign-in.
pub(crate) async fn login_verify(config_dir: Option<&str>) -> Result<Option<String>> {
    let status = claude_auth_status(config_dir).await?;
    validate_auth_status(&status)?;
    Ok(status.email.clone())
}

/// `claude auth login` argument vector. `--claudeai` selects the subscription
/// (first-party) flow that usage tracking requires; `--email` pre-fills the
/// login page when known.
pub(crate) fn claude_login_args(email: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "auth".to_string(),
        "login".to_string(),
        "--claudeai".to_string(),
    ];
    if let Some(email) = email.map(str::trim).filter(|value| !value.is_empty()) {
        args.push("--email".to_string());
        args.push(email.to_string());
    }
    args
}

/// `claude auth logout` argument vector.
pub(crate) fn claude_logout_args() -> Vec<String> {
    vec!["auth".to_string(), "logout".to_string()]
}

fn validate_auth_status(status: &ClaudeAuthStatus) -> Result<()> {
    if !status.logged_in {
        return Err(anyhow!("Claude Code is not signed in. {REAUTH_HINT}"));
    }

    if status
        .api_provider
        .as_deref()
        .is_some_and(|provider| !provider.eq_ignore_ascii_case("firstParty"))
    {
        return Err(anyhow!(
            "Claude Code is signed in with a third-party provider. Usage tracking requires claude.ai first-party OAuth. {REAUTH_HINT}"
        ));
    }

    let auth_method = status.auth_method.as_deref().unwrap_or_default();
    if !auth_method.eq_ignore_ascii_case("claude.ai")
        && !auth_method.eq_ignore_ascii_case("oauth_token")
    {
        return Err(anyhow!(
            "Claude Code auth method is `{auth_method}`. Usage tracking requires claude.ai OAuth. {REAUTH_HINT}"
        ));
    }

    if status.subscription_type.is_none() {
        return Err(anyhow!(
            "Claude Code is signed in, but no Claude subscription was detected. {REAUTH_HINT}"
        ));
    }

    Ok(())
}

pub(crate) fn claude_binary() -> String {
    std::env::var("BURNRATE_CLAUDE_BIN")
        .or_else(|_| std::env::var("CLAUDE_BIN"))
        .unwrap_or_else(|_| "claude".to_string())
}

async fn claude_auth_status(config_dir: Option<&str>) -> Result<ClaudeAuthStatus> {
    let binary = super::resolve_cli(&claude_binary());
    let mut command = TokioCommand::new(&binary);
    command
        .args(["auth", "status", "--json"])
        .env("PATH", super::augmented_path())
        .stdin(std::process::Stdio::null());
    if let Some(dir) = config_dir.filter(|value| !value.trim().is_empty()) {
        command.env("CLAUDE_CONFIG_DIR", dir);
    }
    let output = timeout(
        std::time::Duration::from_millis(AUTH_STATUS_TIMEOUT_MS),
        command.output(),
    )
    .await
    .context("Claude Code auth status check timed out")?
    .context("failed to run `claude auth status --json`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|value| !value.is_empty())
            .unwrap_or("Claude Code auth status check failed.");
        return Err(anyhow!("{detail}"));
    }

    let stdout =
        String::from_utf8(output.stdout).context("invalid UTF-8 from Claude auth status")?;
    serde_json::from_str(stdout.trim()).context("failed to parse Claude Code auth status")
}

fn env_present(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn parse_usage_data(value: &serde_json::Value) -> Result<ClaudeUsageData> {
    if has_oauth_usage_keys(value) {
        return serde_json::from_value(value.clone()).context("failed to parse Claude Code usage");
    }
    if let Some(usage) = value.get("usage") {
        return serde_json::from_value(usage.clone()).context("failed to parse Claude Code usage");
    }
    serde_json::from_value(value.clone()).context("failed to parse Claude Code usage")
}

fn has_oauth_usage_keys(value: &serde_json::Value) -> bool {
    [
        "five_hour",
        "seven_day",
        "seven_day_oauth_apps",
        "seven_day_sonnet",
        "seven_day_opus",
        "extra_usage",
    ]
    .iter()
    .any(|key| value.get(key).is_some())
}

fn parse_claude_oauth_usage(account: &AccountConfig, usage: ClaudeOAuthUsage) -> UsageSnapshot {
    let mut buckets = Vec::new();
    if let Some(limit) = usage.usage.five_hour.as_ref() {
        buckets.push(bucket_from_oauth_limit("session-5h", "5-hour", limit));
    }
    if let Some(limit) = usage.usage.seven_day.as_ref() {
        buckets.push(bucket_from_oauth_limit("weekly", "Weekly", limit));
    }
    if let Some(limit) = usage.usage.seven_day_oauth_apps.as_ref() {
        buckets.push(bucket_from_oauth_limit(
            "weekly-oauth-apps",
            "Weekly OAuth Apps",
            limit,
        ));
    }
    if let Some(limit) = usage.usage.seven_day_sonnet.as_ref() {
        buckets.push(bucket_from_oauth_limit(
            "weekly-sonnet",
            "Weekly Sonnet",
            limit,
        ));
    }
    if let Some(limit) = usage.usage.seven_day_opus.as_ref() {
        buckets.push(bucket_from_oauth_limit("weekly-opus", "Weekly Opus", limit));
    }
    if let Some(extra) = usage
        .usage
        .extra_usage
        .as_ref()
        .filter(|extra| extra.is_enabled)
        && let Some(bucket) = bucket_from_extra_usage(extra)
    {
        buckets.push(bucket);
    }

    let subscription = subscription_from_oauth(
        usage.subscription_type.as_deref(),
        usage.rate_limit_tier.as_deref(),
        usage
            .usage
            .extra_usage
            .as_ref()
            .map(|extra| extra.is_enabled),
    );
    let quota = primary_quota(&buckets);
    let status = overall_status(&buckets);

    UsageSnapshot {
        account_id: account.id.clone(),
        provider: account.provider,
        label: account.label.clone(),
        status,
        subscription,
        usage_buckets: buckets,
        quota,
        email: usage.email,
        message: None,
        fetched_at: Utc::now(),
    }
}

fn bucket_from_oauth_limit(
    id: impl Into<String>,
    label: impl Into<String>,
    limit: &ClaudeUsageLimit,
) -> UsageBucketSnapshot {
    let used = limit.utilization.clamp(0.0, 100.0);
    bucket_from_parts(
        id,
        label,
        None,
        QuotaSnapshot {
            used,
            limit: Some(100.0),
            remaining: Some((100.0 - used).max(0.0)),
            unit: "%".to_string(),
            reset_at: reset_from_value(&limit.resets_at),
        },
    )
}

fn bucket_from_extra_usage(extra: &ClaudeExtraUsage) -> Option<UsageBucketSnapshot> {
    let used = extra
        .used_credits
        .or(extra.utilization)
        .or_else(|| extra.monthly_limit.map(|_| 0.0))?;
    Some(bucket_from_parts(
        "extra-usage",
        "Extra usage",
        None,
        QuotaSnapshot {
            used,
            limit: extra.monthly_limit,
            remaining: extra.monthly_limit.map(|limit| (limit - used).max(0.0)),
            unit: if extra.used_credits.is_some() {
                "credits".to_string()
            } else {
                "%".to_string()
            },
            reset_at: None,
        },
    ))
}

fn subscription_from_oauth(
    subscription_type: Option<&str>,
    rate_limit_tier: Option<&str>,
    extra_usage_enabled: Option<bool>,
) -> Option<SubscriptionSnapshot> {
    subscription_from_json(
        &json!({
            "subscriptionType": subscription_type,
            "rateLimitTier": rate_limit_tier,
            "extraUsageEnabled": extra_usage_enabled,
        }),
        "claude-code-oauth",
        &["/subscriptionType", "/plan"],
    )
}

fn reset_from_value(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    if let Some(text) = value.as_str() {
        return chrono::DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|value| value.with_timezone(&Utc));
    }
    let millis = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))?;
    let millis = if millis < 10_000_000_000 {
        millis * 1000
    } else {
        millis
    };
    Utc.timestamp_millis_opt(millis).single()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn claude_code_user_agent() -> String {
    static USER_AGENT: OnceLock<String> = OnceLock::new();
    USER_AGENT
        .get_or_init(detect_claude_code_user_agent)
        .clone()
}

fn detect_claude_code_user_agent() -> String {
    let output = Command::new(super::resolve_cli(&claude_binary()))
        .arg("--version")
        .env("PATH", super::augmented_path())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let raw = String::from_utf8_lossy(&output.stdout);
            let version = raw.split_whitespace().next().unwrap_or("0.0.0");
            format!("claude-code/{version}")
        }
        _ => "claude-code/0.0.0".to_string(),
    }
}

fn should_check_claude_auth(account: &AccountConfig) -> bool {
    account.endpoint_override.is_none() && !env_present("BURNRATE_CLAUDE_USAGE_URL")
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    use super::*;
    use crate::models::{SecretStorageMode, SnapshotStatus, SubscriptionPlan};

    fn account() -> AccountConfig {
        AccountConfig {
            id: "claude-code-local".to_string(),
            provider: ProviderKind::ClaudeCode,
            label: "Claude Code".to_string(),
            enabled: true,
            auto_detected: true,
            credential_path: None,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Plaintext,
            keyring_account: None,
            plaintext_secret: None,
            email: None,
            config_dir: None,
            order_index: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn auth_status(
        logged_in: bool,
        auth_method: Option<&str>,
        api_provider: Option<&str>,
        subscription_type: Option<&str>,
    ) -> ClaudeAuthStatus {
        ClaudeAuthStatus {
            logged_in,
            auth_method: auth_method.map(ToString::to_string),
            api_provider: api_provider.map(ToString::to_string),
            subscription_type: subscription_type.map(ToString::to_string),
            email: None,
        }
    }

    #[test]
    fn parses_claude_code_credentials() {
        let creds = parse_credential_json(
            r#"{
                "claudeAiOauth": {
                    "accessToken": "sk-ant-oat01-test",
                    "refreshToken": "sk-ant-ort01-test",
                    "expiresAt": 1769163729172,
                    "scopes": [
                        "user:file_upload",
                        "user:inference",
                        "user:mcp_servers",
                        "user:profile",
                        "user:sessions:claude_code"
                    ],
                    "subscriptionType": "max",
                    "rateLimitTier": "default_claude_max_20x"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(creds.claude_ai_oauth.access_token, "sk-ant-oat01-test");
        assert_eq!(
            creds.claude_ai_oauth.subscription_type.as_deref(),
            Some("max")
        );
        assert_eq!(
            creds.claude_ai_oauth.rate_limit_tier.as_deref(),
            Some("default_claude_max_20x")
        );
        assert!(
            creds
                .claude_ai_oauth
                .scopes
                .iter()
                .any(|scope| scope == PROFILE_SCOPE)
        );
    }

    #[test]
    fn rejects_inference_only_credentials() {
        let creds = parse_credential_json(
            r#"{
                "claudeAiOauth": {
                    "accessToken": "sk-ant-oat01-test",
                    "refreshToken": "sk-ant-ort01-test",
                    "expiresAt": 1769163729172,
                    "scopes": ["user:inference"],
                    "subscriptionType": "max"
                }
            }"#,
        )
        .unwrap();

        let error = validate_oauth_credentials(&creds.claude_ai_oauth)
            .unwrap_err()
            .to_string();

        assert!(error.contains("inference-only token"));
        assert!(error.contains("claude auth login"));
    }

    #[test]
    fn validates_claude_ai_auth_status() {
        let status = auth_status(true, Some("claude.ai"), Some("firstParty"), Some("max"));

        validate_auth_status(&status).unwrap();
    }

    #[test]
    fn accepts_legacy_oauth_token_auth_status() {
        let status = auth_status(true, Some("oauth_token"), Some("firstParty"), Some("pro"));

        validate_auth_status(&status).unwrap();
    }

    #[test]
    fn rejects_signed_out_auth_status() {
        let status = auth_status(false, None, Some("firstParty"), None);
        let error = validate_auth_status(&status).unwrap_err().to_string();

        assert!(error.contains("not signed in"));
        assert!(error.contains("claude auth login"));
    }

    #[test]
    fn rejects_third_party_auth_status() {
        let status = auth_status(true, Some("claude.ai"), Some("bedrock"), Some("max"));
        let error = validate_auth_status(&status).unwrap_err().to_string();

        assert!(error.contains("third-party provider"));
        assert!(error.contains("first-party OAuth"));
    }

    #[test]
    fn rejects_auth_status_without_subscription() {
        let status = auth_status(true, Some("claude.ai"), Some("firstParty"), None);
        let error = validate_auth_status(&status).unwrap_err().to_string();

        assert!(error.contains("no Claude subscription"));
        assert!(error.contains("claude auth login"));
    }

    #[test]
    fn formats_usage_auth_errors_with_reauth_hint() {
        let message = format_usage_http_error(
            "Claude Code usage API error",
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"Rate limited. Please try again later."}}"#,
        );

        assert!(message.contains("429 Too Many Requests"));
        assert!(message.contains("Rate limited"));
        assert!(message.contains("claude auth login"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn derives_current_claude_code_keychain_service_names() {
        assert_eq!(
            keychain_service_name_for(None, ""),
            "Claude Code-credentials"
        );
        assert_eq!(
            keychain_service_name_for(Some("abc"), ""),
            "Claude Code-credentials-ba7816bf"
        );
        assert_eq!(
            keychain_service_name_for(Some("abc"), "-staging-oauth"),
            "Claude Code-staging-oauth-credentials-ba7816bf"
        );
        assert_eq!(
            keychain_service_name_for(Some(""), ""),
            "Claude Code-credentials"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn keychain_service_name_isolates_per_account_dirs() {
        // The default account (no per-account dir) keeps the legacy service name,
        // so existing single-account installs read unchanged.
        assert_eq!(
            keychain_service_name_for(None, ""),
            "Claude Code-credentials"
        );
        // Two managed accounts in distinct config dirs get distinct, stable
        // service names — the basis for multi-account Keychain isolation.
        let a = keychain_service_name_for(Some("/cfg/claude-code/acct-a"), "");
        let b = keychain_service_name_for(Some("/cfg/claude-code/acct-b"), "");
        assert_ne!(a, b);
        assert_ne!(a, "Claude Code-credentials");
        assert_eq!(
            a,
            keychain_service_name_for(Some("/cfg/claude-code/acct-a"), "")
        );
    }

    #[test]
    fn maps_claude_oauth_usage_buckets_and_max_plan() {
        let snapshot = parse_claude_oauth_usage(
            &account(),
            ClaudeOAuthUsage {
                subscription_type: Some("max".to_string()),
                rate_limit_tier: Some("default_claude_max_20x".to_string()),
                email: Some("dev@example.com".to_string()),
                usage: serde_json::from_value(json!({
                    "five_hour": {
                        "utilization": 96,
                        "resets_at": "2026-06-01T17:00:00Z"
                    },
                    "seven_day": {
                        "utilization": 40,
                        "resets_at": 1780000000000_i64
                    },
                    "seven_day_oauth_apps": {
                        "utilization": 8,
                        "resets_at": "2026-06-02T18:00:00Z"
                    },
                    "seven_day_sonnet": {
                        "utilization": 12,
                        "resets_at": "2026-06-02T17:00:00Z"
                    },
                    "extra_usage": {
                        "is_enabled": true,
                        "monthly_limit": 100,
                        "used_credits": 25
                    }
                }))
                .unwrap(),
            },
        );

        let subscription = snapshot.subscription.unwrap();
        assert_eq!(subscription.plan, SubscriptionPlan::Max);
        assert_eq!(subscription.plan_label, "Max 20x");
        assert_eq!(subscription.extra_usage_enabled, Some(true));
        assert_eq!(snapshot.status, SnapshotStatus::Exhausted);
        assert_eq!(snapshot.usage_buckets[0].label, "5-hour");
        assert_eq!(snapshot.usage_buckets[1].label, "Weekly");
        assert_eq!(snapshot.usage_buckets[2].label, "Weekly OAuth Apps");
        assert_eq!(snapshot.usage_buckets[3].label, "Weekly Sonnet");
        assert_eq!(snapshot.usage_buckets[4].label, "Extra usage");
        assert_eq!(snapshot.email.as_deref(), Some("dev@example.com"));
    }

    #[test]
    fn claude_auth_status_parses_email_and_subscription() {
        let status: ClaudeAuthStatus = serde_json::from_str(
            r#"{
                "loggedIn": true,
                "authMethod": "claude.ai",
                "apiProvider": "firstParty",
                "email": "dev.urandom.io@gmail.com",
                "orgName": "dev.urandom.io@gmail.com's Organization",
                "subscriptionType": "max"
            }"#,
        )
        .unwrap();

        assert!(status.logged_in);
        assert_eq!(status.email.as_deref(), Some("dev.urandom.io@gmail.com"));
        assert_eq!(status.subscription_type.as_deref(), Some("max"));
        validate_auth_status(&status).unwrap();
    }

    #[test]
    fn claude_login_args_select_subscription_flow() {
        assert_eq!(claude_login_args(None), vec!["auth", "login", "--claudeai"]);
        assert_eq!(
            claude_login_args(Some("a@b.com")),
            vec!["auth", "login", "--claudeai", "--email", "a@b.com"]
        );
        // Blank emails are ignored.
        assert_eq!(
            claude_login_args(Some("   ")),
            vec!["auth", "login", "--claudeai"]
        );
        assert_eq!(claude_logout_args(), vec!["auth", "logout"]);
    }

    #[tokio::test]
    async fn fetches_oauth_usage_with_local_credentials() {
        usage_cache()
            .lock()
            .expect("Claude usage cache should not be poisoned")
            .clear();
        let dir = tempfile::tempdir().unwrap();
        let credentials = dir.path().join(".credentials.json");
        std::fs::write(
            &credentials,
            serde_json::to_string(&json!({
                "claudeAiOauth": {
                    "accessToken": "token",
                    "refreshToken": "refresh",
                    "expiresAt": now_millis() + 120_000,
                    "scopes": [
                        "user:file_upload",
                        "user:inference",
                        "user:mcp_servers",
                        "user:profile",
                        "user:sessions:claude_code"
                    ],
                    "subscriptionType": "pro"
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .and(header("authorization", "Bearer token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "five_hour": {
                    "utilization": 20,
                    "resets_at": "2026-06-01T17:00:00Z"
                }
            })))
            .mount(&server)
            .await;

        let mut account = account();
        account.credential_path = Some(credentials.to_string_lossy().to_string());
        account.endpoint_override = Some(server.uri());
        let snapshot = fetch(&Client::new(), &account).await.unwrap();

        assert_eq!(snapshot.status, SnapshotStatus::Healthy);
        assert_eq!(snapshot.subscription.unwrap().plan, SubscriptionPlan::Pro);
        assert_eq!(snapshot.quota.unwrap().used, 20.0);

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let headers = &requests[0].headers;
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer token")
        );
        assert_eq!(
            headers
                .get("anthropic-beta")
                .and_then(|value| value.to_str().ok()),
            Some(ANTHROPIC_BETA)
        );
        assert_eq!(
            headers.get("accept").and_then(|value| value.to_str().ok()),
            Some("application/json, text/plain, */*")
        );
        assert_eq!(
            headers
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert!(
            headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("claude-code/"))
        );
        assert!(headers.get("x-api-key").is_none());
        assert!(headers.get("anthropic-version").is_none());
    }

    #[tokio::test]
    async fn uses_cached_claude_usage_when_api_is_temporarily_limited() {
        usage_cache()
            .lock()
            .expect("Claude usage cache should not be poisoned")
            .clear();
        let dir = tempfile::tempdir().unwrap();
        let credentials = dir.path().join(".credentials.json");
        std::fs::write(
            &credentials,
            serde_json::to_string(&json!({
                "claudeAiOauth": {
                    "accessToken": "token",
                    "refreshToken": "refresh",
                    "expiresAt": now_millis() + 120_000,
                    "scopes": [
                        "user:file_upload",
                        "user:inference",
                        "user:mcp_servers",
                        "user:profile",
                        "user:sessions:claude_code"
                    ],
                    "subscriptionType": "max",
                    "rateLimitTier": "default_claude_max_20x"
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let ok_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "five_hour": {
                    "utilization": 40,
                    "resets_at": "2026-06-01T17:00:00Z"
                }
            })))
            .mount(&ok_server)
            .await;

        let mut account = account();
        account.id = "claude-code-stale".to_string();
        account.credential_path = Some(credentials.to_string_lossy().to_string());
        account.endpoint_override = Some(ok_server.uri());

        let snapshot = fetch(&Client::new(), &account).await.unwrap();
        assert_eq!(snapshot.status, SnapshotStatus::Healthy);
        assert_eq!(snapshot.quota.unwrap().used, 40.0);

        {
            let mut cache = usage_cache()
                .lock()
                .expect("Claude usage cache should not be poisoned");
            cache.get_mut(&account.id).unwrap().last_attempt_at =
                now_millis().saturating_sub(USAGE_CACHE_TTL_MS + 1);
        }

        let limited_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "error": {
                    "type": "rate_limit_error",
                    "message": "Rate limited. Please try again later."
                }
            })))
            .mount(&limited_server)
            .await;
        account.endpoint_override = Some(limited_server.uri());

        let snapshot = fetch(&Client::new(), &account).await.unwrap();
        assert_eq!(snapshot.status, SnapshotStatus::Stale);
        assert_eq!(snapshot.quota.unwrap().used, 40.0);
        assert!(
            snapshot
                .message
                .unwrap()
                .contains("Rate limited. Please try again later.")
        );
    }
}
