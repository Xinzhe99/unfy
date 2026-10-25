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
//! * It only acts on windows whose current AX role and state can be confirmed;
//!   inaccessible windows and unknown Space membership are not forced into view.
//! * Sizes are not applied exactly in one call. yabai and AeroSpace both do
//!   `set size -> set position -> set size`, because macOS clamps a window to the
//!   visible area of the display it is currently on before the move completes.
//!   This backend does the same and then reads back.
//! * Missing fullscreen state prevents movement rather than implying Normal.

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

        /// Called on the AppKit main thread at startup and periodically (including
        /// Dock and display changes). Publishes bounds/work areas together.
        pub fn prime_work_areas(&self) -> bool {
            self.displays.prime_work_areas()
        }
    }

    impl Platform for MacPlatform {
        type Displays = Self;
        type Windows = Self;

        fn displays(&self) -> &Self::Displays {
            self
        }

        fn windows(&self) -> &Self::Windows {
            self
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
            let observed = self.windows.read_back(id)?;
            display_for_rect(observed.visible_rect, &self.displays.topology()?)
        }
    }

    impl WindowBackend for MacPlatform {
        fn enumerate(&self) -> Result<Vec<WindowSnapshot>> {
            let displays = self.displays.topology()?;
            let mut windows = self.windows.enumerate()?;
            for window in &mut windows {
                if let Ok(display) = display_for_rect(window.visible_rect, &displays) {
                    window.display = display;
                }
            }
            Ok(windows)
        }
        fn snapshot(&self, id: &WindowId) -> Result<WindowSnapshot> {
            let displays = self.displays.topology()?;
            let mut window = self.windows.snapshot(id)?;
            if let Ok(display) = display_for_rect(window.visible_rect, &displays) {
                window.display = display;
            }
            Ok(window)
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
    }

    fn display_for_rect(
        rect: unfy_domain::Rect,
        displays: &[DisplaySnapshot],
    ) -> Result<DisplayKey> {
        displays
            .iter()
            .filter_map(|display| {
                let width = (i64::from(rect.right.min(display.bounds.right))
                    - i64::from(rect.left.max(display.bounds.left)))
                .max(0);
                let height = (i64::from(rect.bottom.min(display.bounds.bottom))
                    - i64::from(rect.top.max(display.bounds.top)))
                .max(0);
                let area = width * height;
                (area > 0).then_some((area, display))
            })
            .max_by_key(|(area, _)| *area)
            .map(|(_, display)| display.key.clone())
            .ok_or(unfy_platform::PlatformError::DisplayGone)
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
