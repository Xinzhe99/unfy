//! Observe remote-session evidence without waiting for the engine writer.
//!
//! WTS verifies the current RDP session's identity and connection state. RDP only
//! rescues offscreen windows: it never rearranges visible windows or restores a
//! manual transaction. Mirror tools remain candidates until a validated inbound
//! lifecycle can own a transaction and its delayed restore.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, Runtime};
use unfy_detect::{Observation, Sensitivity, Transition, Verdict};

use crate::schedule::{Schedule, ScheduleToken};
use crate::AppState;

static STOP: AtomicBool = AtomicBool::new(false);
// Serialize observations, never writers. An explicit refresh cannot publish an
// older sample after a more recent reconnect/disconnect observation.
static POLL: Mutex<()> = Mutex::new(());

pub fn install<R: Runtime>(app: &AppHandle<R>) {
    STOP.store(false, Ordering::SeqCst);
    apply_sensitivity(app);
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut last = std::time::Instant::now();
        loop {
            std::thread::sleep(unfy_detect::POLL_INTERVAL);
            if STOP.load(Ordering::SeqCst) {
                return;
            }
            if last.elapsed() > unfy_detect::POLL_INTERVAL * 3 {
                invalidate_observation(&handle);
            }
            last = std::time::Instant::now();
            tick(&handle);
        }
    });
}

fn invalidate_observation<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    if let Ok(mut schedule) = state.schedule.lock() {
        schedule.cancel_automatic();
    }
    if let Ok(mut tracker) = state.detector.lock() {
        tracker.observe(&Observation::default());
    };
}

pub fn apply_sensitivity<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let sensitivity = match state.settings.lock() {
        Ok(settings) => Sensitivity::parse(&settings.auto_detect),
        Err(_) => Sensitivity::Off,
    };
    invalidate_observation(app);
    if let Ok(mut tracker) = state.detector.lock() {
        tracker.set_sensitivity(sensitivity);
    };
}

fn observe() -> Observation {
    #[cfg(windows)]
    {
        use unfy_platform::DisplayProvider;
        // This backend is independent of the engine's mutex. A queued writer
        // cannot prevent a reconnect from invalidating its generation.
        match crate::platform::make().topology() {
            Ok(topology) => unfy_detect::platform::observe(&topology),
            Err(_) => Observation::default(),
        }
    }
    #[cfg(not(windows))]
    {
        // Do not construct an AX backend merely to report unsupported detection.
        Observation::default()
    }
}

/// Cancel first, including when the eventual action is disabled or suppressed.
/// A manual action during sampling invalidates admission for this sample.
fn activation_token(schedule: &mut Schedule, sampled: &ScheduleToken) -> Option<ScheduleToken> {
    if !sampled.is_current() {
        return None;
    }
    schedule.cancel_automatic();
    Some(schedule.token())
}

pub fn tick<R: Runtime>(app: &AppHandle<R>) {
    let Ok(_poll) = POLL.try_lock() else { return };
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let sampled = match state.schedule.lock() {
        Ok(schedule) => schedule.token(),
        Err(_) => return,
    };
    let observation = observe();
    let (transition, verdict, session_id, lost_admission) = {
        let Ok(mut tracker) = state.detector.lock() else {
            return;
        };
        let previous = tracker.connected_session();
        let transition = tracker.observe(&observation);
        let session = tracker.connected_session();
        (
            transition,
            tracker.verdict(&observation),
            session,
            previous.is_some() && session.is_none(),
        )
    };
    let token = if matches!(transition, Transition::Activated(_)) {
        let Ok(mut schedule) = state.schedule.lock() else {
            return;
        };
        activation_token(&mut schedule, &sampled)
    } else {
        if lost_admission || matches!(transition, Transition::Deactivated) {
            if let Ok(mut schedule) = state.schedule.lock() {
                schedule.cancel_automatic();
            }
        }
        None
    };

    let summary = unfy_detect::platform::summary(&observation);
    if let Ok(mut last) = state.last_observation.lock() {
        if last.as_deref() != Some(summary.as_str()) {
            tracing::info!(summary = %summary, "remote-session evidence changed");
            *last = Some(summary);
        }
    }
    let _ = app.emit("unfy://remote", serde_json::json!({
        "active": matches!(verdict, Verdict::Connected(_)),
        "candidate": matches!(verdict, Verdict::Candidate(_)),
        "unknown": matches!(verdict, Verdict::Unknown),
        "session_id": session_id,
        "evidence": match &verdict {
            Verdict::Connected(evidence) | Verdict::Candidate(evidence) => Some(crate::message::evidence_message(evidence)),
            _ => None,
        },
        "acted": false,
        "reason": "observation",
    }));

    let (Some(token), Some(session_id)) = (token, session_id) else {
        return;
    };
    let enabled = state
        .settings
        .lock()
        .map(|settings| automatic_enabled(&settings))
        .unwrap_or(false);
    if !enabled || state.maintenance.load(Ordering::SeqCst) || STOP.load(Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        let outcome = rescue_automatic(&handle, &token, session_id);
        match outcome {
            Ok(Some(rescued)) => {
                crate::tray::record(&handle, crate::activity::Kind::Rescue, rescued, 0, "rdp");
                let _ = handle.emit(
                    "unfy://remote",
                    serde_json::json!({
                        "active": true, "session_id": session_id, "acted": true,
                        "rescued": rescued, "reason": "rdp_offscreen_rescue",
                    }),
                );
                crate::tray::refresh(&handle);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, session_id, "automatic RDP rescue failed");
                let _ = handle.emit(
                    "unfy://remote",
                    serde_json::json!({
                        "active": true, "session_id": session_id, "acted": false, "error": error,
                    }),
                );
            }
        }
    });
}

fn automatic_enabled(settings: &unfy_settings::Settings) -> bool {
    Sensitivity::parse(&settings.auto_detect) == Sensitivity::Verified
        && settings.auto_gather
        && !settings.paused
}

fn rescue_automatic<R: Runtime>(
    app: &AppHandle<R>,
    token: &ScheduleToken,
    session_id: u32,
) -> Result<Option<usize>, String> {
    let state = app.state::<AppState>();
    let mut engine = state.engine.lock().map_err(|error| error.to_string())?;
    // All admission checks happen after waiting for the sole writer lock.
    if !token.is_current()
        || STOP.load(Ordering::SeqCst)
        || state.maintenance.load(Ordering::SeqCst)
        || engine.is_paused()
    {
        return Ok(None);
    }
    let recovery = crate::commands::recovery_status(&engine, state.settings_error.is_some());
    if recovery.blockers > 0 || !recovery.transactions.is_empty() {
        return Ok(None);
    }
    // Refresh WTS/lock-screen evidence after acquiring the writer as well. A
    // stalled observer cannot grant permission using a pre-suspend sample.
    let fresh = observe();
    if !fresh.available
        || fresh.rdp_session
            != Some(unfy_detect::RdpSession {
                id: session_id,
                connected: true,
            })
    {
        return Ok(None);
    }
    {
        let settings = state.settings.lock().map_err(|error| error.to_string())?;
        if !automatic_enabled(&settings)
            || crate::commands::resolve_confirmed_target(&engine, &settings).is_err()
        {
            return Ok(None);
        }
    }
    let connected = state
        .detector
        .lock()
        .map_err(|error| error.to_string())?
        .connected_session();
    if connected != Some(session_id)
        || !token.is_current()
        || state.maintenance.load(Ordering::SeqCst)
        || STOP.load(Ordering::SeqCst)
    {
        return Ok(None);
    }
    engine
        .rescue_offscreen()
        .map(|windows| Some(windows.len()))
        .map_err(|error| error.to_string())
}

pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_cancels_before_disabled_or_paused_policy() {
        for paused in [false, true] {
            let mut schedule = Schedule::new();
            let manual = schedule.claim_manual();
            let restore = schedule.claim();
            let sample = schedule.token();
            let next = activation_token(&mut schedule, &sample).unwrap();
            let settings = unfy_settings::Settings {
                paused,
                auto_gather: false,
                ..Default::default()
            };
            assert!(!restore.is_current());
            assert!(manual.is_current());
            assert!(!automatic_enabled(&settings));
            assert!(next.is_current());
        }
    }

    #[test]
    fn manual_action_during_observation_prevents_automatic_admission() {
        let mut schedule = Schedule::new();
        let sample = schedule.token();
        let manual = schedule.claim_manual();
        assert!(activation_token(&mut schedule, &sample).is_none());
        assert!(manual.is_current());
    }

    #[test]
    fn queued_automatic_action_is_cancelled_by_manual_or_pause_request() {
        let mut schedule = Schedule::new();
        let sample = schedule.token();
        let queued = activation_token(&mut schedule, &sample).unwrap();
        schedule.cancel();
        assert!(!queued.is_current());
    }
}
