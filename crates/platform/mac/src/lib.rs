//! macOS backend for Unfy.
//!
//! This crate is compiled on every host but only contains real code under
//! `cfg(target_os = "macos")`. On other hosts it exports [`is_available`]
//! returning `false` so the desktop app can show a truthful capability message
//! instead of linking AppKit glue.
//!
//! # What this backend can and cannot do
//!
//! * It moves windows through the Accessibility API (`AXUIElementSetAttributeValue`)
//!   which requires the user to grant Accessibility permission in System Settings.
//! * It cannot move native fullscreen windows: they live in their own Space and
//!   the AX position attribute does not apply. They are reported as
//!   `WindowMode::Fullscreen` and skipped.
//! * It cannot see windows on other Spaces at all; AX only exposes windows on the
//!   currently visible Space, so cross-Space migration is impossible by design.
//! * Sizes are not applied exactly in one call. yabai and AeroSpace both do
//!   `set size -> set position -> set size`, because macOS clamps a window to the
//!   visible area of the display it is currently on before the move completes.
//!   This backend does the same and then reads back.
//! * `AXFullScreen` is not a public constant; it is used as a literal string and
//!   treated as best-effort.

pub fn is_available() -> bool {
    cfg!(target_os = "macos")
}

/// Human-readable name shown in diagnostics.
pub fn name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos-accessibility"
    } else {
        "unavailable"
    }
}

#[cfg(target_os = "macos")]
mod ax;
#[cfg(target_os = "macos")]
mod displays;

#[cfg(target_os = "macos")]
pub use platform::MacPlatform;

#[cfg(target_os = "macos")]
mod platform {
    use unfy_domain::{DisplayKey, DisplaySnapshot, WindowId, WindowSnapshot};
    use unfy_platform::{
        Capabilities, DisplayProvider, ObservedWindow, Platform, Result, WindowBackend, WindowPlan,
    };

    use crate::ax::AccessibilityBackend;
    use crate::displays::QuartzDisplays;

    /// The macOS platform pair.
    #[derive(Debug, Default)]
    pub struct MacPlatform {
        displays: QuartzDisplays,
        windows: AccessibilityBackend,
    }

    impl MacPlatform {
        pub fn new() -> Self {
            Self::default()
        }
    }

    impl Platform for MacPlatform {
        type Displays = QuartzDisplays;
        type Windows = AccessibilityBackend;

        fn displays(&self) -> &Self::Displays {
            &self.displays
        }

        fn windows(&self) -> &Self::Windows {
            &self.windows
        }
    }

    impl DisplayProvider for MacPlatform {
        fn topology(&self) -> Result<Vec<DisplaySnapshot>> {
            self.displays.topology()
        }
        fn scale_of(&self, key: &DisplayKey) -> Result<unfy_domain::Scale> {
            self.displays.scale_of(key)
        }
        fn display_for(&self, id: &WindowId) -> Result<DisplayKey> {
            self.displays.display_for(id)
        }
    }

    impl WindowBackend for MacPlatform {
        fn enumerate(&self) -> Result<Vec<WindowSnapshot>> {
            self.windows.enumerate()
        }
        fn capabilities(&self, id: &WindowId) -> Capabilities {
            self.windows.capabilities(id)
        }
        fn apply(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
            self.windows.apply(id, plan)
        }
        fn read_back(&self, id: &WindowId) -> Result<ObservedWindow> {
            self.windows.read_back(id)
        }
        fn restore_native(&self, id: &WindowId, placement: &[u8]) -> Result<()> {
            self.windows.restore_native(id, placement)
        }
    }
}

/// Ask macOS whether the app currently holds Accessibility permission.
///
/// Returns `false` on non-macOS hosts.
pub fn has_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        ax::is_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Ask macOS to show the Accessibility permission prompt.
///
/// The prompt is asynchronous: the return value reports the *current* trust
/// state, not the user's answer. Callers must re-check later with
/// [`has_accessibility_permission`].
pub fn request_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        ax::request_trust()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Open the exact System Settings pane for Accessibility.
///
/// `AXIsProcessTrustedWithOptions` shows a prompt on first request only; after
/// the user dismisses it there is no way to re-prompt, so the UI needs a button
/// that goes straight to the pane.
pub fn open_accessibility_settings() -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        // `x-apple.systempreferences:` is the documented URL scheme for this
        // pane. It is a UI convenience only: failure just means the user has to
        // navigate manually, so the error is not propagated.
        Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn()
            .is_ok()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
