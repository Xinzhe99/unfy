//! Platform abstraction for Unfy.
//!
//! The engine never calls an OS API directly. Each supported platform provides a
//! `DisplayProvider` and a `WindowBackend`; the fake implementation lives in this
//! crate so engine and layout tests run on any machine, including CI without a
//! desktop.
//!
//! Two invariants every implementation must honour:
//!
//! 1. `apply` followed by `read_back` must eventually report the requested
//!    rectangle, or report a typed rejection. Silently ignoring the request is
//!    the failure mode that produces "restored to the wrong place".
//! 2. `read_back` must never panic on a destroyed window; it returns
//!    `PlatformError::WindowGone` instead.

use unfy_domain::{
    DisplayKey, DisplaySnapshot, Rect, Scale, Size, WindowId, WindowMode, WindowSnapshot,
};

pub mod fake;

pub use fake::FakeBackend;

/// A typed platform failure. `WindowGone` and `PermissionDenied` are expected
/// during normal operation and must not abort a batch.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlatformError {
    #[error("window no longer exists")]
    WindowGone,
    #[error("access denied (elevation or sandbox)")]
    PermissionDenied,
    #[error("operation not supported by this window")]
    Unsupported,
    #[error("display no longer exists")]
    DisplayGone,
    #[error("platform call failed: {0}")]
    Os(String),
}

pub type Result<T, E = PlatformError> = std::result::Result<T, E>;

/// What the backend can do with a particular window. Probed once per window
/// during enumeration; the engine must not assume a capability it did not see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    pub can_move: bool,
    pub can_resize: bool,
    pub can_read_state: bool,
    /// Whether the platform can restore the window's original placement blob.
    pub can_restore_native: bool,
    /// The smallest size the window accepts, when the platform can report it.
    ///
    /// Probed here rather than during enumeration because the answer costs a
    /// cross-process round trip: answering it for every window on every preview
    /// added ~120 ms to an eight-window enumeration. The engine asks only for the
    /// windows it is about to place.
    pub min_size: Option<Size>,
}

/// The rectangle and state a backend observes for a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedWindow {
    pub visible_rect: Rect,
    pub mode: WindowMode,
}

/// A requested end state for one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPlan {
    /// Desired visible rectangle in physical pixels.
    pub visible_rect: Rect,
    pub mode: WindowMode,
}

/// Display topology access.
pub trait DisplayProvider {
    /// All displays currently active for drawing, in platform order.
    fn topology(&self) -> Result<Vec<DisplaySnapshot>>;

    /// The effective scale of a display, or `Scale::IDENTITY` when unknown.
    fn scale_of(&self, key: &DisplayKey) -> Result<Scale>;

    /// The display a window currently occupies most of.
    fn display_for(&self, id: &WindowId) -> Result<DisplayKey>;
}

/// Window enumeration and mutation.
pub trait WindowBackend {
    /// Enumerate candidate top-level windows. Implementations may over-report;
    /// the engine applies the skip rules.
    fn enumerate(&self) -> Result<Vec<WindowSnapshot>>;

    fn capabilities(&self, id: &WindowId) -> Capabilities;

    /// Request the plan. Returning `Ok` only means the request was submitted;
    /// callers must `read_back` to confirm.
    fn apply(&self, id: &WindowId, plan: &WindowPlan) -> Result<()>;

    /// Submit without waiting for the target application to process the request.
    ///
    /// Cross-process window moves are synchronous by default: `SetWindowPos` on
    /// another process's window blocks until that process pumps its message
    /// queue. Doing that for every window serialises a migration behind the
    /// slowest application on the desktop. Callers that batch submissions use
    /// this and then read back once the batch has been submitted.
    ///
    /// The default is the synchronous call, so a backend that cannot do better
    /// stays correct.
    fn apply_async(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
        self.apply(id, plan)
    }

    fn read_back(&self, id: &WindowId) -> Result<ObservedWindow>;

    /// Restore a previously captured native placement blob.
    fn restore_native(&self, id: &WindowId, placement: &[u8]) -> Result<()>;
}

/// A provider + backend pair for one platform.
pub trait Platform {
    type Displays: DisplayProvider;
    type Windows: WindowBackend;

    fn displays(&self) -> &Self::Displays;
    fn windows(&self) -> &Self::Windows;
}

/// Why a window was filtered out before planning. Mirrors
/// `unfy_domain::SkipReason` but is produced at the platform layer, where the
/// evidence exists (window styles, AX subroles).
pub fn rect_delta(a: Rect, b: Rect) -> (i32, i32, i32, i32) {
    (
        a.left - b.left,
        a.top - b.top,
        a.right - b.right,
        a.bottom - b.bottom,
    )
}
