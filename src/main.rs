#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod config;
mod debug;
mod insights;
mod key_store;
#[cfg(target_os = "linux")]
mod linux_desktop;
mod models;
mod providers;
mod storage;
mod tray;
mod updater;

use app_state::AppState;
use models::{
    AccountInput, AccountView, AppSettings, DashboardState, LocalUsageReport, LoginFailed,
    ProviderKind,
};
use std::time::Duration;

#[cfg(target_os = "macos")]
use tauri::menu::{IsMenuItem, PredefinedMenuItem, Submenu};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, LogicalUnit, Manager, PhysicalPosition,
    PhysicalSize, PixelUnit, Position, Size, State, WindowSizeConstraints, Wry, menu::Menu,
};

const BACKGROUND_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const PREFERENCES_MIN_WIDTH: f64 = 360.0;
const PREFERENCES_MIN_HEIGHT: f64 = 360.0;
const PREFERENCES_MAX_CONTENT_WIDTH: f64 = 1180.0;
const PREFERENCES_SCREEN_MARGIN: f64 = 18.0;
const TRAY_CONTENT_WIDTH: f64 = 360.0;
const TRAY_MAX_CONTENT_WIDTH: f64 = 360.0;
const TRAY_MIN_HEIGHT: f64 = 200.0;
const TRAY_MAX_HEIGHT_WORK_RATIO: f64 = 2.0 / 3.0;
const TRAY_SCREEN_MARGIN: f64 = 8.0;
const TRAY_OFFSET_Y: f64 = 12.0;

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
async fn remove_account(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<AccountView>, String> {
    state
        .remove_account(&id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn detect_accounts(state: State<'_, AppState>) -> Result<Vec<AccountView>, String> {
    state.detect_accounts().map_err(|error| error.to_string())
}

#[tauri::command]
fn reorder_accounts(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<Vec<AccountView>, String> {
    state
        .reorder_accounts(&ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_account_login(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: ProviderKind,
    label: String,
    account_id: Option<String>,
) -> Result<AccountView, String> {
    state
        .start_account_login(app.clone(), provider, label, account_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn submit_account_login_code(
    state: State<'_, AppState>,
    id: String,
    code: String,
) -> Result<(), String> {
    state
        .submit_account_login_code(&id, code)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_account_login(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<AccountView>, String> {
    let (canceled, accounts) = state
        .cancel_account_login(&id)
        .map_err(|error| error.to_string())?;
    // Only signal failure when we actually aborted an active sign-in, so a cancel
    // that raced a completing login can't flip the UI from success to failure.
    if canceled {
        let _ = app.emit(
            "burnrate-login-failed",
            LoginFailed {
                id,
                error: "Sign-in canceled.".to_string(),
            },
        );
    }
    Ok(accounts)
}

#[tauri::command]
async fn logout_account(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<AccountView>, String> {
    let accounts = state
        .logout_account(&id)
        .await
        .map_err(|error| error.to_string())?;
    refresh_dashboard_for_app(&app).await;
    Ok(accounts)
}

#[tauri::command]
async fn refresh_snapshots(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DashboardState, String> {
    let dashboard = state.dashboard().await.map_err(|error| error.to_string())?;
    tray::update_summary(&app, &dashboard.tray_summary);
    let _ = app.emit("burnrate-dashboard-updated", &dashboard);
    spawn_local_usage_broadcast(app);
    Ok(dashboard)
}

/// On-demand claudex insights report (the push channel is
/// `burnrate-local-usage-updated`, broadcast after every dashboard refresh).
#[tauri::command]
async fn local_usage(state: State<'_, AppState>) -> Result<LocalUsageReport, String> {
    Ok(state.local_usage().await)
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
fn resize_tray_to_content(
    app: AppHandle,
    state: State<'_, tray::TrayWindowState>,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let window = app
        .get_webview_window(tray::TRAY_WINDOW)
        .ok_or_else(|| "tray window is unavailable".to_string())?;
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
    let anchor = state.anchor();
    let (monitor_scale, work_pos_phys, work_size_phys, work_size) = match anchor {
        Some(anchor) => {
            let (scale, position, size) = tray::cursor_monitor_geometry(&app, anchor);
            (
                scale,
                position,
                size,
                LogicalSize::new(size.width / scale, size.height / scale),
            )
        }
        None => {
            let monitor = window
                .current_monitor()
                .map_err(|error| error.to_string())?
                .or(window
                    .primary_monitor()
                    .map_err(|error| error.to_string())?)
                .ok_or_else(|| "no monitor available for tray window".to_string())?;
            let work_area = monitor.work_area();
            (
                monitor.scale_factor(),
                PhysicalPosition::new(work_area.position.x as f64, work_area.position.y as f64),
                PhysicalSize::new(work_area.size.width as f64, work_area.size.height as f64),
                work_area.size.to_logical::<f64>(monitor.scale_factor()),
            )
        }
    };

    let available_height = (work_size.height - (TRAY_SCREEN_MARGIN * 2.0)).max(1.0);
    let max_tray_height =
        (available_height * TRAY_MAX_HEIGHT_WORK_RATIO).max(TRAY_MIN_HEIGHT.min(available_height));
    let (target_width, target_height) = tray::clamp_tray_size(
        tray::TraySizeInput {
            content_width: width,
            content_height: height,
            chrome_width,
            chrome_height,
        },
        tray::TraySizeLimits {
            work_width: work_size.width,
            work_height: work_size.height,
            margin: TRAY_SCREEN_MARGIN,
            min_content_width: TRAY_CONTENT_WIDTH,
            min_height: TRAY_MIN_HEIGHT,
            max_height: max_tray_height,
            max_content_width: TRAY_MAX_CONTENT_WIDTH,
        },
    );

    window
        .set_size_constraints(WindowSizeConstraints {
            min_width: Some(PixelUnit::Logical(LogicalUnit::new(target_width))),
            min_height: Some(PixelUnit::Logical(LogicalUnit::new(
                TRAY_MIN_HEIGHT.min(target_height),
            ))),
            max_width: Some(PixelUnit::Logical(LogicalUnit::new(target_width))),
            max_height: Some(PixelUnit::Logical(LogicalUnit::new(max_tray_height))),
        })
        .map_err(|error| error.to_string())?;
    window
        .set_size(Size::Logical(LogicalSize::new(target_width, target_height)))
        .map_err(|error| error.to_string())?;

    // Re-anchor in physical pixels (unambiguous across monitors) from the
    // physical cursor anchor recorded at show time. Prefer the monitor under
    // that saved anchor so mixed-DPI resizing stays tied to the clicked display.
    let window_phys =
        PhysicalSize::new(target_width * monitor_scale, target_height * monitor_scale);
    let margin = TRAY_SCREEN_MARGIN * monitor_scale;
    let target = match anchor {
        Some(anchor) => tray::popup_position(
            anchor,
            window_phys,
            work_pos_phys,
            work_size_phys,
            margin,
            TRAY_OFFSET_Y * monitor_scale,
        ),
        None => {
            let current = window.outer_position().map_err(|error| error.to_string())?;
            let min_x = work_pos_phys.x + margin;
            let min_y = work_pos_phys.y + margin;
            let max_x =
                (work_pos_phys.x + work_size_phys.width - window_phys.width - margin).max(min_x);
            let max_y =
                (work_pos_phys.y + work_size_phys.height - window_phys.height - margin).max(min_y);
            PhysicalPosition::new(
                (current.x as f64).clamp(min_x, max_x),
                (current.y as f64).clamp(min_y, max_y),
            )
        }
    };

    window
        .set_position(PhysicalPosition::new(
            target.x.round() as i32,
            target.y.round() as i32,
        ))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn close_preferences(app: AppHandle) {
    tray::close_main_window(&app);
}

/// Open the Preferences window from the tray popover's settings gear, dismissing
/// the popover so focus moves cleanly to Preferences.
#[tauri::command]
fn open_preferences(app: AppHandle) {
    tray::show_main_window(&app);
    tray::hide_tray_window(&app);
}

/// Dismiss the tray popover (Esc in the tray view). Goes through the same hide
/// path as blur/toggle so the reopen guard stays consistent.
#[tauri::command]
fn hide_tray(app: AppHandle) {
    tray::hide_tray_window(&app);
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
            spawn_local_usage_broadcast(app.clone());
        }
        Err(error) => {
            eprintln!("Burnrate background refresh failed: {error}");
        }
    }
}

/// Collect claudex insights off the dashboard path and broadcast the result.
/// Detached on purpose: the first collection can rebuild the whole local
/// session index, and quota cards must never wait for it.
fn spawn_local_usage_broadcast(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let report = app.state::<AppState>().local_usage().await;
        let _ = app.emit("burnrate-local-usage-updated", &report);
    });
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

/// A startup failure has to be visible. Launched from Finder there is no
/// terminal, so a panic dies silently — an old build refusing a newer config
/// database (written by a newer release or a dev run) just looks like the app
/// won't open, and the auto-updater can never run to fix it. Print the error
/// always, and raise a blocking native alert when no terminal is attached.
fn fail_startup(error: &anyhow::Error) -> ! {
    eprintln!("Burnrate failed to start: {error:#}");
    #[cfg(target_os = "macos")]
    {
        use std::io::IsTerminal;
        if !std::io::stderr().is_terminal() {
            show_fatal_alert(&format!("{error:#}"));
        }
    }
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn show_fatal_alert(message: &str) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSAlert, NSAlertStyle, NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::NSString;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // Tauri never ran, so nothing has activated this process yet; without an
    // explicit activation the alert can open behind every other window (e.g.
    // for launch-at-login or an updater relaunch).
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Critical);
    alert.setMessageText(&NSString::from_str("Burnrate failed to start"));
    alert.setInformativeText(&NSString::from_str(message));
    alert.runModal();
}

fn main() {
    #[cfg(target_os = "linux")]
    linux_desktop::apply_runtime_environment();

    // Headless diagnostics: `burnrate debug <env|detect|load|snapshot>` runs the
    // real provider/config code paths and exits without starting the GUI.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("debug") {
        std::process::exit(debug::run(&args[2..]));
    }

    let state = match AppState::load() {
        Ok(state) => state,
        Err(error) => fail_startup(&error),
    };

    let builder = tauri::Builder::default().plugin(tauri_plugin_updater::Builder::new().build());
    // Notifications back the updater's "update available" alert; the dep is
    // macOS-only (see Cargo.toml), so the registration is too.
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_plugin_notification::init());
    builder
        .menu(build_app_menu)
        .manage(state)
        .manage(tray::TrayWindowState::default())
        .manage(updater::UpdaterState::default())
        .setup(move |app| {
            tray::apply_activation_policy(app.handle(), true);
            tray::set_dock_icon_if_unbundled();
            if let Some(window) = app.get_webview_window(tray::MAIN_WINDOW) {
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        tray::close_main_window(&app_handle);
                    }
                });
            }
            // Dismiss the tray popover when it loses focus (click-away / app
            // switch). Linux AppIndicator/XWayland focus can be lost while
            // moving from the tray icon into the popover, so Linux keeps the
            // panel open until the user toggles it or presses Esc.
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if let Some(window) = app.get_webview_window(tray::TRAY_WINDOW) {
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        tray::hide_tray_window(&app_handle);
                    }
                });
            }
            tray::install(app)?;
            tray::apply_tray_vibrancy(app.handle());
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
            reorder_accounts,
            start_account_login,
            submit_account_login_code,
            cancel_account_login,
            logout_account,
            refresh_snapshots,
            local_usage,
            resize_preferences_to_content,
            resize_tray_to_content,
            close_preferences,
            open_preferences,
            hide_tray,
            updater::updater_available,
            updater::check_for_updates,
            updater::install_pending_update,
            updater::notify_update_available
        ])
        .run(tauri::generate_context!())
        .expect("error while running Burnrate");
}
