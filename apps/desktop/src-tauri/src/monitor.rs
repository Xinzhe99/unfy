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

    std::thread::spawn(move || loop {
        if STOP.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(conflux_detect::POLL_INTERVAL);
        tick(&handle);
    });
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
        .load(std::sync::atomic::Ordering::Relaxed);

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
            if let Ok(mut last) = state.last_remote.lock() {
                *last = Some(evidence.label());
            }
            // Publish the verdict even when automation is disabled, so the
            // overview can show "检测到远程会话" and let the user act on it.
            let _ = app.emit(
                "conflux://remote",
                serde_json::json!({
                    "active": true,
                    "evidence": evidence.label(),
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
            match gather(app, &evidence) {
                Ok(applied) => {
                    crate::tray::record(app, crate::activity::Kind::AutoGather, applied, 0, "auto");
                    tracing::info!(evidence = %evidence, applied, "auto-gather finished");
                    let _ = app.emit(
                        "conflux://remote",
                        serde_json::json!({
                            "active": true,
                            "evidence": evidence.label(),
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
                            "evidence": evidence.label(),
                            "acted": false,
                            "error": message,
                        }),
                    );
                }
            }
        }
        Transition::Deactivated => {
            if let Ok(mut last) = state.last_remote.lock() {
                *last = None;
            }
            let _ = app.emit(
                "conflux://remote",
                serde_json::json!({
                    "active": false,
                    "acted": false,
                }),
            );

            if !settings.auto_restore || paused {
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
            match restore(app) {
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
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let mut engine = state.engine.lock().map_err(|e| e.to_string())?;

    let target = engine
        .resolve_target(
            settings
                .target_display
                .as_ref()
                .map(|t| t.native_id.as_str()),
        )
        .ok_or_else(|| "没有可用的显示器".to_string())?;

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
    // Automatic restores ignore the configured delay: that delay exists to absorb
    // a remote session that *drops and reconnects*, and by the time the detector
    // has concluded the session ended, the gap has already been observed for
    // several polls. Waiting again would double it. The delay still applies to
    // the manual button, where a user may press it by accident.
    let state = app.state::<AppState>();
    if let Ok(mut schedule) = state.schedule.lock() {
        schedule.cancel();
    }
    let mut engine = state.engine.lock().map_err(|e| e.to_string())?;
    let report = engine.restore().map_err(|e| e.to_string())?;
    let summary = RestoreSummary {
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
}

/// Leak-free shutdown hook for the monitor thread.
pub fn stop() {
    STOP.store(true, Ordering::Relaxed);
}
