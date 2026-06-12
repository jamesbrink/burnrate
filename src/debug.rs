//! Hidden `burnrate debug <command>` CLI for headless diagnostics.
//!
//! Runs the real provider/config code paths without launching the Tauri app,
//! so detection, startup load, and usage fetches can be exercised and verified
//! from a shell (sandbox with `BURNRATE_CONFIG_DIR` to avoid the real config).
//! Not a public surface: only reachable via an explicit `debug` first argument,
//! which a normal GUI launch never passes.

use serde_json::json;

use crate::{app_state::AppState, insights, models::ProviderKind, providers};

pub(crate) fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("env") => env_report(),
        Some("detect") => detect(),
        Some("load") => load(),
        Some("snapshot") => snapshot(),
        Some("insights") => local_insights(),
        _ => {
            eprintln!("usage: burnrate debug <env|detect|load|snapshot|insights>");
            eprintln!("  env       report credential-override env vars and resolved provider CLIs");
            eprintln!("  detect    run provider auto-detection and print the result (read-only)");
            eprintln!(
                "  load      full startup load: detect + merge + orphan GC (persists config)"
            );
            eprintln!("  snapshot  load, then fetch a usage snapshot for every enabled account");
            eprintln!(
                "  insights  collect claudex-backed local usage for all local-source providers"
            );
            2
        }
    }
}

/// Which credential-override env vars the app inherited (and strips from
/// spawned CLIs), the detection-relevant home overrides, and the provider
/// binaries it would actually run.
fn env_report() -> i32 {
    let overrides: Vec<_> = providers::CREDENTIAL_ENV_OVERRIDES
        .iter()
        .map(|key| {
            json!({
                "var": key,
                "set": std::env::var_os(key).is_some(),
                "strippedFromCliSpawns": true,
            })
        })
        .collect();
    let homes: Vec<_> = ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "BURNRATE_CONFIG_DIR"]
        .iter()
        .map(|key| json!({ "var": key, "value": std::env::var(key).ok() }))
        .collect();
    let report = json!({
        "credentialOverrides": overrides,
        "homeOverrides": homes,
        "claudeBinary": providers::resolve_cli(&providers::claude_binary_name())
            .display()
            .to_string(),
        "codexBinary": providers::resolve_cli(&providers::codex_binary_name())
            .display()
            .to_string(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("serialize env report")
    );
    0
}

fn detect() -> i32 {
    let detected: Vec<_> = providers::detect_accounts()
        .into_iter()
        .map(|account| {
            json!({
                "id": account.id,
                "provider": account.provider,
                "label": account.label,
                "email": account.email,
                "credentialPath": account.credential_path,
                "configDir": account.config_dir,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&detected).expect("serialize detection")
    );
    if detected.is_empty() {
        eprintln!("warning: no accounts detected — check `burnrate debug env` for home overrides");
    }
    0
}

fn load() -> i32 {
    match AppState::load() {
        Ok(state) => match state.list_accounts() {
            Ok(views) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&views).expect("serialize account views")
                );
                0
            }
            Err(error) => {
                eprintln!("error: {error:#}");
                1
            }
        },
        Err(error) => {
            eprintln!("error: {error:#}");
            1
        }
    }
}

fn snapshot() -> i32 {
    let state = match AppState::load() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("error: {error:#}");
            return 1;
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let snapshots = runtime.block_on(state.snapshots());
    println!(
        "{}",
        serde_json::to_string_pretty(&snapshots).expect("serialize snapshots")
    );
    0
}

/// Collect claudex-backed local usage for every provider that has a local
/// session source, regardless of configured accounts — the headless way to
/// verify the insights pipeline (and trigger a first index build).
fn local_insights() -> i32 {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let report = runtime.block_on(insights::collect_local_usage(vec![
        ProviderKind::ClaudeCode,
        ProviderKind::Codex,
        ProviderKind::Copilot,
    ]));
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("serialize insights report")
    );
    if report.available { 0 } else { 1 }
}
