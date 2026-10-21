//! Windows implementation of the Conflux platform traits.
//!
//! The three rules that this module exists to enforce:
//!
//! * every call that reads a rectangle first switches the calling thread to
//!   Per-Monitor-V2 awareness, so a DPI-unaware target window cannot make us
//!   believe it lives at 96 DPI;
//! * visible rectangles come from `DWMWA_EXTENDED_FRAME_BOUNDS`, never from
//!   `GetWindowRect`, which includes ~7 px of invisible resize border per side;
//! * a maximized window is moved by restoring, moving and re-maximizing, because
//!   `SetWindowPos` on a maximized window is silently overwritten by Chromium's
//!   `OnWindowPosChanging` handler.

mod displays;
mod dpi;
mod labels;
mod thumbnails;
mod windows_impl;

pub use displays::{display_at, WinDisplays};
pub use windows_impl::{capture_placement, nudge_window, RawWindow, WinWindows, WindowStyleFlags};

use conflux_platform::{DisplayProvider, Platform, WindowBackend};

/// The Windows platform pair.
#[derive(Debug, Default)]
pub struct WinPlatform {
    displays: WinDisplays,
    windows: WinWindows,
}

impl WinPlatform {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Platform for WinPlatform {
    type Displays = WinDisplays;
    type Windows = WinWindows;

    fn displays(&self) -> &Self::Displays {
        &self.displays
    }

    fn windows(&self) -> &Self::Windows {
        &self.windows
    }
}

impl DisplayProvider for WinPlatform {
    fn topology(&self) -> conflux_platform::Result<Vec<conflux_domain::DisplaySnapshot>> {
        self.displays.topology()
    }

    fn scale_of(
        &self,
        key: &conflux_domain::DisplayKey,
    ) -> conflux_platform::Result<conflux_domain::Scale> {
        self.displays.scale_of(key)
    }

    fn display_for(
        &self,
        id: &conflux_domain::WindowId,
    ) -> conflux_platform::Result<conflux_domain::DisplayKey> {
        self.displays.display_for(id)
    }
}

impl WindowBackend for WinPlatform {
    fn enumerate(&self) -> conflux_platform::Result<Vec<conflux_domain::WindowSnapshot>> {
        self.windows.enumerate()
    }

    fn capabilities(&self, id: &conflux_domain::WindowId) -> conflux_platform::Capabilities {
        self.windows.capabilities(id)
    }

    fn apply(
        &self,
        id: &conflux_domain::WindowId,
        plan: &conflux_platform::WindowPlan,
    ) -> conflux_platform::Result<()> {
        self.windows.apply(id, plan)
    }

    fn read_back(
        &self,
        id: &conflux_domain::WindowId,
    ) -> conflux_platform::Result<conflux_platform::ObservedWindow> {
        self.windows.read_back(id)
    }

    fn restore_native(
        &self,
        id: &conflux_domain::WindowId,
        placement: &[u8],
    ) -> conflux_platform::Result<()> {
        self.windows.restore_native(id, placement)
    }
}

/// Show a large numbered badge on each display for `seconds`.
///
/// The badge is a borderless, click-through, topmost window per display. It is
/// removed automatically, so it cannot be left behind even if the app crashes
/// (the thread that owns it is detached and exits on its own timer).
pub fn flash_display_labels(seconds: u64) -> bool {
    crate::labels::flash(seconds)
}

pub use thumbnails::DisplayThumbnail;

/// Capture a thumbnail of every display, for the display picker.
///
/// Thumbnails are returned in memory as PNG data URLs and are never written to
/// disk or sent anywhere.
pub fn display_thumbnails() -> Vec<DisplayThumbnail> {
    thumbnails::display_thumbnails()
}

/// Executables that currently own at least one visible window.
///
/// Used by the exclusion picker so the user selects from what is actually
/// running instead of typing a file name they have to guess.
pub fn running_applications() -> Vec<String> {
    use conflux_platform::WindowBackend;

    let backend = crate::WinWindows::new();
    let mut names: Vec<String> = backend
        .enumerate()
        .unwrap_or_default()
        .into_iter()
        .map(|w| w.app_name)
        .filter(|name| !name.is_empty())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}
