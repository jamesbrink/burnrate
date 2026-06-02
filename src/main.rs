#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod config;
mod key_store;
mod models;
mod providers;
mod tray;

use app_state::AppState;
use models::{AccountInput, AccountView, AppSettings, DashboardState, UsageSnapshot};
use tauri::{AppHandle, State};

#[tauri::command]
async fn dashboard(app: AppHandle, state: State<'_, AppState>) -> Result<DashboardState, String> {
    let dashboard = state.dashboard().await.map_err(|error| error.to_string())?;
    tray::update_summary(&app, &dashboard.tray_summary);
    Ok(dashboard)
}

#[tauri::command]
fn list_accounts(state: State<'_, AppState>) -> Result<Vec<AccountView>, String> {
    state.list_accounts().map_err(|error| error.to_string())
}

#[tauri::command]
fn save_account(
    state: State<'_, AppState>,
    input: AccountInput,
) -> Result<Vec<AccountView>, String> {
    state.save_account(input).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let settings = state
        .save_settings(settings)
        .map_err(|error| error.to_string())?;
    apply_activation_policy(&app, settings.hide_from_dock);
    Ok(settings)
}

#[tauri::command]
fn remove_account(state: State<'_, AppState>, id: String) -> Result<Vec<AccountView>, String> {
    state.remove_account(&id).map_err(|error| error.to_string())
}

#[tauri::command]
fn detect_accounts(state: State<'_, AppState>) -> Result<Vec<AccountView>, String> {
    state.detect_accounts().map_err(|error| error.to_string())
}

#[tauri::command]
async fn refresh_snapshots(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<UsageSnapshot>, String> {
    let snapshots = state.snapshots().await;
    let summary = tray::summarize(&snapshots);
    tray::update_summary(&app, &summary);
    Ok(snapshots)
}

fn main() {
    let state = AppState::load().expect("failed to initialize Burnrate state");
    let hide_from_dock = state.settings().hide_from_dock;

    tauri::Builder::default()
        .manage(state)
        .setup(move |app| {
            apply_activation_policy(app.handle(), hide_from_dock);
            tray::install(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dashboard,
            list_accounts,
            save_account,
            save_settings,
            remove_account,
            detect_accounts,
            refresh_snapshots
        ])
        .run(tauri::generate_context!())
        .expect("error while running Burnrate");
}

fn apply_activation_policy(app: &AppHandle, hide_from_dock: bool) {
    let policy = if hide_from_dock {
        tauri::ActivationPolicy::Accessory
    } else {
        tauri::ActivationPolicy::Regular
    };
    let _ = app.set_activation_policy(policy);
}
