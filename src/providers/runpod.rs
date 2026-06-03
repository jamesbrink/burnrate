use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::{Duration as ChronoDuration, Utc};
use reqwest::{Client, Url};
use serde_json::json;

use crate::models::{AccountConfig, SnapshotStatus, UsageBucketSnapshot, UsageSnapshot};

use super::{bool_value, endpoint, number, primary_quota, require_token, text, validate_endpoint};

const DEFAULT_REST_ENDPOINT: &str = "https://rest.runpod.io/v1";
const DEFAULT_GRAPHQL_ENDPOINT: &str = "https://api.runpod.io/graphql";
const RUNPOD_STOP_PROTECTION_SECONDS: f64 = 10.0;
const WARNING_RUNWAY_SECONDS: f64 = 60.0 * 60.0;
const ENRICHMENT_TIMEOUT: Duration = Duration::from_secs(3);

const MYSELF_QUERY: &str = r#"
query BurnrateMyself {
  myself {
    clientBalance
    currentSpendPerHr
    spendLimit
    minBalance
    underBalance
  }
}
"#;

#[derive(Debug, Clone, Copy, Default)]
struct RunpodAccountState {
    balance: Option<f64>,
    current_spend_per_hr: Option<f64>,
    spend_limit: Option<f64>,
    min_balance: Option<f64>,
    under_balance: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default)]
struct BillingSummary {
    pods_24h: Option<f64>,
    serverless_24h: Option<f64>,
    storage_24h: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ResourceSummary {
    pods: usize,
    active_pods: usize,
    endpoints: usize,
    active_endpoints: usize,
}

pub(crate) async fn fetch(http: &Client, account: &AccountConfig) -> Result<UsageSnapshot> {
    let token = require_token(account)?;
    let rest_base = endpoint(account, "BURNRATE_RUNPOD_REST_URL", DEFAULT_REST_ENDPOINT)?;
    let graphql_url = graphql_endpoint(account, &rest_base)?;

    let account_json = fetch_graphql_myself(http, &token, graphql_url).await?;
    let account_state = parse_account_state(&account_json);
    if account_state.balance.is_none() {
        return Err(anyhow!("Runpod account state missing clientBalance"));
    }

    // Billing/resource calls enrich the snapshot, but the GraphQL account state
    // is the source of truth for health. Keep a balance snapshot available even
    // if one of the REST attribution endpoints is temporarily unavailable or
    // changes shape.
    let pods_url = rest_url(&rest_base, "pods")?;
    let endpoints_url = rest_url(&rest_base, "endpoints")?;
    let token_ref = token.as_str();
    let rest_base_ref = rest_base.as_str();
    let (pods_24h, serverless_24h, storage_24h, pods, endpoints) = tokio::join!(
        async {
            tokio::time::timeout(
                ENRICHMENT_TIMEOUT,
                fetch_billing_sum(http, token_ref, rest_base_ref, "billing/pods"),
            )
            .await
            .ok()
            .and_then(|result| result.ok())
        },
        async {
            tokio::time::timeout(
                ENRICHMENT_TIMEOUT,
                fetch_billing_sum(http, token_ref, rest_base_ref, "billing/endpoints"),
            )
            .await
            .ok()
            .and_then(|result| result.ok())
        },
        async {
            tokio::time::timeout(
                ENRICHMENT_TIMEOUT,
                fetch_billing_sum(http, token_ref, rest_base_ref, "billing/networkvolumes"),
            )
            .await
            .ok()
            .and_then(|result| result.ok())
        },
        async move {
            tokio::time::timeout(ENRICHMENT_TIMEOUT, fetch_json(http, token_ref, pods_url))
                .await
                .ok()
                .and_then(|result| result.ok())
        },
        async move {
            tokio::time::timeout(
                ENRICHMENT_TIMEOUT,
                fetch_json(http, token_ref, endpoints_url),
            )
            .await
            .ok()
            .and_then(|result| result.ok())
        },
    );

    Ok(build_snapshot(
        account,
        account_state,
        BillingSummary {
            pods_24h,
            serverless_24h,
            storage_24h,
        },
        ResourceSummary {
            ..parse_resource_summary(pods.as_ref(), endpoints.as_ref())
        },
    ))
}

async fn fetch_graphql_myself(
    http: &Client,
    token: &str,
    url: String,
) -> Result<serde_json::Value> {
    let value: serde_json::Value = http
        .post(url)
        .bearer_auth(token)
        .json(&json!({ "query": MYSELF_QUERY }))
        .send()
        .await
        .context("failed to fetch Runpod account state")?
        .error_for_status()
        .context("Runpod account state request failed")?
        .json()
        .await
        .context("failed to decode Runpod account state")?;

    if let Some(errors) = value.pointer("/errors").and_then(|value| value.as_array())
        && !errors.is_empty()
    {
        let message = errors
            .iter()
            .filter_map(|error| text(error, &["/message"]))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(anyhow!(
            "Runpod account state request failed{}",
            if message.is_empty() {
                String::new()
            } else {
                format!(": {message}")
            }
        ));
    }

    Ok(value)
}

async fn fetch_billing_sum(http: &Client, token: &str, rest_base: &str, path: &str) -> Result<f64> {
    let mut url = rest_url(rest_base, path)?;
    let end = Utc::now();
    let start = end - ChronoDuration::hours(24);
    url.query_pairs_mut()
        .append_pair("startTime", &start.to_rfc3339())
        .append_pair("endTime", &end.to_rfc3339());

    let value = fetch_json(http, token, url).await?;
    Ok(sum_billing_amounts(&value))
}

async fn fetch_json(http: &Client, token: &str, url: Url) -> Result<serde_json::Value> {
    http.get(url)
        .bearer_auth(token)
        .send()
        .await
        .context("failed to fetch Runpod REST resource")?
        .error_for_status()
        .context("Runpod REST resource request failed")?
        .json()
        .await
        .context("failed to decode Runpod REST resource")
}

fn graphql_endpoint(account: &AccountConfig, rest_base: &str) -> Result<String> {
    if let Ok(value) = std::env::var("BURNRATE_RUNPOD_GRAPHQL_URL") {
        validate_endpoint(&value)?;
        return Ok(value);
    }

    if account.endpoint_override.is_some() {
        let mut url = Url::parse(rest_base)
            .with_context(|| format!("invalid Runpod REST endpoint URL: {rest_base}"))?;
        url.set_path("/graphql");
        url.set_query(None);
        return Ok(url.to_string());
    }

    Ok(DEFAULT_GRAPHQL_ENDPOINT.to_string())
}

fn rest_url(base: &str, path: &str) -> Result<Url> {
    let normalized = if base.ends_with('/') {
        base.to_string()
    } else {
        format!("{base}/")
    };
    Url::parse(&normalized)
        .with_context(|| format!("invalid Runpod REST endpoint URL: {base}"))?
        .join(path.trim_start_matches('/'))
        .with_context(|| format!("invalid Runpod REST path: {path}"))
}

fn parse_account_state(value: &serde_json::Value) -> RunpodAccountState {
    RunpodAccountState {
        balance: number(
            value,
            &[
                "/data/myself/clientBalance",
                "/myself/clientBalance",
                "/clientBalance",
            ],
        ),
        current_spend_per_hr: number(
            value,
            &[
                "/data/myself/currentSpendPerHr",
                "/myself/currentSpendPerHr",
                "/currentSpendPerHr",
            ],
        ),
        spend_limit: number(
            value,
            &[
                "/data/myself/spendLimit",
                "/myself/spendLimit",
                "/spendLimit",
            ],
        )
        .filter(|limit| *limit > 0.0),
        min_balance: number(
            value,
            &[
                "/data/myself/minBalance",
                "/myself/minBalance",
                "/minBalance",
            ],
        ),
        under_balance: bool_value(
            value,
            &[
                "/data/myself/underBalance",
                "/myself/underBalance",
                "/underBalance",
            ],
        ),
    }
}

fn build_snapshot(
    account: &AccountConfig,
    account_state: RunpodAccountState,
    billing: BillingSummary,
    resources: ResourceSummary,
) -> UsageSnapshot {
    let status = runpod_status(account_state);
    let mut buckets = Vec::new();

    if account_state.balance.is_some() {
        buckets.push(UsageBucketSnapshot {
            id: "balance".to_string(),
            label: "Balance".to_string(),
            window: None,
            used: 0.0,
            limit: None,
            remaining: account_state.balance,
            unit: "USD".to_string(),
            reset_at: None,
            status,
        });
    }

    if let Some(current_spend) = account_state.current_spend_per_hr {
        buckets.push(UsageBucketSnapshot {
            id: "current-burn".to_string(),
            label: "Current burn".to_string(),
            window: None,
            used: current_spend,
            limit: account_state.spend_limit,
            remaining: None,
            unit: "USD/hr".to_string(),
            reset_at: None,
            status: burn_status(current_spend, account_state.spend_limit),
        });
    }

    push_spend_bucket(&mut buckets, "pods-24h", "Pods 24h", billing.pods_24h);
    push_spend_bucket(
        &mut buckets,
        "serverless-24h",
        "Serverless 24h",
        billing.serverless_24h,
    );
    push_spend_bucket(
        &mut buckets,
        "storage-24h",
        "Storage 24h",
        billing.storage_24h,
    );

    let quota = primary_quota(&buckets);
    let message = status_message(account_state, resources);

    UsageSnapshot {
        account_id: account.id.clone(),
        provider: account.provider,
        label: account.label.clone(),
        status,
        email: None,
        subscription: None,
        usage_buckets: buckets,
        quota,
        message,
        fetched_at: Utc::now(),
    }
}

fn push_spend_bucket(
    buckets: &mut Vec<UsageBucketSnapshot>,
    id: &str,
    label: &str,
    amount: Option<f64>,
) {
    let Some(amount) = amount else {
        return;
    };
    if amount <= 0.0 {
        return;
    }
    buckets.push(UsageBucketSnapshot {
        id: id.to_string(),
        label: label.to_string(),
        window: Some("24h".to_string()),
        used: amount.max(0.0),
        limit: None,
        remaining: None,
        unit: "USD".to_string(),
        reset_at: None,
        status: SnapshotStatus::Healthy,
    });
}

fn runpod_status(account: RunpodAccountState) -> SnapshotStatus {
    let balance = account.balance.unwrap_or(0.0);
    let min_balance = account.min_balance.unwrap_or(0.0).max(0.0);
    let current_spend = account.current_spend_per_hr.unwrap_or(0.0);

    if account.under_balance.unwrap_or(false)
        || balance <= min_balance
        || account
            .spend_limit
            .is_some_and(|limit| limit > 0.0 && current_spend >= limit)
        || runway_seconds(account).is_some_and(|seconds| seconds <= RUNPOD_STOP_PROTECTION_SECONDS)
    {
        return SnapshotStatus::Exhausted;
    }

    if runway_seconds(account).is_some_and(|seconds| seconds < WARNING_RUNWAY_SECONDS)
        || account
            .spend_limit
            .is_some_and(|limit| limit > 0.0 && current_spend / limit >= 0.8)
    {
        return SnapshotStatus::Warning;
    }

    SnapshotStatus::Healthy
}

fn burn_status(current_spend: f64, spend_limit: Option<f64>) -> SnapshotStatus {
    match spend_limit {
        Some(limit) if limit > 0.0 && current_spend >= limit => SnapshotStatus::Exhausted,
        Some(limit) if limit > 0.0 && current_spend / limit >= 0.8 => SnapshotStatus::Warning,
        _ => SnapshotStatus::Healthy,
    }
}

fn runway_seconds(account: RunpodAccountState) -> Option<f64> {
    let balance = account.balance?;
    let current_spend = account.current_spend_per_hr?;
    if current_spend <= 0.0 {
        return None;
    }
    let usable_balance = (balance - account.min_balance.unwrap_or(0.0).max(0.0)).max(0.0);
    Some((usable_balance / current_spend) * 60.0 * 60.0)
}

fn status_message(account: RunpodAccountState, resources: ResourceSummary) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(balance) = account.balance {
        parts.push(format!("balance {}", format_usd(balance)));
    }
    if let Some(current_spend) = account.current_spend_per_hr {
        parts.push(format!("burn {}/hr", format_usd(current_spend)));
    }
    if let Some(seconds) = runway_seconds(account) {
        parts.push(format!("runway {}", format_runway(seconds)));
    }
    if resources.pods > 0 || resources.endpoints > 0 {
        parts.push(format!(
            "{} active pod{}, {} active endpoint{}",
            resources.active_pods,
            plural(resources.active_pods),
            resources.active_endpoints,
            plural(resources.active_endpoints)
        ));
    }

    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn sum_billing_amounts(value: &serde_json::Value) -> f64 {
    billing_items(value)
        .into_iter()
        .filter_map(|item| number(item, &["/amount", "/cost", "/value", "/total"]))
        .sum::<f64>()
}

fn billing_items(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }

    for pointer in [
        "/data",
        "/data/records",
        "/result",
        "/result/records",
        "/billing",
        "/records",
    ] {
        if let Some(items) = value.pointer(pointer).and_then(|value| value.as_array()) {
            return items.iter().collect();
        }
    }

    Vec::new()
}

fn parse_resource_summary(
    pods: Option<&serde_json::Value>,
    endpoints: Option<&serde_json::Value>,
) -> ResourceSummary {
    let pod_items = resource_items(pods);
    let endpoint_items = resource_items(endpoints);
    ResourceSummary {
        pods: pod_items.len(),
        active_pods: pod_items.iter().filter(|item| is_active_pod(item)).count(),
        endpoints: endpoint_items.len(),
        active_endpoints: endpoint_items
            .iter()
            .filter(|item| is_active_endpoint(item))
            .count(),
    }
}

fn resource_items(value: Option<&serde_json::Value>) -> Vec<&serde_json::Value> {
    let Some(value) = value else {
        return Vec::new();
    };
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }
    for pointer in ["/data", "/result", "/pods", "/endpoints"] {
        if let Some(items) = value.pointer(pointer).and_then(|value| value.as_array()) {
            return items.iter().collect();
        }
    }
    Vec::new()
}

fn is_active_pod(value: &serde_json::Value) -> bool {
    let status = text(value, &["/desiredStatus", "/status", "/runtime/status"])
        .unwrap_or_default()
        .to_ascii_uppercase();
    if status.contains("RUNNING") || status.contains("START") {
        return true;
    }
    if status.contains("STOP") || status.contains("EXIT") || status.contains("TERMINAT") {
        return false;
    }
    text(value, &["/lastStartedAt"]).is_some()
}

fn is_active_endpoint(value: &serde_json::Value) -> bool {
    number(value, &["/workersMin"]).is_some_and(|workers| workers > 0.0)
        || value
            .pointer("/workers")
            .and_then(|value| value.as_array())
            .is_some_and(|workers| !workers.is_empty())
}

fn format_usd(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("${value:.0}")
    } else {
        format!("${value:.2}")
    }
}

fn format_runway(seconds: f64) -> String {
    if seconds < 60.0 {
        return format!("{:.0}s", seconds.max(0.0));
    }
    if seconds < 60.0 * 60.0 {
        return format!("{:.0}m", seconds / 60.0);
    }
    if seconds < 48.0 * 60.0 * 60.0 {
        return format!("{:.1}h", seconds / 60.0 / 60.0);
    }
    format!("{:.0}d", seconds / 60.0 / 60.0 / 24.0)
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
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
    use crate::models::{ProviderKind, SecretStorageMode};

    fn account() -> AccountConfig {
        AccountConfig {
            id: "runpod-main".to_string(),
            provider: ProviderKind::Runpod,
            label: "Runpod".to_string(),
            enabled: true,
            auto_detected: false,
            credential_path: None,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Plaintext,
            keyring_account: None,
            plaintext_secret: Some("rp-test".to_string()),
            email: None,
            config_dir: None,
            order_index: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn maps_graphql_account_state_and_cost_buckets() {
        let state = parse_account_state(&json!({
            "data": {
                "myself": {
                    "clientBalance": "12.50",
                    "currentSpendPerHr": 2.5,
                    "spendLimit": 10.0,
                    "minBalance": 0.0,
                    "underBalance": false
                }
            }
        }));
        let snapshot = build_snapshot(
            &account(),
            state,
            BillingSummary {
                pods_24h: Some(3.25),
                serverless_24h: Some(1.5),
                storage_24h: Some(0.75),
            },
            ResourceSummary {
                pods: 2,
                active_pods: 1,
                endpoints: 1,
                active_endpoints: 1,
            },
        );

        assert_eq!(snapshot.status, SnapshotStatus::Healthy);
        assert_eq!(snapshot.quota.unwrap().remaining, Some(12.5));
        assert_eq!(snapshot.usage_buckets.len(), 5);
        assert_eq!(snapshot.usage_buckets[1].id, "current-burn");
        assert_eq!(snapshot.usage_buckets[2].used, 3.25);
        assert!(snapshot.message.unwrap().contains("runway 5.0h"));

        let zero_limit_state = parse_account_state(&json!({
            "data": {
                "myself": {
                    "clientBalance": 12.5,
                    "currentSpendPerHr": 2.5,
                    "spendLimit": 0.0
                }
            }
        }));
        assert_eq!(zero_limit_state.spend_limit, None);
    }

    #[test]
    fn derives_low_balance_status_from_runway_and_under_balance() {
        let low_runway = RunpodAccountState {
            balance: Some(1.0),
            current_spend_per_hr: Some(10.0),
            min_balance: Some(0.0),
            ..Default::default()
        };
        assert_eq!(runpod_status(low_runway), SnapshotStatus::Warning);

        let stop_threshold = RunpodAccountState {
            balance: Some(0.001),
            current_spend_per_hr: Some(10.0),
            min_balance: Some(0.0),
            ..Default::default()
        };
        assert_eq!(runpod_status(stop_threshold), SnapshotStatus::Exhausted);

        let under_balance = RunpodAccountState {
            balance: Some(100.0),
            current_spend_per_hr: Some(0.0),
            under_balance: Some(true),
            ..Default::default()
        };
        assert_eq!(runpod_status(under_balance), SnapshotStatus::Exhausted);

        let over_spend_limit = RunpodAccountState {
            balance: Some(100.0),
            current_spend_per_hr: Some(80.0),
            spend_limit: Some(80.0),
            ..Default::default()
        };
        assert_eq!(runpod_status(over_spend_limit), SnapshotStatus::Exhausted);
    }

    #[test]
    fn sums_billing_records_from_common_shapes() {
        let amount = sum_billing_amounts(&json!({
            "data": [
                { "amount": "1.25", "timeBilledMs": 1000 },
                { "amount": 2.5, "timeBilledMs": 1000 }
            ]
        }));

        assert_eq!(amount, 3.75);
    }

    #[test]
    fn counts_active_resources() {
        let resources = parse_resource_summary(
            Some(&json!({
                "data": [
                    { "desiredStatus": "RUNNING" },
                    { "desiredStatus": "STOPPED" }
                ]
            })),
            Some(&json!({
                "data": [
                    { "workersMin": 0, "workers": [] },
                    { "workersMin": 1, "workers": [] }
                ]
            })),
        );

        assert_eq!(resources.pods, 2);
        assert_eq!(resources.active_pods, 1);
        assert_eq!(resources.endpoints, 2);
        assert_eq!(resources.active_endpoints, 1);
    }

    #[tokio::test]
    async fn fetches_runpod_snapshot_with_manual_key_and_override() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(header("authorization", "Bearer rp-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "myself": {
                        "clientBalance": 25.0,
                        "currentSpendPerHr": 5.0,
                        "spendLimit": 20.0,
                        "minBalance": 0.0,
                        "underBalance": false
                    }
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/billing/pods"))
            .and(header("authorization", "Bearer rp-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "amount": 4.0 }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/pods"))
            .and(header("authorization", "Bearer rp-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "desiredStatus": "RUNNING" }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/endpoints"))
            .and(header("authorization", "Bearer rp-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "workersMin": 1 }]
            })))
            .mount(&server)
            .await;

        let mut account = account();
        account.endpoint_override = Some(server.uri());
        let snapshot = fetch(&Client::new(), &account).await.unwrap();

        assert_eq!(snapshot.status, SnapshotStatus::Healthy);
        assert_eq!(snapshot.quota.unwrap().remaining, Some(25.0));
        assert!(
            snapshot
                .usage_buckets
                .iter()
                .any(|bucket| bucket.id == "pods-24h")
        );
    }

    #[tokio::test]
    async fn fetch_requires_graphql_balance() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "myself": {
                        "currentSpendPerHr": 5.0,
                        "spendLimit": 20.0
                    }
                }
            })))
            .mount(&server)
            .await;

        let mut account = account();
        account.endpoint_override = Some(server.uri());
        let error = fetch(&Client::new(), &account)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("missing clientBalance"));
    }

    #[tokio::test]
    async fn fetch_rejects_untrusted_http_endpoint_override() {
        let mut account = account();
        account.endpoint_override = Some("http://example.test".to_string());

        let error = fetch(&Client::new(), &account)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("HTTPS"));
    }

    #[tokio::test]
    async fn fetch_surfaces_graphql_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "errors": [{ "message": "bad token" }]
            })))
            .mount(&server)
            .await;

        let mut account = account();
        account.endpoint_override = Some(server.uri());
        let error = fetch(&Client::new(), &account)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("bad token"));
    }
}
