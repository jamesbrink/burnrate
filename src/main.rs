#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod config;
mod key_store;
mod models;
mod providers;
mod tray;

use app_state::AppState;
use models::{AccountInput, AccountView, AppSettings, DashboardState, UsageSnapshot};
use tauri::{AppHandle, LogicalSize, Manager, Size, State};

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
    tray::apply_activation_policy(&app, settings.hide_from_dock);
    tray::rebuild(&app, settings.clone()).map_err(|error| error.to_string())?;
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

#[tauri::command]
fn resize_preferences_to_content(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let window = app
        .get_webview_window(tray::MAIN_WINDOW)
        .ok_or_else(|| "preferences window is unavailable".to_string())?;
    let scale_factor = window.scale_factor().map_err(|error| error.to_string())?;
    let inner = window
        .inner_size()
        .map_err(|error| error.to_string())?
        .to_logical::<f64>(scale_factor);
    let outer = window
        .outer_size()
        .map_err(|error| error.to_string())?
        .to_logical::<f64>(scale_factor);
    let chrome_width = (outer.width - inner.width).max(0.0);
    let chrome_height = (outer.height - inner.height).max(0.0);
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or(window
            .primary_monitor()
            .map_err(|error| error.to_string())?)
        .ok_or_else(|| "no monitor available for preferences window".to_string())?;
    let work_area = monitor
        .work_area()
        .size
        .to_logical::<f64>(monitor.scale_factor());
    let target_width = (width + chrome_width).ceil().clamp(1.0, work_area.width);
    let target_height = (height + chrome_height).ceil().clamp(1.0, work_area.height);

    window
        .set_min_size(None::<Size>)
        .map_err(|error| error.to_string())?;
    window
        .set_size(Size::Logical(LogicalSize::new(target_width, target_height)))
        .map_err(|error| error.to_string())
}

fn main() {
    let state = AppState::load().expect("failed to initialize Burnrate state");
    let hide_from_dock = state.settings().hide_from_dock;

    tauri::Builder::default()
        .manage(state)
        .setup(move |app| {
            tray::apply_activation_policy(app.handle(), hide_from_dock);
            let _ = app.handle().remove_menu();
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
            refresh_snapshots,
            resize_preferences_to_content
        ])
        .run(tauri::generate_context!())
        .expect("error while running Burnrate");
}
