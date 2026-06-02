use chrono::Utc;
use tauri::{
    App, AppHandle, Emitter, Manager, Wry,
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::models::{SnapshotStatus, TraySummary, UsageSnapshot};

const TRAY_ID: &str = "main";

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

    let status = if critical_count > 0 {
        SnapshotStatus::Exhausted
    } else if warning_count > 0 {
        SnapshotStatus::Warning
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
    let show = MenuItem::with_id(app, "show", "Show Burnrate", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &refresh, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon()?)
        .icon_as_template(true)
        .tooltip("Burnrate")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "refresh" => {
                let _ = app.emit("burnrate-refresh-requested", ());
                show_main_window(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

pub(crate) fn update_summary(app: &AppHandle<Wry>, summary: &TraySummary) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(summary.label.as_str()));
    }
}

fn show_main_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
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
    fn tray_icon_loads_packaged_asset() {
        let icon = tray_icon().expect("tray icon should decode");

        assert_eq!(icon.width(), 32);
        assert_eq!(icon.height(), 32);
    }
}
