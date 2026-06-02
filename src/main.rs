#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod config;
mod key_store;
mod models;
mod providers;
mod tray;

use app_state::AppState;
use models::{AccountInput, AccountView, AppSettings, DashboardState};
use std::time::Duration;

#[cfg(target_os = "macos")]
use tauri::menu::{IsMenuItem, PredefinedMenuItem, Submenu};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, LogicalUnit, Manager, PixelUnit, Position,
    Size, State, WindowSizeConstraints, Wry, menu::Menu,
};

const BACKGROUND_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const PREFERENCES_MIN_WIDTH: f64 = 360.0;
const PREFERENCES_MIN_HEIGHT: f64 = 360.0;
const PREFERENCES_MAX_CONTENT_WIDTH: f64 = 1180.0;
const PREFERENCES_SCREEN_MARGIN: f64 = 18.0;

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
    let _ = app.emit("burnrate-settings-updated", &settings);
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
) -> Result<DashboardState, String> {
    let dashboard = state.dashboard().await.map_err(|error| error.to_string())?;
    tray::update_summary(&app, &dashboard.tray_summary);
    let _ = app.emit("burnrate-dashboard-updated", &dashboard);
    Ok(dashboard)
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
    let work_area = monitor.work_area();
    let work_size = work_area.size.to_logical::<f64>(monitor.scale_factor());
    let work_position = work_area.position.to_logical::<f64>(monitor.scale_factor());
    let available_width = (work_size.width - (PREFERENCES_SCREEN_MARGIN * 2.0)).max(1.0);
    let available_height = (work_size.height - (PREFERENCES_SCREEN_MARGIN * 2.0)).max(1.0);
    let min_width = PREFERENCES_MIN_WIDTH.min(available_width);
    let min_height = PREFERENCES_MIN_HEIGHT.min(available_height);
    let preferred_width = width.min(PREFERENCES_MAX_CONTENT_WIDTH);
    let target_width = (preferred_width + chrome_width)
        .ceil()
        .clamp(min_width, available_width);
    let target_height = (height + chrome_height)
        .ceil()
        .clamp(min_height, available_height);

    window
        .set_size_constraints(WindowSizeConstraints {
            min_width: Some(PixelUnit::Logical(LogicalUnit::new(min_width))),
            min_height: Some(PixelUnit::Logical(LogicalUnit::new(min_height))),
            max_width: Some(PixelUnit::Logical(LogicalUnit::new(available_width))),
            max_height: Some(PixelUnit::Logical(LogicalUnit::new(available_height))),
        })
        .map_err(|error| error.to_string())?;
    window
        .set_size(Size::Logical(LogicalSize::new(target_width, target_height)))
        .map_err(|error| error.to_string())?;

    let current_position = window
        .outer_position()
        .map_err(|error| error.to_string())?
        .to_logical::<f64>(scale_factor);
    let min_x = work_position.x + PREFERENCES_SCREEN_MARGIN;
    let min_y = work_position.y + PREFERENCES_SCREEN_MARGIN;
    let max_x =
        (work_position.x + work_size.width - target_width - PREFERENCES_SCREEN_MARGIN).max(min_x);
    let max_y =
        (work_position.y + work_size.height - target_height - PREFERENCES_SCREEN_MARGIN).max(min_y);
    let target_x = current_position.x.clamp(min_x, max_x);
    let target_y = current_position.y.clamp(min_y, max_y);

    window
        .set_position(Position::Logical(LogicalPosition::new(target_x, target_y)))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn close_preferences(app: AppHandle) {
    tray::close_main_window(&app);
}

fn spawn_background_refresh(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            refresh_dashboard_for_app(&app).await;
            tokio::time::sleep(BACKGROUND_REFRESH_INTERVAL).await;
        }
    });
}

async fn refresh_dashboard_for_app(app: &AppHandle) {
    let state = app.state::<AppState>();
    match state.dashboard().await {
        Ok(dashboard) => {
            tray::update_summary(app, &dashboard.tray_summary);
            let _ = app.emit("burnrate-dashboard-updated", &dashboard);
        }
        Err(error) => {
            eprintln!("Burnrate background refresh failed: {error}");
        }
    }
}

#[cfg(target_os = "macos")]
fn build_app_menu(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    let undo = PredefinedMenuItem::undo(app, None)?;
    let redo = PredefinedMenuItem::redo(app, None)?;
    let separator_one = PredefinedMenuItem::separator(app)?;
    let cut = PredefinedMenuItem::cut(app, None)?;
    let copy = PredefinedMenuItem::copy(app, None)?;
    let paste = PredefinedMenuItem::paste(app, None)?;
    let select_all = PredefinedMenuItem::select_all(app, None)?;
    let separator_two = PredefinedMenuItem::separator(app)?;
    let edit_items: [&dyn IsMenuItem<Wry>; 8] = [
        &undo,
        &redo,
        &separator_one,
        &cut,
        &copy,
        &paste,
        &select_all,
        &separator_two,
    ];
    let edit = Submenu::with_items(app, "Edit", true, &edit_items)?;
    let close = PredefinedMenuItem::close_window(app, None)?;
    let window = Submenu::with_items(app, "Window", true, &[&close])?;

    Menu::with_items(app, &[&edit, &window])
}

#[cfg(not(target_os = "macos"))]
fn build_app_menu(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    Menu::new(app)
}

fn main() {
    let state = AppState::load().expect("failed to initialize Burnrate state");

    tauri::Builder::default()
        .menu(build_app_menu)
        .manage(state)
        .setup(move |app| {
            tray::apply_activation_policy(app.handle(), true);
            if let Some(window) = app.get_webview_window(tray::MAIN_WINDOW) {
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        tray::close_main_window(&app_handle);
                    }
                });
            }
            tray::install(app)?;
            spawn_background_refresh(app.handle().clone());
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
            resize_preferences_to_content,
            close_preferences
        ])
        .run(tauri::generate_context!())
        .expect("error while running Burnrate");
}
