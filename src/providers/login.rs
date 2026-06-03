//! Interactive sign-in for Claude Code and Codex.
//!
//! Burnrate does not implement OAuth itself — it shells out to the official
//! `claude` / `codex` CLIs, which open the system browser and run their own
//! localhost callback listener. We stream a redacted view of their output to the
//! UI (surfacing the auth URL so the user can click it if the browser does not
//! open), then verify the result and read the account email.
//!
//! All credential storage is owned by the CLIs (macOS Keychain / `auth.json`).
//! Raw tokens must never reach the UI or logs, so progress lines pass through an
//! allowlist redactor ([`sanitize_line`]) before being emitted.

use std::{process::Stdio, sync::Mutex, time::Duration};

use anyhow::{Result, anyhow};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
    task::AbortHandle,
    time::timeout,
};

use crate::models::ProviderKind;

use super::{claude, codex};

/// Browser sign-in is slow (a human is in the loop), so allow a generous window.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(180);
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(30);

/// Event name for streamed sign-in progress lines.
pub(crate) const LOGIN_PROGRESS_EVENT: &str = "burnrate-login-progress";

/// Payload for [`LOGIN_PROGRESS_EVENT`]. Mirrors `LoginProgress` in `types.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginProgress {
    pub id: String,
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub needs_code: bool,
}

/// Result of a successful sign-in.
#[derive(Debug, Clone, Default)]
pub(crate) struct LoginOutcome {
    pub email: Option<String>,
}

pub(crate) async fn ensure_provider_login_supported(provider: ProviderKind) -> Result<()> {
    match provider {
        ProviderKind::ClaudeCode => claude::ensure_login_supported().await,
        ProviderKind::Codex => Ok(()),
        _ => Err(anyhow!(
            "Interactive sign-in is only available for Claude Code and Codex."
        )),
    }
}

/// Single-flight guard for interactive logins. Only one browser flow runs at a
/// time (two would fight over the localhost callback port), and the held
/// [`AbortHandle`] lets a cancel request kill the spawned task — which drops the
/// child process via `kill_on_drop`.
#[derive(Default)]
pub(crate) struct LoginManager {
    active: Mutex<Option<ActiveLogin>>,
}

struct ActiveLogin {
    account_id: String,
    /// True when re-authenticating an existing account in place (vs. a brand-new
    /// pending account). Cancellation must not delete a re-auth target.
    is_reauth: bool,
    abort: Option<AbortHandle>,
    input: mpsc::UnboundedSender<String>,
}

impl LoginManager {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Reserve the single login slot for `account_id`. Fails if a sign-in is
    /// already running.
    pub(crate) fn reserve(
        &self,
        account_id: &str,
        is_reauth: bool,
        input: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let mut guard = self.active.lock().expect("login manager lock");
        if let Some(active) = guard.as_ref() {
            return Err(anyhow!(
                "A sign-in is already in progress for {}.",
                active.account_id
            ));
        }
        *guard = Some(ActiveLogin {
            account_id: account_id.to_string(),
            is_reauth,
            abort: None,
            input,
        });
        Ok(())
    }

    /// Attach the running task's abort handle so the login can be cancelled.
    pub(crate) fn attach(&self, account_id: &str, abort: AbortHandle) {
        let mut guard = self.active.lock().expect("login manager lock");
        if let Some(active) = guard.as_mut()
            && active.account_id == account_id
        {
            active.abort = Some(abort);
        }
    }

    pub(crate) fn submit_input(&self, account_id: &str, input: String) -> Result<()> {
        let guard = self.active.lock().expect("login manager lock");
        let Some(active) = guard.as_ref() else {
            return Err(anyhow!("No sign-in is in progress."));
        };
        if active.account_id != account_id {
            return Err(anyhow!("That sign-in is no longer active."));
        }
        active
            .input
            .send(input)
            .map_err(|_| anyhow!("The sign-in process is no longer accepting input."))
    }

    /// Clear the slot once a login finishes (success or failure).
    pub(crate) fn finish(&self, account_id: &str) {
        let mut guard = self.active.lock().expect("login manager lock");
        if guard
            .as_ref()
            .is_some_and(|active| active.account_id == account_id)
        {
            *guard = None;
        }
    }

    /// Cancel an in-progress login. Returns `Some(is_reauth)` if `account_id` was
    /// the active login (and was aborted), or `None` if it was not active (e.g. a
    /// late cancel that races a completing login) — in which case the caller must
    /// not tear down the account.
    pub(crate) fn cancel(&self, account_id: &str) -> Option<bool> {
        let mut guard = self.active.lock().expect("login manager lock");
        let active = guard.as_ref()?;
        if active.account_id != account_id {
            return None;
        }
        let is_reauth = active.is_reauth;
        if let Some(abort) = active.abort.as_ref() {
            abort.abort();
        }
        *guard = None;
        Some(is_reauth)
    }
}

/// Run the interactive sign-in for `provider` into `config_dir`, emitting
/// progress events through `app`, then verify the result and return the email.
pub(crate) async fn run_login(
    app: AppHandle,
    provider: ProviderKind,
    account_id: String,
    config_dir: Option<String>,
    email_hint: Option<String>,
    input_rx: mpsc::UnboundedReceiver<String>,
) -> Result<LoginOutcome> {
    let (binary, args, env_key) = login_command(provider, email_hint.as_deref())?;
    let id = account_id.clone();
    let mut on_progress = move |line: &str, url: Option<&str>| {
        let needs_code = provider == ProviderKind::ClaudeCode && url.is_some();
        let line = if needs_code {
            "Copy the authentication code from the browser, then paste it here."
        } else {
            line
        };
        let _ = app.emit(
            LOGIN_PROGRESS_EVENT,
            LoginProgress {
                id: id.clone(),
                line: line.to_string(),
                url: url.map(str::to_string),
                needs_code,
            },
        );
    };
    run_login_inner(
        &binary,
        &args,
        env_key,
        config_dir.as_deref(),
        provider != ProviderKind::ClaudeCode,
        input_rx,
        &mut on_progress,
    )
    .await?;
    let email = verify(provider, config_dir.as_deref()).await?;
    Ok(LoginOutcome { email })
}

/// macOS fallback: drop the Claude Keychain credential for an account directly,
/// in case `claude auth logout` failed and would otherwise orphan it. (The
/// `claude` module is private to `providers`, so this wrapper exposes it.)
#[cfg(target_os = "macos")]
pub(crate) fn delete_claude_keychain(account: &crate::models::AccountConfig) {
    claude::delete_keychain_credentials(account);
}

/// macOS fallback keyed by a config dir, for clearing the Keychain credential of
/// a stale managed dir that is being discarded (reuse-policy adoption).
#[cfg(target_os = "macos")]
pub(crate) fn delete_claude_keychain_for_dir(config_dir: Option<&str>) {
    claude::delete_keychain_credentials_for_dir(config_dir);
}

/// Sign the account out via the provider CLI (clears Keychain / `auth.json`).
pub(crate) async fn run_logout(provider: ProviderKind, config_dir: Option<&str>) -> Result<()> {
    let (binary, args, env_key) = logout_command(provider)?;
    let resolved = super::resolve_cli(&binary);
    let mut command = Command::new(&resolved);
    command
        .args(&args)
        .env("PATH", super::augmented_path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(dir) = config_dir.map(str::trim).filter(|dir| !dir.is_empty()) {
        command.env(env_key, dir);
    }
    let status = timeout(LOGOUT_TIMEOUT, command.status())
        .await
        .map_err(|_| anyhow!("Sign-out timed out."))?
        .map_err(|err| anyhow!("Failed to run sign-out: {err}"))?;
    if !status.success() {
        return Err(anyhow!("{} sign-out did not complete.", provider.as_str()));
    }
    Ok(())
}

/// Spawn the CLI, stream redacted output (calling `on_progress` with a status
/// line and an optional auth URL), and resolve when it exits. Provider-agnostic
/// so it can be exercised with a fixture binary in tests.
async fn run_login_inner(
    binary: &str,
    args: &[String],
    env_key: &str,
    config_dir: Option<&str>,
    open_browser: bool,
    mut input_rx: mpsc::UnboundedReceiver<String>,
    on_progress: &mut (dyn FnMut(&str, Option<&str>) + Send),
) -> Result<()> {
    let resolved = super::resolve_cli(binary);
    let mut command = Command::new(&resolved);
    command
        .args(args)
        .env("PATH", super::augmented_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // `None` means the system-default location (e.g. re-authenticating the
    // auto-detected account refreshes `~/.claude` / `~/.codex` directly).
    if let Some(dir) = config_dir.filter(|dir| !dir.trim().is_empty()) {
        command.env(env_key, dir);
    }
    let mut child = command.spawn().map_err(|err| {
        anyhow!(
            "Failed to launch `{binary}`: {err}. Set BURNRATE_CLAUDE_BIN / BURNRATE_CODEX_BIN if the CLI lives elsewhere."
        )
    })?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("no stdin for sign-in process"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("no stdout from sign-in process"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("no stderr from sign-in process"))?;
    let mut out = BufReader::new(stdout).lines();
    let mut err = BufReader::new(stderr).lines();
    let mut opened = false;
    let mut input_done = false;
    let mut last_status: Option<String> = None;

    let status = timeout(LOGIN_TIMEOUT, async {
        let (mut out_done, mut err_done) = (false, false);
        while !(out_done && err_done) {
            let line = tokio::select! {
                input = input_rx.recv(), if !input_done => {
                    match input {
                        Some(input) => {
                            let mut input = input.trim().to_string();
                            if !input.ends_with('\n') {
                                input.push('\n');
                            }
                            stdin.write_all(input.as_bytes()).await?;
                            stdin.flush().await?;
                        }
                        None => input_done = true,
                    }
                    None
                },
                res = out.next_line(), if !out_done => match res? {
                    Some(line) => Some(line),
                    None => { out_done = true; None }
                },
                res = err.next_line(), if !err_done => match res? {
                    Some(line) => Some(line),
                    None => { err_done = true; None }
                },
            };
            let Some(line) = line else { continue };
            if let Some(url) = extract_url(&line) {
                if open_browser && !opened {
                    open_url(&url);
                }
                opened = true;
                on_progress("Opening your browser to finish signing in…", Some(&url));
            } else if let Some(safe) = sanitize_line(&line) {
                last_status = Some(safe.clone());
                on_progress(&safe, None);
            }
        }
        Ok::<_, anyhow::Error>(child.wait().await?)
    })
    .await
    .map_err(|_| {
        anyhow!(
            "Sign-in timed out after {} seconds.",
            LOGIN_TIMEOUT.as_secs()
        )
    })??;

    if !status.success() {
        return Err(anyhow!(
            "{}",
            format_login_exit_error(last_status.as_deref())
        ));
    }
    Ok(())
}

fn format_login_exit_error(last_status: Option<&str>) -> String {
    let detail = last_status
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("the CLI exited before authenticating");
    let detail = detail.strip_prefix("error: ").unwrap_or(detail).trim();
    if detail.starts_with("unknown option") || detail.starts_with("unknown command") {
        return format!(
            "Sign-in failed because the CLI rejected the login command: {detail}. Update the provider CLI or set BURNRATE_CLAUDE_BIN / BURNRATE_CODEX_BIN to a compatible binary."
        );
    }
    format!("Sign-in did not complete: {detail}")
}

fn login_command(
    provider: ProviderKind,
    email_hint: Option<&str>,
) -> Result<(String, Vec<String>, &'static str)> {
    match provider {
        ProviderKind::ClaudeCode => Ok((
            claude::claude_binary(),
            claude::claude_login_args(email_hint),
            "CLAUDE_CONFIG_DIR",
        )),
        ProviderKind::Codex => Ok((
            codex::codex_binary(),
            codex::codex_login_args(),
            "CODEX_HOME",
        )),
        _ => Err(anyhow!(
            "Interactive sign-in is only available for Claude Code and Codex."
        )),
    }
}

fn logout_command(provider: ProviderKind) -> Result<(String, Vec<String>, &'static str)> {
    match provider {
        ProviderKind::ClaudeCode => Ok((
            claude::claude_binary(),
            claude::claude_logout_args(),
            "CLAUDE_CONFIG_DIR",
        )),
        ProviderKind::Codex => Ok((
            codex::codex_binary(),
            codex::codex_logout_args(),
            "CODEX_HOME",
        )),
        _ => Err(anyhow!(
            "Sign-out is only available for Claude Code and Codex."
        )),
    }
}

async fn verify(provider: ProviderKind, config_dir: Option<&str>) -> Result<Option<String>> {
    match provider {
        ProviderKind::ClaudeCode => claude::login_verify(config_dir).await,
        ProviderKind::Codex => codex::login_verify(config_dir),
        _ => Ok(None),
    }
}

/// Find the first `https://` URL in a line, trimming trailing punctuation.
fn extract_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(['.', ',', ')', ']', '}', '"', '\'']);
    (url.len() > "https://".len()).then(|| url.to_string())
}

/// True if a line looks like it carries a secret (token, key, or JWT).
fn contains_secret(line: &str) -> bool {
    line.contains("sk-ant-") || line.contains("eyJ") || has_long_token_run(line)
}

/// True if the line contains an unbroken 40+ char run of token-shaped chars.
fn has_long_token_run(line: &str) -> bool {
    let mut run = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            run += 1;
            if run >= 40 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// Allowlist a progress line for display: drop empties and anything that looks
/// like it contains a secret; truncate the rest.
fn sanitize_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || contains_secret(trimmed) {
        return None;
    }
    Some(trimmed.chars().take(200).collect())
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let program = "open";
    let _ = std::process::Command::new(program).arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_https_url_and_trims_punctuation() {
        assert_eq!(
            extract_url("Visit https://auth.openai.com/device?code=ABC to continue."),
            Some("https://auth.openai.com/device?code=ABC".to_string())
        );
        assert_eq!(
            extract_url("Open (https://claude.ai/oauth/authorize?x=1)"),
            Some("https://claude.ai/oauth/authorize?x=1".to_string())
        );
        assert_eq!(extract_url("no link here"), None);
        assert_eq!(extract_url("http://insecure.example"), None);
    }

    #[test]
    fn detects_secret_shaped_lines() {
        assert!(contains_secret("token: sk-ant-oat01-deadbeef"));
        assert!(contains_secret("id_token eyJhbGciOiJSUzI1NiJ9.payload"));
        assert!(contains_secret(&format!("blob {}", "a".repeat(40))));
        assert!(!contains_secret(
            "Waiting for you to authorize in the browser"
        ));
    }

    #[test]
    fn sanitize_line_drops_secrets_and_empties_keeps_status() {
        assert_eq!(sanitize_line("   "), None);
        assert_eq!(sanitize_line("sk-ant-oat01-secretvalue"), None);
        assert_eq!(
            sanitize_line("  Waiting for authorization  "),
            Some("Waiting for authorization".to_string())
        );
    }

    #[test]
    fn login_manager_is_single_flight() {
        let manager = LoginManager::new();
        let (tx, _) = mpsc::unbounded_channel();
        manager.reserve("claude-code-a", false, tx).unwrap();
        // A second concurrent login is rejected.
        let (tx, _) = mpsc::unbounded_channel();
        assert!(manager.reserve("codex-b", false, tx).is_err());
        // After finishing, the slot frees up.
        manager.finish("claude-code-a");
        let (tx, _) = mpsc::unbounded_channel();
        manager.reserve("codex-b", true, tx).unwrap();
        // Cancel of a stale id is a no-op (returns None).
        assert_eq!(manager.cancel("claude-code-a"), None);
        // Cancelling the active reauth login reports is_reauth = true.
        assert_eq!(manager.cancel("codex-b"), Some(true));
        let (tx, _) = mpsc::unbounded_channel();
        manager.reserve("claude-code-c", false, tx).unwrap();
        assert_eq!(manager.cancel("claude-code-c"), Some(false));
    }

    #[test]
    fn login_manager_forwards_submitted_input_to_active_login() {
        let manager = LoginManager::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        manager.reserve("claude-code-a", false, tx).unwrap();

        manager
            .submit_input("claude-code-a", "auth-code#state".to_string())
            .unwrap();

        assert_eq!(rx.try_recv().unwrap(), "auth-code#state");
        assert!(
            manager
                .submit_input("other", "ignored".to_string())
                .is_err()
        );
    }

    #[test]
    fn login_command_rejects_non_cli_providers() {
        assert!(login_command(ProviderKind::OpenRouter, None).is_err());
        assert!(logout_command(ProviderKind::Runpod).is_err());
        let (_, args, env_key) = login_command(ProviderKind::Codex, None).unwrap();
        assert_eq!(args, vec!["login"]);
        assert_eq!(env_key, "CODEX_HOME");
        let (_, args, env_key) = login_command(ProviderKind::ClaudeCode, Some("a@b.com")).unwrap();
        assert_eq!(env_key, "CLAUDE_CONFIG_DIR");
        assert_eq!(args, vec!["auth", "login", "--email", "a@b.com"]);
    }

    #[test]
    fn formats_cli_usage_failures_without_nested_error_prefix() {
        let error = format_login_exit_error(Some("error: unknown option '--claudeai'"));

        assert!(!error.contains("Sign-in did not complete: error:"));
        assert!(error.contains("unknown option '--claudeai'"));
        assert!(error.contains("set BURNRATE_CLAUDE_BIN"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_login_inner_streams_url_and_passes_config_dir() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("login-fixture");
        // Fixture prints a secret (must be redacted), an auth URL, records the
        // config-dir env it received, then exits 0.
        std::fs::write(
            &binary,
            r#"#!/bin/sh
echo "token sk-ant-oat01-supersecretvalue"
echo "Open https://auth.example/device?code=XYZ in your browser"
echo "configdir=$CODEX_HOME" > "$CODEX_HOME/seen"
exit 0
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&binary).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary, perms).unwrap();

        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();

        let mut events: Vec<(String, Option<String>)> = Vec::new();
        let mut on_progress = |line: &str, url: Option<&str>| {
            events.push((line.to_string(), url.map(str::to_string)))
        };

        run_login_inner(
            &binary.to_string_lossy(),
            &["login".to_string()],
            "CODEX_HOME",
            Some(home.to_str().unwrap()),
            false,
            mpsc::unbounded_channel().1,
            &mut on_progress,
        )
        .await
        .unwrap();

        // The auth URL was surfaced.
        assert!(
            events
                .iter()
                .any(|(_, url)| url.as_deref() == Some("https://auth.example/device?code=XYZ"))
        );
        // The secret line was never forwarded.
        assert!(events.iter().all(|(line, _)| !line.contains("sk-ant-")));
        // The config dir was threaded into the child env.
        let seen = std::fs::read_to_string(home.join("seen")).unwrap();
        assert!(seen.contains(home.to_str().unwrap()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_login_inner_errors_on_nonzero_exit() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("fail-fixture");
        std::fs::write(
            &binary,
            "#!/bin/sh\necho \"could not reach the auth server\"\nexit 1\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&binary).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary, perms).unwrap();

        let mut on_progress = |_: &str, _: Option<&str>| {};
        let error = run_login_inner(
            &binary.to_string_lossy(),
            &["login".to_string()],
            "CODEX_HOME",
            Some(dir.path().to_str().unwrap()),
            false,
            mpsc::unbounded_channel().1,
            &mut on_progress,
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("Sign-in did not complete"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_login_inner_writes_submitted_input_to_child_stdin() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("stdin-fixture");
        let seen = dir.path().join("seen");
        std::fs::write(
            &binary,
            format!(
                r#"#!/bin/sh
echo "Open https://auth.example/manual in your browser"
IFS= read -r code
printf "%s" "$code" > "{}"
exit 0
"#,
                seen.display()
            ),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&binary).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary, perms).unwrap();

        let (tx, rx) = mpsc::unbounded_channel();
        let mut on_progress = |_: &str, _: Option<&str>| {};
        let binary = binary.to_string_lossy().to_string();
        let args = vec!["login".to_string()];
        let pending = run_login_inner(
            &binary,
            &args,
            "CODEX_HOME",
            Some(dir.path().to_str().unwrap()),
            false,
            rx,
            &mut on_progress,
        );

        tx.send("auth-code#state".to_string()).unwrap();
        pending.await.unwrap();

        assert_eq!(std::fs::read_to_string(seen).unwrap(), "auth-code#state");
    }
}
