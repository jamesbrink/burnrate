//! Read-only Nous Portal balances from an existing Hermes login.
//! Hermes owns token refresh; Burnrate never writes its credential files.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::{NOUS_ACCOUNT_ID, default_auto_account},
    models::{
        AccountConfig, ProviderKind, SnapshotStatus, SubscriptionPlan, SubscriptionSnapshot,
        UsageBucketSnapshot, UsageSnapshot,
    },
};

use super::{datetime, error_snapshot, number, primary_quota, text, validate_endpoint};

const PORTAL_URL: &str = "https://portal.nousresearch.com";
const ACCOUNT_ID: &str = NOUS_ACCOUNT_ID;

/// Shared credentials are outside named profiles. Only the selected profile
/// is a legacy fallback; scanning other profiles could choose another account.
struct AuthPaths {
    shared: PathBuf,
    profile: PathBuf,
}

impl AuthPaths {
    fn discover() -> Option<Self> {
        let default_home = dirs::home_dir()?.join(".hermes");
        let home = env_path("HERMES_HOME");
        let shared = env_path("HERMES_SHARED_AUTH_DIR");
        Some(Self::new(&default_home, home.as_deref(), shared.as_deref()))
    }

    fn new(default_home: &Path, home: Option<&Path>, shared: Option<&Path>) -> Self {
        let home = home.unwrap_or(default_home);
        // Mirrors Hermes get_default_hermes_root for standard/custom roots.
        let resolved = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
        let default_resolved = default_home
            .canonicalize()
            .unwrap_or_else(|_| default_home.to_path_buf());
        let root = if resolved.starts_with(default_resolved) {
            default_home
        } else if home
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "profiles")
        {
            home.parent().and_then(Path::parent).unwrap_or(home)
        } else {
            home
        };
        Self {
            shared: shared
                .map(Path::to_path_buf)
                .unwrap_or_else(|| root.join("shared"))
                .join("nous_auth.json"),
            profile: home.join("auth.json"),
        }
    }

    fn load(&self) -> Option<(Credential, &Path)> {
        let shared = std::fs::read(&self.shared)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Credential>(&bytes).ok())
            .filter(Credential::has_token);
        if let Some(credential) = shared {
            if credential.expired()
                && let Some(profile) = self.load_profile()
                && profile.expiry().is_some_and(|expiry| expiry > Utc::now())
                && credential.same_identity(&profile)
            {
                return Some((profile, &self.profile));
            }
            return Some((credential, &self.shared));
        }
        self.load_profile()
            .map(|credential| (credential, self.profile.as_path()))
    }

    fn load_profile(&self) -> Option<Credential> {
        let bytes = std::fs::read(&self.profile).ok()?;
        let profile: ProfileAuth = serde_json::from_slice(&bytes).ok()?;
        profile.providers.nous.filter(Credential::has_token)
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    let value = std::env::var(name).ok()?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return dirs::home_dir().map(|home| home.join(rest));
    }
    Some(PathBuf::from(value))
}

// Unknown fields (including refresh_token) are skipped by serde.
#[derive(Deserialize)]
struct Credential {
    access_token: Option<String>,
    expires_at: Option<Value>,
    portal_base_url: Option<String>,
}

impl Credential {
    fn has_token(&self) -> bool {
        self.access_token
            .as_ref()
            .is_some_and(|token| !token.trim().is_empty())
    }

    fn expiry(&self) -> Option<DateTime<Utc>> {
        self.expires_at.as_ref().and_then(|value| {
            value
                .as_i64()
                .and_then(|epoch| DateTime::from_timestamp(epoch, 0))
                .or_else(|| {
                    value
                        .as_str()
                        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
                        .map(|time| time.with_timezone(&Utc))
                })
        })
    }

    fn expired(&self) -> bool {
        self.expiry().is_some_and(|expiry| expiry <= Utc::now())
    }

    fn same_identity(&self, other: &Self) -> bool {
        // These are local credential claims, not signature validation. The
        // Portal still validates the bearer token. Require user, organization,
        // issuer and routing to match; never switch accounts based on email.
        fn identity(credential: &Credential) -> Option<(String, String, String)> {
            use base64::Engine;
            let token = credential.access_token.as_deref()?;
            let parts: Vec<_> = token.split('.').collect();
            if parts.len() != 3 {
                return None;
            }
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(parts[1])
                .ok()?;
            let claims: Value = serde_json::from_slice(&bytes).ok()?;
            let field = |name: &str| -> Option<String> {
                claims
                    .get(name)?
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_owned)
            };
            Some((field("iss")?, field("sub")?, field("org_id")?))
        }
        let base = |credential: &Self| {
            credential
                .portal_base_url
                .as_deref()
                .filter(|url| !url.trim().is_empty())
                .unwrap_or(PORTAL_URL)
                .trim_end_matches('/')
                .to_owned()
        };
        base(self) == base(other) && identity(self).is_some_and(|id| Some(id) == identity(other))
    }
}

#[derive(Deserialize)]
struct ProfileAuth {
    providers: ProfileProviders,
}
#[derive(Deserialize)]
struct ProfileProviders {
    nous: Option<Credential>,
}

pub(crate) fn detect() -> Option<AccountConfig> {
    detect_at(&AuthPaths::discover()?)
}

fn detect_at(paths: &AuthPaths) -> Option<AccountConfig> {
    let (_, path) = paths.load()?;
    Some(default_auto_account(
        ACCOUNT_ID,
        ProviderKind::Nous,
        "Nous Portal",
        path.to_path_buf(),
    ))
}

pub(crate) async fn fetch(http: &Client, account: &AccountConfig) -> Result<UsageSnapshot> {
    let paths = AuthPaths::discover()
        .ok_or_else(|| anyhow!("could not locate the Hermes home directory"))?;
    fetch_at(http, account, &paths).await
}

async fn fetch_at(
    http: &Client,
    account: &AccountConfig,
    paths: &AuthPaths,
) -> Result<UsageSnapshot> {
    let (credential, source) = paths.load().ok_or_else(|| {
        anyhow!("No Nous login found. Sign in to Nous in Hermes, then refresh Burnrate.")
    })?;
    if credential.expired() {
        if source == paths.shared {
            return Err(anyhow!(
                "Nous shared access token expired. No valid same-account profile credential could be verified. Sign in to Nous in Hermes and ensure it can update its shared nous_auth.json store, then refresh Burnrate."
            ));
        }
        return Err(anyhow!(
            "Nous access token expired. Refresh your Nous login in Hermes, then refresh Burnrate."
        ));
    }
    let base = account
        .endpoint_override
        .as_deref()
        .or(credential.portal_base_url.as_deref())
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(PORTAL_URL);
    validate_endpoint(base)?;
    let token = credential
        .access_token
        .as_deref()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            anyhow!("Nous credential has no usable access token. Sign in to Nous in Hermes, then refresh Burnrate.")
        })?;
    let response = http
        .get(format!("{}/api/oauth/account", base.trim_end_matches('/')))
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await
        .context("failed to fetch Nous Portal account")?;
    match response.status() {
        StatusCode::UNAUTHORIZED => Err(anyhow!(
            "Nous rejected the access token. Sign in to Nous in Hermes, then refresh Burnrate."
        )),
        StatusCode::FORBIDDEN => Err(anyhow!(
            "Nous Portal denied access to account balances. Check your Nous login and permissions in Hermes."
        )),
        status if status.is_success() => {
            let value = response
                .json()
                .await
                .context("failed to decode Nous Portal response")?;
            let snapshot = parse_account_snapshot(account, &value);
            if snapshot.status == SnapshotStatus::Error {
                return Err(anyhow!("Nous Portal returned no usable credit balances"));
            }
            Ok(snapshot)
        }
        status => Err(anyhow!("Nous Portal returned {status}")),
    }
}

/// Only subscription credits have a monthly-allowance meter. Preserve rollover
/// above that allowance; total and purchased balances have no denominator.
fn parse_account_snapshot(account: &AccountConfig, value: &Value) -> UsageSnapshot {
    let total = balance(value, &["/paid_service_access/total_usable_credits"]);
    let subscription = balance(
        value,
        &[
            "/paid_service_access/subscription_credits_remaining",
            "/subscription/credits_remaining",
        ],
    );
    let purchased = balance(value, &["/paid_service_access/purchased_credits_remaining"]);
    let mut buckets = Vec::new();
    for (id, label, remaining, reset_at) in [
        ("total-credits", "Total available", total, None),
        (
            "subscription-credits",
            "Subscription credits",
            subscription,
            datetime(value, &["/subscription/current_period_end"]),
        ),
        ("purchased-credits", "Purchased credits", purchased, None),
    ] {
        if let Some(remaining) = remaining {
            buckets.push(UsageBucketSnapshot {
                id: id.to_string(),
                label: label.to_string(),
                window: None,
                used: 0.0,
                limit: if id == "subscription-credits" {
                    balance(value, &["/subscription/monthly_credits"])
                        .filter(|allowance| *allowance > 0.0)
                } else {
                    None
                },
                remaining: Some(remaining),
                unit: "USD".to_string(),
                reset_at,
                status: balance_status(remaining),
            });
        }
    }
    if buckets.is_empty() {
        return error_snapshot(
            account,
            anyhow!("Nous Portal returned no usable credit balances"),
        );
    }
    // Informational rollover metadata is not an additional spendable balance.
    if let Some(rollover) = balance(value, &["/subscription/rollover_credits"]) {
        buckets.push(UsageBucketSnapshot {
            id: "subscription-rollover".to_string(),
            label: "Subscription rollover (reported)".to_string(),
            window: None,
            used: 0.0,
            limit: None,
            remaining: Some(rollover),
            unit: "USD".to_string(),
            reset_at: None,
            status: SnapshotStatus::Healthy,
        });
    }
    // The total is authoritative. If absent, available component balances
    // still prevent a depleted subscription from hiding usable top-ups.
    let status = total.map(balance_status).unwrap_or_else(|| {
        if [subscription, purchased]
            .into_iter()
            .flatten()
            .any(|remaining| remaining > 0.0)
        {
            SnapshotStatus::Healthy
        } else {
            SnapshotStatus::Exhausted
        }
    });
    UsageSnapshot {
        account_id: account.id.clone(),
        provider: account.provider,
        label: account.label.clone(),
        status,
        email: text(value, &["/user/email"]),
        subscription: text(value, &["/subscription/plan"])
            .filter(|plan| !plan.trim().is_empty())
            .map(|plan_label| SubscriptionSnapshot {
                plan: SubscriptionPlan::Unknown,
                plan_label,
                rate_limit_tier: None,
                extra_usage_enabled: None,
                source: "nous-portal".to_string(),
            }),
        quota: primary_quota(&buckets),
        usage_buckets: buckets,
        message: None,
        fetched_at: Utc::now(),
    }
}

fn balance(value: &Value, keys: &[&str]) -> Option<f64> {
    number(value, keys).filter(|value| value.is_finite() && *value >= 0.0)
}

fn balance_status(remaining: f64) -> SnapshotStatus {
    if remaining > 0.0 {
        SnapshotStatus::Healthy
    } else {
        SnapshotStatus::Exhausted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    fn account() -> AccountConfig {
        default_auto_account(
            ACCOUNT_ID,
            ProviderKind::Nous,
            "Nous Portal",
            PathBuf::from("unused"),
        )
    }

    fn write(path: &Path, value: Value) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, value.to_string()).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, AuthPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AuthPaths::new(dir.path(), None, None);
        write(
            &paths.shared,
            json!({"access_token": "test-token", "refresh_token": "untouched"}),
        );
        (dir, paths)
    }

    #[test]
    fn shared_login_is_detected_without_default_profile_login() {
        let (_dir, paths) = fixture();
        assert!(!paths.profile.exists());
        let detected = detect_at(&paths).unwrap();
        assert_eq!(detected.id, ACCOUNT_ID);
        assert_eq!(
            detected.credential_path,
            Some(paths.shared.display().to_string())
        );
        assert!(detected.plaintext_secret.is_none());
    }

    #[test]
    fn paths_follow_standard_custom_and_explicit_shared_locations() {
        let default = Path::new("/default/hermes");
        for home in [
            "/default/hermes/profiles/work",
            "/custom/hermes/profiles/work",
        ] {
            let home = Path::new(home);
            let paths = AuthPaths::new(default, Some(home), None);
            assert_eq!(
                paths.shared,
                home.parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("shared/nous_auth.json")
            );
            assert_eq!(paths.profile, home.join("auth.json"));
        }
        let custom = Path::new("/custom/root");
        assert_eq!(
            AuthPaths::new(default, Some(custom), None).shared,
            custom.join("shared/nous_auth.json")
        );
        assert_eq!(
            AuthPaths::new(default, Some(custom), Some(Path::new("/shared/auth"))).shared,
            PathBuf::from("/shared/auth/nous_auth.json")
        );
    }

    #[test]
    fn selected_profile_fallback_records_real_path_and_unverified_shared_wins() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("profiles/work");
        let paths = AuthPaths::new(dir.path(), Some(&profile), None);
        write(
            &paths.profile,
            json!({"access_token": "wrong-root", "providers": {"nous": {"access_token": "profile"}}}),
        );
        assert_eq!(
            paths.load().unwrap().0.access_token.as_deref(),
            Some("profile")
        );
        assert_eq!(
            detect_at(&paths).unwrap().credential_path,
            Some(paths.profile.display().to_string())
        );
        write(
            &paths.shared,
            json!({"access_token": "shared", "expires_at": "2000-01-01T00:00:00Z"}),
        );
        let credential = paths.load().unwrap().0;
        assert_eq!(credential.access_token.as_deref(), Some("shared"));
        assert!(credential.expired());
    }

    fn jwt(subject: &str, org: &str) -> String {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            json!({"iss": "https://portal.nousresearch.com", "sub": subject, "org_id": org})
                .to_string(),
        );
        format!("header.{payload}.signature")
    }

    #[test]
    fn identity_requires_all_stable_claims_and_matching_issuer() {
        use base64::Engine;
        let reference: Credential = serde_json::from_value(json!({
            "access_token": jwt("user-a", "org-a")
        }))
        .unwrap();
        for claims in [
            json!({"sub": "user-a", "org_id": "org-a"}),
            json!({"iss": PORTAL_URL, "org_id": "org-a"}),
            json!({"iss": PORTAL_URL, "sub": "user-a"}),
            json!({"iss": PORTAL_URL, "sub": "", "org_id": "org-a"}),
            json!({"iss": "other-issuer", "sub": "user-a", "org_id": "org-a"}),
        ] {
            let payload =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
            let candidate: Credential = serde_json::from_value(json!({
                "access_token": format!("header.{payload}.signature")
            }))
            .unwrap();
            assert!(!reference.same_identity(&candidate));
            assert!(!candidate.same_identity(&reference));
        }
    }

    #[test]
    fn expired_shared_falls_back_only_to_known_valid_matching_profile() {
        let (_dir, paths) = fixture();
        write(
            &paths.shared,
            json!({
                "access_token": jwt("user-a", "org-a"), "expires_at": "2000-01-01T00:00:00Z"
            }),
        );
        for (token, expiry, portal, use_profile) in [
            (
                jwt("user-a", "org-a"),
                json!("2999-01-01T00:00:00Z"),
                PORTAL_URL,
                true,
            ),
            (
                jwt("user-b", "org-a"),
                json!("2999-01-01T00:00:00Z"),
                PORTAL_URL,
                false,
            ),
            (
                jwt("user-a", "org-b"),
                json!("2999-01-01T00:00:00Z"),
                PORTAL_URL,
                false,
            ),
            (
                "opaque".into(),
                json!("2999-01-01T00:00:00Z"),
                PORTAL_URL,
                false,
            ),
            (jwt("user-a", "org-a"), Value::Null, PORTAL_URL, false),
            (
                jwt("user-a", "org-a"),
                json!("2000-01-01T00:00:00Z"),
                PORTAL_URL,
                false,
            ),
            (
                jwt("user-a", "org-a"),
                json!("2999-01-01T00:00:00Z"),
                "https://other.example",
                false,
            ),
        ] {
            write(
                &paths.profile,
                json!({"providers": {"nous": {
                    "access_token": token, "expires_at": expiry, "portal_base_url": portal
                }}}),
            );
            let before = (
                std::fs::read(&paths.shared).unwrap(),
                std::fs::read(&paths.profile).unwrap(),
            );
            assert_eq!(
                paths.load().unwrap().1,
                if use_profile {
                    &paths.profile
                } else {
                    &paths.shared
                }
            );
            assert_eq!(
                before,
                (
                    std::fs::read(&paths.shared).unwrap(),
                    std::fs::read(&paths.profile).unwrap()
                )
            );
        }
    }

    #[test]
    fn missing_malformed_and_tokenless_credentials_are_not_detected() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AuthPaths::new(dir.path(), None, None);
        assert!(detect_at(&paths).is_none());
        write(
            &paths.profile,
            json!({"access_token": "not-a-nous-profile"}),
        );
        for value in [
            json!({}),
            json!({"refresh_token": "only"}),
            json!({"access_token": " "}),
            json!({"access_token": null}),
        ] {
            write(&paths.shared, value);
            assert!(detect_at(&paths).is_none());
        }
        std::fs::write(&paths.shared, "{invalid").unwrap();
        assert!(detect_at(&paths).is_none());
        // A sibling profile is never selected implicitly.
        write(
            &dir.path().join("profiles/other/auth.json"),
            json!({"providers": {"nous": {"access_token": "other"}}}),
        );
        assert!(detect_at(&paths).is_none());
    }

    #[test]
    fn expiry_accepts_iso_and_epoch_and_tolerates_unknown_expiry() {
        for expiry in [json!("2000-01-01T00:00:00Z"), json!(946684800)] {
            let credential: Credential =
                serde_json::from_value(json!({"access_token": "t", "expires_at": expiry})).unwrap();
            assert!(credential.expired());
        }
        for expiry in [Value::Null, json!("invalid"), json!("2999-01-01T00:00:00Z")] {
            let credential: Credential =
                serde_json::from_value(json!({"access_token": "t", "expires_at": expiry})).unwrap();
            assert!(!credential.expired());
        }
    }

    #[test]
    fn available_topups_keep_account_healthy() {
        let snapshot = parse_account_snapshot(
            &account(),
            &json!({
                "user": {"email": "dev@example.com"},
                "subscription": {"plan": "Super", "monthly_credits": 110, "credits_remaining": 0, "rollover_credits": 10, "current_period_end": "2999-01-01T00:00:00Z"},
                "paid_service_access": {"total_usable_credits": 30, "purchased_credits_remaining": 30}
            }),
        );
        assert_eq!(snapshot.status, SnapshotStatus::Healthy);
        assert_eq!(snapshot.email.as_deref(), Some("dev@example.com"));
        let rollover = snapshot
            .usage_buckets
            .iter()
            .find(|bucket| bucket.id == "subscription-rollover")
            .unwrap();
        assert_eq!(rollover.remaining, Some(10.0));
        let quota = snapshot.quota.unwrap();
        assert_eq!(quota.remaining, Some(30.0));
        assert_eq!(quota.limit, None);
        assert_eq!(quota.reset_at, None);
        assert_eq!(snapshot.usage_buckets[1].limit, Some(110.0));
        assert_eq!(snapshot.usage_buckets[1].status, SnapshotStatus::Exhausted);
        assert!(snapshot.usage_buckets[1].reset_at.is_some());
        assert_eq!(
            snapshot.subscription.unwrap().plan,
            SubscriptionPlan::Unknown
        );
    }

    #[test]
    fn balances_are_not_clamped_to_allowance_or_summed_into_invented_total() {
        let snapshot = parse_account_snapshot(
            &account(),
            &json!({
                "subscription": {"monthly_credits": 110, "credits_remaining": 150},
                "paid_service_access": {"subscription_credits_remaining": 160, "purchased_credits_remaining": 30}
            }),
        );
        assert_eq!(snapshot.usage_buckets.len(), 2);
        assert_eq!(snapshot.usage_buckets[0].remaining, Some(160.0));
        assert_eq!(snapshot.usage_buckets[0].limit, Some(110.0));
        assert_eq!(snapshot.usage_buckets[1].limit, None);
        assert!(
            snapshot
                .usage_buckets
                .iter()
                .all(|bucket| bucket.used == 0.0)
        );
        assert!(
            !snapshot
                .usage_buckets
                .iter()
                .any(|bucket| bucket.id == "total-credits")
        );
    }

    #[test]
    fn authoritative_zero_total_is_exhausted_even_when_components_are_positive() {
        let snapshot = parse_account_snapshot(
            &account(),
            &json!({"paid_service_access": {
                "total_usable_credits": 0, "purchased_credits_remaining": 30
            }}),
        );
        assert_eq!(snapshot.status, SnapshotStatus::Exhausted);
    }

    #[test]
    fn partial_response_shows_only_reported_balances() {
        for value in [
            json!({"subscription": {"credits_remaining": 5}}),
            json!({"paid_service_access": {"purchased_credits_remaining": 5}}),
        ] {
            let snapshot = parse_account_snapshot(&account(), &value);
            assert_eq!(snapshot.status, SnapshotStatus::Healthy);
            assert_eq!(snapshot.usage_buckets.len(), 1);
            assert_eq!(snapshot.quota.unwrap().remaining, Some(5.0));
        }
    }

    #[test]
    fn empty_or_invalid_payload_is_not_healthy() {
        for value in [
            json!({}),
            Value::Null,
            json!({"paid_service_access": {"total_usable_credits": "NaN"}}),
            json!({"subscription": {"credits_remaining": -10}}),
        ] {
            assert_eq!(
                parse_account_snapshot(&account(), &value).status,
                SnapshotStatus::Error
            );
        }
    }

    #[tokio::test]
    async fn rereads_tokens_and_only_gets_account_without_modifying_credentials() {
        let (_dir, paths) = fixture();
        let server = MockServer::start().await;
        let mut account = account();
        account.endpoint_override = Some(server.uri());
        for token in ["first-token", "rotated-token"] {
            write(
                &paths.shared,
                json!({"access_token": token, "refresh_token": "never-use"}),
            );
            let before = std::fs::read(&paths.shared).unwrap();
            Mock::given(method("GET"))
                .and(path("/api/oauth/account"))
                .and(header("authorization", format!("Bearer {token}")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(
                        json!({"paid_service_access": {"total_usable_credits": 30}}),
                    ),
                )
                .expect(1)
                .mount(&server)
                .await;
            let snapshot = fetch_at(&Client::new(), &account, &paths).await.unwrap();
            assert_eq!(snapshot.quota.unwrap().remaining, Some(30.0));
            assert_eq!(std::fs::read(&paths.shared).unwrap(), before);
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
        assert!(!paths.profile.exists());
    }

    #[tokio::test]
    async fn matching_profile_fallback_fetches_without_writing_either_store() {
        let (_dir, paths) = fixture();
        let server = MockServer::start().await;
        let token = jwt("user-a", "org-a");
        write(
            &paths.shared,
            json!({
                "access_token": token, "expires_at": "2000-01-01T00:00:00Z"
            }),
        );
        write(
            &paths.profile,
            json!({"providers": {"nous": {
                "access_token": token, "expires_at": "2999-01-01T00:00:00Z"
            }}}),
        );
        let before = (
            std::fs::read(&paths.shared).unwrap(),
            std::fs::read(&paths.profile).unwrap(),
        );
        Mock::given(method("GET"))
            .and(path("/api/oauth/account"))
            .and(header("authorization", format!("Bearer {token}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"paid_service_access": {"total_usable_credits": 3}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let mut account = account();
        account.endpoint_override = Some(server.uri());
        assert!(fetch_at(&Client::new(), &account, &paths).await.is_ok());
        assert_eq!(
            before,
            (
                std::fs::read(&paths.shared).unwrap(),
                std::fs::read(&paths.profile).unwrap()
            )
        );

        // A live shared login remains preferred even with a different profile.
        write(
            &paths.shared,
            json!({
                "access_token": jwt("user-b", "org-b"), "expires_at": "2999-01-01T00:00:00Z"
            }),
        );
        assert_eq!(paths.load().unwrap().1, &paths.shared);
    }

    #[tokio::test]
    async fn expired_credential_does_not_make_http_requests() {
        let (_dir, paths) = fixture();
        write(
            &paths.shared,
            json!({"access_token": "old", "expires_at": "2000-01-01T00:00:00Z"}),
        );
        let server = MockServer::start().await;
        let mut account = account();
        account.endpoint_override = Some(server.uri());
        assert!(
            fetch_at(&Client::new(), &account, &paths)
                .await
                .unwrap_err()
                .to_string()
                .contains("expired")
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn http_errors_and_invalid_responses_do_not_trigger_fallback_requests() {
        let (_dir, paths) = fixture();
        for (status, body, message) in [
            (401, "{}", "rejected"),
            (403, "{}", "denied access"),
            (500, "{}", "500"),
            (200, "{}", "no usable"),
            (200, "invalid", "decode"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/api/oauth/account"))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .expect(1)
                .mount(&server)
                .await;
            let mut account = account();
            account.endpoint_override = Some(server.uri());
            assert!(
                fetch_at(&Client::new(), &account, &paths)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains(message)
            );
            assert_eq!(server.received_requests().await.unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn rejects_insecure_persisted_endpoint() {
        let (_dir, paths) = fixture();
        write(
            &paths.shared,
            json!({"access_token": "t", "portal_base_url": "http://example.com"}),
        );
        assert!(fetch_at(&Client::new(), &account(), &paths).await.is_err());
    }
}
