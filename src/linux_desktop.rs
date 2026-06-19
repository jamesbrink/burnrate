use std::{collections::BTreeSet, env, ffi::OsStr, fs, path::Path};

const WLR_DESKTOPS: &[&str] = &["hyprland", "sway", "river", "wayfire", "niri", "labwc"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinuxDesktopInfo {
    desktops: Vec<String>,
    desktop_session: Option<String>,
    session_type: Option<String>,
    has_wayland_display: bool,
    has_x11_display: bool,
    processes: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinuxRuntimeEnv {
    pub(crate) gdk_backend: Option<&'static str>,
    pub(crate) webkit_disable_dmabuf_renderer: Option<&'static str>,
}

impl LinuxDesktopInfo {
    pub(crate) fn current() -> Self {
        Self::from_parts(
            env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
            env::var("DESKTOP_SESSION").ok().as_deref(),
            env::var("XDG_SESSION_TYPE").ok().as_deref(),
            env::var_os("WAYLAND_DISPLAY").is_some(),
            env::var_os("DISPLAY").is_some(),
            running_process_names(),
        )
    }

    fn from_parts(
        current_desktop: Option<&str>,
        desktop_session: Option<&str>,
        session_type: Option<&str>,
        has_wayland_display: bool,
        has_x11_display: bool,
        processes: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        let mut desktops = normalize_desktops(current_desktop);
        if let Some(session) = desktop_session {
            let normalized = normalize_token(session);
            if !normalized.is_empty() && !desktops.contains(&normalized) {
                desktops.push(normalized);
            }
        }

        Self {
            desktops,
            desktop_session: desktop_session.map(ToOwned::to_owned),
            session_type: session_type.map(|value| value.to_ascii_lowercase()),
            has_wayland_display,
            has_x11_display,
            processes: processes
                .into_iter()
                .map(|name| normalize_token(name.as_ref()))
                .filter(|name| !name.is_empty())
                .collect(),
        }
    }

    pub(crate) fn is_wayland(&self) -> bool {
        self.session_type.as_deref() == Some("wayland") || self.has_wayland_display
    }

    pub(crate) fn is_waybar(&self) -> bool {
        self.processes
            .iter()
            .any(|process| process == "waybar" || process.contains("waybar"))
    }

    pub(crate) fn is_wlroots_like(&self) -> bool {
        self.desktops
            .iter()
            .any(|desktop| WLR_DESKTOPS.contains(&desktop.as_str()))
    }

    pub(crate) fn recommended_runtime_env(&self) -> LinuxRuntimeEnv {
        let needs_xwayland_gtk = self.is_wayland() && (self.is_waybar() || self.is_wlroots_like());
        LinuxRuntimeEnv {
            gdk_backend: needs_xwayland_gtk.then_some("x11,wayland"),
            webkit_disable_dmabuf_renderer: self.is_wayland().then_some("1"),
        }
    }

    pub(crate) fn summary(&self) -> serde_json::Value {
        let recommended = self.recommended_runtime_env();
        serde_json::json!({
            "desktops": self.desktops,
            "desktopSession": self.desktop_session,
            "sessionType": self.session_type,
            "waylandDisplay": self.has_wayland_display,
            "x11Display": self.has_x11_display,
            "waybar": self.is_waybar(),
            "wlrootsLike": self.is_wlroots_like(),
            "recommendedEnv": {
                "GDK_BACKEND": recommended.gdk_backend,
                "WEBKIT_DISABLE_DMABUF_RENDERER": recommended.webkit_disable_dmabuf_renderer,
            },
            "effectiveEnv": {
                "GDK_BACKEND": env::var("GDK_BACKEND").ok(),
                "WEBKIT_DISABLE_DMABUF_RENDERER": env::var("WEBKIT_DISABLE_DMABUF_RENDERER").ok(),
            },
        })
    }
}

pub(crate) fn apply_runtime_environment() {
    let info = LinuxDesktopInfo::current();
    let recommended = info.recommended_runtime_env();
    if let Some(value) = recommended.gdk_backend
        && env::var_os("GDK_BACKEND").is_none()
    {
        set_env_before_threads("GDK_BACKEND", value);
    }
    if let Some(value) = recommended.webkit_disable_dmabuf_renderer
        && env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
    {
        set_env_before_threads("WEBKIT_DISABLE_DMABUF_RENDERER", value);
    }

    if env::var_os("BURNRATE_DEBUG_LINUX_DESKTOP").is_some() {
        eprintln!("burnrate linux desktop: {}", info.summary());
    }
}

fn set_env_before_threads(key: &str, value: &str) {
    // This runs at the very top of `main`, before Tauri, GTK/WebKit, or our
    // background tasks start. Mutating process env is unsafe once other threads
    // may read it; here it is the startup compatibility handshake.
    unsafe {
        env::set_var(key, value);
    }
}

fn normalize_desktops(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split([':', ';', ',', ' '])
        .map(normalize_token)
        .filter(|desktop| !desktop.is_empty())
        .collect()
}

fn normalize_token(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn running_process_names() -> Vec<String> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| process_name(entry.path()))
        .collect()
}

fn process_name(path: impl AsRef<Path>) -> Option<String> {
    let path = path.as_ref();
    if !path.file_name().is_some_and(is_pid_dir) {
        return None;
    }

    fs::read_to_string(path.join("comm"))
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            fs::read(path.join("cmdline")).ok().and_then(|bytes| {
                bytes
                    .split(|byte| *byte == 0)
                    .find(|part| !part.is_empty())
                    .and_then(|part| {
                        Path::new(OsStr::from_bytes(part))
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
            })
        })
}

fn is_pid_dir(name: &OsStr) -> bool {
    name.as_encoded_bytes()
        .iter()
        .all(|byte| byte.is_ascii_digit())
}

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommends_xwayland_for_hyprland_wayland() {
        let info = LinuxDesktopInfo::from_parts(
            Some("Hyprland"),
            Some("hyprland"),
            Some("wayland"),
            true,
            true,
            std::iter::empty::<&str>(),
        );

        let env = info.recommended_runtime_env();

        assert_eq!(env.gdk_backend, Some("x11,wayland"));
        assert_eq!(env.webkit_disable_dmabuf_renderer, Some("1"));
        assert!(info.is_wlroots_like());
    }

    #[test]
    fn recommends_xwayland_for_waybar_even_with_unknown_desktop() {
        let info = LinuxDesktopInfo::from_parts(
            None,
            None,
            Some("wayland"),
            true,
            true,
            [".waybar-wrapped", "burnrate"],
        );

        assert_eq!(
            info.recommended_runtime_env().gdk_backend,
            Some("x11,wayland")
        );
        assert!(info.is_waybar());
    }

    #[test]
    fn leaves_gdk_backend_alone_for_plain_x11() {
        let info = LinuxDesktopInfo::from_parts(
            Some("GNOME"),
            Some("gnome"),
            Some("x11"),
            false,
            true,
            ["gnome-shell"],
        );

        let env = info.recommended_runtime_env();

        assert_eq!(env.gdk_backend, None);
        assert_eq!(env.webkit_disable_dmabuf_renderer, None);
    }

    #[test]
    fn normalizes_colon_separated_desktop_names() {
        let info = LinuxDesktopInfo::from_parts(
            Some("GNOME:Hyprland"),
            None,
            Some("wayland"),
            true,
            true,
            std::iter::empty::<&str>(),
        );

        assert_eq!(info.desktops, vec!["gnome", "hyprland"]);
        assert!(info.is_wlroots_like());
    }

    #[test]
    fn merges_desktop_session_when_current_desktop_is_missing_or_distinct() {
        let info = LinuxDesktopInfo::from_parts(
            Some("GNOME"),
            Some("niri"),
            Some("Wayland"),
            true,
            false,
            ["burnrate"],
        );

        assert_eq!(info.desktops, vec!["gnome", "niri"]);
        assert_eq!(info.desktop_session.as_deref(), Some("niri"));
        assert_eq!(info.session_type.as_deref(), Some("wayland"));
        assert!(info.is_wayland());
        assert!(info.is_wlroots_like());
    }

    #[test]
    fn ignores_blank_and_delimited_desktop_tokens() {
        let info = LinuxDesktopInfo::from_parts(
            Some(" GNOME; ;Hyprland Sway "),
            Some("hyprland"),
            None,
            false,
            true,
            [" burnrate ", "", "WAYBAR"],
        );

        assert_eq!(info.desktops, vec!["gnome", "hyprland", "sway"]);
        assert!(info.processes.contains("burnrate"));
        assert!(info.processes.contains("waybar"));
        assert!(!info.processes.contains(""));
        assert!(info.is_waybar());
    }

    #[test]
    fn summary_reports_detected_desktop_and_recommended_env() {
        let info = LinuxDesktopInfo::from_parts(
            Some("river"),
            Some("river"),
            Some("wayland"),
            true,
            false,
            ["waybar"],
        );

        let summary = info.summary();

        assert_eq!(summary["desktops"], serde_json::json!(["river"]));
        assert_eq!(summary["desktopSession"], "river");
        assert_eq!(summary["sessionType"], "wayland");
        assert_eq!(summary["waylandDisplay"], true);
        assert_eq!(summary["x11Display"], false);
        assert_eq!(summary["waybar"], true);
        assert_eq!(summary["wlrootsLike"], true);
        assert_eq!(summary["recommendedEnv"]["GDK_BACKEND"], "x11,wayland");
        assert_eq!(
            summary["recommendedEnv"]["WEBKIT_DISABLE_DMABUF_RENDERER"],
            "1"
        );
        assert!(summary["effectiveEnv"].is_object());
    }

    #[test]
    fn process_name_ignores_non_pid_directories() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("self");
        fs::create_dir(&path).expect("create non-pid dir");
        fs::write(path.join("comm"), "waybar\n").expect("write comm");

        assert_eq!(process_name(&path), None);
    }

    #[test]
    fn process_name_reads_trimmed_comm_for_pid_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("1234");
        fs::create_dir(&path).expect("create pid dir");
        fs::write(path.join("comm"), " waybar \n").expect("write comm");
        fs::write(path.join("cmdline"), b"/usr/bin/ignored\0").expect("write cmdline");

        assert_eq!(process_name(&path).as_deref(), Some("waybar"));
    }

    #[test]
    fn process_name_falls_back_to_cmdline_binary_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("5678");
        fs::create_dir(&path).expect("create pid dir");
        fs::write(path.join("comm"), "\n").expect("write blank comm");
        fs::write(
            path.join("cmdline"),
            b"/nix/store/hash-waybar/bin/waybar\0--log\0",
        )
        .expect("write cmdline");

        assert_eq!(process_name(&path).as_deref(), Some("waybar"));
    }

    #[test]
    fn process_name_ignores_empty_cmdline() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("9012");
        fs::create_dir(&path).expect("create pid dir");
        fs::write(path.join("cmdline"), b"\0\0").expect("write empty cmdline");

        assert_eq!(process_name(&path), None);
    }

    #[test]
    fn pid_directory_detection_requires_digits() {
        assert!(is_pid_dir(OsStr::new("12345")));
        assert!(!is_pid_dir(OsStr::new("12a45")));
    }
}
