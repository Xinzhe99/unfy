//! Scheduled restore.
//!
//! # Why this is not a `sleep` inside the engine
//!
//! The user can configure a delay before windows go back, so a remote session
//! that drops and reconnects within a few seconds does not cause two full
//! migrations. The first implementation slept inside `Engine::restore`, which
//! holds the engine mutex: with a 30-second delay, `get_status` and every tray
//! refresh blocked for 30 seconds. The UI froze and looked crashed, which is a
//! far worse failure than moving windows slightly early.
//!
//! So the wait lives here, and it is **cancellable**. If the user gathers again
//! or presses the shortcut during the wait, the pending restore must be dropped:
//! otherwise a stale timer fires a minute later and undoes a layout the user just
//! asked for.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A delay that can be cancelled by a newer request.
///
/// Pure state machine: no threads, no clock reads except the ones handed in, so
/// the cancellation rule is testable without sleeping.
#[derive(Debug, Clone)]
pub struct Schedule {
    generation: Arc<AtomicU64>,
    current: u64,
}

impl Default for Schedule {
    fn default() -> Self {
        Self::new()
    }
}

impl Schedule {
    pub fn new() -> Self {
        Self {
            generation: Arc::new(AtomicU64::new(0)),
            current: 0,
        }
    }

    /// Claim the right to restore. Returns a token the waiter must check.
    ///
    /// Every new claim invalidates the previous one, which is exactly the
    /// semantics the UI needs: "restore in 10 s" pressed twice restores once.
    pub fn claim(&mut self) -> ScheduleToken {
        self.current = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        ScheduleToken {
            generation: self.generation.clone(),
            mine: self.current,
        }
    }

    /// Invalidate any pending restore without scheduling a new one.
    pub fn cancel(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.current = self.generation.load(Ordering::SeqCst);
    }
}

/// Permission to act, granted by [`Schedule::claim`].
#[derive(Debug, Clone)]
pub struct ScheduleToken {
    generation: Arc<AtomicU64>,
    mine: u64,
}

impl ScheduleToken {
    /// Whether this token is still the newest request.
    pub fn is_current(&self) -> bool {
        self.generation.load(Ordering::SeqCst) == self.mine
    }
}

/// Wait `delay`, giving up as soon as `token` is superseded.
///
/// The sleep is sliced so cancellation takes effect within ~100 ms instead of
/// after the full delay. Returns whether the caller should proceed.
pub fn wait_cancellable(delay: Duration, token: &ScheduleToken) -> bool {
    const SLICE: Duration = Duration::from_millis(100);
    let deadline = Instant::now() + delay;
    loop {
        if !token.is_current() {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return token.is_current();
        }
        std::thread::sleep(SLICE.min(deadline - now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_claim_supersedes_the_previous_one() {
        let mut schedule = Schedule::new();
        let first = schedule.claim();
        assert!(first.is_current());

        let second = schedule.claim();
        assert!(!first.is_current(), "the older claim must lose");
        assert!(second.is_current());
    }

    #[test]
    fn cancel_invalidates_a_pending_restore() {
        let mut schedule = Schedule::new();
        let token = schedule.claim();
        schedule.cancel();
        assert!(!token.is_current());
        assert!(!wait_cancellable(Duration::from_secs(30), &token));
    }

    #[test]
    fn waiting_returns_true_for_the_current_token() {
        let mut schedule = Schedule::new();
        let token = schedule.claim();
        assert!(wait_cancellable(Duration::from_millis(120), &token));
    }

    #[test]
    fn waiting_aborts_early_when_superseded() {
        let mut schedule = Schedule::new();
        let token = schedule.claim();

        let handle_schedule = schedule.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            let mut copy = handle_schedule;
            copy.cancel();
        });

        let started = Instant::now();
        let proceed = wait_cancellable(Duration::from_secs(5), &token);
        assert!(!proceed, "a cancelled wait must not proceed");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "cancellation must not wait out the full delay, took {:?}",
            started.elapsed()
        );
    }
}

#[cfg(test)]
mod restore_cancellation_tests {
    //! A pending restore must be cancellable, because that is the whole point of
    //! the delay: a remote session that drops and reconnects within it must not
    //! cause two full migrations. These tests drive the real `Schedule`, not a
    //! stand-in, so the generation counter is covered.

    use super::*;

    #[test]
    fn a_gather_during_the_wait_cancels_the_restore() {
        let mut schedule = Schedule::new();
        let restore = schedule.claim();

        // A gather arrives while the restore is waiting.
        schedule.cancel();

        assert!(
            !wait_cancellable(Duration::from_millis(50), &restore),
            "the waiting restore must observe the cancellation"
        );
    }

    #[test]
    fn an_uncancelled_wait_still_proceeds() {
        let mut schedule = Schedule::new();
        let restore = schedule.claim();
        assert!(wait_cancellable(Duration::from_millis(30), &restore));
    }

    #[test]
    fn cancelling_twice_is_harmless() {
        let mut schedule = Schedule::new();
        let restore = schedule.claim();
        schedule.cancel();
        schedule.cancel();
        assert!(!wait_cancellable(Duration::from_millis(10), &restore));
    }
}
