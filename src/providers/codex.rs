use std::{path::PathBuf, process::Stdio, time::Duration};

use anyhow::{Context, Result, anyhow};
use chrono::{TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::Command,
    time::timeout,
};

use crate::{
    config::default_auto_account,
    models::{
        AccountConfig, ProviderKind, QuotaSnapshot, SubscriptionSnapshot, UsageBucketSnapshot,
        UsageSnapshot,
    },
};

use super::{
    bucket_from_parts, datetime, endpoint, number, overall_status, parse_usage_buckets,
    primary_quota, require_token, subscription_from_json,
};

const DEFAULT_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/rate_limits";
const APP_SERVER_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexRateLimitWindow {
    #[serde(alias = "used_percent")]
    used_percent: i32,
    #[serde(default, alias = "resets_at")]
    resets_at: Option<i64>,
    #[serde(default, alias = "window_duration_mins")]
    window_duration_mins: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexCreditsSnapshot {
    #[serde(default)]
    balance: Option<String>,
    #[serde(default)]
    has_credits: bool,
    #[serde(default)]
    unlimited: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexRateLimitSnapshot {
    #[serde(default, alias = "limit_id")]
    limit_id: Option<String>,
    #[serde(default, alias = "limit_name")]
    limit_name: Option<String>,
    #[serde(default, alias = "plan_type")]
    plan_type: Option<String>,
    #[serde(default)]
    primary: Option<CodexRateLimitWindow>,
    #[serde(default)]
    secondary: Option<CodexRateLimitWindow>,
    #[serde(default)]
    credits: Option<CodexCreditsSnapshot>,
    #[serde(default, alias = "rate_limit_reached_type")]
    rate_limit_reached_type: Option<String>,
}

pub(crate) fn detect() -> Option<AccountConfig> {
    let base = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))?;

    if !base.exists() {
        return None;
    }

    let credential_path = if base.join("auth.json").exists() {
        base.join("auth.json")
    } else {
        base
    };

    Some(default_auto_account(
        "codex-local",
        ProviderKind::Codex,
        "Codex",
        credential_path,
    ))
}

pub(crate) async fn fetch(http: &Client, account: &AccountConfig) -> Result<UsageSnapshot> {
    if account.endpoint_override.is_some()
        || std::env::var("BURNRATE_CODEX_RATE_LIMITS_URL").is_ok()
    {
        return fetch_http_override(http, account).await;
    }

    let value = read_codex_app_server_rate_limits().await?;
    Ok(parse_codex_rate_limits(account, &value))
}

async fn fetch_http_override(http: &Client, account: &AccountConfig) -> Result<UsageSnapshot> {
    let token = require_token(account)?;
    let value: serde_json::Value = http
        .get(endpoint(
            account,
            "BURNRATE_CODEX_RATE_LIMITS_URL",
            DEFAULT_ENDPOINT,
        )?)
        .bearer_auth(token)
        .send()
        .await
        .context("failed to fetch Codex rate limits")?
        .error_for_status()
        .context("Codex rate limit request failed")?
        .json()
        .await
        .context("failed to decode Codex rate limits")?;

    Ok(parse_codex_rate_limits(account, &value))
}

async fn read_codex_app_server_rate_limits() -> Result<Value> {
    let binary = std::env::var("BURNRATE_CODEX_BIN")
        .or_else(|_| std::env::var("CODEX_BIN"))
        .unwrap_or_else(|_| "codex".to_string());
    read_codex_app_server_rate_limits_with_binary(&binary).await
}

async fn read_codex_app_server_rate_limits_with_binary(binary: &str) -> Result<Value> {
    let resolved = super::resolve_cli(binary);
    let path_env = super::augmented_path();
    timeout(APP_SERVER_TIMEOUT, async move {
        let mut command = Command::new(&resolved);
        command
            .args(["app-server", "--listen", "stdio://"])
            .env("PATH", &path_env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        let mut child = command.spawn().with_context(|| {
            format!(
                "failed to start `{} app-server --listen stdio://`",
                resolved.display()
            )
        })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to open Codex app-server stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to open Codex app-server stdout"))?;
        let mut reader = BufReader::new(stdout);

        write_json_line(
            &mut stdin,
            &json!({
                "id": 1,
                "method": "initialize",
                "params": {
                    "clientInfo": {
                        "name": "burnrate",
                        "title": "Burnrate",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "capabilities": {
                        "experimentalApi": true,
                    },
                },
            }),
        )
        .await?;
        write_json_line(&mut stdin, &json!({ "method": "initialized" })).await?;
        write_json_line(
            &mut stdin,
            &json!({
                "id": 2,
                "method": "account/rateLimits/read",
            }),
        )
        .await?;

        let result = read_response_with_id(&mut reader, 2).await;
        let _ = child.kill().await;
        result
    })
    .await
    .map_err(|_| anyhow!("Codex app-server rate limit read timed out"))?
}

async fn write_json_line<W>(writer: &mut W, value: &Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_string(value).context("failed to encode Codex request")?;
    writer
        .write_all(payload.as_bytes())
        .await
        .context("failed to write Codex request")?;
    writer
        .write_all(b"\n")
        .await
        .context("failed to write Codex request newline")?;
    writer
        .flush()
        .await
        .context("failed to flush Codex request")
}

async fn read_response_with_id<R>(reader: &mut R, expected_id: i64) -> Result<Value>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = String::new();
    let mut skipped = 0usize;
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .await
            .context("failed to read Codex app-server response")?;
        if bytes == 0 {
            return Err(anyhow!(
                "Codex app-server exited before returning rate limits"
            ));
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.starts_with('{') {
            skipped += 1;
            if skipped > 20 {
                return Err(anyhow!("Codex app-server emitted too many non-JSON lines"));
            }
            continue;
        }

        let value: Value =
            serde_json::from_str(trimmed).context("failed to parse Codex app-server JSON")?;
        if value.get("id").and_then(Value::as_i64) != Some(expected_id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Codex app-server rate limit request failed");
            return Err(anyhow!("{message}"));
        }
        return Ok(value);
    }
}

pub(crate) fn parse_codex_rate_limits(
    account: &AccountConfig,
    value: &serde_json::Value,
) -> UsageSnapshot {
    let snapshots = extract_app_server_snapshots(value);
    if !snapshots.is_empty() {
        return parse_codex_app_server_snapshots(account, snapshots);
    }

    parse_codex_legacy_rate_limits(account, value)
}

fn parse_codex_app_server_snapshots(
    account: &AccountConfig,
    snapshots: Vec<CodexRateLimitSnapshot>,
) -> UsageSnapshot {
    let mut buckets = Vec::new();
    let mut subscription = None;
    let mut reached_type = None;
    for (index, snapshot) in snapshots.iter().enumerate() {
        if subscription.is_none() {
            subscription = subscription_from_app_server(snapshot.plan_type.as_deref());
        }
        if reached_type.is_none() {
            reached_type = snapshot
                .rate_limit_reached_type
                .as_ref()
                .filter(|reason| !reason.is_empty())
                .cloned();
        }

        let prefix = if index == 0 {
            None
        } else {
            Some(limit_group_label(snapshot, index))
        };
        let id_prefix = snapshot.limit_id.as_deref().unwrap_or(if index == 0 {
            "codex"
        } else {
            "codex-extra"
        });
        if let Some(primary) = snapshot.primary.as_ref() {
            buckets.push(bucket_from_window(
                format!("{id_prefix}-primary"),
                prefix.as_deref(),
                "Session",
                primary,
            ));
        }
        if let Some(secondary) = snapshot.secondary.as_ref() {
            buckets.push(bucket_from_window(
                format!("{id_prefix}-secondary"),
                prefix.as_deref(),
                "Weekly",
                secondary,
            ));
        }
        if let Some(credits) = snapshot.credits.as_ref()
            && let Some(bucket) = bucket_from_credits(credits)
        {
            buckets.push(bucket);
        }
    }

    let quota = primary_quota(&buckets);
    let mut status = overall_status(&buckets);
    if reached_type.is_some() {
        status = crate::models::SnapshotStatus::Exhausted;
    }

    UsageSnapshot {
        account_id: account.id.clone(),
        provider: account.provider,
        label: account.label.clone(),
        status,
        subscription,
        usage_buckets: buckets,
        quota,
        message: reached_type.map(|reason| format!("Rate limit reached: {reason}")),
        fetched_at: Utc::now(),
    }
}

fn extract_app_server_snapshots(value: &Value) -> Vec<CodexRateLimitSnapshot> {
    let result = value.get("result").unwrap_or(value);
    if let Some(limits) = result
        .get("rateLimitsByLimitId")
        .or_else(|| result.get("rate_limits_by_limit_id"))
        .and_then(Value::as_object)
    {
        let mut values = Vec::new();
        if let Some(codex) = limits.get("codex") {
            values.push(codex.clone());
            let mut extras = limits
                .iter()
                .filter(|(key, _)| key.as_str() != "codex")
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>();
            extras.sort_by_key(|value| {
                value
                    .get("limitName")
                    .or_else(|| value.get("limit_name"))
                    .or_else(|| value.get("limitId"))
                    .or_else(|| value.get("limit_id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            });
            values.extend(extras);
        } else if limits.len() == 1 {
            values.extend(limits.values().cloned());
        }

        return values
            .into_iter()
            .filter_map(app_server_snapshot_from_value)
            .collect();
    }

    let result = result
        .get("rateLimits")
        .cloned()
        .or_else(|| result.get("rate_limits").cloned())
        .unwrap_or_else(|| result.clone());

    app_server_snapshot_from_value(result).into_iter().collect()
}

fn app_server_snapshot_from_value(value: Value) -> Option<CodexRateLimitSnapshot> {
    if value.get("primary").is_none()
        && value.get("secondary").is_none()
        && value.get("planType").is_none()
        && value.get("plan_type").is_none()
    {
        return None;
    }

    serde_json::from_value(value).ok()
}

fn bucket_from_window(
    id: impl Into<String>,
    prefix: Option<&str>,
    fallback_label: &str,
    window: &CodexRateLimitWindow,
) -> UsageBucketSnapshot {
    let used = f64::from(window.used_percent.clamp(0, 100));
    let label = label_for_window(fallback_label, window.window_duration_mins);
    bucket_from_parts(
        id,
        prefix
            .map(|prefix| format!("{prefix} {label}"))
            .unwrap_or(label),
        window.window_duration_mins.map(window_duration_label),
        QuotaSnapshot {
            used,
            limit: Some(100.0),
            remaining: Some((100.0 - used).max(0.0)),
            unit: "%".to_string(),
            reset_at: window.resets_at.and_then(timestamp_codex),
        },
    )
}

fn limit_group_label(snapshot: &CodexRateLimitSnapshot, index: usize) -> String {
    snapshot
        .limit_name
        .as_deref()
        .and_then(compact_limit_name)
        .or_else(|| snapshot.limit_id.as_deref().and_then(compact_limit_name))
        .unwrap_or_else(|| format!("Limit {}", index + 1))
}

fn compact_limit_name(value: &str) -> Option<String> {
    let normalized = value.replace(['_', '-'], " ");
    if normalized.to_ascii_lowercase().contains("spark") {
        return Some("Spark".to_string());
    }
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(normalized.trim().to_string())
    }
}

fn bucket_from_credits(credits: &CodexCreditsSnapshot) -> Option<UsageBucketSnapshot> {
    if !credits.has_credits || credits.unlimited {
        return None;
    }
    let balance = credits
        .balance
        .as_deref()
        .and_then(|balance| balance.parse::<f64>().ok())?;
    Some(bucket_from_parts(
        "codex-credits",
        "Credits",
        None,
        QuotaSnapshot {
            used: balance,
            limit: None,
            remaining: Some(balance),
            unit: "$".to_string(),
            reset_at: None,
        },
    ))
}

fn label_for_window(fallback: &str, duration_mins: Option<i64>) -> String {
    match duration_mins {
        Some(300) => "5-hour".to_string(),
        Some(10080) => "Weekly".to_string(),
        Some(43200) => "Monthly".to_string(),
        _ => fallback.to_string(),
    }
}

fn window_duration_label(duration_mins: i64) -> String {
    match duration_mins {
        300 => "5 hours".to_string(),
        10080 => "7 days".to_string(),
        43200 => "30 days".to_string(),
        minutes if minutes >= 1440 => format!("{} days", minutes / 1440),
        minutes if minutes >= 60 => format!("{} hours", minutes / 60),
        minutes => format!("{minutes} minutes"),
    }
}

fn timestamp_codex(value: i64) -> Option<chrono::DateTime<Utc>> {
    if value.abs() < 10_000_000_000 {
        Utc.timestamp_opt(value, 0).single()
    } else {
        Utc.timestamp_millis_opt(value).single()
    }
}

fn subscription_from_app_server(plan_type: Option<&str>) -> Option<SubscriptionSnapshot> {
    subscription_from_json(
        &json!({ "planType": plan_type }),
        "codex-app-server",
        &["/planType", "/plan"],
    )
}

fn parse_codex_legacy_rate_limits(
    account: &AccountConfig,
    value: &serde_json::Value,
) -> UsageSnapshot {
    let result = value.get("result").unwrap_or(value);
    let mut buckets = parse_usage_buckets(value, "requests");
    if buckets.is_empty() {
        let limit = number(result, &["/limit", "/quota/limit", "/rate_limits/0/limit"]);
        let remaining = number(
            result,
            &["/remaining", "/quota/remaining", "/rate_limits/0/remaining"],
        );
        let used = number(result, &["/used", "/quota/used", "/rate_limits/0/used"])
            .or_else(|| {
                limit
                    .zip(remaining)
                    .map(|(limit, remaining)| limit - remaining)
            })
            .unwrap_or(0.0);
        if limit.is_some() || remaining.is_some() || used > 0.0 {
            let reset_at = datetime(
                result,
                &["/reset_at", "/quota/reset_at", "/rate_limits/0/reset_at"],
            );
            buckets.push(bucket_from_parts(
                "requests",
                "Requests",
                None,
                QuotaSnapshot {
                    used,
                    limit,
                    remaining,
                    unit: "requests".to_string(),
                    reset_at,
                },
            ));
        }
    }

    let subscription = subscription_from_json(
        value,
        "codex-app-server",
        &[
            "/result/plan",
            "/result/plan_type",
            "/result/subscription/plan",
            "/result/account/plan",
            "/plan",
            "/plan_type",
            "/subscription/plan",
            "/account/plan",
        ],
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
        message: None,
        fetched_at: Utc::now(),
    }
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
        account_with_id("codex-local")
    }

    fn account_with_id(id: &str) -> AccountConfig {
        AccountConfig {
            id: id.to_string(),
            provider: ProviderKind::Codex,
            label: "Codex".to_string(),
            enabled: true,
            auto_detected: true,
            credential_path: None,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Plaintext,
            keyring_account: None,
            plaintext_secret: Some("token".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn maps_json_rpc_rate_limits() {
        let snapshot = parse_codex_rate_limits(
            &account(),
            &json!({
                "jsonrpc": "2.0",
                "result": {
                    "rate_limits": [
                        {
                            "limit": 100,
                            "remaining": 4,
                            "reset_at": "2026-06-01T12:00:00Z"
                        }
                    ]
                }
            }),
        );

        assert_eq!(snapshot.status, SnapshotStatus::Exhausted);
        assert_eq!(snapshot.quota.unwrap().used, 96.0);
        assert_eq!(snapshot.usage_buckets[0].label, "Requests");
    }

    #[test]
    fn maps_codex_app_server_subscription_buckets() {
        let snapshot = parse_codex_rate_limits(
            &account(),
            &json!({
                "id": 2,
                "result": {
                    "rateLimits": {
                        "planType": "pro",
                        "primary": {
                            "usedPercent": 96,
                            "resetsAt": 1780000000000_i64,
                            "windowDurationMins": 300
                        },
                        "secondary": {
                            "usedPercent": 40,
                            "resetsAt": 1780600000000_i64,
                            "windowDurationMins": 10080
                        },
                        "credits": {
                            "balance": "12.50",
                            "hasCredits": true,
                            "unlimited": false
                        }
                    }
                }
            }),
        );

        let subscription = snapshot.subscription.unwrap();
        assert_eq!(subscription.plan, SubscriptionPlan::Pro);
        assert_eq!(snapshot.status, SnapshotStatus::Exhausted);
        assert_eq!(snapshot.usage_buckets.len(), 3);
        assert_eq!(snapshot.usage_buckets[0].label, "5-hour");
        assert_eq!(snapshot.usage_buckets[1].label, "Weekly");
        assert_eq!(snapshot.usage_buckets[2].label, "Credits");
    }

    #[test]
    fn maps_codex_app_server_extra_spark_buckets() {
        let snapshot = parse_codex_rate_limits(
            &account(),
            &json!({
                "id": 2,
                "result": {
                    "rateLimits": {
                        "limitId": "codex",
                        "planType": "pro",
                        "primary": {
                            "usedPercent": 51,
                            "resetsAt": 1780416091_i64,
                            "windowDurationMins": 300
                        },
                        "secondary": {
                            "usedPercent": 38,
                            "resetsAt": 1780925866_i64,
                            "windowDurationMins": 10080
                        }
                    },
                    "rateLimitsByLimitId": {
                        "codex": {
                            "limitId": "codex",
                            "planType": "pro",
                            "primary": {
                                "usedPercent": 51,
                                "resetsAt": 1780416091_i64,
                                "windowDurationMins": 300
                            },
                            "secondary": {
                                "usedPercent": 38,
                                "resetsAt": 1780925866_i64,
                                "windowDurationMins": 10080
                            }
                        },
                        "codex_bengalfox": {
                            "limitId": "codex_bengalfox",
                            "limitName": "GPT-5.3-Codex-Spark",
                            "planType": "pro",
                            "primary": {
                                "usedPercent": 0,
                                "resetsAt": 1780424416_i64,
                                "windowDurationMins": 300
                            },
                            "secondary": {
                                "usedPercent": 0,
                                "resetsAt": 1781011216_i64,
                                "windowDurationMins": 10080
                            }
                        }
                    }
                }
            }),
        );

        assert_eq!(snapshot.usage_buckets.len(), 4);
        assert_eq!(snapshot.usage_buckets[0].label, "5-hour");
        assert_eq!(snapshot.usage_buckets[1].label, "Weekly");
        assert_eq!(snapshot.usage_buckets[2].label, "Spark 5-hour");
        assert_eq!(snapshot.usage_buckets[3].label, "Spark Weekly");
        assert_eq!(
            snapshot.usage_buckets[0].reset_at.unwrap().timestamp(),
            1_780_416_091
        );
    }

    #[test]
    fn compact_limit_name_normalizes_non_spark_names() {
        assert_eq!(
            compact_limit_name("codex_bengalfox"),
            Some("codex bengalfox".to_string())
        );
        assert_eq!(
            compact_limit_name("codex-team-alpha"),
            Some("codex team alpha".to_string())
        );
        assert_eq!(compact_limit_name("  "), None);
    }

    #[test]
    fn ignores_ambiguous_app_server_limit_maps_without_codex_key() {
        let snapshot = parse_codex_rate_limits(
            &account(),
            &json!({
                "id": 2,
                "result": {
                    "rateLimitsByLimitId": {
                        "chat": {
                            "planType": "pro",
                            "primary": {
                                "usedPercent": 99,
                                "windowDurationMins": 300
                            }
                        },
                        "gpt-image": {
                            "planType": "pro",
                            "primary": {
                                "usedPercent": 10,
                                "windowDurationMins": 300
                            }
                        }
                    }
                }
            }),
        );

        assert!(snapshot.usage_buckets.is_empty());
    }

    #[tokio::test]
    async fn fetches_json_rpc_rate_limits_with_http_override() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .and(header("authorization", "Bearer token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "result": {
                    "rate_limits": [
                        { "limit": 120, "remaining": 24 }
                    ]
                }
            })))
            .mount(&server)
            .await;

        let mut account = account();
        account.endpoint_override = Some(server.uri());
        let snapshot = fetch(&Client::new(), &account).await.unwrap();

        assert_eq!(snapshot.status, SnapshotStatus::Warning);
        assert_eq!(snapshot.quota.unwrap().used, 96.0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reads_codex_app_server_rate_limits_from_stdio() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("codex-fixture");
        std::fs::write(
            &binary,
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *account/rateLimits/read*)
      echo '{"id":2,"result":{"rateLimits":{"planType":"pro","primary":{"usedPercent":10,"resetsAt":1780000000000,"windowDurationMins":300}}}}'
      exit 0
      ;;
  esac
done
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&binary).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary, perms).unwrap();

        let value = read_codex_app_server_rate_limits_with_binary(&binary.to_string_lossy())
            .await
            .unwrap();

        assert_eq!(value["result"]["rateLimits"]["planType"], "pro");
    }
}
