//! Background remote-session monitor.
//!
//! Polls the detector and drives the engine when the verdict changes. Three
//! properties this module exists to guarantee:
//!
//! 1. **Never move windows on weak, unconfirmed evidence.** The detector requires
//!    consecutive polls for process-level signals; this loop adds nothing to that
//!    decision but must not bypass it.
//! 2. **Never fight the user.** If automation fails, is paused, or the user asked
//!    for the opposite, the manual controls keep working and the state stays
//!    consistent. A failed automatic gather is not retried in a loop.
//! 3. **Always explain itself.** Every automatic action emits an event with the
//!    evidence string, so the UI can say "因为检测到 AnyDesk 已连接，已整理窗口"
//!    instead of windows moving with no explanation.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager, Runtime};
use unfy_detect::{Evidence, Sensitivity, Tracker, Transition};

use crate::AppState;

/// Stop the monitor. Kept tiny so `session::install` can flip it on shutdown.
static STOP: AtomicBool = AtomicBool::new(false);

/// Start the monitor thread.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let state = app.state::<AppState>();
    let sensitivity = current_sensitivity(app);
    state
        .detector
        .lock()
        .map(|mut tracker| *tracker = Tracker::new(sensitivity))
        .ok();

    std::thread::spawn(move || {
        let mut last = std::time::Instant::now();
        loop {
            if STOP.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(unfy_detect::POLL_INTERVAL);

            // Spec P0-10-B: after the machine wakes, nothing observed before sleep
            // may be trusted. The wall clock jumped by far more than the poll
            // interval, which is the one signal for "we were suspended" that does
            // not need a per-platform power callback.
            //
            // This matters because both the detector and the topology can change
            // while the process is frozen: a remote session may have ended, a
            // monitor may have been unplugged. Acting on the pre-sleep verdict
            // would move windows based on a stale observation.
            let elapsed = last.elapsed();
            last = std::time::Instant::now();
            if elapsed > unfy_detect::POLL_INTERVAL * 3 {
                tracing::info!(
                    elapsed_secs = elapsed.as_secs(),
                    "long gap between polls; assuming the machine was suspended"
                );
                resume_after_sleep(&handle);
            }

            tick(&handle);
        }
    });
}

/// Clear observations that predate a suspend, without inventing a state change.
///
/// The tempting implementation - rebuild the tracker - is wrong. A fresh tracker
/// has no active evidence, so the next poll reports `Deactivated` and the app
/// restores every window, even though the user is still connected and the machine
/// only slept. That is worse than acting on stale data.
///
/// What is actually stale is everything *observed*: the streak counter and the
/// last logged evidence summary. The tracker's active verdict is kept, so a
/// session that survived the sleep stays active and one that ended is detected by
/// the normal path on the next poll.
fn resume_after_sleep<R: Runtime>(app: &AppHandle<R>) {
    {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        // `Tracker::force` resets the streak while keeping the current evidence.
        if let Ok(mut tracker) = state.detector.lock() {
            let evidence = tracker.evidence().cloned();
            tracker.force(evidence);
        }
        // The last-observation summary is compared against the next poll; keeping a
        // pre-sleep value would suppress the "evidence changed" log line for a genuine
        // change. The manual marker is a user statement and survives regardless.
        //
        // `let` rather than `if let`: the guard from an `if let` lives to the end of the
        // enclosing block, which outlives `state` here and made the borrow fail.
        let mut last = match state.last_observation.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        *last = None;
    }
    // No event is emitted. This is backend bookkeeping - the streak and the observation
    // summary that suppresses the next "evidence changed" log line - and the frontend has
    // nothing to do with it: the status poll re-reads the same tracker within four
    // seconds. An event was sent here for a while and nothing listened, which is worse
    // than silence because it looks like a contract.
}

/// Re-read the sensitivity after the user changes it.
///
/// Called from the settings command so a change takes effect immediately instead
/// of at the next launch.
pub fn apply_sensitivity<R: Runtime>(app: &AppHandle<R>) {
    let sensitivity = current_sensitivity(app);
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut tracker) = state.detector.lock() {
            tracker.set_sensitivity(sensitivity);
        }
    }
}

/// One poll. Public so the UI can request an immediate re-check after the user
/// changes a setting, instead of waiting up to one interval.
pub fn tick<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    let (settings, paused) = match state.settings.lock() {
        Ok(settings) => (settings.clone(), settings.paused),
        Err(_) => return,
    };

    // Read the topology for virtual-display evidence. A failure here means the
    // platform is momentarily unavailable; reporting "no evidence" would end an
    // active session and move every window back. Skip the poll instead.
    let topology = {
        let Ok(engine) = state.engine.lock() else {
            return;
        };
        match unfy_platform::DisplayProvider::topology(engine.backend()) {
            Ok(topology) => topology,
            Err(_) => return,
        }
    };

    let observation = unfy_detect::platform::observe(&topology);

    // Log only when the *observation* changes, not on every poll: two lines a
    // second would bury every other event within minutes. The interesting moment
    // is when the evidence set changes, which is exactly when a verdict could
    // flip.
    let summary = unfy_detect::platform::summary(&observation);
    let changed = {
        let mut last = match state.last_observation.lock() {
            Ok(last) => last,
            Err(_) => return,
        };
        if last.as_deref() == Some(summary.as_str()) {
            false
        } else {
            *last = Some(summary.clone());
            true
        }
    };
    if changed {
        tracing::info!(summary = %summary, "remote-session evidence changed");
    }

    let transition = {
        let Ok(mut tracker) = state.detector.lock() else {
            return;
        };
        tracker.observe(&observation)
    };

    // Publish what is visible, including evidence that does not authorise an
    // automatic action. Without this the overview could only say "nothing
    // detected" while a remote tool was plainly running.
    if let Some(verdict) = tracker_verdict(&state, &observation) {
        let _ = app.emit(
            "unfy://remote",
            serde_json::json!({
                "active": matches!(verdict, unfy_detect::Verdict::Connected(_)),
                "candidate": matches!(verdict, unfy_detect::Verdict::Candidate(_)),
                "evidence": match &verdict {
                    unfy_detect::Verdict::Connected(e) | unfy_detect::Verdict::Candidate(e) => {
                        Some(crate::message::evidence_message(e))
                    }
                    _ => None,
                },
            }),
        );
    }

    match transition {
        Transition::None => {}
        Transition::Activated(evidence) => {
            // Publish the verdict even when automation is disabled, so the
            // overview can show "检测到远程会话" and let the user act on it.
            let _ = app.emit(
                "unfy://remote",
                serde_json::json!({
                    "active": true,
                    "evidence": crate::message::evidence_message(&evidence),
                    "acted": false,
                }),
            );

            let blocked = {
                let last = state.last_auto_gather.lock().ok().and_then(|guard| *guard);
                suppression(&settings, paused, last, std::time::Instant::now())
            };
            match blocked {
                Some(Suppression::AutomationOff) => {
                    tracing::info!(
                        evidence = %evidence,
                        auto_gather = settings.auto_gather,
                        paused,
                        "remote session started; not gathering"
                    );
                    return;
                }
                // The user told us the remote client already shows every screen,
                // so there is nothing hidden to gather. Acting anyway would
                // rearrange a desktop the user can already see in full.
                Some(Suppression::AllScreens) => {
                    tracing::info!(
                        evidence = %evidence,
                        "remote shows all screens; nothing to gather"
                    );
                    let _ = app.emit(
                        "unfy://remote",
                        serde_json::json!({
                            "active": true,
                            "evidence": crate::message::evidence_message(&evidence),
                            "acted": false,
                            "suppressed": "all_screens",
                        }),
                    );
                    return;
                }
                // Emergency rescues are not gated by the cooldown - they are
                // handled by the topology watcher, which never reaches this code.
                Some(Suppression::Cooldown) => {
                    tracing::info!(evidence = %evidence, "auto-gather suppressed by cooldown");
                    let _ = app.emit(
                        "unfy://remote",
                        serde_json::json!({
                            "active": true,
                            "evidence": crate::message::evidence_message(&evidence),
                            "acted": false,
                            "suppressed": "cooldown",
                        }),
                    );
                    return;
                }
                None => {
                    // Stamped before the attempt: a gather that fails still starts
                    // the cooldown, so a desktop that refuses every move cannot be
                    // retried in a loop.
                    if let Ok(mut slot) = state.last_auto_gather.lock() {
                        *slot = Some(std::time::Instant::now());
                    }
                }
            }
            match gather(app, &evidence) {
                Ok(applied) => {
                    crate::tray::record(app, crate::activity::Kind::AutoGather, applied, 0, "auto");
                    tracing::info!(evidence = %evidence, applied, "auto-gather finished");
                    let _ = app.emit(
                        "unfy://remote",
                        serde_json::json!({
                            "active": true,
                            "evidence": crate::message::evidence_message(&evidence),
                            "acted": true,
                            "applied": applied,
                        }),
                    );
                }
                Err(message) => {
                    tracing::warn!(evidence = %evidence, error = %message, "auto-gather failed");
                    let _ = app.emit(
                        "unfy://remote",
                        serde_json::json!({
                            "active": true,
                            "evidence": crate::message::evidence_message(&evidence),
                            "acted": false,
                            "error": message,
                        }),
                    );
                }
            }
        }
        Transition::Deactivated => {
            let _ = app.emit(
                "unfy://remote",
                serde_json::json!({
                    "active": false,
                    "acted": false,
                }),
            );

            match session_end(settings.auto_restore, paused) {
                SessionEnd::Restore => {}
                SessionEnd::Retain => {
                    // The transaction is kept, and the UI keeps offering the manual
                    // restore. Nothing is remembered "for later": the detector fires
                    // the end of a session once, so a flag consumed by an unrelated
                    // resume was a hidden debt the user never asked for.
                    tracing::info!(
                        auto_restore = settings.auto_restore,
                        paused,
                        "remote session ended; windows stay gathered and the manual restore stays available"
                    );
                    return;
                }
            }
            // Only restore something this feature (or the user) actually
            // gathered. Restoring an idle engine is a no-op, so this is safe
            // either way, but it keeps the log honest.
            // A cancelled restore means a new gather superseded it during the
            // wait - the session came back. Doing nothing is the correct outcome,
            // and it must not be recorded as a restore that moved zero windows.
            let outcome = restore(app);
            match &outcome {
                Ok(report) if report.cancelled => {
                    tracing::info!("auto-restore cancelled by a newer request");
                }
                Ok(report) => {
                    crate::tray::record(
                        app,
                        crate::activity::Kind::AutoRestore,
                        report.restored,
                        report.pending,
                        "auto",
                    );
                    tracing::info!(
                        restored = report.restored,
                        pending = report.pending,
                        "auto-restore finished"
                    );
                    let _ = app.emit(
                        "unfy://remote",
                        serde_json::json!({
                            "active": false,
                            "acted": true,
                            "restored": report.restored,
                            "pending": report.pending,
                        }),
                    );
                }
                Err(message) => {
                    tracing::warn!(error = %message, "auto-restore failed");
                    let _ = app.emit(
                        "unfy://remote",
                        serde_json::json!({
                            "active": false,
                            "acted": false,
                            "error": message,
                        }),
                    );
                }
            }
        }
    }
}

/// The tracker's view of the observation just taken, if it can be read.
fn tracker_verdict(
    state: &AppState,
    observation: &unfy_detect::Observation,
) -> Option<unfy_detect::Verdict> {
    let tracker = state.detector.lock().ok()?;
    Some(tracker.verdict(observation))
}

fn current_sensitivity<R: Runtime>(app: &AppHandle<R>) -> Sensitivity {
    let Some(state) = app.try_state::<AppState>() else {
        return Sensitivity::Off;
    };
    // The guard borrows from `state`, which is a `State<'_, _>` living only for
    // this call: clone before the match ends.
    state
        .settings
        .lock()
        .map(|settings| Sensitivity::parse(&settings.auto_detect))
        .unwrap_or(Sensitivity::Off)
}

/// Gather, mapping engine refusals to a message the UI can act on.
fn gather<R: Runtime>(app: &AppHandle<R>, evidence: &Evidence) -> Result<usize, String> {
    let state = app.state::<AppState>();
    // A gather supersedes a pending delayed restore. This is the reconnect case:
    // a session that ends and comes back within the delay must end up gathered,
    // not restored a moment later by a timer the user has forgotten about.
    if let Ok(mut schedule) = state.schedule.lock() {
        schedule.cancel();
    }

    // The engine first, and the settings only long enough to read the target id.
    //
    // `tray::refresh` below takes the engine lock again to build the menu, and a
    // `Mutex` is not reentrant, so holding either guard across it deadlocks this
    // thread against itself. The symptom is not a slow refresh: the settings guard
    // stays held, the window's next status poll waits for it on the main thread, and
    // the whole app stops answering. This is exactly how an automatic gather froze
    // the window.
    let mut engine = state.engine.lock().map_err(|e| e.to_string())?;
    let target_id = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings
            .target_display
            .as_ref()
            .map(|target| target.native_id.clone())
    };

    let target = engine
        .resolve_target(target_id.as_deref())
        // A sentinel, not a sentence: the frontend maps it to a translated message
        // (`humanError`). A Chinese sentence here reached the user verbatim.
        .ok_or_else(|| "no_displays".to_string())?;

    // A session that starts while a layout is already in effect must not
    // redefine where the windows came from. `retarget` re-plans onto the new
    // target and keeps every first preimage, which is exactly the behaviour
    // needed for "the second remote tool sees a different screen".
    match engine.retarget(&target) {
        Ok(retarget) => {
            let applied = retarget.migration.applied_count();
            drop(engine);
            crate::tray::refresh(app);
            tracing::info!(evidence = %evidence, applied, "auto-gather applied");
            Ok(applied)
        }
        Err(e) => Err(e.to_string()),
    }
}

/// How long after an automatic gather the next one is refused.
///
/// A session that flaps - connect, drop, connect - produces a sequence of
/// activations. The detector's streak filter removes a single flaky sample; this is
/// the second layer, and the spec fixes it at one cycle per 30 seconds.
const AUTO_GATHER_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(30);

/// What the end of a session owes the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionEnd {
    /// Automatic restore is off, or automation is paused: the layout stays until
    /// the user restores it by hand. The recovery record is never discarded.
    Retain,
    /// Put the windows back now.
    Restore,
}

fn restore<R: Runtime>(app: &AppHandle<R>) -> Result<RestoreSummary, String> {
    // The delay applies here too, and for the reason it exists: a session that
    // drops and reconnects must not cause two full migrations. RDP and the manual
    // marker transition immediately, so without the wait a reconnect two seconds
    // later would restore and re-gather every window. The engine reports the end
    // once, so the wait has to be observed rather than skipped.
    //
    // A pending restore is cancelled by a new gather (see `gather`), which is
    // exactly the reconnect case.
    let state = app.state::<AppState>();
    let delay = state
        .settings
        .lock()
        .ok()
        .map(|settings| settings.restore_delay_secs)
        .unwrap_or(0);
    if delay > 0 {
        let token = match state.schedule.lock() {
            Ok(mut schedule) => schedule.claim(),
            Err(_) => return Err("schedule lock".to_string()),
        };
        // Tell the window a wait is in progress. Without this the automatic restore
        // looks like the app has stopped responding for the whole delay: the user
        // disconnects, sees nothing, and has no idea a timer is running or that
        // gathering again would cancel it. `commands::restore_with_trigger` emits
        // the same event for the delayed path there; this one hand-rolled the wait
        // and dropped the notification.
        let _ = app.emit(
            "unfy://progress",
            serde_json::json!({ "phase": "waiting", "seconds": delay }),
        );
        if !crate::schedule::wait_cancellable(
            std::time::Duration::from_secs(u64::from(delay)),
            &token,
        ) {
            return Ok(RestoreSummary {
                restored: 0,
                pending: 0,
                cancelled: true,
            });
        }
    }
    // No cancellation here: this restore owns the newest token, and clearing it
    // would invalidate nothing useful while making the next claim start from a
    // different generation. The wait above already checked the token.
    let mut engine = state.engine.lock().map_err(|e| e.to_string())?;
    let report = engine.restore().map_err(|e| e.to_string())?;
    let summary = RestoreSummary {
        cancelled: false,
        restored: report
            .outcomes
            .values()
            .filter(|o| {
                **o == unfy_domain::WindowOutcome::Restored
                    || **o == unfy_domain::WindowOutcome::FallbackVisible
            })
            .count(),
        pending: report.pending.len(),
    };
    drop(engine);
    // Close the progress cycle this function opened with "waiting", after the lock
    // is released so a slow tray refresh cannot hold the engine. Without it the
    // window keeps saying "正在恢复布局…" once the windows are already back.
    let _ = app.emit("unfy://progress", serde_json::json!({ "phase": "done" }));
    crate::tray::refresh(app);
    Ok(summary)
}

struct RestoreSummary {
    restored: usize,
    pending: usize,
    /// A newer request superseded this restore during its wait.
    cancelled: bool,
}

/// Decide what the end of a session should do.
fn session_end(auto_restore: bool, paused: bool) -> SessionEnd {
    if !auto_restore || paused {
        return SessionEnd::Retain;
    }
    SessionEnd::Restore
}

/// Why automatic gathering is not running for this activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Suppression {
    /// Automatic gathering is off, or the engine is paused.
    AutomationOff,
    /// The remote client already shows every screen, so nothing is hidden.
    AllScreens,
    /// A gather for a recent session is still inside the cooldown.
    Cooldown,
}

/// Decide whether an activation should gather, and if not, why.
///
/// Pure so the rules can be tested without a desktop: they are the product's
/// promise ("gather when a session starts, unless..."), and every branch means a
/// different thing to the user - a switch they set, a screen they can already see,
/// or a wait.
fn suppression(
    settings: &unfy_settings::Settings,
    paused: bool,
    last_auto_gather: Option<std::time::Instant>,
    now: std::time::Instant,
) -> Option<Suppression> {
    if !settings.auto_gather || paused {
        return Some(Suppression::AutomationOff);
    }
    if settings.remote_screens == unfy_settings::RemoteScreens::All {
        return Some(Suppression::AllScreens);
    }
    if last_auto_gather.is_some_and(|at| now.duration_since(at) < AUTO_GATHER_COOLDOWN) {
        return Some(Suppression::Cooldown);
    }
    None
}

/// Leak-free shutdown hook for the monitor thread.
pub fn stop() {
    STOP.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use unfy_settings::{RemoteScreens, Settings};

    fn settings(auto_gather: bool, remote_screens: RemoteScreens) -> Settings {
        Settings {
            auto_gather,
            remote_screens,
            ..Settings::default()
        }
    }

    #[test]
    fn an_activation_gathers_when_nothing_forbids_it() {
        let now = Instant::now();
        assert_eq!(
            suppression(&settings(true, RemoteScreens::Single), false, None, now),
            None
        );
    }

    #[test]
    fn the_switch_and_the_pause_both_stop_it() {
        let now = Instant::now();
        // Off is off, whether or not the engine is also paused.
        assert_eq!(
            suppression(&settings(false, RemoteScreens::Single), false, None, now),
            Some(Suppression::AutomationOff)
        );
        assert_eq!(
            suppression(&settings(true, RemoteScreens::Single), true, None, now),
            Some(Suppression::AutomationOff)
        );
    }

    #[test]
    fn a_remote_that_sees_every_screen_has_nothing_to_gather() {
        let now = Instant::now();
        assert_eq!(
            suppression(&settings(true, RemoteScreens::All), false, None, now),
            Some(Suppression::AllScreens)
        );
    }

    #[test]
    fn the_cooldown_lasts_thirty_seconds() {
        let now = Instant::now();
        let recent = now - Duration::from_secs(29);
        let stale = now - Duration::from_secs(31);
        assert_eq!(
            suppression(
                &settings(true, RemoteScreens::Single),
                false,
                Some(recent),
                now
            ),
            Some(Suppression::Cooldown)
        );
        assert_eq!(
            suppression(
                &settings(true, RemoteScreens::Single),
                false,
                Some(stale),
                now
            ),
            None,
            "a session that drops and reconnects after the cooldown gathers again"
        );
    }

    #[test]
    fn the_switches_are_reported_before_the_cooldown() {
        // The reason decides what the user is told. "You turned it off" is true and
        // actionable; "wait 30 seconds" would be misleading, and a user who then
        // turns the switch on would still be waiting for a timer they cannot see.
        let now = Instant::now();
        let recent = now - Duration::from_secs(1);
        assert_eq!(
            suppression(
                &settings(false, RemoteScreens::All),
                false,
                Some(recent),
                now
            ),
            Some(Suppression::AutomationOff)
        );
        assert_eq!(
            suppression(
                &settings(true, RemoteScreens::All),
                false,
                Some(recent),
                now
            ),
            Some(Suppression::AllScreens)
        );
    }

    #[test]
    fn the_end_of_a_session_restores_only_when_asked() {
        assert_eq!(session_end(true, false), SessionEnd::Restore);
        // Off means the layout stays; the user restores it by hand.
        assert_eq!(session_end(false, false), SessionEnd::Retain);
        // Paused keeps the layout too. The recovery record is retained, and the
        // manual restore stays offered; nothing is deferred to an unrelated resume.
        assert_eq!(session_end(true, true), SessionEnd::Retain);
        assert_eq!(session_end(false, true), SessionEnd::Retain);
    }

    #[test]
    fn a_paused_engine_outranks_every_other_rule() {
        let now = Instant::now();
        let recent = now - Duration::from_secs(1);
        assert_eq!(
            suppression(&settings(true, RemoteScreens::All), true, Some(recent), now),
            Some(Suppression::AutomationOff)
        );
    }
}
