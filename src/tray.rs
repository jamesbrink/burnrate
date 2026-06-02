use chrono::Utc;
use tauri::{
    App, AppHandle, Emitter, LogicalPosition, Manager, Wry,
    image::Image,
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::{
    app_state::AppState,
    models::{AppSettings, SnapshotStatus, TraySummary, UsageSnapshot},
};

const TRAY_ID: &str = "main";
pub(crate) const MAIN_WINDOW: &str = "main";
const TRAY_WINDOW: &str = "tray";

pub(crate) fn summarize(snapshots: &[UsageSnapshot]) -> TraySummary {
    let critical_count = snapshots
        .iter()
        .filter(|snapshot| {
            matches!(
                snapshot.status,
                SnapshotStatus::Exhausted | SnapshotStatus::Error
            )
        })
        .count();
    let warning_count = snapshots
        .iter()
        .filter(|snapshot| snapshot.status == SnapshotStatus::Warning)
        .count();
    let stale_count = snapshots
        .iter()
        .filter(|snapshot| snapshot.status == SnapshotStatus::Stale)
        .count();

    let status = if critical_count > 0 {
        SnapshotStatus::Exhausted
    } else if warning_count > 0 {
        SnapshotStatus::Warning
    } else if stale_count > 0 {
        SnapshotStatus::Stale
    } else if snapshots.is_empty() {
        SnapshotStatus::NotConfigured
    } else {
        SnapshotStatus::Healthy
    };

    let label = match status {
        SnapshotStatus::Healthy => "Burnrate: all quotas healthy".to_string(),
        SnapshotStatus::Warning => format!("Burnrate: {warning_count} warning"),
        SnapshotStatus::Exhausted => format!("Burnrate: {critical_count} critical"),
        SnapshotStatus::NotConfigured => "Burnrate: no enabled accounts".to_string(),
        SnapshotStatus::Error => "Burnrate: refresh error".to_string(),
        SnapshotStatus::Stale => "Burnrate: data is stale".to_string(),
    };

    TraySummary {
        label,
        status,
        critical_count,
        warning_count,
        updated_at: Utc::now(),
    }
}

pub(crate) fn install(app: &mut App<Wry>) -> tauri::Result<()> {
    rebuild(app.handle(), app.state::<AppState>().settings())
}

pub(crate) fn rebuild(app: &AppHandle<Wry>, settings: AppSettings) -> tauri::Result<()> {
    let preferences =
        MenuItem::with_id(app, "preferences", "Open Preferences", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
    let hide_dock = CheckMenuItem::with_id(
        app,
        "hide-dock",
        "Hide Dock Icon",
        true,
        settings.hide_from_dock,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit Burnrate", true, None::<&str>)?;
    let items: [&dyn IsMenuItem<Wry>; 4] = [&preferences, &refresh, &hide_dock, &quit];
    let menu = Menu::with_items(app, &items)?;

    let _ = app.remove_tray_by_id(TRAY_ID);

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon()?)
        .icon_as_template(true)
        .tooltip("Burnrate")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "preferences" => show_main_window(app),
            "refresh" => {
                let _ = app.emit("burnrate-refresh-requested", ());
            }
            "hide-dock" => toggle_hide_dock(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                show_tray_window(tray.app_handle(), position);
            }
        })
        .build(app)?;

    Ok(())
}

pub(crate) fn apply_activation_policy(app: &AppHandle<Wry>, hide_from_dock: bool) {
    let policy = if hide_from_dock {
        tauri::ActivationPolicy::Accessory
    } else {
        tauri::ActivationPolicy::Regular
    };
    let _ = app.set_activation_policy(policy);
}

pub(crate) fn update_summary(app: &AppHandle<Wry>, summary: &TraySummary) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(summary.label.as_str()));
    }
}

fn show_main_window(app: &AppHandle<Wry>) {
    #[cfg(target_os = "macos")]
    let _ = app.show();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn toggle_hide_dock(app: &AppHandle<Wry>) {
    let state = app.state::<AppState>();
    let mut settings = state.settings();
    settings.hide_from_dock = !settings.hide_from_dock;
    if let Ok(settings) = state.save_settings(settings) {
        apply_activation_policy(app, settings.hide_from_dock);
        let _ = rebuild(app, settings);
    }
}

fn show_tray_window(app: &AppHandle<Wry>, position: tauri::PhysicalPosition<f64>) {
    if let Some(window) = app.get_webview_window(TRAY_WINDOW) {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            return;
        }

        let scale_factor = window.scale_factor().unwrap_or(1.0);
        let position = position.to_logical::<f64>(scale_factor);
        let _ = window.set_position(tray_popup_position(position));
        let _ = window.show();
        let _ = app.emit("burnrate-refresh-requested", ());
    }
}

fn tray_popup_position(position: LogicalPosition<f64>) -> LogicalPosition<f64> {
    LogicalPosition::new((position.x - 180.0).max(8.0), (position.y + 12.0).max(8.0))
}

fn tray_icon() -> tauri::Result<Image<'static>> {
    Image::from_bytes(include_bytes!("../icons/tray.png"))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::models::{ProviderKind, UsageSnapshot};

    fn snapshot(status: SnapshotStatus) -> UsageSnapshot {
        UsageSnapshot {
            account_id: "account".to_string(),
            provider: ProviderKind::OpenRouter,
            label: "OpenRouter".to_string(),
            status,
            subscription: None,
            usage_buckets: Vec::new(),
            quota: None,
            burn_rate: None,
            message: None,
            fetched_at: Utc::now(),
        }
    }

    #[test]
    fn summary_promotes_errors_to_critical() {
        let summary = summarize(&[
            snapshot(SnapshotStatus::Healthy),
            snapshot(SnapshotStatus::Error),
        ]);

        assert_eq!(summary.status, SnapshotStatus::Exhausted);
        assert_eq!(summary.critical_count, 1);
    }

    #[test]
    fn summary_reports_empty_state() {
        let summary = summarize(&[]);

        assert_eq!(summary.status, SnapshotStatus::NotConfigured);
    }

    #[test]
    fn summary_reports_stale_when_cached_data_is_used() {
        let summary = summarize(&[snapshot(SnapshotStatus::Stale)]);

        assert_eq!(summary.status, SnapshotStatus::Stale);
        assert_eq!(summary.label, "Burnrate: data is stale");
    }

    #[test]
    fn tray_icon_loads_packaged_asset() {
        let icon = tray_icon().expect("tray icon should decode");

        assert_eq!(icon.width(), 32);
        assert_eq!(icon.height(), 32);
    }

    #[test]
    fn tray_popup_position_clamps_left_and_top_edges() {
        let position = tray_popup_position(LogicalPosition::new(20.0, -40.0));

        assert_eq!(position.x, 8.0);
        assert_eq!(position.y, 8.0);
    }

    #[test]
    fn install_removes_existing_tray_before_rebuild() {
        let src = include_str!("tray.rs");
        let remove_pos = src
            .find("remove_tray_by_id(TRAY_ID)")
            .expect("tray rebuild should remove the previous tray by id");
        let build_pos = src
            .find("TrayIconBuilder::with_id(TRAY_ID)")
            .expect("tray rebuild should build the tray by id");

        assert!(remove_pos < build_pos);
    }
}
