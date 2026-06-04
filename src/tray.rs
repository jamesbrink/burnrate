use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Utc;
use tauri::{
    App, AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Wry,
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
/// Gap (in logical px, scaled to physical at use) between the popover and the
/// work-area edges, and the drop below the cursor.
const TRAY_MARGIN: f64 = 8.0;
const TRAY_OFFSET_Y: f64 = 12.0;

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
    /// Cursor anchor (global physical desktop coords) recorded at show time, so a
    /// later content resize can re-run `popup_position` and stay anchored on the
    /// monitor where the tray was clicked.
    last_anchor: Option<PhysicalPosition<f64>>,
}

impl TrayWindowState {
    fn mark_hidden(&self) {
        self.inner
            .lock()
            .expect("tray window state lock")
            .last_hidden_at = Some(Instant::now());
    }

    fn set_anchor(&self, anchor: PhysicalPosition<f64>) {
        self.inner
            .lock()
            .expect("tray window state lock")
            .last_anchor = Some(anchor);
    }

    pub(crate) fn anchor(&self) -> Option<PhysicalPosition<f64>> {
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

pub(crate) struct TraySizeInput {
    pub(crate) content_width: f64,
    pub(crate) content_height: f64,
    pub(crate) chrome_width: f64,
    pub(crate) chrome_height: f64,
}

pub(crate) struct TraySizeLimits {
    pub(crate) work_width: f64,
    pub(crate) work_height: f64,
    pub(crate) margin: f64,
    pub(crate) min_content_width: f64,
    pub(crate) min_height: f64,
    pub(crate) max_height: f64,
    pub(crate) max_content_width: f64,
}

/// Pure size clamp for the tray content resize (unit-tested): fit reported
/// content plus native chrome inside the clicked monitor's safe work area while
/// preserving compact minimums and capping adaptive width to the design maximum.
pub(crate) fn clamp_tray_size(input: TraySizeInput, limits: TraySizeLimits) -> (f64, f64) {
    let available_width = (limits.work_width - 2.0 * limits.margin).max(1.0);
    let width_floor = (limits.min_content_width + input.chrome_width)
        .ceil()
        .min(available_width);
    let width_ceiling = (limits.max_content_width + input.chrome_width)
        .ceil()
        .min(available_width)
        .max(width_floor);
    let width = (input.content_width + input.chrome_width)
        .ceil()
        .clamp(width_floor, width_ceiling);
    let available_height = (limits.work_height - 2.0 * limits.margin).max(1.0);
    let height_floor = limits.min_height.min(available_height);
    let height_ceiling = limits.max_height.min(available_height).max(height_floor);
    let height = clamp_tray_height(
        input.content_height,
        input.chrome_height,
        limits.work_height,
        limits.margin,
        limits.min_height,
    )
    .clamp(height_floor, height_ceiling);
    (width, height)
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
    // Only advertise "Check for Updates…" where the in-app updater actually
    // works (a signed, bundled macOS app); on other builds it would be a no-op.
    let check_updates = if crate::updater::updater_available() {
        Some(MenuItem::with_id(
            app,
            "check-updates",
            "Check for Updates…",
            true,
            None::<&str>,
        )?)
    } else {
        None
    };
    let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&preferences, &refresh];
    if let Some(ref check_updates) = check_updates {
        items.push(check_updates);
    }
    items.push(&quit);
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
            "check-updates" => {
                let _ = app.emit("burnrate-check-update-requested", ());
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
pub(crate) fn running_in_app_bundle() -> bool {
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

    // Work in global physical pixels anchored to the monitor under the cursor —
    // not the window's current monitor — so the popover opens on whichever
    // display the tray was clicked on, and lands correctly under mixed DPI.
    let (scale, work_pos, work_size) = cursor_monitor_geometry(app, position);
    // outer_size() is physical at the window's *current* monitor scale; the
    // popover will render at the cursor monitor's scale, so convert through
    // logical to size it for the target display (mixed-DPI correctness).
    let current_scale = window.scale_factor().unwrap_or(scale);
    let window_size = window
        .outer_size()
        .map(|size| {
            let logical = size.to_logical::<f64>(current_scale);
            PhysicalSize::new(logical.width * scale, logical.height * scale)
        })
        .unwrap_or_else(|_| PhysicalSize::new(360.0 * scale, 440.0 * scale));
    tray_state.set_anchor(position);
    let target = popup_position(
        position,
        window_size,
        work_pos,
        work_size,
        TRAY_MARGIN * scale,
        TRAY_OFFSET_Y * scale,
    );
    // Position while still hidden (physical coords are unambiguous across
    // monitors), then reveal.
    let _ = window.set_position(PhysicalPosition::new(
        target.x.round() as i32,
        target.y.round() as i32,
    ));
    let _ = window.show();
    #[cfg(target_os = "macos")]
    activate_app();
    let _ = window.set_focus();
    let _ = app.emit("burnrate-refresh-requested", ());
}

/// Physical work-area geometry (scale, origin, size) of the monitor under
/// `point`, falling back to the primary monitor, then a sane default.
pub(crate) fn cursor_monitor_geometry(
    app: &AppHandle<Wry>,
    point: PhysicalPosition<f64>,
) -> (f64, PhysicalPosition<f64>, PhysicalSize<f64>) {
    let monitor = app
        .monitor_from_point(point.x, point.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    match monitor {
        Some(monitor) => {
            let area = monitor.work_area();
            (
                monitor.scale_factor(),
                PhysicalPosition::new(area.position.x as f64, area.position.y as f64),
                PhysicalSize::new(area.size.width as f64, area.size.height as f64),
            )
        }
        None => (
            1.0,
            PhysicalPosition::new(0.0, 0.0),
            PhysicalSize::new(1920.0, 1080.0),
        ),
    }
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

/// Place the popover's top-left so it's centered under `cursor` and dropped
/// `offset_y` below it, clamped to stay fully within the work area. All inputs
/// and the result are in the same physical-pixel space (the monitor under the
/// cursor), which keeps multi-monitor and mixed-DPI placement correct.
pub(crate) fn popup_position(
    cursor: PhysicalPosition<f64>,
    window_size: PhysicalSize<f64>,
    work_position: PhysicalPosition<f64>,
    work_size: PhysicalSize<f64>,
    margin: f64,
    offset_y: f64,
) -> PhysicalPosition<f64> {
    let min_x = work_position.x + margin;
    let min_y = work_position.y + margin;
    let max_x = (work_position.x + work_size.width - window_size.width - margin).max(min_x);
    let max_y = (work_position.y + work_size.height - window_size.height - margin).max(min_y);
    PhysicalPosition::new(
        (cursor.x - window_size.width / 2.0).clamp(min_x, max_x),
        (cursor.y + offset_y).clamp(min_y, max_y),
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
            email: None,
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
    fn popup_position_clamps_to_work_area() {
        let size = PhysicalSize::new(380.0, 520.0);
        let work_pos = PhysicalPosition::new(0.0, 0.0);
        let work_size = PhysicalSize::new(1024.0, 768.0);
        let position = popup_position(
            PhysicalPosition::new(20.0, -40.0),
            size,
            work_pos,
            work_size,
            8.0,
            12.0,
        );

        assert_eq!(position.x, 8.0);
        assert_eq!(position.y, 8.0);

        let position = popup_position(
            PhysicalPosition::new(1000.0, 760.0),
            size,
            work_pos,
            work_size,
            8.0,
            12.0,
        );

        assert_eq!(position.x, 636.0);
        assert_eq!(position.y, 240.0);
    }

    #[test]
    fn popup_position_anchors_to_a_secondary_monitor() {
        // A second monitor sitting to the right of the primary (origin 1440,0).
        // The popover must land on it, not be clamped back to the primary.
        let size = PhysicalSize::new(360.0, 440.0);
        let work_pos = PhysicalPosition::new(1440.0, 0.0);
        let work_size = PhysicalSize::new(1440.0, 900.0);
        let position = popup_position(
            PhysicalPosition::new(1700.0, 100.0),
            size,
            work_pos,
            work_size,
            8.0,
            12.0,
        );

        // x: 1700 - 180 = 1520 (within [1448, 2512]); y: 100 + 12 = 112.
        assert_eq!(position.x, 1520.0);
        assert_eq!(position.y, 112.0);
        assert!(position.x >= 1448.0, "stays on the secondary monitor");
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
    fn clamp_tray_size_adapts_width_and_height_to_work_area() {
        let size = |input: TraySizeInput, work_width: f64, work_height: f64| {
            clamp_tray_size(
                input,
                TraySizeLimits {
                    work_width,
                    work_height,
                    margin: 8.0,
                    min_content_width: 360.0,
                    min_height: 200.0,
                    max_height: 760.0,
                    max_content_width: 360.0,
                },
            )
        };

        // Compact content keeps the menu-sized width and minimum height.
        assert_eq!(
            size(
                TraySizeInput {
                    content_width: 320.0,
                    content_height: 80.0,
                    chrome_width: 0.0,
                    chrome_height: 0.0,
                },
                1440.0,
                900.0,
            ),
            (360.0, 200.0)
        );

        // Wider content is capped to the compact native-menu width.
        assert_eq!(
            size(
                TraySizeInput {
                    content_width: 400.2,
                    content_height: 500.2,
                    chrome_width: 2.0,
                    chrome_height: 1.0,
                },
                1440.0,
                900.0,
            ),
            (362.0, 502.0)
        );
        assert_eq!(
            size(
                TraySizeInput {
                    content_width: 900.0,
                    content_height: 1200.0,
                    chrome_width: 0.0,
                    chrome_height: 0.0,
                },
                1440.0,
                900.0,
            ),
            (360.0, 760.0)
        );

        // Tiny work areas win over both minimums so the popover still fits.
        assert_eq!(
            size(
                TraySizeInput {
                    content_width: 900.0,
                    content_height: 1200.0,
                    chrome_width: 0.0,
                    chrome_height: 0.0,
                },
                300.0,
                120.0,
            ),
            (284.0, 104.0)
        );
    }

    #[test]
    fn popup_position_keeps_wide_window_on_screen() {
        let size = PhysicalSize::new(360.0, 440.0);
        let position = popup_position(
            PhysicalPosition::new(1000.0, 100.0),
            size,
            PhysicalPosition::new(0.0, 0.0),
            PhysicalSize::new(1024.0, 768.0),
            8.0,
            12.0,
        );

        assert_eq!(position.x, 1024.0 - 360.0 - 8.0);
        assert_eq!(position.y, 112.0);
    }

    #[test]
    fn popup_position_keeps_tall_window_on_screen() {
        // A resized (taller) popover anchored near the bottom edge still clamps
        // fully on-screen.
        let size = PhysicalSize::new(360.0, 700.0);
        let position = popup_position(
            PhysicalPosition::new(500.0, 760.0),
            size,
            PhysicalPosition::new(0.0, 0.0),
            PhysicalSize::new(1024.0, 768.0),
            8.0,
            12.0,
        );

        assert_eq!(position.y, 768.0 - 700.0 - 8.0);
        assert!(position.y >= 8.0);
        assert_eq!(position.x, 320.0);
    }
}
