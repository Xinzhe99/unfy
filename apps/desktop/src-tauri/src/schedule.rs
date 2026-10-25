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
    manual_generation: Arc<AtomicU64>,
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
            manual_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Capture the generation before observing or queueing an automatic request.
    /// Unlike `claim`, this never supersedes an intervening user action.
    pub fn token(&self) -> ScheduleToken {
        ScheduleToken {
            generation: Arc::clone(&self.generation),
            mine: self.generation.load(Ordering::SeqCst),
        }
    }

    /// Claim an automatic request, superseding only earlier automatic requests.
    pub fn claim(&mut self) -> ScheduleToken {
        let mine = self
            .generation
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        ScheduleToken {
            generation: self.generation.clone(),
            mine,
        }
    }

    /// A user request supersedes older user requests and all automatic work.
    pub fn claim_manual(&mut self) -> ScheduleToken {
        self.cancel_automatic();
        let mine = self
            .manual_generation
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        ScheduleToken {
            generation: Arc::clone(&self.manual_generation),
            mine,
        }
    }

    /// Observation changes revoke automatic work without cancelling user actions.
    pub fn cancel_automatic(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Explicit cancellation, settings changes and maintenance invalidate all work.
    pub fn cancel(&mut self) {
        self.cancel_automatic();
        self.manual_generation.fetch_add(1, Ordering::SeqCst);
    }
}

/// Permission to act, granted by [`Schedule::claim`].
#[derive(Debug, Clone)]
pub struct ScheduleToken {
    generation: Arc<AtomicU64>,
    mine: u64,
}

impl ScheduleToken {
    /// Whether this token is still the newest request. Writers must recheck this
    /// after acquiring their engine lock, including when the delay was zero.
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
        assert!(wait_cancellable(Duration::ZERO, &token));
    }

    #[test]
    fn observer_cancellation_preserves_manual_requests() {
        let mut schedule = Schedule::new();
        let manual = schedule.claim_manual();
        let automatic = schedule.claim();
        assert!(manual.is_current());
        schedule.cancel_automatic();
        assert!(manual.is_current());
        assert!(!automatic.is_current());
    }

    #[test]
    fn manual_request_supersedes_automatic_and_earlier_manual_work() {
        let mut schedule = Schedule::new();
        let first_manual = schedule.claim_manual();
        let automatic = schedule.claim();
        let latest_manual = schedule.claim_manual();
        assert!(!first_manual.is_current());
        assert!(!automatic.is_current());
        assert!(latest_manual.is_current());
        let latest_auto = schedule.claim();
        schedule.cancel();
        assert!(!latest_manual.is_current());
        assert!(!latest_auto.is_current());
    }

    #[test]
    fn cancellation_after_deadline_but_before_writer_lock_prevents_writes() {
        use std::sync::{mpsc, Mutex};

        for reason in ["reconnect", "manual", "pause_or_maintenance"] {
            let mut schedule = Schedule::new();
            let token = schedule.claim();
            let writer = Arc::new(Mutex::new(0));
            let held = writer.lock().unwrap();
            let (ready, reached_deadline) = mpsc::channel();
            let worker_writer = Arc::clone(&writer);
            let worker = std::thread::spawn(move || {
                assert!(wait_cancellable(Duration::ZERO, &token));
                ready.send(()).unwrap();
                let mut writes = worker_writer.lock().unwrap();
                if token.is_current() {
                    *writes += 1;
                }
            });
            reached_deadline.recv().unwrap();
            match reason {
                "reconnect" => schedule.cancel_automatic(),
                "manual" => {
                    schedule.claim_manual();
                }
                _ => schedule.cancel(),
            }
            drop(held);
            worker.join().unwrap();
            assert_eq!(*writer.lock().unwrap(), 0, "{reason}");
        }
    }
}
