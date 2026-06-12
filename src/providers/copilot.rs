//! GitHub Copilot premium-request usage.
//!
//! Copilot bills by premium requests per calendar month, not tokens. Usage is
//! counted from local Copilot CLI session logs (via the claudex index) as a
//! lower-bound estimate, or fetched authoritatively from GitHub's billing API
//! when the account has a GitHub token configured.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use reqwest::Client;

use crate::{
    config::default_auto_account,
    models::{AccountConfig, ProviderKind, UsageSnapshot},
};

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

pub(crate) async fn fetch(_http: &Client, _account: &AccountConfig) -> Result<UsageSnapshot> {
    Err(anyhow!(
        "GitHub Copilot usage tracking is not available in this build"
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

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
}
