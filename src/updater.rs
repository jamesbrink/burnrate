//! Auto-updater IPC: channel-aware `check_for_updates`,
//! `install_pending_update`, and the session-deduped `notify_update_available`
//! system notification. Wraps `tauri_plugin_updater` so the manual "Check for
//! Updates" tray entry and the frontend's 30-minute background poll flow
//! through the same code path; every completed check also broadcasts
//! `burnrate-update-available` so the tray popover can mirror availability.
//!
//! Adapted from the implementation in the sibling `aethon` project, trimmed
//! to the core check → download → install → restart flow (no boot-probation
//! rollback) and rebased on Burnrate's `jamesbrink/burnrate` release URLs.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;
use url::Url;

const STABLE_URL: &str =
    "https://github.com/jamesbrink/burnrate/releases/latest/download/latest.json";
const NIGHTLY_URL: &str =
    "https://github.com/jamesbrink/burnrate/releases/download/nightly/latest.json";

const GITHUB_RELEASES_API: &str =
    "https://api.github.com/repos/jamesbrink/burnrate/releases?per_page=10";
const NIGHTLY_CANDIDATE_LIMIT: usize = 3;
const USER_AGENT: &str = concat!("burnrate-updater/", env!("CARGO_PKG_VERSION"));
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);

/// Managed state holding the [`tauri_plugin_updater::Update`] returned by a
/// successful check until [`install_pending_update`] consumes it. The `Update`
/// is not serializable, so it can't cross the IPC boundary; only [`UpdateInfo`]
/// does.
#[derive(Default)]
pub(crate) struct UpdaterState {
    pub pending_update: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
    /// Last version a system notification was posted for this session. Dedupes
    /// the repeated 30-minute polls and the two webview windows so each newly
    /// discovered version notifies at most once per app run.
    pub last_notified_version: std::sync::Mutex<Option<String>>,
}

/// Serializable view of an available update returned to the frontend.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub body: Option<String>,
    pub date: Option<String>,
}

/// True when `tauri.conf.json` carries a non-empty updater pubkey. A build
/// without a configured key can't verify signatures, so we refuse to promise
/// updates rather than fail noisily on every poll (dev / unsigned builds).
pub(crate) fn updater_pubkey_configured() -> bool {
    static CONF: &str = include_str!("../tauri.conf.json");
    serde_json::from_str::<serde_json::Value>(CONF)
        .ok()
        .as_ref()
        .and_then(|v| v.get("plugins"))
        .and_then(|v| v.get("updater"))
        .and_then(|v| v.get("pubkey"))
        .and_then(|v| v.as_str())
        .map(|key| !key.trim().is_empty())
        .unwrap_or(false)
}

fn endpoint_for(channel: &str) -> &'static str {
    match channel {
        "stable" => STABLE_URL,
        "nightly" => NIGHTLY_URL,
        other => {
            eprintln!("burnrate updater: unknown channel {other:?} — falling back to stable");
            STABLE_URL
        }
    }
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// Subset of the GitHub Releases API payload we filter on.
#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
}

/// Parse a GitHub Releases API response and return the top `limit` candidate
/// `latest.json` URLs for the nightly channel, newest first. Pure function so
/// the filtering/sorting logic is unit-testable without HTTP.
fn nightly_candidate_urls_from_json(body: &str, limit: usize) -> Vec<Url> {
    if limit == 0 {
        return Vec::new();
    }
    let releases: Vec<GhRelease> = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut filtered: Vec<GhRelease> = releases
        .into_iter()
        .filter(|r| {
            !r.draft
                && r.prerelease
                && r.tag_name != "nightly-staging"
                && (r.tag_name == "nightly" || r.tag_name.starts_with("nightly-"))
        })
        .collect();

    // Newest first. ISO-8601 Z-suffixed timestamps sort correctly as strings.
    // Releases with no published_at sort last.
    filtered.sort_by(|a, b| b.published_at.cmp(&a.published_at));

    let mut urls: Vec<Url> = Vec::new();
    for r in filtered {
        let raw = format!(
            "https://github.com/jamesbrink/burnrate/releases/download/{}/latest.json",
            r.tag_name
        );
        if let Ok(url) = Url::parse(&raw)
            && !urls.contains(&url)
        {
            urls.push(url);
            if urls.len() >= limit {
                break;
            }
        }
    }
    urls
}

/// Discover nightly `latest.json` candidate URLs via the GitHub Releases API.
/// Always returns a (possibly empty) vec; transport, HTTP, or parse failures
/// log once and downgrade to "no candidates," letting the caller fall back to
/// the static [`NIGHTLY_URL`].
async fn discover_nightly_endpoints() -> Vec<Url> {
    let resp = match http_client()
        .get(GITHUB_RELEASES_API)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .timeout(DISCOVERY_TIMEOUT)
        .send()
        .await
    {
        Ok(r) => r,
        Err(error) => {
            eprintln!("burnrate updater: nightly discovery request failed: {error}");
            return Vec::new();
        }
    };

    if !resp.status().is_success() {
        eprintln!(
            "burnrate updater: nightly discovery returned {} — falling back to static URL",
            resp.status()
        );
        return Vec::new();
    }

    match resp.text().await {
        Ok(body) => nightly_candidate_urls_from_json(&body, NIGHTLY_CANDIDATE_LIMIT),
        Err(error) => {
            eprintln!("burnrate updater: nightly discovery body read failed: {error}");
            Vec::new()
        }
    }
}

/// Build the ordered endpoint list to feed to the Tauri updater plugin. The
/// plugin tries each in order and stops at the first one that fetches + parses,
/// so a broken `latest.json` on the most recent nightly fails over to the
/// previous one.
async fn endpoints_for(channel: &str) -> Result<Vec<Url>, String> {
    if channel == "nightly" {
        let mut endpoints = discover_nightly_endpoints().await;
        let static_fallback: Url = NIGHTLY_URL
            .parse()
            .map_err(|e: url::ParseError| e.to_string())?;
        if !endpoints.contains(&static_fallback) {
            endpoints.push(static_fallback);
        }
        return Ok(endpoints);
    }

    let url: Url = endpoint_for(channel)
        .parse()
        .map_err(|e: url::ParseError| e.to_string())?;
    Ok(vec![url])
}

/// Classifies an updater check error: `Ok(())` means "downgrade to no update
/// available" (a benign missing/draft manifest), `Err(...)` is a real
/// transport/parse failure that should surface in the UI.
fn classify_check_error(err: tauri_plugin_updater::Error) -> Result<(), String> {
    match err {
        tauri_plugin_updater::Error::ReleaseNotFound => Ok(()),
        other => Err(other.to_string()),
    }
}

/// Whether this build can actually self-update. We only publish a macOS
/// (`darwin-*`) updater manifest, and `tauri-plugin-updater` resolves the
/// install target from the running `.app` bundle — so a bare `cargo install`
/// binary or any Linux/Windows build can't install our `.app.tar.gz`. Gate the
/// updater to a bundled macOS app so those builds don't advertise (and then
/// fail) updates.
#[cfg(target_os = "macos")]
fn updater_supported() -> bool {
    crate::tray::running_in_app_bundle()
}

#[cfg(not(target_os = "macos"))]
fn updater_supported() -> bool {
    false
}

/// True only on a bundled macOS `.app` with a configured pubkey. The frontend
/// gates its update UI on this so dev / unsigned / unsupported builds stay quiet.
#[tauri::command]
pub(crate) fn updater_available() -> bool {
    updater_supported() && updater_pubkey_configured()
}

/// True when `version` differs from the last version notified this session.
/// The caller records the version only after the notification actually posts,
/// so a failed post retries on the next poll instead of going silent.
pub(crate) fn should_notify(last: &Option<String>, version: &str) -> bool {
    last.as_deref() != Some(version)
}

/// Post a system notification for an update found by the background poll. The
/// frontend calls this only for auto checks — a manual check shows the result
/// dialog instead. Deduped per session via [`should_notify`].
#[tauri::command]
pub(crate) fn notify_update_available(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    version: String,
) -> Result<(), String> {
    // Defense in depth: the frontend already gates on `updater_available`, and
    // macOS only shows notifications for bundled apps anyway.
    if !updater_available() {
        return Ok(());
    }
    // Held across `.show()` so a concurrent caller (the other window) can't
    // double-post the same version in the gap before it's recorded.
    let mut last = state
        .last_notified_version
        .lock()
        .map_err(|e| e.to_string())?;
    if !should_notify(&last, &version) {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_notification::NotificationExt;
        app.notification()
            .builder()
            .title(format!("Burnrate {version} is available"))
            .body("Open Burnrate to see what's changed and install the update.")
            .show()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app; // `updater_available()` is false off-macOS; kept for cfg parity.
    *last = Some(version);
    Ok(())
}

/// Check the given channel's release feed for an update. On success the
/// [`tauri_plugin_updater::Update`] is stashed in [`UpdaterState`] for
/// [`install_pending_update`]; the serializable [`UpdateInfo`] is returned to
/// the UI.
#[tauri::command]
pub(crate) async fn check_for_updates(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    channel: String,
) -> Result<Option<UpdateInfo>, String> {
    // Gate on the full support check (bundled macOS + pubkey), not just the
    // pubkey: a direct invoke on an unsupported build must not stash a pending
    // update that could never be installed.
    if !updater_available() {
        return Ok(None);
    }
    let endpoints = endpoints_for(&channel).await?;

    let result = app
        .updater_builder()
        .endpoints(endpoints)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await;

    let update = match result {
        Ok(u) => u,
        Err(e) => match classify_check_error(e) {
            Ok(()) => None,
            Err(msg) => return Err(msg),
        },
    };

    let mut slot = state.pending_update.lock().await;
    let info = match update {
        Some(u) => {
            let info = UpdateInfo {
                version: u.version.clone(),
                current_version: u.current_version.clone(),
                body: u.body.clone(),
                date: u.date.map(|d| d.to_string()),
            };
            *slot = Some(u);
            Some(info)
        }
        None => {
            *slot = None;
            None
        }
    };
    // Broadcast the outcome so the tray window can mirror availability (and
    // clear it when a later check comes back empty) without its own poll.
    let _ = app.emit("burnrate-update-available", &info);
    Ok(info)
}

/// Download and install the pending update, then restart the app. Emits
/// `burnrate-update-progress` (u32, 0–100) as bytes arrive.
///
/// `version` is the version the UI is offering to install; if a later check
/// (e.g. from the other window or a channel switch) replaced the pending slot,
/// this returns an error instead of silently installing a different
/// channel/version. The lock is held for the whole download so a concurrent
/// check can't swap the slot mid-install, and the pending update is kept on
/// failure so the banner's retry can try the same version again.
#[tauri::command]
pub(crate) async fn install_pending_update(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    version: String,
) -> Result<(), String> {
    if !updater_available() {
        return Err("Updates are not supported on this build.".to_string());
    }
    let slot = state.pending_update.lock().await;
    let update = slot
        .as_ref()
        .ok_or_else(|| "No pending update".to_string())?;
    if update.version != version {
        return Err(format!(
            "Pending update changed (have {}, expected {version}); re-check for updates.",
            update.version
        ));
    }

    let app_for_cb = app.clone();
    let mut total: u64 = 0;
    let mut downloaded: u64 = 0;

    update
        .download_and_install(
            move |chunk_len, content_len| {
                if let Some(c) = content_len {
                    total = c;
                }
                downloaded += chunk_len as u64;
                // Hold back the emit until the payload size is known: the
                // plugin's callback may report `content_len = None` on the
                // first chunk, and dividing by `total = 0` would pin the bar
                // at 0% until the size arrives.
                if total == 0 {
                    return;
                }
                let pct = downloaded
                    .checked_mul(100)
                    .and_then(|v| v.checked_div(total))
                    .unwrap_or(0)
                    .min(100) as u32;
                let _ = app_for_cb.emit("burnrate-update-progress", pct);
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;

    // `AppHandle::restart` returns `!`.
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_for_known_channels() {
        assert_eq!(endpoint_for("stable"), STABLE_URL);
        assert_eq!(endpoint_for("nightly"), NIGHTLY_URL);
        assert_eq!(endpoint_for("garbage"), STABLE_URL);
    }

    #[test]
    fn pubkey_is_configured_in_shipped_config() {
        // The committed tauri.conf.json must carry a real updater pubkey so
        // release builds promise updates. (A blank key here would silently
        // disable the updater for everyone.)
        assert!(
            updater_pubkey_configured(),
            "tauri.conf.json is missing a non-empty plugins.updater.pubkey"
        );
    }

    #[test]
    fn release_not_found_is_treated_as_no_update() {
        let result = classify_check_error(tauri_plugin_updater::Error::ReleaseNotFound);
        assert!(matches!(result, Ok(())));
    }

    #[test]
    fn other_errors_bubble_up_as_strings() {
        let err = tauri_plugin_updater::Error::EmptyEndpoints;
        let expected = err.to_string();
        match classify_check_error(err) {
            Err(msg) => assert_eq!(msg, expected),
            Ok(_) => panic!("EmptyEndpoints should not be downgraded"),
        }
    }

    fn release_json(
        tag: &str,
        draft: bool,
        prerelease: bool,
        published_at: Option<&str>,
    ) -> String {
        let pa = match published_at {
            Some(s) => format!("\"{s}\""),
            None => "null".to_string(),
        };
        format!(
            "{{\"tag_name\":\"{tag}\",\"draft\":{draft},\"prerelease\":{prerelease},\"published_at\":{pa}}}"
        )
    }

    fn url(tag: &str) -> Url {
        Url::parse(&format!(
            "https://github.com/jamesbrink/burnrate/releases/download/{tag}/latest.json"
        ))
        .unwrap()
    }

    #[test]
    fn parses_and_filters_top_three_nightlies() {
        let body = format!(
            "[{},{},{},{},{}]",
            release_json("v0.4.0", false, false, Some("2026-05-25T00:00:00Z")),
            release_json("nightly-staging", true, true, Some("2026-05-26T18:00:00Z")),
            release_json("nightly", false, true, Some("2026-05-26T15:00:00Z")),
            release_json(
                "nightly-2026-05-25",
                false,
                true,
                Some("2026-05-25T12:00:00Z")
            ),
            release_json(
                "nightly-2026-05-24",
                false,
                true,
                Some("2026-05-24T12:00:00Z")
            ),
        );
        let got = nightly_candidate_urls_from_json(&body, NIGHTLY_CANDIDATE_LIMIT);
        assert_eq!(
            got,
            vec![
                url("nightly"),
                url("nightly-2026-05-25"),
                url("nightly-2026-05-24"),
            ]
        );
    }

    #[test]
    fn excludes_drafts() {
        let body = format!(
            "[{}]",
            release_json("nightly", true, true, Some("2026-05-26T15:00:00Z"))
        );
        assert!(nightly_candidate_urls_from_json(&body, NIGHTLY_CANDIDATE_LIMIT).is_empty());
    }

    #[test]
    fn excludes_nightly_staging_even_when_published() {
        let body = format!(
            "[{}]",
            release_json("nightly-staging", false, true, Some("2026-05-26T15:00:00Z"))
        );
        assert!(nightly_candidate_urls_from_json(&body, NIGHTLY_CANDIDATE_LIMIT).is_empty());
    }

    #[test]
    fn excludes_non_prerelease() {
        let body = format!(
            "[{}]",
            release_json("nightly", false, false, Some("2026-05-26T15:00:00Z"))
        );
        assert!(nightly_candidate_urls_from_json(&body, NIGHTLY_CANDIDATE_LIMIT).is_empty());
    }

    #[test]
    fn malformed_json_returns_empty() {
        assert!(nightly_candidate_urls_from_json("not json", NIGHTLY_CANDIDATE_LIMIT).is_empty());
    }

    #[test]
    fn limit_zero_returns_empty() {
        let body = format!(
            "[{}]",
            release_json("nightly", false, true, Some("2026-05-26T15:00:00Z"))
        );
        assert!(nightly_candidate_urls_from_json(&body, 0).is_empty());
    }

    #[test]
    fn missing_published_at_sorts_last() {
        let body = format!(
            "[{},{}]",
            release_json("nightly-undated", false, true, None),
            release_json("nightly", false, true, Some("2026-05-26T15:00:00Z")),
        );
        let got = nightly_candidate_urls_from_json(&body, NIGHTLY_CANDIDATE_LIMIT);
        assert_eq!(got, vec![url("nightly"), url("nightly-undated")]);
    }

    #[test]
    fn notifies_first_time_for_a_version() {
        assert!(should_notify(&None, "0.2.0"));
    }

    #[test]
    fn repeat_polls_of_the_same_version_stay_silent() {
        assert!(!should_notify(&Some("0.2.0".to_string()), "0.2.0"));
    }

    #[test]
    fn a_newer_version_notifies_again() {
        assert!(should_notify(&Some("0.2.0".to_string()), "0.3.0"));
    }
}
