use std::sync::Mutex;
use std::time::{Duration, Instant};

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
pub(crate) const TRAY_WINDOW: &str = "tray";

/// After the popover hides (toggle or blur), ignore a re-show for this long.
/// Clicking the status item first blurs the popover (hiding it) and then fires
/// a click event; without this guard the click would immediately reopen it.
const TRAY_REOPEN_GUARD: Duration = Duration::from_millis(250);

/// Runtime UI state for the tray popover window, managed by Tauri. Kept separate
/// from `AppState` so window concerns don't leak into config/provider state.
#[derive(Default)]
pub(crate) struct TrayWindowState {
    inner: Mutex<TrayWindowInner>,
}

#[derive(Default)]
struct TrayWindowInner {
    /// When the popover was last hidden (toggle or blur), to drive the reopen guard.
    last_hidden_at: Option<Instant>,
    /// Cursor anchor (logical desktop coords) recorded at show time, so a later
    /// content resize can re-run `tray_popup_position` and stay anchored.
    last_anchor: Option<LogicalPosition<f64>>,
}

impl TrayWindowState {
    fn mark_hidden(&self) {
        self.inner
            .lock()
            .expect("tray window state lock")
            .last_hidden_at = Some(Instant::now());
    }

    fn set_anchor(&self, anchor: LogicalPosition<f64>) {
        self.inner
            .lock()
            .expect("tray window state lock")
            .last_anchor = Some(anchor);
    }

    pub(crate) fn anchor(&self) -> Option<LogicalPosition<f64>> {
        self.inner
            .lock()
            .expect("tray window state lock")
            .last_anchor
    }

    fn should_suppress_show(&self, now: Instant) -> bool {
        self.inner
            .lock()
            .expect("tray window state lock")
            .last_hidden_at
            .is_some_and(|hidden| should_suppress_show(hidden, now, TRAY_REOPEN_GUARD))
    }
}

/// Pure timing decision for the reopen guard (unit-tested).
fn should_suppress_show(last_hidden_at: Instant, now: Instant, guard: Duration) -> bool {
    now.duration_since(last_hidden_at) < guard
}

/// Pure height clamp for the tray content resize (unit-tested): fit `content +
/// chrome` into `[min, work_height - 2*margin]`, never returning a height taller
/// than the available work area even on a very small screen.
pub(crate) fn clamp_tray_height(
    content_height: f64,
    chrome: f64,
    work_height: f64,
    margin: f64,
    min: f64,
) -> f64 {
    let available = (work_height - 2.0 * margin).max(1.0);
    let lower = min.min(available);
    (content_height + chrome).ceil().clamp(lower, available)
}

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

/// On macOS the Dock icon comes from the `.app` bundle's `CFBundleIconFile`. A
/// bare binary (`cargo build` / `cargo install burnrate`) has no bundle, so
/// when Preferences flips the activation policy to `Regular` the Dock shows a
/// generic executable icon. Set the application icon at runtime from the
/// embedded PNG so the unbundled binary still shows the Burnrate icon. The
/// bundled app already ships a multi-resolution `icon.icns`, so leave it be.
#[cfg(target_os = "macos")]
pub(crate) fn set_dock_icon_if_unbundled() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    const ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

    if running_in_app_bundle() {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(ICON_PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: `NSApplication` is touched on the main thread (proven by `mtm`)
    // and `image` is a valid, freshly-decoded `NSImage`.
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_dock_icon_if_unbundled() {}

#[cfg(target_os = "macos")]
fn running_in_app_bundle() -> bool {
    std::env::current_exe()
        .map(|path| path_is_in_app_bundle(&path.to_string_lossy()))
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn path_is_in_app_bundle(path: &str) -> bool {
    path.contains(".app/Contents/MacOS/")
}

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
    let Some(window) = app.get_webview_window(TRAY_WINDOW) else {
        return;
    };
    // Clicking the tray icon while open closes it.
    if window.is_visible().unwrap_or(false) {
        hide_tray_window(app);
        return;
    }
    let tray_state = app.state::<TrayWindowState>();
    // Beat the blur→click race: a status-item click first blurs (hides) the
    // popover, then fires this event; suppress the immediate reopen.
    if tray_state.should_suppress_show(Instant::now()) {
        return;
    }

    let scale_factor = window.scale_factor().unwrap_or(1.0);
    let position = position.to_logical::<f64>(scale_factor);
    let window_size = window
        .outer_size()
        .map(|size| size.to_logical::<f64>(scale_factor))
        .unwrap_or_else(|_| LogicalSize::new(360.0, 440.0));
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
    tray_state.set_anchor(position);
    // Position while still hidden so there's no visible jump, then reveal.
    let _ = window.set_position(tray_popup_position(position, window_size, work_area));
    let _ = window.show();
    #[cfg(target_os = "macos")]
    activate_app();
    let _ = window.set_focus();
    let _ = app.emit("burnrate-refresh-requested", ());
}

pub(crate) fn hide_tray_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window(TRAY_WINDOW) {
        let _ = window.hide();
    }
    app.state::<TrayWindowState>().mark_hidden();
}

/// Bring the process to the foreground so the borderless popover can become the
/// key window under Accessory activation policy (otherwise it never receives the
/// blur that dismisses it).
#[cfg(target_os = "macos")]
fn activate_app() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // `activateIgnoringOtherApps` is deprecated in favor of `NSApp.activate`, but
    // it's the variant that reliably activates across the macOS versions we ship.
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
}

/// Give the tray popover the native macOS translucent material + rounded corners.
#[cfg(target_os = "macos")]
pub(crate) fn apply_tray_vibrancy(app: &AppHandle<Wry>) {
    use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState, apply_vibrancy};

    if let Some(window) = app.get_webview_window(TRAY_WINDOW) {
        let _ = apply_vibrancy(
            &window,
            NSVisualEffectMaterial::Popover,
            Some(NSVisualEffectState::Active),
            Some(12.0),
        );
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn apply_tray_vibrancy(_app: &AppHandle<Wry>) {}

pub(crate) fn tray_popup_position(
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

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_app_bundle_versus_bare_binary() {
        assert!(path_is_in_app_bundle(
            "/Applications/Burnrate.app/Contents/MacOS/burnrate"
        ));
        assert!(!path_is_in_app_bundle(
            "/Users/dev/Projects/burnrate/target/release/burnrate"
        ));
        assert!(!path_is_in_app_bundle("/usr/local/bin/burnrate"));
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

    #[test]
    fn suppresses_reopen_within_guard_window() {
        let now = Instant::now();
        let guard = Duration::from_millis(250);
        // Hidden 50ms ago → within the guard → suppress the reopen.
        assert!(should_suppress_show(
            now - Duration::from_millis(50),
            now,
            guard
        ));
        // Hidden 300ms ago → guard elapsed → allow the show.
        assert!(!should_suppress_show(
            now - Duration::from_millis(300),
            now,
            guard
        ));
        // Exactly at the boundary → allowed (strict `<`).
        assert!(!should_suppress_show(now - guard, now, guard));
    }

    #[test]
    fn clamp_tray_height_fits_content_within_work_area() {
        let margin = 8.0;
        // Below the floor → clamped up to the minimum.
        assert_eq!(clamp_tray_height(50.0, 0.0, 1000.0, margin, 200.0), 200.0);
        // Normal content → ceil(content + chrome).
        assert_eq!(clamp_tray_height(500.4, 2.0, 1000.0, margin, 200.0), 503.0);
        // Taller than the work area → clamped down to the available height.
        assert_eq!(
            clamp_tray_height(5000.0, 0.0, 1000.0, margin, 200.0),
            1000.0 - 2.0 * margin
        );
        // Tiny screen (available < min) → fit the screen, not the minimum.
        assert_eq!(clamp_tray_height(500.0, 0.0, 10.0, margin, 200.0), 1.0);
    }

    #[test]
    fn tray_popup_position_keeps_tall_window_on_screen() {
        // A resized (taller) popover anchored near the bottom edge still clamps
        // fully on-screen.
        let size = LogicalSize::new(360.0, 700.0);
        let work_area = (
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(1024.0, 768.0),
        );
        let position = tray_popup_position(LogicalPosition::new(500.0, 760.0), size, work_area);

        assert_eq!(position.y, 768.0 - 700.0 - 8.0);
        assert!(position.y >= 8.0);
        assert_eq!(position.x, 320.0);
    }
}
