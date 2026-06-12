//! GitHub Copilot premium-request usage.
//!
//! Copilot bills by premium requests per calendar month (reset on the 1st,
//! 00:00 UTC), not tokens. Without credentials, usage is counted from local
//! Copilot CLI session logs via the claudex index — a lower-bound estimate
//! (CLI sessions on this machine only, and only ones that shut down cleanly).
//! With an optional GitHub token (classic PAT; the billing endpoints do not
//! support fine-grained tokens), usage comes authoritatively from GitHub's
//! premium-request billing report; on billing-API failure the snapshot falls
//! back to the local estimate with an explanatory note rather than erroring.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use reqwest::Client;

use crate::{
    config::default_auto_account,
    insights,
    models::{
        AccountConfig, CopilotPlan, ProviderKind, QuotaSnapshot, SubscriptionPlan,
        SubscriptionSnapshot, UsageSnapshot,
    },
};

const DEFAULT_API_BASE: &str = "https://api.github.com";
const GITHUB_API_VERSION: &str = "2026-03-10";

/// Auto-detect a local Copilot CLI installation: a `~/.copilot/session-state`
/// directory with at least one session. `CLAUDEX_COPILOT_DIR` overrides the
/// base dir so detection and the claudex indexer always look at the same tree.
pub(crate) fn detect() -> Option<AccountConfig> {
    let base = std::env::var("CLAUDEX_COPILOT_DIR")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|home| home.join(".copilot")))?;
    detect_in(&base)
}

fn detect_in(base: &Path) -> Option<AccountConfig> {
    let sessions = base.join("session-state");
    let has_sessions = std::fs::read_dir(&sessions)
        .ok()?
        .flatten()
        .any(|entry| entry.path().is_dir());
    if !has_sessions {
        return None;
    }

    Some(default_auto_account(
        "copilot-local",
        ProviderKind::Copilot,
        "GitHub Copilot",
        sessions,
    ))
}

pub(crate) async fn fetch(http: &Client, account: &AccountConfig) -> Result<UsageSnapshot> {
    let now = Utc::now();

    if let Some(token) = super::token_from_config(account)? {
        match billing_premium_requests(http, account, &token, now).await {
            Ok(used) => {
                return Ok(snapshot_from_premium_requests(
                    account,
                    used,
                    now,
                    UsageSource::Billing,
                ));
            }
            Err(error) => {
                // User-facing policy: a dead token or flaky network degrades
                // to the still-useful local estimate with a note, instead of
                // a red error card or a false quota alarm.
                let used = insights::copilot_premium_requests_mtd().await? as f64;
                return Ok(snapshot_from_premium_requests(
                    account,
                    used,
                    now,
                    UsageSource::BillingFallback {
                        reason: error.to_string(),
                    },
                ));
            }
        }
    }

    let used = insights::copilot_premium_requests_mtd().await? as f64;
    Ok(snapshot_from_premium_requests(
        account,
        used,
        now,
        UsageSource::LocalEstimate,
    ))
}

/// Where the premium-request count came from; drives the snapshot message and
/// subscription source so the UI never overstates the data's authority.
enum UsageSource {
    Billing,
    LocalEstimate,
    BillingFallback { reason: String },
}

impl UsageSource {
    fn message(&self) -> String {
        match self {
            UsageSource::Billing => "Premium requests from GitHub billing.".to_string(),
            UsageSource::LocalEstimate => {
                "Local estimate — counts Copilot CLI sessions on this Mac only.".to_string()
            }
            UsageSource::BillingFallback { reason } => {
                format!("GitHub billing unavailable ({reason}); showing local CLI estimate.")
            }
        }
    }

    fn subscription_source(&self) -> &'static str {
        match self {
            UsageSource::Billing => "github-billing",
            UsageSource::LocalEstimate | UsageSource::BillingFallback { .. } => {
                "copilot-local-estimate"
            }
        }
    }
}

/// Month-to-date premium requests from GitHub's enhanced-billing report:
/// resolve the token's login, then sum `usageItems[].grossQuantity` for the
/// current year/month (gross includes the plan's included allowance, which is
/// exactly what counts against the monthly quota).
async fn billing_premium_requests(
    http: &Client,
    account: &AccountConfig,
    token: &str,
    now: DateTime<Utc>,
) -> Result<f64> {
    let base = super::endpoint(account, "BURNRATE_GITHUB_API_URL", DEFAULT_API_BASE)?;
    let base = base.trim_end_matches('/');

    let login = github_login(http, base, token).await?;
    let url = format!(
        "{base}/users/{login}/settings/billing/premium_request/usage?year={}&month={}",
        now.year(),
        now.month()
    );
    let value = github_get_json(http, &url, token).await?;
    premium_requests_from_billing(&value)
        .ok_or_else(|| anyhow!("GitHub billing response had no usageItems"))
}

async fn github_login(http: &Client, base: &str, token: &str) -> Result<String> {
    let value = github_get_json(http, &format!("{base}/user"), token).await?;
    value
        .pointer("/login")
        .and_then(|login| login.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("GitHub /user response had no login"))
}

async fn github_get_json(http: &Client, url: &str, token: &str) -> Result<serde_json::Value> {
    let response = http
        .get(url)
        .bearer_auth(token)
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", GITHUB_API_VERSION)
        .header("user-agent", "burnrate")
        .send()
        .await
        .context("GitHub API request failed")?;
    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!("GitHub API returned {status}"));
    }
    response
        .json::<serde_json::Value>()
        .await
        .context("GitHub API returned invalid JSON")
}

/// Sum of `grossQuantity` across the report's usage items. `None` when the
/// response carries no `usageItems` array at all (unexpected shape); an empty
/// array is a real zero.
fn premium_requests_from_billing(value: &serde_json::Value) -> Option<f64> {
    let items = value.pointer("/usageItems")?.as_array()?;
    Some(
        items
            .iter()
            .filter_map(|item| item.pointer("/grossQuantity").and_then(|v| v.as_f64()))
            .sum(),
    )
}

/// The account's effective monthly premium-request allowance, if configured.
fn premium_limit(account: &AccountConfig) -> Option<f64> {
    match account.copilot_plan? {
        CopilotPlan::Custom => account.copilot_custom_limit,
        plan => plan.monthly_limit(),
    }
}

fn snapshot_from_premium_requests(
    account: &AccountConfig,
    used: f64,
    now: DateTime<Utc>,
    source: UsageSource,
) -> UsageSnapshot {
    let limit = premium_limit(account);
    let bucket = super::bucket_from_parts(
        "copilot-premium-mtd",
        "Premium requests",
        Some("monthly".to_string()),
        QuotaSnapshot {
            used,
            limit,
            remaining: limit.map(|limit| (limit - used).max(0.0)),
            unit: "requests".to_string(),
            reset_at: Some(first_of_next_month_utc(now)),
        },
    );
    let buckets = vec![bucket];

    UsageSnapshot {
        account_id: account.id.clone(),
        provider: account.provider,
        label: account.label.clone(),
        status: super::overall_status(&buckets),
        email: account.email.clone(),
        subscription: subscription(account, &source),
        quota: super::primary_quota(&buckets),
        usage_buckets: buckets,
        message: Some(source.message()),
        fetched_at: now,
    }
}

fn subscription(account: &AccountConfig, source: &UsageSource) -> Option<SubscriptionSnapshot> {
    let plan = account.copilot_plan?;
    let mapped = match plan {
        CopilotPlan::Free => SubscriptionPlan::Free,
        CopilotPlan::Pro | CopilotPlan::ProPlus => SubscriptionPlan::Pro,
        CopilotPlan::Business => SubscriptionPlan::Team,
        CopilotPlan::Enterprise => SubscriptionPlan::Enterprise,
        CopilotPlan::Custom => SubscriptionPlan::Unknown,
    };
    Some(SubscriptionSnapshot {
        plan: mapped,
        plan_label: plan.label().to_string(),
        rate_limit_tier: None,
        extra_usage_enabled: None,
        source: source.subscription_source().to_string(),
    })
}

/// Copilot premium-request allowances reset on the first of the month at
/// 00:00 UTC.
fn first_of_next_month_utc(now: DateTime<Utc>) -> DateTime<Utc> {
    let (year, month) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };
    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0)
        .single()
        .expect("first of month is a valid UTC instant")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use serde_json::json;
    use tempfile::tempdir;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::models::{SecretStorageMode, SnapshotStatus};

    fn account(plan: Option<CopilotPlan>) -> AccountConfig {
        AccountConfig {
            id: "copilot-local".to_string(),
            provider: ProviderKind::Copilot,
            label: "GitHub Copilot".to_string(),
            enabled: true,
            auto_detected: true,
            credential_path: None,
            endpoint_override: None,
            secret_storage: SecretStorageMode::Plaintext,
            keyring_account: None,
            plaintext_secret: None,
            email: None,
            config_dir: None,
            aws_profile: None,
            aws_region: None,
            aws_monthly_budget_usd: None,
            aws_categories: Vec::new(),
            copilot_plan: plan,
            copilot_custom_limit: None,
            order_index: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn detects_copilot_cli_with_sessions() {
        let dir = tempdir().unwrap();
        let session = dir.path().join("session-state").join("abc-123");
        fs::create_dir_all(&session).unwrap();

        let account = detect_in(dir.path()).expect("session dir should be detected");

        assert_eq!(account.id, "copilot-local");
        assert_eq!(account.provider, ProviderKind::Copilot);
        assert!(account.auto_detected);
        assert!(account.enabled);
        assert_eq!(
            account.credential_path.as_deref(),
            Some(dir.path().join("session-state").to_str().unwrap())
        );
    }

    #[test]
    fn does_not_detect_without_sessions() {
        let dir = tempdir().unwrap();
        // No session-state dir at all.
        assert!(detect_in(dir.path()).is_none());

        // An empty session-state dir is not a signed-in Copilot CLI either.
        fs::create_dir_all(dir.path().join("session-state")).unwrap();
        assert!(detect_in(dir.path()).is_none());

        // A stray file does not count as a session.
        fs::write(dir.path().join("session-state").join("stray.txt"), "x").unwrap();
        assert!(detect_in(dir.path()).is_none());
    }

    #[test]
    fn premium_limit_resolves_plans_and_custom_limits() {
        assert_eq!(premium_limit(&account(None)), None);
        assert_eq!(premium_limit(&account(Some(CopilotPlan::Free))), Some(50.0));
        assert_eq!(premium_limit(&account(Some(CopilotPlan::Pro))), Some(300.0));
        assert_eq!(
            premium_limit(&account(Some(CopilotPlan::ProPlus))),
            Some(1500.0)
        );
        assert_eq!(
            premium_limit(&account(Some(CopilotPlan::Business))),
            Some(300.0)
        );
        assert_eq!(
            premium_limit(&account(Some(CopilotPlan::Enterprise))),
            Some(1000.0)
        );

        // Custom without a limit set is usage-only.
        assert_eq!(premium_limit(&account(Some(CopilotPlan::Custom))), None);
        let mut custom = account(Some(CopilotPlan::Custom));
        custom.copilot_custom_limit = Some(2500.0);
        assert_eq!(premium_limit(&custom), Some(2500.0));
    }

    #[test]
    fn snapshot_reflects_quota_thresholds_per_plan() {
        let now = Utc::now();
        let pro = account(Some(CopilotPlan::Pro));

        let healthy = snapshot_from_premium_requests(&pro, 100.0, now, UsageSource::LocalEstimate);
        assert_eq!(healthy.status, SnapshotStatus::Healthy);
        assert_eq!(healthy.quota.as_ref().unwrap().remaining, Some(200.0));

        // 250/300 leaves 16.7% — Warning territory.
        let warning = snapshot_from_premium_requests(&pro, 250.0, now, UsageSource::LocalEstimate);
        assert_eq!(warning.status, SnapshotStatus::Warning);

        // 295/300 leaves 1.7% — Exhausted.
        let exhausted =
            snapshot_from_premium_requests(&pro, 295.0, now, UsageSource::LocalEstimate);
        assert_eq!(exhausted.status, SnapshotStatus::Exhausted);

        // Overage never reports negative remaining.
        let over = snapshot_from_premium_requests(&pro, 360.0, now, UsageSource::LocalEstimate);
        assert_eq!(over.quota.as_ref().unwrap().remaining, Some(0.0));

        let subscription = healthy.subscription.as_ref().unwrap();
        assert_eq!(subscription.plan, SubscriptionPlan::Pro);
        assert_eq!(subscription.plan_label, "Pro");
        assert_eq!(subscription.source, "copilot-local-estimate");
    }

    #[test]
    fn snapshot_without_plan_is_usage_only_and_labeled_an_estimate() {
        let snapshot = snapshot_from_premium_requests(
            &account(None),
            42.0,
            Utc::now(),
            UsageSource::LocalEstimate,
        );

        assert_eq!(snapshot.status, SnapshotStatus::Healthy);
        let quota = snapshot.quota.as_ref().unwrap();
        assert_eq!(quota.used, 42.0);
        assert_eq!(quota.limit, None);
        assert_eq!(quota.remaining, None);
        assert!(snapshot.subscription.is_none());
        assert!(snapshot.message.as_deref().unwrap().contains("estimate"));
        assert_eq!(snapshot.usage_buckets.len(), 1);
        assert_eq!(snapshot.usage_buckets[0].unit, "requests");
        assert_eq!(snapshot.usage_buckets[0].window.as_deref(), Some("monthly"));
    }

    #[test]
    fn resets_on_the_first_of_next_month_utc() {
        let june = Utc.with_ymd_and_hms(2026, 6, 11, 15, 30, 0).unwrap();
        assert_eq!(
            first_of_next_month_utc(june),
            Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap()
        );

        let december = Utc.with_ymd_and_hms(2026, 12, 31, 23, 59, 0).unwrap();
        assert_eq!(
            first_of_next_month_utc(december),
            Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn parses_premium_requests_from_billing_report() {
        let report = json!({
            "timePeriod": { "year": 2026, "month": 6 },
            "user": "octocat",
            "usageItems": [
                { "product": "copilot", "model": "gpt-5", "grossQuantity": 12.0,
                  "discountQuantity": 12.0, "netQuantity": 0.0 },
                { "product": "copilot", "model": "claude-fable-5", "grossQuantity": 30.5,
                  "discountQuantity": 20.0, "netQuantity": 10.5 }
            ]
        });
        assert_eq!(premium_requests_from_billing(&report), Some(42.5));

        // No usage yet this month is a real zero, not an error.
        assert_eq!(
            premium_requests_from_billing(&json!({ "usageItems": [] })),
            Some(0.0)
        );

        // A response without usageItems is an unexpected shape.
        assert_eq!(premium_requests_from_billing(&json!({ "ok": true })), None);
    }

    #[tokio::test]
    async fn fetches_authoritative_usage_with_a_github_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .and(header("authorization", "Bearer ghp_test"))
            .and(header("x-github-api-version", GITHUB_API_VERSION))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "login": "octocat" })))
            .expect(1)
            .mount(&server)
            .await;
        let now = Utc::now();
        Mock::given(method("GET"))
            .and(path(
                "/users/octocat/settings/billing/premium_request/usage",
            ))
            .and(query_param("year", now.year().to_string()))
            .and(query_param("month", now.month().to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "usageItems": [
                    { "product": "copilot", "model": "gpt-5", "grossQuantity": 142.0 }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut account = account(Some(CopilotPlan::Pro));
        account.endpoint_override = Some(server.uri());
        account.plaintext_secret = Some("ghp_test".to_string());

        let snapshot = fetch(&Client::new(), &account).await.unwrap();

        assert_eq!(snapshot.status, SnapshotStatus::Healthy);
        let quota = snapshot.quota.as_ref().unwrap();
        assert_eq!(quota.used, 142.0);
        assert_eq!(quota.limit, Some(300.0));
        assert_eq!(
            snapshot.message.as_deref(),
            Some("Premium requests from GitHub billing.")
        );
        assert_eq!(
            snapshot.subscription.as_ref().unwrap().source,
            "github-billing"
        );
    }

    #[test]
    fn billing_failure_falls_back_to_local_estimate_with_a_note() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let state = tempdir().unwrap();
        let copilot_home = tempdir().unwrap();
        crate::insights::test_support::write_copilot_fixture(
            copilot_home.path(),
            "55555555-eeee",
            Utc::now(),
            17,
        );

        // Hermetic claudex env (index + Copilot data root) for the local
        // fallback path; a plain #[test] holding the env lock around
        // block_on avoids holding it across an await.
        let snapshot = crate::insights::test_support::with_claudex_env(
            Some(state.path()),
            Some(copilot_home.path()),
            || {
                runtime.block_on(async {
                    let server = MockServer::start().await;
                    Mock::given(method("GET"))
                        .and(path("/user"))
                        .respond_with(ResponseTemplate::new(401))
                        .mount(&server)
                        .await;

                    let mut account = account(Some(CopilotPlan::Pro));
                    account.endpoint_override = Some(server.uri());
                    account.plaintext_secret = Some("ghp_revoked".to_string());

                    fetch(&Client::new(), &account).await.unwrap()
                })
            },
        );

        assert_ne!(snapshot.status, SnapshotStatus::Error);
        assert_eq!(snapshot.quota.as_ref().unwrap().used, 17.0);
        let message = snapshot.message.as_deref().unwrap();
        assert!(message.contains("GitHub billing unavailable"), "{message}");
        assert!(message.contains("local CLI estimate"), "{message}");
        assert_eq!(
            snapshot.subscription.as_ref().unwrap().source,
            "copilot-local-estimate"
        );
    }
}
