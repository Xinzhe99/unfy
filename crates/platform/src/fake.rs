//! In-memory backend used by engine, layout and journal tests.
//!
//! It models the behaviours that break naive implementations:
//!
//! * a window that clamps itself to a minimum size (like `WM_GETMINMAXINFO`),
//! * a window that "corrects" the position back after being moved (Chromium),
//! * a window that denies access entirely (elevated / sandboxed),
//! * a window that disappears mid-batch.

use std::cell::RefCell;
use std::collections::HashMap;

use unfy_domain::{
    DisplayKey, DisplaySnapshot, IdentityQuality, Rect, Scale, WindowId, WindowMode, WindowSnapshot,
};

use crate::{
    Capabilities, DisplayProvider, ObservedWindow, PlatformError, Result, WindowBackend, WindowPlan,
};

/// How a fake window reacts to `apply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeBehaviour {
    /// Accepts any rectangle.
    Compliant,
    /// Accepts the move, then snaps back on the next read (Chromium-like).
    SelfCorrecting,
    /// Refuses with `PermissionDenied` (elevated window).
    Denied,
    /// Disappears after the first read.
    Vanishes,
    /// Never shrinks below `min_size`, but accepts any position.
    MinSize(u32, u32),
    /// Takes the new size and keeps its own position.
    ///
    /// clash-verge on Windows behaves this way: it accepts `SetWindowPos`'s
    /// dimensions but snaps back to the corner it pins itself to, so a gather that
    /// cannot place it still changes what the user sees.
    SizeOnly,
}

#[derive(Debug, Clone)]
struct FakeWindow {
    snapshot: WindowSnapshot,
    behaviour: FakeBehaviour,
    /// Number of `apply` calls so far; `SelfCorrecting` reverts on every one.
    applies: u32,
    alive: bool,
}

/// A deterministic, thread-local fake platform.
#[derive(Debug, Default)]
pub struct FakeBackend {
    displays: Vec<DisplaySnapshot>,
    windows: RefCell<HashMap<u64, FakeWindow>>,
    /// Rectangles that `SelfCorrecting` windows return to.
    home: HashMap<u64, Rect>,
}

impl FakeBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Two displays with different scale factors, arranged side by side.
    pub fn with_two_displays() -> Self {
        let mut backend = Self::new();
        backend.displays = vec![
            DisplaySnapshot {
                key: DisplayKey {
                    native_id: "fake-display-a".into(),
                    quality: IdentityQuality::Strong,
                },
                bounds: Rect::new(0, 0, 2560, 1440),
                work_area: Rect::new(0, 0, 2560, 1400),
                scale: Scale { x: 1.5, y: 1.5 },
                physical_size_mm: None,
                primary: true,
                label: "Display A".into(),
            },
            DisplaySnapshot {
                key: DisplayKey {
                    native_id: "fake-display-b".into(),
                    quality: IdentityQuality::Strong,
                },
                bounds: Rect::new(2560, 0, 4480, 1080),
                work_area: Rect::new(2560, 0, 4480, 1040),
                scale: Scale { x: 1.0, y: 1.0 },
                physical_size_mm: None,
                primary: false,
                label: "Display B".into(),
            },
        ];
        backend
    }

    /// Rename the app owning a window, so exclusion rules can be exercised.
    pub fn set_app_name(&mut self, id: u64, name: &str) {
        if let Some(w) = self.windows.borrow_mut().get_mut(&id) {
            w.snapshot.app_name = name.to_string();
        }
    }

    /// Add a window. `rect` is its visible rectangle in physical pixels.
    /// Mark a window as focused, as the real backend does for the foreground one.
    ///
    /// Tests that exercise `PrimarySide` need this: without a focused candidate
    /// that layout silently degenerates into a grid, which is exactly the bug this
    /// setter exists to make testable.
    pub fn focus(&mut self, id: u64) {
        for window in self.windows.borrow_mut().values_mut() {
            window.snapshot.focused = window.snapshot.id.native_id == id;
        }
    }

    /// Record the minimum size a window claims for itself.
    pub fn set_min_size(&mut self, id: u64, size: unfy_domain::Size) {
        if let Some(window) = self.windows.borrow_mut().get_mut(&id) {
            window.snapshot.min_size = Some(size);
        }
    }

    pub fn add_window(
        &mut self,
        id: u64,
        rect: Rect,
        display: &str,
        mode: WindowMode,
        behaviour: FakeBehaviour,
    ) -> WindowId {
        let snapshot = WindowSnapshot {
            app_name: format!("fake-{id}.exe"),
            id: WindowId {
                native_id: id,
                process_id: 1000 + id as u32,
                process_start: 1,
            },
            display: DisplayKey {
                native_id: display.to_string(),
                quality: IdentityQuality::Strong,
            },
            visible_rect: rect,
            outer_rect: Rect::new(rect.left - 7, rect.top - 7, rect.right + 7, rect.bottom + 7),
            mode,
            native_placement: None,
            topmost: false,
            min_size: None,
            // The fake backend has no concept of focus; a test that needs one
            // sets it explicitly.
            focused: false,
        };
        self.home.insert(id, rect);
        self.windows.borrow_mut().insert(
            id,
            FakeWindow {
                snapshot,
                behaviour,
                applies: 0,
                alive: true,
            },
        );
        WindowId {
            native_id: id,
            process_id: 1000 + id as u32,
            process_start: 1,
        }
    }

    pub fn display(&self, native_id: &str) -> Option<&DisplaySnapshot> {
        self.displays.iter().find(|d| d.key.native_id == native_id)
    }
}

impl DisplayProvider for FakeBackend {
    fn topology(&self) -> Result<Vec<DisplaySnapshot>> {
        Ok(self.displays.clone())
    }

    fn scale_of(&self, key: &DisplayKey) -> Result<Scale> {
        self.displays
            .iter()
            .find(|d| d.key == *key)
            .map(|d| d.scale)
            .ok_or(PlatformError::DisplayGone)
    }

    fn display_for(&self, id: &WindowId) -> Result<DisplayKey> {
        let windows = self.windows.borrow();
        let w = windows
            .get(&id.native_id)
            .ok_or(PlatformError::WindowGone)?;
        Ok(w.snapshot.display.clone())
    }
}

impl WindowBackend for FakeBackend {
    fn enumerate(&self) -> Result<Vec<WindowSnapshot>> {
        let mut out: Vec<_> = self
            .windows
            .borrow()
            .values()
            .filter(|w| w.alive)
            .map(|w| w.snapshot.clone())
            .collect();
        out.sort_by_key(|w| w.id.native_id);
        Ok(out)
    }

    fn capabilities(&self, id: &WindowId) -> Capabilities {
        let windows = self.windows.borrow();
        match windows.get(&id.native_id) {
            None => Capabilities::default(),
            Some(w) if w.behaviour == FakeBehaviour::Denied => Capabilities::default(),
            Some(w) => Capabilities {
                can_move: true,
                can_resize: true,
                can_read_state: true,
                can_restore_native: false,
                min_size: w.snapshot.min_size,
            },
        }
    }

    fn apply(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
        let mut windows = self.windows.borrow_mut();
        let w = windows
            .get_mut(&id.native_id)
            .ok_or(PlatformError::WindowGone)?;
        if !w.alive {
            return Err(PlatformError::WindowGone);
        }
        if w.behaviour == FakeBehaviour::Denied {
            return Err(PlatformError::PermissionDenied);
        }
        w.applies += 1;

        let mut target = plan.visible_rect;
        if w.behaviour == FakeBehaviour::SizeOnly {
            // Keep the position it already had; take only the requested size.
            target = Rect::new(
                w.snapshot.visible_rect.left,
                w.snapshot.visible_rect.top,
                w.snapshot.visible_rect.left + plan.visible_rect.width(),
                w.snapshot.visible_rect.top + plan.visible_rect.height(),
            );
        }
        if let FakeBehaviour::MinSize(min_w, min_h) = w.behaviour {
            let width = target.width().max(min_w as i32);
            let height = target.height().max(min_h as i32);
            target = Rect::new(
                target.left,
                target.top,
                target.left + width,
                target.top + height,
            );
        }

        let outer_inset = (7, 7, 7, 7);
        w.snapshot.visible_rect = target;
        w.snapshot.outer_rect = Rect::new(
            target.left - outer_inset.0,
            target.top - outer_inset.1,
            target.right + outer_inset.2,
            target.bottom + outer_inset.3,
        );
        w.snapshot.mode = plan.mode;
        Ok(())
    }

    fn read_back(&self, id: &WindowId) -> Result<ObservedWindow> {
        let mut windows = self.windows.borrow_mut();
        let w = windows
            .get_mut(&id.native_id)
            .ok_or(PlatformError::WindowGone)?;
        if !w.alive {
            return Err(PlatformError::WindowGone);
        }

        match w.behaviour {
            FakeBehaviour::Vanishes => {
                w.alive = false;
                Err(PlatformError::WindowGone)
            }
            FakeBehaviour::SelfCorrecting if w.applies > 0 => {
                let home = self
                    .home
                    .get(&id.native_id)
                    .copied()
                    .unwrap_or(w.snapshot.visible_rect);
                w.snapshot.visible_rect = home;
                Ok(ObservedWindow {
                    visible_rect: home,
                    mode: WindowMode::Normal,
                })
            }
            _ => Ok(ObservedWindow {
                visible_rect: w.snapshot.visible_rect,
                mode: w.snapshot.mode,
            }),
        }
    }

    fn restore_native(&self, _id: &WindowId, _placement: &[u8]) -> Result<()> {
        Err(PlatformError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_denies_elevated_window() {
        let mut backend = FakeBackend::with_two_displays();
        let id = backend.add_window(
            1,
            Rect::new(100, 100, 400, 400),
            "fake-display-a",
            WindowMode::Normal,
            FakeBehaviour::Denied,
        );
        let plan = WindowPlan {
            visible_rect: Rect::new(3000, 100, 3300, 400),
            mode: WindowMode::Normal,
        };
        assert_eq!(
            backend.apply(&id, &plan),
            Err(PlatformError::PermissionDenied)
        );
        assert!(!backend.capabilities(&id).can_move);
    }

    #[test]
    fn fake_self_correcting_window_returns_home() {
        let mut backend = FakeBackend::with_two_displays();
        let home = Rect::new(100, 100, 400, 400);
        let id = backend.add_window(
            2,
            home,
            "fake-display-a",
            WindowMode::Normal,
            FakeBehaviour::SelfCorrecting,
        );
        let plan = WindowPlan {
            visible_rect: Rect::new(3000, 100, 3300, 400),
            mode: WindowMode::Normal,
        };
        backend.apply(&id, &plan).unwrap();
        let observed = backend.read_back(&id).unwrap();
        assert_eq!(observed.visible_rect, home);
    }

    #[test]
    fn fake_min_size_is_enforced() {
        let mut backend = FakeBackend::with_two_displays();
        let id = backend.add_window(
            3,
            Rect::new(100, 100, 400, 400),
            "fake-display-a",
            WindowMode::Normal,
            FakeBehaviour::MinSize(320, 200),
        );
        let plan = WindowPlan {
            visible_rect: Rect::new(3000, 100, 3100, 200),
            mode: WindowMode::Normal,
        };
        backend.apply(&id, &plan).unwrap();
        let observed = backend.read_back(&id).unwrap();
        assert_eq!(observed.visible_rect.width(), 320);
        assert_eq!(observed.visible_rect.height(), 200);
    }

    #[test]
    fn fake_vanishing_window_reports_gone() {
        let mut backend = FakeBackend::with_two_displays();
        let id = backend.add_window(
            4,
            Rect::new(100, 100, 400, 400),
            "fake-display-a",
            WindowMode::Normal,
            FakeBehaviour::Vanishes,
        );
        assert_eq!(backend.read_back(&id), Err(PlatformError::WindowGone));
        assert!(backend.enumerate().unwrap().is_empty());
    }
}
