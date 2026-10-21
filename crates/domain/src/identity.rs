//! Display and window identity.
//!
//! Identity is the hardest part of this product: a snapshot is worthless if it
//! cannot be matched to the same physical display or the same window after a
//! restart, a hot-plug, or a resolution change. Both types therefore carry a
//! *quality* flag rather than pretending every platform gives a stable key.

use serde::{Deserialize, Serialize};

use crate::geom::{Rect, Scale, Size};

/// How much we trust a display key. A `Weak` key may only be used for a
/// suggestion; restoring a window onto a `Weak`-matched display requires the
/// user to confirm the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentityQuality {
    /// Device path / UUID, stable across reboots and port changes.
    Strong,
    /// EDID or vendor+product+serial, stable unless the monitor is replaced.
    Medium,
    /// Geometry only. Changes when the user rearranges displays.
    Weak,
}

impl IdentityQuality {
    /// Whether the identity is good enough to restore without asking.
    pub fn is_restorable(self) -> bool {
        matches!(self, IdentityQuality::Strong | IdentityQuality::Medium)
    }
}

/// A stable key for one physical display.
///
/// `primary` is the platform's primary display at capture time and is kept for
/// human-readable diagnostics only — it is **not** part of equality, because
/// Windows lets the user change the primary display without touching the
/// hardware.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayKey {
    /// Platform-native stable identifier. Windows: `monitorDevicePath`.
    /// macOS: `CGDisplayCreateUUIDFromDisplayID` stringified.
    pub native_id: String,
    pub quality: IdentityQuality,
}

/// Everything needed to map a window onto a display and back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplaySnapshot {
    pub key: DisplayKey,
    /// Physical-pixel bounds of the whole display.
    pub bounds: Rect,
    /// Physical-pixel work area (bounds minus taskbar/dock/menu bar).
    pub work_area: Rect,
    pub scale: Scale,
    /// Physical pixel size, used as a fallback identity signal.
    pub physical_size_mm: Option<Size>,
    pub primary: bool,
    /// Human-readable label for the UI. Never used for matching.
    pub label: String,
}

impl DisplaySnapshot {
    pub fn is_usable(&self) -> bool {
        !self.work_area.is_empty() && self.scale.is_valid()
    }
}

/// How a window is currently presented. Only `Normal` and `Maximized` are
/// migrated by v1; the rest are skipped with a reason code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowMode {
    Normal,
    Maximized,
    Minimized,
    /// macOS native fullscreen, or a Windows window that covers its display
    /// exactly with no border. Never migrated.
    Fullscreen,
}

/// A window identifier that survives until the window is destroyed.
///
/// `native_id` alone is not enough: Windows reuses `HWND` values, and macOS
/// `CGWindowID` is only unique within a session. `process_start` disambiguates
/// a recycled handle that now belongs to a different process instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowId {
    pub native_id: u64,
    pub process_id: u32,
    /// Process creation time in 100 ns intervals since the Unix epoch, or 0 if
    /// the platform cannot report it.
    pub process_start: u64,
}

/// A captured window, ready to be moved or restored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub id: WindowId,
    /// Executable name of the owning process (`Code.exe`), or empty when the
    /// platform could not determine it.
    ///
    /// This is what an exclusion rule matches on. It is the file name only: the
    /// full path is never stored, logged, or sent anywhere, because a path can
    /// contain a user name.
    pub app_name: String,
    pub display: DisplayKey,
    /// Visible rectangle in physical pixels, excluding invisible resize borders.
    pub visible_rect: Rect,
    /// Full rectangle in physical pixels, including invisible borders. The
    /// difference between this and `visible_rect` must be preserved on restore,
    /// otherwise the window creeps on every cycle.
    pub outer_rect: Rect,
    pub mode: WindowMode,
    /// Platform-native placement blob (Windows `WINDOWPLACEMENT`). Only valid on
    /// the display topology it was captured on.
    pub native_placement: Option<Vec<u8>>,
    pub topmost: bool,
}

impl WindowSnapshot {
    /// The invisible border applied to each side by the compositor.
    pub fn border_inset(&self) -> (i32, i32, i32, i32) {
        (
            self.visible_rect.left - self.outer_rect.left,
            self.visible_rect.top - self.outer_rect.top,
            self.outer_rect.right - self.visible_rect.right,
            self.outer_rect.bottom - self.visible_rect.bottom,
        )
    }

    /// Whether this window may be moved by the engine at all.
    pub fn is_migratable(&self) -> bool {
        matches!(self.mode, WindowMode::Normal | WindowMode::Maximized)
            && !self.visible_rect.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(mode: WindowMode) -> WindowSnapshot {
        WindowSnapshot {
            id: WindowId {
                native_id: 1,
                process_id: 2,
                process_start: 3,
            },
            app_name: "test-app.exe".into(),
            display: DisplayKey {
                native_id: "display-a".into(),
                quality: IdentityQuality::Strong,
            },
            visible_rect: Rect::new(100, 100, 900, 700),
            outer_rect: Rect::new(93, 93, 907, 707),
            mode,
            native_placement: None,
            topmost: false,
        }
    }

    #[test]
    fn border_inset_matches_rect_difference() {
        let (l, t, r, b) = snapshot(WindowMode::Normal).border_inset();
        assert_eq!((l, t, r, b), (7, 7, 7, 7));
    }

    #[test]
    fn fullscreen_and_minimized_are_not_migratable() {
        assert!(snapshot(WindowMode::Normal).is_migratable());
        assert!(snapshot(WindowMode::Maximized).is_migratable());
        assert!(!snapshot(WindowMode::Fullscreen).is_migratable());
        assert!(!snapshot(WindowMode::Minimized).is_migratable());
    }

    #[test]
    fn weak_identity_is_not_restorable() {
        assert!(!IdentityQuality::Weak.is_restorable());
        assert!(IdentityQuality::Medium.is_restorable());
        assert!(IdentityQuality::Strong.is_restorable());
    }
}
