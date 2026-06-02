use chrono::Utc;
use tauri::{
    App, AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Wry,
    image::Image,
    menu::{IsMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::models::{SnapshotStatus, TraySummary, UsageSnapshot};

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
    rebuild(app.handle())
}

pub(crate) fn rebuild(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let preferences =
        MenuItem::with_id(app, "preferences", "Open Preferences", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Burnrate", true, None::<&str>)?;
    let items: [&dyn IsMenuItem<Wry>; 3] = [&preferences, &refresh, &quit];
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

#[cfg(target_os = "macos")]
pub(crate) fn apply_activation_policy(app: &AppHandle<Wry>, hide_from_dock: bool) {
    let policy = if hide_from_dock {
        tauri::ActivationPolicy::Accessory
    } else {
        tauri::ActivationPolicy::Regular
    };
    let _ = app.set_activation_policy(policy);
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn apply_activation_policy(_app: &AppHandle<Wry>, _hide_from_dock: bool) {}

pub(crate) fn update_summary(app: &AppHandle<Wry>, summary: &TraySummary) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(summary.label.as_str()));
    }
}

pub(crate) fn show_main_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        if let Ok(icon) = app_icon() {
            let _ = window.set_icon(icon);
        }
        #[cfg(target_os = "macos")]
        {
            apply_activation_policy(app, false);
            let _ = app.show();
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub(crate) fn close_main_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    apply_activation_policy(app, true);
}

fn show_tray_window(app: &AppHandle<Wry>, position: tauri::PhysicalPosition<f64>) {
    if let Some(window) = app.get_webview_window(TRAY_WINDOW) {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            return;
        }

        let scale_factor = window.scale_factor().unwrap_or(1.0);
        let position = position.to_logical::<f64>(scale_factor);
        let window_size = window
            .outer_size()
            .map(|size| size.to_logical::<f64>(scale_factor))
            .unwrap_or_else(|_| LogicalSize::new(380.0, 520.0));
        let work_area = window
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| window.primary_monitor().ok().flatten())
            .map(|monitor| {
                let area = monitor.work_area();
                (
                    area.position.to_logical::<f64>(monitor.scale_factor()),
                    area.size.to_logical::<f64>(monitor.scale_factor()),
                )
            })
            .unwrap_or_else(|| {
                (
                    LogicalPosition::new(0.0, 0.0),
                    LogicalSize::new(1920.0, 1080.0),
                )
            });
        let _ = window.set_position(tray_popup_position(position, window_size, work_area));
        let _ = window.show();
        let _ = app.emit("burnrate-refresh-requested", ());
    }
}

fn tray_popup_position(
    position: LogicalPosition<f64>,
    window_size: LogicalSize<f64>,
    work_area: (LogicalPosition<f64>, LogicalSize<f64>),
) -> LogicalPosition<f64> {
    let (work_position, work_size) = work_area;
    let min_x = work_position.x + 8.0;
    let min_y = work_position.y + 8.0;
    let max_x = (work_position.x + work_size.width - window_size.width - 8.0).max(min_x);
    let max_y = (work_position.y + work_size.height - window_size.height - 8.0).max(min_y);
    LogicalPosition::new(
        (position.x - window_size.width / 2.0).clamp(min_x, max_x),
        (position.y + 12.0).clamp(min_y, max_y),
    )
}

fn tray_icon() -> tauri::Result<Image<'static>> {
    Image::from_bytes(include_bytes!("../icons/tray.png"))
}

fn app_icon() -> tauri::Result<Image<'static>> {
    Image::from_bytes(include_bytes!("../icons/icon.png"))
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
    fn app_icon_loads_packaged_asset() {
        let icon = app_icon().expect("app icon should decode");

        assert!(icon.width() >= 128);
        assert!(icon.height() >= 128);
    }

    #[test]
    fn tray_popup_position_clamps_to_work_area() {
        let size = LogicalSize::new(380.0, 520.0);
        let work_area = (
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(1024.0, 768.0),
        );
        let position = tray_popup_position(LogicalPosition::new(20.0, -40.0), size, work_area);

        assert_eq!(position.x, 8.0);
        assert_eq!(position.y, 8.0);

        let position = tray_popup_position(LogicalPosition::new(1000.0, 760.0), size, work_area);

        assert_eq!(position.x, 636.0);
        assert_eq!(position.y, 240.0);
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
