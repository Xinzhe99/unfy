//! Engine state and error taxonomy.
//!
//! The state machine is deliberately small: with manual triggering there is no
//! `Armed`/`Detecting`/`Degraded-by-signal` dimension. Each state answers one
//! question — "may the engine write to windows right now?" — and the answer is
//! `yes` only in `Migrating` and `Restoring`.

use serde::{Deserialize, Serialize};

/// The single writer's state. One engine instance per user session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineState {
    /// Nothing captured, nothing in flight. Reads are allowed.
    Idle,
    /// A plan exists in memory only. No window has been touched.
    Planning,
    /// Journal committed; windows are being moved.
    Migrating,
    /// At least one window moved. A journal exists and must be resolved.
    Active,
    /// Restoring captured windows.
    Restoring,
    /// Something is blocked: permissions, a missing display, or unresolved
    /// records. Windows may still be displaced; the journal is retained.
    Degraded,
    /// Cannot proceed safely (corrupt journal, failed durable write). No new
    /// writes are permitted; recovery and off-screen rescue remain available.
    Error,
}

impl EngineState {
    /// Whether the engine may issue window writes in this state.
    pub fn may_write(self) -> bool {
        matches!(self, EngineState::Migrating | EngineState::Restoring)
    }

    /// Whether an unresolved journal may exist.
    pub fn has_pending_work(self) -> bool {
        matches!(
            self,
            EngineState::Active | EngineState::Restoring | EngineState::Degraded
        )
    }

    /// Whether the tray should show a warning affordance.
    pub fn is_attention(self) -> bool {
        matches!(self, EngineState::Degraded | EngineState::Error)
    }
}

/// Per-window result. Deliberately finer-grained than a boolean: the UI must be
/// able to say "17 restored, 2 kept where you put them, 1 closed".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowOutcome {
    /// Moved to the planned rectangle and the read-back matched.
    Applied,
    /// Restored to its exact original rectangle.
    Restored,
    /// The user moved it during the session; kept the new position.
    KeptUserChange,
    /// The original display is gone; placed in a visible work area instead.
    FallbackVisible,
    /// The window rejected the move, or kept correcting itself.
    Rejected,
    /// The window was destroyed before it could be handled.
    Gone,
    /// Not attempted because a precondition failed (permissions, fullscreen…).
    Skipped,
    /// Attempted, result unknown (crash, permission revoked mid-flight).
    Unknown,
}

impl WindowOutcome {
    /// Whether this outcome counts as a safe terminal state.
    pub fn is_safe(self) -> bool {
        matches!(
            self,
            WindowOutcome::Applied
                | WindowOutcome::Restored
                | WindowOutcome::KeptUserChange
                | WindowOutcome::FallbackVisible
                | WindowOutcome::Gone
        )
    }
}

/// Why a window was skipped. Surfaced verbatim in the UI, so each variant maps
/// to one Chinese sentence in `apps/desktop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkipReason {
    OwnWindow,
    Invisible,
    ZeroSize,
    ToolWindow,
    ShellWindow,
    ModalChild,
    Fullscreen,
    OtherVirtualDesktop,
    PermissionDenied,
    Minimized,
}

impl SkipReason {
    pub fn code(self) -> &'static str {
        match self {
            SkipReason::OwnWindow => "own_window",
            SkipReason::Invisible => "invisible",
            SkipReason::ZeroSize => "zero_size",
            SkipReason::ToolWindow => "tool_window",
            SkipReason::ShellWindow => "shell_window",
            SkipReason::ModalChild => "modal_child",
            SkipReason::Fullscreen => "fullscreen",
            SkipReason::OtherVirtualDesktop => "other_virtual_desktop",
            SkipReason::PermissionDenied => "permission_denied",
            SkipReason::Minimized => "minimized",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineError {
    /// The journal could not be written durably. No window was touched.
    JournalWrite(String),
    /// The journal on disk is unreadable. Windows may be displaced.
    JournalCorrupt(String),
    /// The target display disappeared between planning and migration.
    TargetDisplayMissing,
    /// The platform refused an operation on a specific window.
    Platform(String),
    /// The engine was asked to act from a state that forbids writes.
    InvalidTransition { from: EngineState },
    /// A gather was requested while windows are still captured. Restore first,
    /// otherwise the new preimages would record the positions this engine just
    /// produced as if they were the user's original layout.
    TransactionAlreadyOpen,
    /// The user paused automation; the current layout is still in effect.
    Paused,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::JournalWrite(e) => write!(f, "journal write failed: {e}"),
            EngineError::JournalCorrupt(e) => write!(f, "journal is corrupt: {e}"),
            EngineError::TargetDisplayMissing => write!(f, "target display is gone"),
            EngineError::Platform(e) => write!(f, "platform error: {e}"),
            EngineError::InvalidTransition { from } => {
                write!(f, "cannot write windows from state {from:?}")
            }
            EngineError::TransactionAlreadyOpen => {
                write!(
                    f,
                    "windows are still captured; restore before gathering again"
                )
            }
            EngineError::Paused => write!(f, "automation is paused"),
        }
    }
}

impl std::error::Error for EngineError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_two_states_may_write() {
        for state in [
            EngineState::Idle,
            EngineState::Planning,
            EngineState::Active,
            EngineState::Degraded,
            EngineState::Error,
        ] {
            assert!(!state.may_write(), "{state:?} must not write");
        }
        assert!(EngineState::Migrating.may_write());
        assert!(EngineState::Restoring.may_write());
    }

    #[test]
    fn active_and_degraded_keep_pending_work() {
        assert!(EngineState::Active.has_pending_work());
        assert!(EngineState::Degraded.has_pending_work());
        assert!(EngineState::Restoring.has_pending_work());
        assert!(!EngineState::Idle.has_pending_work());
        assert!(!EngineState::Planning.has_pending_work());
        assert!(!EngineState::Error.has_pending_work());
    }

    #[test]
    fn rejected_and_unknown_are_not_safe_terminal_states() {
        assert!(!WindowOutcome::Rejected.is_safe());
        assert!(!WindowOutcome::Unknown.is_safe());
        assert!(!WindowOutcome::Skipped.is_safe());
        assert!(WindowOutcome::Restored.is_safe());
        assert!(WindowOutcome::FallbackVisible.is_safe());
    }

    #[test]
    fn skip_codes_are_unique() {
        let all = [
            SkipReason::OwnWindow,
            SkipReason::Invisible,
            SkipReason::ZeroSize,
            SkipReason::ToolWindow,
            SkipReason::ShellWindow,
            SkipReason::ModalChild,
            SkipReason::Fullscreen,
            SkipReason::OtherVirtualDesktop,
            SkipReason::PermissionDenied,
            SkipReason::Minimized,
        ];
        let mut codes: Vec<_> = all.iter().map(|r| r.code()).collect();
        codes.sort_unstable();
        let len = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), len);
    }
}
