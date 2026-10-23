//! Platform-neutral domain types for Unfy.
//!
//! Nothing here touches an operating system. That keeps layout, journal and the
//! engine testable on any machine, and it is why `docs/impl-plan.md` puts this
//! crate first in the task order.

pub mod geom;
pub mod identity;
pub mod state;

pub use geom::{rescale_between, Point, Rect, Scale, Size};
pub use identity::{
    DisplayKey, DisplaySnapshot, IdentityQuality, WindowId, WindowMode, WindowSnapshot,
};
pub use state::{EngineError, EngineState, SkipReason, WindowOutcome};
