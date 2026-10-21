//! Pure geometry types. All coordinates are **physical pixels** in the platform's
//! virtual-screen space unless a name says otherwise.
//!
//! Two coordinate spaces exist in this product and must never be mixed:
//!
//! * `Physical` — what `GetWindowRect`, `DWMWA_EXTENDED_FRAME_BOUNDS` and
//!   `MONITORINFO::rcWork` return on a Per-Monitor-V2 thread. Physical pixels.
//! * `Logical`  — physical pixels divided by the effective scale of the display
//!   they sit on. Used for layout, so a window keeps its apparent size when it
//!   crosses displays with different scale factors.
//!
//! `Rect` is deliberately untyped about which space it is in; the type of the
//! function carrying it (`scale_between`, `project_onto`) states the space.

use serde::{Deserialize, Serialize};

/// An integer point in virtual-screen space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// An integer rectangle: `right`/`bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn width(&self) -> i32 {
        self.right - self.left
    }

    pub const fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub const fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    pub const fn origin(&self) -> Point {
        Point {
            x: self.left,
            y: self.top,
        }
    }

    /// Overlap area in square pixels. Zero when the rectangles are disjoint.
    pub fn intersection_area(&self, other: &Rect) -> i64 {
        let w = (self.right.min(other.right) - self.left.max(other.left)).max(0) as i64;
        let h = (self.bottom.min(other.bottom) - self.top.max(other.top)).max(0) as i64;
        w * h
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.left && p.x < self.right && p.y >= self.top && p.y < self.bottom
    }

    /// Whether any part of the window can be grabbed with the mouse: at least
    /// `min_w x min_h` of the rectangle must fall inside `area`.
    ///
    /// A single visible pixel is not "reachable"; a user cannot grab it.
    pub fn reachable_in(&self, area: &Rect, min_w: i32, min_h: i32) -> bool {
        let w = (self.right.min(area.right) - self.left.max(area.left)).max(0);
        let h = (self.bottom.min(area.bottom) - self.top.max(area.top)).max(0);
        w >= min_w && h >= min_h
    }

    /// Translate so the rectangle lies inside `area` as far as its size allows.
    /// A rectangle larger than the area is aligned to the area's origin.
    pub fn clamp_into(&self, area: &Rect) -> Rect {
        let w = self.width();
        let h = self.height();
        let left = if w >= area.width() {
            area.left
        } else {
            self.left.clamp(area.left, area.right - w)
        };
        let top = if h >= area.height() {
            area.top
        } else {
            self.top.clamp(area.top, area.bottom - h)
        };
        Rect::new(left, top, left + w, top + h)
    }

    /// Move without resizing so the rectangle is inside `area` when possible.
    pub fn shifted_into(&self, area: &Rect) -> Rect {
        let mut dx = 0;
        let mut dy = 0;
        if self.left < area.left {
            dx = area.left - self.left;
        } else if self.right > area.right {
            dx = area.right - self.right;
        }
        if self.top < area.top {
            dy = area.top - self.top;
        } else if self.bottom > area.bottom {
            dy = area.bottom - self.bottom;
        }
        Rect::new(
            self.left + dx,
            self.top + dy,
            self.right + dx,
            self.bottom + dy,
        )
    }
}

/// A width/height pair in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

/// Scale factors as a ratio of effective DPI over 96. `1.0` is 100 %.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Scale {
    pub x: f64,
    pub y: f64,
}

impl Scale {
    pub const IDENTITY: Scale = Scale { x: 1.0, y: 1.0 };

    pub fn from_dpi(dpi_x: u32, dpi_y: u32) -> Self {
        Self {
            x: f64::from(dpi_x) / 96.0,
            y: f64::from(dpi_y) / 96.0,
        }
    }

    pub fn is_identity(&self) -> bool {
        (self.x - 1.0).abs() < f64::EPSILON && (self.y - 1.0).abs() < f64::EPSILON
    }

    /// A scale is usable only if both axes are finite and strictly positive.
    /// Platforms occasionally report 0 for a disconnected or virtual display.
    pub fn is_valid(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.x > 0.0 && self.y > 0.0
    }
}

/// Rescale a rectangle between two display scales while keeping its relative
/// position inside its own work area.
///
/// This is the verified algorithm from `docs/verify/windows.md` §2.3, expressed
/// in platform-neutral terms. Callers pass *visible* rectangles (DWM extended
/// frame bounds on Windows, AX frame on macOS), never rectangles that include
/// invisible resize borders, or the visual size drifts on every round trip.
pub fn rescale_between(
    rect: Rect,
    source_work: Rect,
    source_scale: Scale,
    target_work: Rect,
    target_scale: Scale,
) -> Rect {
    if !source_scale.is_valid() || !target_scale.is_valid() {
        return rect.shifted_into(&target_work);
    }

    // Fractional position inside the source work area. Guard against a zero-size
    // work area, which some platforms report while a display is being configured.
    let (fx, fy) = if source_work.width() > 0 && source_work.height() > 0 {
        (
            f64::from(rect.left - source_work.left) / f64::from(source_work.width()),
            f64::from(rect.top - source_work.top) / f64::from(source_work.height()),
        )
    } else {
        (0.0, 0.0)
    };

    let ratio_x = target_scale.x / source_scale.x;
    let ratio_y = target_scale.y / source_scale.y;

    let width = ((f64::from(rect.width()) * ratio_x).round() as i32).max(1);
    let height = ((f64::from(rect.height()) * ratio_y).round() as i32).max(1);

    let left = target_work.left + (fx * f64::from(target_work.width())).round() as i32;
    let top = target_work.top + (fy * f64::from(target_work.height())).round() as i32;

    // `clamp_into` aligns an oversized window to the work-area origin instead of
    // pushing it across the neighbouring display, which `shifted_into` would do.
    Rect::new(left, top, left + width, top + height).clamp_into(&target_work)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A_WORK: Rect = Rect::new(0, 0, 2560, 1400); // 150 % display
    const B_WORK: Rect = Rect::new(2560, 0, 4480, 1040); // 100 % display
    const A_SCALE: Scale = Scale { x: 1.5, y: 1.5 };
    const B_SCALE: Scale = Scale { x: 1.0, y: 1.0 };

    #[test]
    fn downscaling_preserves_apparent_size() {
        let window = Rect::new(300, 300, 1500, 1200); // 1200x900 at 150 %
        let moved = rescale_between(window, A_WORK, A_SCALE, B_WORK, B_SCALE);
        assert_eq!(moved.width(), 800);
        assert_eq!(moved.height(), 600);
        assert!(moved.reachable_in(&B_WORK, 64, 32));
    }

    #[test]
    fn round_trip_is_stable_within_one_pixel() {
        let window = Rect::new(300, 300, 1500, 1200);
        let there = rescale_between(window, A_WORK, A_SCALE, B_WORK, B_SCALE);
        let back = rescale_between(there, B_WORK, B_SCALE, A_WORK, A_SCALE);
        assert!((back.width() - window.width()).abs() <= 1);
        assert!((back.height() - window.height()).abs() <= 1);
        assert!((back.left - window.left).abs() <= 1);
    }

    #[test]
    fn oversized_window_is_aligned_to_target_origin() {
        let huge = Rect::new(0, 0, 4000, 2000);
        let moved = rescale_between(huge, A_WORK, A_SCALE, B_WORK, B_SCALE);
        assert_eq!(moved.left, B_WORK.left);
        assert_eq!(moved.top, B_WORK.top);
    }

    #[test]
    fn invalid_scale_degrades_to_shift() {
        let zero = Scale { x: 0.0, y: 1.0 };
        let window = Rect::new(5000, 2000, 5600, 2400);
        let moved = rescale_between(window, A_WORK, A_SCALE, B_WORK, zero);
        assert_eq!(moved.width(), window.width());
        assert!(moved.reachable_in(&B_WORK, 64, 32));
    }

    #[test]
    fn clamp_into_keeps_size_and_moves_origin() {
        let area = Rect::new(0, 0, 1920, 1040);
        let window = Rect::new(1900, 1000, 2300, 1400);
        let clamped = window.clamp_into(&area);
        assert_eq!(clamped.width(), window.width());
        assert_eq!(clamped.right, area.right);
        assert_eq!(clamped.bottom, area.bottom);
    }

    #[test]
    fn reachability_rejects_sliver() {
        let area = Rect::new(0, 0, 1920, 1040);
        let sliver = Rect::new(1919, 100, 2400, 900);
        assert!(!sliver.reachable_in(&area, 64, 32));
    }

    #[test]
    fn intersection_area_handles_disjoint() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(200, 200, 300, 300);
        assert_eq!(a.intersection_area(&b), 0);
    }
}
