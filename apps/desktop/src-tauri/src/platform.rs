//! Platform selection for the desktop app.
//!
//! Each supported target gets its native backend. Any other target compiles the
//! fake backend so the UI can still be reviewed, but `is_supported()` reports
//! `false` and the UI must show that rather than pretending to work.

#[cfg(target_os = "windows")]
pub type Backend = unfy_platform_win::WinPlatform;

#[cfg(target_os = "macos")]
pub type Backend = unfy_platform_mac::MacPlatform;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub type Backend = unfy_platform::fake::FakeBackend;

/// Whether this build can actually move other applications' windows.
pub fn is_supported() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}

/// Human-readable backend name, shown in the diagnostics panel.
pub fn name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unsupported"
    }
}

/// Create the backend for this platform.
pub fn make() -> Backend {
    #[cfg(target_os = "windows")]
    {
        unfy_platform_win::WinPlatform::new()
    }
    #[cfg(target_os = "macos")]
    {
        unfy_platform_mac::MacPlatform::new()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        unfy_platform::fake::FakeBackend::with_two_displays()
    }
}

/// Whether the operating system currently permits window control.
///
/// On Windows the answer is always yes: ordinary user processes may move other
/// windows, and elevated windows are handled per-window by the engine. On macOS
/// this is the Accessibility (TCC) permission, which the user must grant.
pub fn has_window_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        unfy_platform_mac::has_accessibility_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Ask the OS to prompt for window-control permission.
///
/// Returns the permission state *before* the user answers; the UI must re-check.
pub fn request_window_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        unfy_platform_mac::request_accessibility_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Open the OS pane where the permission is granted.
pub fn open_permission_settings() -> bool {
    #[cfg(target_os = "macos")]
    {
        unfy_platform_mac::open_accessibility_settings()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Name of the permission in the OS settings, for the UI's instructions.
pub fn permission_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "辅助功能"
    } else {
        "窗口控制"
    }
}

/// Reveal a directory in the OS file manager.
///
/// Unsupported platforms return false; the UI must not claim it opened anything.
pub fn open_directory(path: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        unfy_platform_win::open_directory(std::path::Path::new(path))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        false
    }
}

/// Show a numbered badge on every display for a few seconds.
///
/// This answers the question the product cannot answer by inspection: which
/// physical screen is the remote client actually showing? The user sees "2" in
/// the remote window and picks display 2. Without it, "choose the target
/// display" is a guess based on resolution labels the remote view does not show.
pub fn flash_display_labels(seconds: u64) -> FlashOutcome {
    #[cfg(target_os = "windows")]
    {
        match unfy_platform_win::flash_display_labels(seconds) {
            unfy_platform_win::FlashOutcome::Shown => FlashOutcome::Shown,
            unfy_platform_win::FlashOutcome::AlreadyShowing => FlashOutcome::AlreadyShowing,
            unfy_platform_win::FlashOutcome::Unavailable => FlashOutcome::Unavailable,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = seconds;
        // macOS has no badge overlay yet; saying "unavailable" is the truth, and
        // the window tells the user the numbers cannot be shown rather than
        // implying they were.
        FlashOutcome::Unavailable
    }
}

/// Whether the numbered badges were drawn, are still on screen, or cannot be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashOutcome {
    Shown,
    AlreadyShowing,
    Unavailable,
}

/// Screenshot of one display.
pub struct Thumbnail {
    pub native_id: String,
    pub data_url: String,
    /// True when the display captured as solid black, which means it is off, on
    /// another input, or a virtual display with no client attached.
    pub blank: bool,
}

/// Screenshot of every display, or an empty list when unsupported.
pub fn display_thumbnails() -> Vec<Thumbnail> {
    #[cfg(target_os = "windows")]
    {
        unfy_platform_win::display_thumbnails()
            .into_iter()
            .map(|t| Thumbnail {
                native_id: t.native_id,
                data_url: t.data_url,
                blank: t.blank,
            })
            .collect()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// Executables that currently own a visible window.
pub fn running_applications() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        unfy_platform_win::running_applications()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}
