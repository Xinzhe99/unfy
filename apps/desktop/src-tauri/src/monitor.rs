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

use conflux_detect::{Evidence, Sensitivity, Tracker, Transition};
use tauri::{AppHandle, Emitter, Manager, Runtime};

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
            std::thread::sleep(conflux_detect::POLL_INTERVAL);

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
            if elapsed > conflux_detect::POLL_INTERVAL * 3 {
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
    if let Ok(mut last) = state.last_observation.lock() {
        *last = None;
    }
    let _ = app.emit(
        "conflux://resumed",
        serde_json::json!({ "detection": "revalidated" }),
    );
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
        match conflux_platform::DisplayProvider::topology(engine.backend()) {
            Ok(topology) => topology,
            Err(_) => return,
        }
    };

    let mut observation = conflux_detect::platform::observe(&topology);
    // The manual marker lives in memory, not in the OS: the detector's platform
    // probe always reports `false` for it. Merging it here means a user-marked
    // session keeps activating regardless of what the probes see, and ending it is
    // also a user action (clearing the flag).
    observation.manual_marker = state
        .manual_remote
        .load(std::sync::atomic::Ordering::SeqCst);

    // Log only when the *observation* changes, not on every poll: two lines a
    // second would bury every other event within minutes. The interesting moment
    // is when the evidence set changes, which is exactly when a verdict could
    // flip.
    let summary = conflux_detect::platform::summary(&observation);
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

    match transition {
        Transition::None => {}
        Transition::Activated(evidence) => {
            // Publish the verdict even when automation is disabled, so the
            // overview can show "检测到远程会话" and let the user act on it.
            let _ = app.emit(
                "conflux://remote",
                serde_json::json!({
                    "active": true,
                    "evidence": crate::message::evidence_message(&evidence),
                    "acted": false,
                }),
            );

            if !settings.auto_gather || paused {
                tracing::info!(
                    evidence = %evidence,
                    auto_gather = settings.auto_gather,
                    paused,
                    "remote session started; not gathering"
                );
                return;
            }

            // The user told us the remote client already shows every screen, so
            // there is nothing hidden to gather. Acting anyway would rearrange a
            // desktop the user can already see in full.
            if settings.remote_screens == conflux_settings::RemoteScreens::All {
                tracing::info!(
                    evidence = %evidence,
                    "remote shows all screens; nothing to gather"
                );
                let _ = app.emit(
                    "conflux://remote",
                    serde_json::json!({
                        "active": true,
                        "evidence": crate::message::evidence_message(&evidence),
                        "acted": false,
                        "suppressed": "all_screens",
                    }),
                );
                return;
            }

            // Spec: at most one automatic cycle per 30 s for the same evidence.
            // Emergency rescues are not gated by this - they are handled by the
            // topology watcher, which never reaches this code.
            {
                const COOLDOWN: std::time::Duration = std::time::Duration::from_secs(30);
                let recent = state
                    .last_auto_gather
                    .lock()
                    .ok()
                    .and_then(|guard| *guard)
                    .is_some_and(|at| at.elapsed() < COOLDOWN);
                if recent {
                    tracing::info!(evidence = %evidence, "auto-gather suppressed by cooldown");
                    let _ = app.emit(
                        "conflux://remote",
                        serde_json::json!({
                            "active": true,
                            "evidence": crate::message::evidence_message(&evidence),
                            "acted": false,
                            "suppressed": "cooldown",
                        }),
                    );
                    return;
                }
                if let Ok(mut slot) = state.last_auto_gather.lock() {
                    *slot = Some(std::time::Instant::now());
                }
            }
            match gather(app, &evidence) {
                Ok(applied) => {
                    crate::tray::record(app, crate::activity::Kind::AutoGather, applied, 0, "auto");
                    tracing::info!(evidence = %evidence, applied, "auto-gather finished");
                    let _ = app.emit(
                        "conflux://remote",
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
                        "conflux://remote",
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
                "conflux://remote",
                serde_json::json!({
                    "active": false,
                    "acted": false,
                }),
            );

            if !settings.auto_restore || paused {
                // Remember that cleanup is owed. The detector reports `Deactivated`
                // exactly once, so without this the session ends silently and the
                // windows stay where the gather put them.
                if paused && settings.auto_restore {
                    state
                        .deferred_restore
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    tracing::info!("remote session ended while paused; restore deferred");
                }
                tracing::info!(
                    auto_restore = settings.auto_restore,
                    paused,
                    "remote session ended; not restoring"
                );
                return;
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
                        "conflux://remote",
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
                        "conflux://remote",
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
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let mut engine = state.engine.lock().map_err(|e| e.to_string())?;

    let target = engine
        .resolve_target(
            settings
                .target_display
                .as_ref()
                .map(|t| t.native_id.as_str()),
        )
        // A sentinel, not a sentence: the frontend maps it to a translated message
        // (`humanError`). A Chinese sentence here reached the user verbatim.
        .ok_or_else(|| "no_displays".to_string())?;

    // `retarget` handles the case where a layout is already active: restoring
    // first keeps the original preimages, so an auto-gather triggered by a second
    // remote tool cannot redefine where the windows "came from".
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
                **o == conflux_domain::WindowOutcome::Restored
                    || **o == conflux_domain::WindowOutcome::FallbackVisible
            })
            .count(),
        pending: report.pending.len(),
    };
    drop(engine);
    crate::tray::refresh(app);
    Ok(summary)
}

struct RestoreSummary {
    restored: usize,
    pending: usize,
    /// A newer request superseded this restore during its wait.
    cancelled: bool,
}

/// Leak-free shutdown hook for the monitor thread.
pub fn stop() {
    STOP.store(true, Ordering::Relaxed);
}
