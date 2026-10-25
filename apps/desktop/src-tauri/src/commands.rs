//! Tauri commands. This is the entire surface the frontend can reach.
//!
//! Every variant takes and returns plain data: no handles, no paths, no shell.
//! The frontend cannot ask the engine to touch an arbitrary window id — it can
//! only ask for a preview, a migration, a restore, or a setting change.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tauri_plugin_autostart::ManagerExt;
use unfy_domain::{DisplaySnapshot, Rect, WindowId};
use unfy_engine::{EngineConfig, EngineState, MigrationReport, Preview, RestoreReport};
use unfy_platform::{DisplayProvider, WindowBackend};
use unfy_settings::{Settings, TargetDisplay};

use crate::message::Message;
use crate::{platform, AppState};

/// Captured before queueing; rechecked under the single-writer lock.
struct WriteAdmission {
    maintenance: Arc<std::sync::atomic::AtomicBool>,
    token: crate::schedule::ScheduleToken,
    startup_error: Option<String>,
}

impl WriteAdmission {
    fn claim(app: &AppState) -> Result<Self, String> {
        if in_maintenance(app) {
            return Err("maintenance".into());
        }
        Ok(Self {
            maintenance: app.maintenance.clone(),
            token: app
                .schedule
                .lock()
                .map_err(|error| error.to_string())?
                .claim_manual(),
            startup_error: app.settings_error.clone(),
        })
    }

    fn check(&self) -> Result<(), String> {
        if self.maintenance.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("maintenance".into());
        }
        if !self.token.is_current() {
            return Err("restore_cancelled".into());
        }
        Ok(())
    }
}

fn ensure_settings_writable(app: &AppState) -> Result<(), String> {
    match &app.settings_error {
        Some(error) => Err(error.clone()),
        None => Ok(()),
    }
}

fn commit_settings<B: unfy_engine::Backend>(
    path: &std::path::Path,
    engine: &mut unfy_engine::Engine<B>,
    settings: &mut Settings,
    candidate: Settings,
) -> Result<(), String> {
    candidate.validate().map_err(|error| error.to_string())?;
    let previous = engine.config().clone();
    let mut config = engine_config_from_settings(&candidate);
    config.journal_dir = previous.journal_dir.clone();
    config.on_progress = previous.on_progress.clone();
    engine
        .update_config(config)
        .map_err(|error| error.to_string())?;
    if let Err(error) = candidate.save(path) {
        engine
            .update_config(previous)
            .map_err(|rollback| format!("{error}; {rollback}"))?;
        return Err(error.to_string());
    }
    if candidate.paused {
        engine.pause()
    } else {
        engine.resume()
    }
    .map_err(|error| error.to_string())?;
    *settings = candidate;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct StatusDto {
    pub state: &'static str,
    pub platform: &'static str,
    pub supported: bool,
    /// False on macOS before the user grants Accessibility permission.
    pub has_permission: bool,
    pub permission_name: &'static str,
    pub display_count: usize,
    pub target: Option<TargetDto>,
    pub pending_windows: usize,
    pub recovery_blockers: usize,
    pub last_error: Option<Message>,
    /// Automation paused; the current layout is retained.
    pub paused: bool,
    /// How many windows the open transaction holds.
    pub captured_windows: usize,
    /// The settings file could not be read, so defaults are in effect and the
    /// original is kept as `settings.json.invalid`.
    pub settings_recovered: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct DisplayDto {
    pub native_id: String,
    pub label: String,
    pub width: i32,
    pub height: i32,
    /// Position in the virtual desktop, so the UI can draw the real arrangement.
    pub origin_x: i32,
    pub origin_y: i32,
    pub work_width: i32,
    pub work_height: i32,
    pub scale: f64,
    pub primary: bool,
    /// Whether the identity is strong enough to restore onto without asking.
    pub restorable: bool,
    pub is_target: bool,
    /// 1-based number shown to the user. Matches the badge flashed on the physical
    /// screen. Computed once on the backend so no two surfaces can disagree.
    pub number: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct TargetDto {
    pub native_id: String,
    pub label: String,
    /// False when the saved target is not present in the current topology.
    pub present: bool,
    pub confirmed: bool,
    /// The user-facing number, from the live topology when the display is present
    /// and from the saved value when it is not. `None` only for settings written
    /// before the number was recorded.
    pub number: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct RectDto {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Serialize)]
pub struct PlacementDto {
    pub window_id: String,
    /// Executable name of the owning application, so a rectangle in the preview
    /// can be matched to an app the user recognises. Empty when the platform
    /// could not read it (elevated processes deny the query).
    ///
    /// Never the window title: titles carry document names, URLs and message
    /// previews, and this value ends up in a screenshot-able UI.
    pub app_name: String,
    /// Index of the placement in the plan, 1-based. The preview numbers the
    /// rectangles in the same order, so "窗口 3" always means the same thing in
    /// the drawing and in the list.
    pub order: usize,
    pub rect: RectDto,
    pub clamped: bool,
}

#[derive(Debug, Serialize)]
pub struct PreviewDto {
    /// The layout mode the plan was computed with. The notes under the drawing
    /// mean different things in each mode: overlap is the normal result of moving
    /// windows over, and a "cell" only exists in the grid.
    pub mode: &'static str,
    /// True when the plan targets the display the user configured. False means
    /// the configured display is gone and the plan fell back to the primary one -
    /// the user must see that before pressing "gather", not after.
    pub target_present: bool,
    pub placements: Vec<PlacementDto>,
    pub unplaced: Vec<String>,
    pub skipped: Vec<SkipDto>,
    pub cascaded: bool,
    /// Work area of the target display, so the preview draws real proportions.
    pub target_work: RectDto,
}

#[derive(Debug, Serialize)]
pub struct SkipDto {
    pub window_id: String,
    /// Executable name, so the user can recognise "obs64.exe" but not a
    /// `pid:hwnd` pair. Empty when the platform could not determine it.
    pub app_name: String,
    pub reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct MigrationDto {
    pub transaction: String,
    pub applied: usize,
    pub rejected: usize,
    pub gone: usize,
    pub skipped: usize,
    pub unplaced: usize,
    /// Windows already usable on the target; zero platform writes.
    pub unchanged: usize,
    /// Windows whose preimage could not be captured; zero platform writes.
    pub not_attempted: usize,
    pub cascaded: bool,
    pub state: &'static str,
    /// Names of the applications that refused to move. A pid means nothing to a
    /// user; "Code.exe 没有接受" is something they can act on.
    pub refused_apps: Vec<String>,
    pub results: Vec<WindowResultDto>,
}

#[derive(Debug, Serialize)]
pub struct RestoreDto {
    /// Window counts per result category.
    pub restored: usize,
    pub kept_user_change: usize,
    pub fallback_visible: usize,
    pub rejected: usize,
    pub pending: usize,
    /// Safe but still owing their original position, because their display is gone.
    pub awaiting_original: usize,
    pub state: &'static str,
    /// Applications whose windows could not be restored, for the same reason.
    pub pending_apps: Vec<String>,
    pub results: Vec<WindowResultDto>,
}

#[derive(Debug, Serialize)]
pub struct WindowResultDto {
    pub window_id: String,
    pub app_name: Option<String>,
    pub outcome: &'static str,
    pub reason: Option<&'static str>,
}

fn window_results(
    outcomes: &std::collections::HashMap<WindowId, unfy_domain::WindowOutcome>,
    names: &std::collections::HashMap<u32, String>,
) -> Vec<WindowResultDto> {
    use unfy_domain::WindowOutcome;
    let mut results: Vec<_> = outcomes
        .iter()
        .map(|(id, outcome)| WindowResultDto {
            window_id: window_label(id),
            app_name: names
                .get(&id.process_id)
                .cloned()
                .filter(|name| !name.is_empty()),
            outcome: match outcome {
                WindowOutcome::Applied => "applied",
                WindowOutcome::Restored => "restored",
                WindowOutcome::KeptUserChange => "kept_user_change",
                WindowOutcome::FallbackVisible => "fallback_visible",
                WindowOutcome::Rejected => "rejected",
                WindowOutcome::Gone => "gone",
                WindowOutcome::Skipped => "skipped",
                WindowOutcome::Unknown => "unknown",
            },
            reason: None,
        })
        .collect();
    results.sort_by(|a, b| a.window_id.cmp(&b.window_id));
    results
}

#[derive(Debug, Deserialize)]
pub struct SaveSettingsRequest {
    pub layout_mode: Option<String>,
    pub margin: Option<i32>,
    pub gap: Option<i32>,
    pub autostart: Option<bool>,
    pub theme: Option<String>,
    pub language: Option<String>,
    pub hotkey_gather: Option<String>,
    pub hotkey_restore: Option<String>,
    pub restore_delay_secs: Option<u32>,
    pub auto_update: Option<bool>,
    pub excluded_apps: Option<Vec<String>>,
    pub onboarding_done: Option<bool>,
    pub onboarding_step: Option<u8>,
    pub auto_detect: Option<String>,
    pub auto_gather: Option<bool>,
    pub auto_restore: Option<bool>,
    pub remote_screens: Option<String>,
    pub dismiss_migration_notice: Option<bool>,
}

/// Refresh the tray after a state change so its labels, enabled set and tooltip
/// never disagree with the engine.
fn sync_tray<R: tauri::Runtime>(handle: &tauri::AppHandle<R>) {
    crate::tray::refresh(handle);
}

/// Record the outcome of an operation for the overview banner.
///
/// `None` clears: a success means the previous failure is no longer the current
/// state of the app, and leaving it visible would cry wolf.
fn note_result(app: &State<'_, AppState>, error: Option<Message>) {
    if let Ok(mut slot) = app.last_error.lock() {
        *slot = error;
    }
}

fn state_name(state: EngineState) -> &'static str {
    match state {
        EngineState::Idle => "idle",
        EngineState::Planning => "planning",
        EngineState::Migrating => "migrating",
        EngineState::Active => "active",
        EngineState::Restoring => "restoring",
        EngineState::Degraded => "degraded",
        EngineState::Error => "error",
    }
}

fn display_dto(d: &DisplaySnapshot, target: Option<&TargetDisplay>, number: usize) -> DisplayDto {
    DisplayDto {
        native_id: d.key.native_id.clone(),
        label: d.label.clone(),
        width: d.bounds.width(),
        height: d.bounds.height(),
        origin_x: d.bounds.left,
        origin_y: d.bounds.top,
        work_width: d.work_area.width(),
        work_height: d.work_area.height(),
        scale: d.scale.x,
        primary: d.primary,
        restorable: d.key.quality.is_restorable(),
        is_target: target.is_some_and(|t| t.matches(&d.key)),
        number,
    }
}

fn rect_dto(rect: Rect) -> RectDto {
    RectDto {
        left: rect.left,
        top: rect.top,
        width: rect.width(),
        height: rect.height(),
    }
}

fn window_label(id: &WindowId) -> String {
    format!("{}:{}", id.process_id, id.native_id)
}

/// Whether the settings window is on screen right now.
///
/// The frontend starts when the webview loads, which for an autostart launch is *after*
/// the window was hidden. `document.visibilityState` stays "visible" through a Tauri
/// hide, so the page cannot tell; asking the window is the only reliable answer. Without
/// this the poll started anyway on every login and ran eight commands - including a
/// journal scan and a disk write probe - every four seconds until the user opened the
/// window and closed it again.
#[tauri::command]
pub fn window_visible<R: tauri::Runtime>(window: tauri::WebviewWindow<R>) -> bool {
    window.is_visible().unwrap_or(true)
}

#[tauri::command]
pub fn get_status(app: State<'_, AppState>) -> Result<StatusDto, String> {
    let engine = app.engine.lock().map_err(|e| e.to_string())?;
    let settings = app.settings.lock().map_err(|e| e.to_string())?;

    let topology = engine.backend().topology();
    let topology_error = topology
        .as_ref()
        .err()
        .map(|error| Message::new(error.to_string()));
    let displays = topology.unwrap_or_default();

    // Live number when the display is present, saved number when it is not: a
    // disconnected target still has to be named the way the user last saw it.
    let ordered = unfy_domain::DisplaySnapshot::ordered_by_position(&displays);
    let target = settings.target_display.as_ref().map(|t| {
        let live = ordered
            .iter()
            .position(|d| t.matches(&d.key))
            .map(|index| index + 1);
        TargetDto {
            native_id: t.native_id.clone(),
            label: t.label.clone(),
            present: live.is_some(),
            confirmed: resolve_confirmed_target(&engine, &settings).is_ok(),
            number: live.or(t.number),
        }
    });

    let recovery = recovery_status(&engine, app.settings_error.is_some());
    let last_error = app
        .last_error
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .or_else(|| {
            app.settings_error
                .as_ref()
                .map(|error| Message::new(error.clone()))
        })
        .or(topology_error);

    Ok(StatusDto {
        state: if recovery.blockers > 0 {
            "error"
        } else {
            state_name(engine.state())
        },
        platform: platform::name(),
        supported: platform::is_supported(),
        has_permission: platform::has_window_permission(),
        permission_name: platform::permission_name(),
        display_count: displays.len(),
        target,
        // Counted from the journal, not invented. The tray and the exit barrier
        // both depend on this number, and a hardcoded zero made a degraded state
        // look idle in every surface that read the status.
        pending_windows: recovery.windows,
        recovery_blockers: recovery.blockers,
        // The engine keeps the last failure so a caller can explain it; the
        // status is the natural place to carry it for the overview banner.
        last_error,
        paused: engine.is_paused(),
        captured_windows: engine.captured_window_count(),
        settings_recovered: app.settings_recovered,
    })
}

#[tauri::command]
pub fn list_displays(app: State<'_, AppState>) -> Result<Vec<DisplayDto>, String> {
    let engine = app.engine.lock().map_err(|e| e.to_string())?;
    let settings = app.settings.lock().map_err(|e| e.to_string())?;
    let displays = engine.backend().topology().map_err(|e| e.to_string())?;

    // Numbered here, once, in the same order the on-screen badges use. Every
    // caller that shows a display number - the picker cards, the overview's
    // "target display" line, the onboarding guide - then agrees by construction.
    //
    // The bug this fixes: `list_displays` used to return the platform's own
    // enumeration order while the picker sorted by position, so the overview said
    // "显示器 1" for a monitor the picker labelled "显示器 2".
    let ordered = unfy_domain::DisplaySnapshot::ordered_by_position(&displays);
    Ok(ordered
        .iter()
        .enumerate()
        .map(|(index, d)| display_dto(d, settings.target_display.as_ref(), index + 1))
        .collect())
}

#[tauri::command]
pub fn preview(app: State<'_, AppState>) -> Result<PreviewDto, String> {
    if in_maintenance(&app) {
        return Err("maintenance".to_string());
    }
    let engine = app.engine.lock().map_err(|e| e.to_string())?;
    let settings = app.settings.lock().map_err(|e| e.to_string())?;

    // No target, or the saved target is gone: refuse rather than silently
    // gathering onto the primary display. The remote client may be showing a
    // different screen, so "helpfully" choosing one moves every window somewhere
    // the user cannot see.
    let target = resolve_confirmed_target(&engine, &settings)?;

    let preview: Preview = engine.preview(&target).map_err(|e| e.to_string())?;

    let target_work = engine
        .backend()
        .topology()
        .ok()
        .and_then(|displays| displays.into_iter().find(|d| d.key == target))
        .map(|d| rect_dto(d.work_area))
        .unwrap_or(RectDto {
            left: 0,
            top: 0,
            width: 1920,
            height: 1080,
        });

    // The plan carries window ids only; the names live on the window snapshots.
    // Read them once, while the engine lock is still held.
    let app_names: std::collections::HashMap<u32, String> = engine
        .backend()
        .enumerate()
        .unwrap_or_default()
        .into_iter()
        .map(|w| (w.id.process_id, w.app_name))
        .collect();

    // The preview was computed for a target that the command just resolved, so it
    // is present by construction. `false` is reserved for a future preview that can
    // describe a plan for a display that is no longer attached; claiming `false`
    // here made the window warn "this will use the primary display" even though
    // nothing falls back any more.
    let target_present = true;

    Ok(PreviewDto {
        mode: match preview.plan.mode {
            unfy_layout::LayoutMode::Grid => "grid",
            unfy_layout::LayoutMode::MoveInto => "default",
        },
        target_present,
        placements: preview
            .plan
            .placements
            .iter()
            .enumerate()
            .map(|(index, p)| PlacementDto {
                window_id: window_label(&p.id),
                app_name: app_names.get(&p.id.process_id).cloned().unwrap_or_default(),
                order: index + 1,
                rect: rect_dto(p.rect),
                clamped: p.clamped_to_min,
            })
            .collect(),
        unplaced: preview
            .plan
            .unplaced
            .iter()
            .map(|(id, _)| window_label(id))
            .collect(),
        skipped: preview
            .skipped
            .iter()
            .map(|s| SkipDto {
                window_id: window_label(&s.id),
                app_name: s.app_name.clone(),
                reason: s.reason.code(),
            })
            .collect(),
        cascaded: preview.plan.cascaded,
        target_work,
    })
}

/// Gather every eligible window onto the target display.
///
/// Fails with `already_active` when a transaction is still open. That is not a
/// transient error: the caller must restore first, otherwise the new preimages
/// would record the moved positions as the user's original layout.
#[tauri::command]
pub async fn migrate(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
) -> Result<MigrationDto, String> {
    migrate_with_trigger(&handle, &app, "window").await
}

/// Gather, recording what invoked it.
///
/// The shortcut used to run its own gather: resolve the target, call the engine,
/// write the activity entry. That copy drifted - it never persisted which
/// applications refused to move, so the overview's "exclude this application"
/// offer was missing after a shortcut gather, and a failure reached the log but
/// not `last_error`. One implementation, told what triggered it, cannot drift.
pub async fn migrate_with_trigger<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
    app: &State<'_, AppState>,
    trigger: &'static str,
) -> Result<MigrationDto, String> {
    // Window moves block on each target application's message pump. Running that
    // on the command thread freezes the WebView for the duration, which looks
    // like a hang on a busy desktop. The engine stays behind the same mutex, so
    // moving the work to a blocking thread does not change the single-writer
    // guarantee.
    // A gather supersedes a pending delayed restore: the user just asked for the
    // windows to be gathered, so a timer that would scatter them again must die.
    ensure_settings_writable(app)?;
    let admission = WriteAdmission::claim(app)?;

    let engine_state = app.engine.clone();
    let settings = app.settings.clone();
    let emitter = handle.clone();

    // Announce the start so the UI can show a busy state instead of appearing to
    // ignore the click while the first windows are being captured.
    let _ = tauri::Emitter::emit(
        &emitter,
        "unfy://progress",
        serde_json::json!({ "phase": "migrating", "done": 0, "total": 0 }),
    );

    let activity = app.activity.clone();
    let progress_emitter = emitter.clone();
    let emit: Arc<dyn Fn(usize, usize) + Send + Sync> = Arc::new(move |done, total| {
        let _ = tauri::Emitter::emit(
            &progress_emitter,
            "unfy://progress",
            serde_json::json!({ "phase": "migrating", "done": done, "total": total }),
        );
    });

    let result = tokio::task::spawn_blocking(move || {
        migrate_blocking(engine_state, settings, activity, emit, trigger, admission)
    })
    .await
    .unwrap_or_else(|error| Err(format!("migration task failed: {error}")));

    let _ = tauri::Emitter::emit(
        &emitter,
        "unfy://progress",
        serde_json::json!({ "phase": "done" }),
    );
    note_result(app, result.as_ref().err().cloned().map(Message::new));
    sync_tray(handle);
    result
}

fn migrate_blocking(
    engine: Arc<std::sync::Mutex<unfy_engine::Engine<crate::platform::Backend>>>,
    settings: Arc<std::sync::Mutex<Settings>>,
    activity: Arc<std::sync::Mutex<crate::activity::Activity>>,
    emit: Arc<dyn Fn(usize, usize) + Send + Sync>,
    trigger: &'static str,
    admission: WriteAdmission,
) -> Result<MigrationDto, String> {
    let mut engine = engine.lock().map_err(|e| e.to_string())?;
    admission.check()?;
    let recovery = recovery_status(&engine, admission.startup_error.is_some());
    if recovery.blockers > 0 || !recovery.transactions.is_empty() {
        return Err("recovery_required".into());
    }
    let settings = settings.lock().map_err(|e| e.to_string())?;

    // Install the progress hook for this run. It is replaced rather than
    // accumulated because a stale hook would fire into a dead window after the
    // command returned.
    {
        let mut config = engine.config().clone();
        config.on_progress = Some(emit);
        if let Err(e) = engine.update_config(config) {
            tracing::warn!(error = %e, "could not install the progress hook");
        }
    }

    // No target, or the saved target is gone: refuse rather than silently
    // gathering onto the primary display. The remote client may be showing a
    // different screen, so "helpfully" choosing one moves every window somewhere
    // the user cannot see.
    let target = resolve_confirmed_target(&engine, &settings)?;

    // Replanning preserves the transaction's first preimages.
    let retarget = engine.retarget(&target).map_err(|e| match e {
        unfy_domain::EngineError::Paused => "paused".to_string(),
        unfy_domain::EngineError::RestoreIncomplete { pending } => {
            format!("restore_incomplete:{pending}")
        }
        other => other.to_string(),
    })?;
    let report: MigrationReport = retarget.migration;

    // Record before returning: the user's question ("what did it just do?") is
    // asked immediately after the action, not after a later poll.
    if let Ok(mut log) = activity.lock() {
        log.record(crate::activity::Entry {
            at: crate::activity::now(),
            kind: crate::activity::Kind::Gather,
            affected: report.applied_count(),
            unsettled: report.unfinished() + report.unplaced.len(),
            note: None,
            trigger: trigger.into(),
        });
    }
    let names: std::collections::HashMap<u32, String> = engine
        .backend()
        .enumerate()
        .unwrap_or_default()
        .into_iter()
        .map(|window| (window.id.process_id, window.app_name))
        .collect();
    let mut results = window_results(&report.outcomes, &names);
    for skipped in &report.skipped {
        results.push(WindowResultDto {
            window_id: window_label(&skipped.id),
            app_name: (!skipped.app_name.is_empty()).then(|| skipped.app_name.clone()),
            outcome: "skipped",
            reason: Some(skipped.reason.code()),
        });
    }
    Ok(MigrationDto {
        transaction: report.transaction.to_string(),
        applied: report.applied_count(),
        rejected: report.failed_count(),
        gone: report
            .outcomes
            .values()
            .filter(|o| **o == unfy_domain::WindowOutcome::Gone)
            .count(),
        skipped: report.skipped.len(),
        unplaced: report.unplaced.len(),
        unchanged: report.unchanged,
        not_attempted: report.not_attempted.len(),
        cascaded: report.cascaded,
        state: state_name(engine.state()),
        results,
        refused_apps: {
            let mut apps: Vec<String> = report
                .refused
                .iter()
                .filter_map(|r| names.get(&r.id.process_id).cloned())
                .filter(|name| !name.is_empty())
                .collect();
            apps.sort();
            apps.dedup();
            apps
        },
    })
}

#[tauri::command]
pub async fn restore(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
) -> Result<RestoreDto, String> {
    // The button is an explicit instruction; no delay.
    restore_after_delay(&handle, &app, false).await
}

/// Restore, optionally honouring the configured delay.
///
/// The delay exists to absorb "the remote session dropped and came back": during
/// it a new gather cancels the restore, so the windows stay where they are.
///
/// `honour_delay` is therefore true only for the **automatic** restore that follows
/// a detected session end. A restore the user asked for - the button, the tray, the
/// shortcut, quitting - runs immediately: they are waiting for the app, and a
/// fifteen-second pause looks like a broken button.
pub async fn restore_after_delay<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
    app: &State<'_, AppState>,
    honour_delay: bool,
) -> Result<RestoreDto, String> {
    restore_with_trigger(handle, app, honour_delay, "window").await
}

/// Restore, recording what invoked it.
///
/// The trigger used to be hardcoded while the shortcut and the tray each appended
/// their own history entry afterwards: restoring from either produced two rows, and
/// the shared one claimed the window had done it.
pub async fn restore_with_trigger<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
    app: &State<'_, AppState>,
    honour_delay: bool,
    trigger: &'static str,
) -> Result<RestoreDto, String> {
    let delay = if honour_delay {
        let secs = {
            let settings = app.settings.lock().map_err(|e| e.to_string())?;
            settings.restore_delay_secs
        };
        std::time::Duration::from_secs(u64::from(secs))
    } else {
        std::time::Duration::ZERO
    };

    let admission = if honour_delay {
        WriteAdmission {
            maintenance: app.maintenance.clone(),
            token: app
                .schedule
                .lock()
                .map_err(|error| error.to_string())?
                .claim(),
            startup_error: app.settings_error.clone(),
        }
    } else {
        WriteAdmission::claim(app)?
    };
    let token = admission.token.clone();

    if delay > std::time::Duration::ZERO {
        let seconds = delay.as_secs();
        tracing::info!(seconds, "restore delayed; a new gather will cancel it");
        let _ = tauri::Emitter::emit(
            handle,
            "unfy://progress",
            serde_json::json!({ "phase": "waiting", "seconds": seconds }),
        );
        let waiting = token.clone();
        let cancelled = tokio::task::spawn_blocking(move || {
            !crate::schedule::wait_cancellable(delay, &waiting)
        })
        .await
        .unwrap_or(true);
        if cancelled {
            // A newer request superseded this one - the session came back. Report
            // it as an error so the UI does not print "已恢复 0 个窗口" for a
            // restore that never ran.
            tracing::info!("restore cancelled during its delay by a newer request");
            return Err("restore_cancelled".to_string());
        }
        tracing::info!("restore delay elapsed; restoring now");
    }

    let engine_state = app.engine.clone();
    let settings = app.settings.clone();
    let settings_path = app.settings_path.clone();
    let activity = app.activity.clone();
    let result = tokio::task::spawn_blocking(move || {
        restore_blocking(
            engine_state,
            settings,
            settings_path,
            activity,
            trigger,
            admission,
            None,
        )
    })
    .await
    .unwrap_or_else(|error| Err(format!("restore task failed: {error}")));
    // Close the progress cycle. The gather path has always emitted this; the restore
    // path did not, and the UI sets `busy` from these events. A delayed restore left
    // it saying "正在恢复布局…" - the countdown's final message - with `busy` stuck at
    // the false that "waiting" sets, so the notice never cleared and every button in
    // the row stayed enabled while the windows were already moving back.
    let _ = tauri::Emitter::emit(
        handle,
        "unfy://progress",
        serde_json::json!({ "phase": "done" }),
    );
    note_result(app, result.as_ref().err().cloned().map(Message::new));
    crate::tray::refresh(handle);
    result
}

fn restore_blocking(
    engine: Arc<std::sync::Mutex<unfy_engine::Engine<crate::platform::Backend>>>,
    settings: Arc<std::sync::Mutex<Settings>>,
    settings_path: std::path::PathBuf,
    activity: Arc<std::sync::Mutex<crate::activity::Activity>>,
    trigger: &'static str,
    admission: WriteAdmission,
    recovery_transaction: Option<uuid::Uuid>,
) -> Result<RestoreDto, String> {
    let mut engine = engine.lock().map_err(|e| e.to_string())?;
    admission.check()?;
    if let Some(transaction) = recovery_transaction {
        if engine.current_transaction().is_some() {
            return Err("restore_incomplete".into());
        }
        let path = find_recovery_transaction(&engine.config().journal_dir, transaction)?;
        engine.recover(path).map_err(|error| error.to_string())?;
    } else if engine.current_transaction().is_none() {
        let directory = engine.config().journal_dir.clone();
        engine
            .recover_latest(directory)
            .map_err(|error| error.to_string())?;
    }
    let report: RestoreReport = engine.restore().map_err(|e| e.to_string())?;
    let state = state_name(engine.state());
    let complete = report.is_complete();
    let names: std::collections::HashMap<u32, String> = engine
        .backend()
        .enumerate()
        .unwrap_or_default()
        .into_iter()
        .map(|w| (w.id.process_id, w.app_name))
        .collect();
    let mut pending_apps: Vec<String> = report
        .pending
        .iter()
        .chain(report.awaiting_original.iter())
        .filter_map(|id| names.get(&id.process_id).cloned())
        .filter(|name| !name.is_empty())
        .collect();
    pending_apps.sort();
    pending_apps.dedup();
    drop(engine);

    // A completed restore means there is nothing left to warn about, so an
    // earlier dismissal is stale. Leaving it would suppress the next, unrelated
    // crash banner.
    if complete && admission.startup_error.is_none() {
        if let Ok(mut guard) = settings.lock() {
            let mut candidate = guard.clone();
            clear_recovery_dismissal(&mut candidate);
            if candidate.save(&settings_path).is_ok() {
                *guard = candidate;
            }
        }
    }

    if let Ok(mut log) = activity.lock() {
        log.record(crate::activity::Entry {
            at: crate::activity::now(),
            kind: crate::activity::Kind::Restore,
            affected: report.count(unfy_domain::WindowOutcome::Restored)
                + report.count(unfy_domain::WindowOutcome::FallbackVisible),
            unsettled: report.outstanding(),
            note: None,
            trigger: trigger.into(),
        });
    }

    Ok(RestoreDto {
        restored: report.count(unfy_domain::WindowOutcome::Restored),
        kept_user_change: report.count(unfy_domain::WindowOutcome::KeptUserChange),
        fallback_visible: report.count(unfy_domain::WindowOutcome::FallbackVisible),
        rejected: report.count(unfy_domain::WindowOutcome::Rejected),
        pending: report.pending.len(),
        awaiting_original: report.awaiting_original.len(),
        state,
        pending_apps,
        results: window_results(&report.outcomes, &names),
    })
}

#[tauri::command]
pub fn rescue_offscreen(app: State<'_, AppState>) -> Result<usize, String> {
    ensure_settings_writable(&app)?;
    let admission = WriteAdmission::claim(&app)?;
    let rescued = {
        let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
        admission.check()?;
        let recovery = recovery_status(&engine, false);
        if recovery.blockers > 0 || !recovery.transactions.is_empty() {
            return Err("recovery_required".into());
        }
        engine.rescue_offscreen().map_err(|e| e.to_string())?
    };
    // Rescue moves real windows, so it belongs in the history for the same reason
    // gather and restore do: "why did my window jump?" must have an answer.
    if !rescued.is_empty() {
        if let Ok(mut log) = app.activity.lock() {
            log.record(crate::activity::Entry {
                at: crate::activity::now(),
                kind: crate::activity::Kind::Rescue,
                affected: rescued.len(),
                unsettled: 0,
                note: None,
                trigger: "window".into(),
            });
        }
    }
    Ok(rescued.len())
}

#[tauri::command]
pub fn get_settings<'a>(
    app: State<'a, AppState>,
    handle: tauri::AppHandle,
) -> Result<SettingsDto, String> {
    settings_dto(&app, &handle)
}

/// Build the settings payload, including the live detector verdict.
///
/// The detector state is not part of `Settings`, so it is attached here rather
/// than stored: persisting "a remote session is happening" would make a crash
/// during a session look like a permanent one on the next launch.
fn settings_dto(
    app: &State<'_, AppState>,
    handle: &tauri::AppHandle,
) -> Result<SettingsDto, String> {
    let mut dto = {
        let engine = app.engine.lock().map_err(|error| error.to_string())?;
        let settings = app.settings.lock().map_err(|error| error.to_string())?;
        let mut dto = SettingsDto::from(&*settings);
        if let Some(target) = dto.target.as_mut() {
            target.confirmed = resolve_confirmed_target(&engine, &settings).is_ok();
            if let Ok(displays) = engine.backend().topology() {
                let ordered = unfy_domain::DisplaySnapshot::ordered_by_position(&displays);
                let number = ordered
                    .iter()
                    .position(|display| display.key.native_id == target.native_id)
                    .map(|index| index + 1);
                target.present = number.is_some();
                target.number = number.or(target.number);
            }
        }
        dto
    };
    if let Ok(tracker) = app.detector.lock() {
        dto.remote_active = tracker.connected_session().is_some();
        dto.remote_unknown = tracker.is_unknown();
        dto.remote_evidence = tracker.evidence().map(crate::message::evidence_message);
    }
    match handle.autolaunch().is_enabled() {
        Ok(actual) => dto.autostart_actual = Some(actual),
        Err(error) => dto.autostart_error = Some(error.to_string()),
    }
    Ok(dto)
}

#[derive(Debug, Serialize)]
pub struct SettingsDto {
    pub layout_mode: String,
    pub margin: i32,
    pub gap: i32,
    pub autostart: bool,
    pub autostart_actual: Option<bool>,
    pub autostart_error: Option<String>,
    pub migration_notice: bool,
    pub theme: String,
    pub language: String,
    pub hotkey_gather: Option<String>,
    pub hotkey_restore: Option<String>,
    pub restore_delay_secs: u32,
    pub auto_update: bool,
    pub excluded_apps: Vec<String>,
    pub onboarding_done: bool,
    pub onboarding_step: u8,
    pub auto_detect: String,
    pub auto_gather: bool,
    pub auto_restore: bool,
    /// `single` or `all`: what the remote client can display.
    pub remote_screens: String,
    /// The saved display and whether it is currently confirmed.
    pub target: Option<TargetDto>,
    /// What the detector currently believes, and why. Independent of whether
    /// automation is enabled: the overview shows the evidence either way.
    pub remote_active: bool,
    pub remote_unknown: bool,
    pub remote_evidence: Option<Message>,
}

impl From<&Settings> for SettingsDto {
    fn from(s: &Settings) -> Self {
        Self {
            layout_mode: match s.layout_mode {
                unfy_settings::LayoutModeName::Grid => "grid",
                unfy_settings::LayoutModeName::MoveInto => "default",
            }
            .into(),
            margin: s.margin,
            gap: s.gap,
            autostart: s.autostart,
            autostart_actual: None,
            autostart_error: None,
            migration_notice: s.migration_notice,
            language: match s.language {
                unfy_settings::LanguageName::System => "system",
                unfy_settings::LanguageName::Zh => "zh",
                unfy_settings::LanguageName::En => "en",
            }
            .into(),
            theme: match s.theme {
                unfy_settings::ThemeName::System => "system",
                unfy_settings::ThemeName::Light => "light",
                unfy_settings::ThemeName::Dark => "dark",
            }
            .into(),
            hotkey_gather: s.hotkey_gather.clone(),
            hotkey_restore: s.hotkey_restore.clone(),
            restore_delay_secs: s.restore_delay_secs,
            auto_update: s.auto_update,
            excluded_apps: s.excluded_apps.clone(),
            auto_detect: s.auto_detect.clone(),
            auto_gather: s.auto_gather,
            auto_restore: s.auto_restore,
            remote_screens: match s.remote_screens {
                unfy_settings::RemoteScreens::Single => "single".into(),
                unfy_settings::RemoteScreens::All => "all".into(),
            },
            // Filled in by `settings_dto`, which has access to the detector.
            remote_active: false,
            remote_unknown: false,
            remote_evidence: None,
            onboarding_done: s.onboarding_done,
            onboarding_step: s.onboarding_step,
            // `present` and the live number need the topology, which this `From`
            // impl cannot reach; `settings_dto` fills both in. Claiming
            // `present: true` here was a second hardcoded truth that made a
            // disconnected display look available.
            target: s.target_display.as_ref().map(|t| TargetDto {
                native_id: t.native_id.clone(),
                label: t.label.clone(),
                present: false,
                confirmed: false,
                number: t.number,
            }),
        }
    }
}

/// Whether new window work is currently forbidden.
///
/// The flag is set before exit and before installing an update. It is checked by
/// every entry point that can start a write, because "the user asked to quit" and
/// "a hotkey fired a millisecond later" must not both run.
pub fn in_maintenance(app: &AppState) -> bool {
    app.maintenance.load(std::sync::atomic::Ordering::SeqCst)
}

pub(crate) struct MaintenanceGuard {
    flag: Arc<std::sync::atomic::AtomicBool>,
    release: bool,
}

impl MaintenanceGuard {
    pub(crate) fn acquire(app: &AppState) -> Result<Self, String> {
        use std::sync::atomic::Ordering;
        app.maintenance
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "maintenance".to_string())?;
        let guard = Self {
            flag: app.maintenance.clone(),
            release: true,
        };
        app.schedule
            .lock()
            .map_err(|error| error.to_string())?
            .cancel();
        Ok(guard)
    }

    pub(crate) fn keep_closed(mut self) {
        self.release = false;
    }
}

impl Drop for MaintenanceGuard {
    fn drop(&mut self) {
        if self.release {
            self.flag.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// Resolve the display the user confirmed, or explain what is missing.
pub(crate) fn resolve_confirmed_target(
    engine: &unfy_engine::Engine<crate::platform::Backend>,
    settings: &Settings,
) -> Result<unfy_domain::DisplayKey, String> {
    let Some(saved) = settings.target_display.as_ref() else {
        return Err("no_target".to_string());
    };
    let live = engine
        .resolve_target(Some(saved.native_id.as_str()))
        .ok_or_else(|| "target_missing".to_string())?;
    if !(saved.quality.is_restorable() && live.quality.is_restorable()) {
        let signature = engine.topology_signature();
        if saved.confirmed_topology.is_none() || saved.confirmed_topology != signature {
            return Err("target_unconfirmed".into());
        }
    }
    Ok(live)
}

/// Convert the persisted settings into the engine configuration.
///
/// This is the only place that decides what the engine reads. Startup, a save and
/// a reset all call it, so the three can never disagree - which they did: a reset
/// wrote "grid, margin 12, delay 0" into the live engine while the saved file said
/// "default, delay 15", and the next launch silently changed behaviour.
pub fn engine_config_from_settings(settings: &Settings) -> EngineConfig {
    let mut layout = unfy_layout::LayoutConfig {
        mode: settings.layout_mode.into(),
        margin: settings.margin,
        gap: settings.gap,
        ..Default::default()
    };
    // The move-in mode ignores these two, but keeping them in the config lets the
    // explicit grid honour the same document.
    layout.source_bounds = None;
    EngineConfig {
        layout,
        journal_dir: unfy_settings::config_dir().join("journal"),
        restore_delay_secs: settings.restore_delay_secs,
        excluded_apps: settings.excluded_apps.clone(),
        ..Default::default()
    }
}

#[tauri::command]
pub fn save_settings(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
    request: SaveSettingsRequest,
) -> Result<SettingsDto, String> {
    ensure_settings_writable(&app)?;
    let mut engine = app.engine.lock().map_err(|error| error.to_string())?;
    if in_maintenance(&app) {
        return Err("maintenance".into());
    }

    let mut settings = app.settings.lock().map_err(|e| e.to_string())?;

    // Merge into a copy. A rejected value must leave the live settings - and the
    // file - exactly as they were; mutating in place and returning an error
    // halfway through would keep the fields that happened to be applied first.
    let mut candidate = settings.clone();
    if request.dismiss_migration_notice == Some(true) {
        candidate.migration_notice = false;
    }

    if let Some(mode) = request.layout_mode {
        candidate.layout_mode = match mode.as_str() {
            "grid" => unfy_settings::LayoutModeName::Grid,
            "default" => unfy_settings::LayoutModeName::MoveInto,
            other => return Err(format!("unknown layout mode: {other}")),
        };
    }
    if let Some(margin) = request.margin {
        candidate.margin = margin;
    }
    if let Some(gap) = request.gap {
        candidate.gap = gap;
    }
    if let Some(autostart) = request.autostart {
        candidate.autostart = autostart;
    }
    let autostart_changed = request.autostart;
    let hotkeys_changed = request.hotkey_gather.is_some() || request.hotkey_restore.is_some();
    if let Some(language) = request.language {
        candidate.language = match language.as_str() {
            "system" => unfy_settings::LanguageName::System,
            "zh" => unfy_settings::LanguageName::Zh,
            "en" => unfy_settings::LanguageName::En,
            other => return Err(format!("unknown language: {other}")),
        };
    }
    if let Some(theme) = request.theme {
        candidate.theme = match theme.as_str() {
            "system" => unfy_settings::ThemeName::System,
            "light" => unfy_settings::ThemeName::Light,
            "dark" => unfy_settings::ThemeName::Dark,
            other => return Err(format!("unknown theme: {other}")),
        };
    }
    if let Some(hotkey) = request.hotkey_gather {
        candidate.hotkey_gather = if hotkey.is_empty() {
            None
        } else {
            Some(hotkey)
        };
    }
    if let Some(hotkey) = request.hotkey_restore {
        candidate.hotkey_restore = if hotkey.is_empty() {
            None
        } else {
            Some(hotkey)
        };
    }
    if let Some(delay) = request.restore_delay_secs {
        candidate.restore_delay_secs = delay;
    }
    if let Some(auto_update) = request.auto_update {
        candidate.auto_update = auto_update;
    }
    if let Some(level) = request.auto_detect {
        if !matches!(level.as_str(), "off" | "verified") {
            return Err(format!("unknown detection level: {level}"));
        }
        candidate.auto_detect = level;
    }
    // Turning automatic gathering off does not move anything. The old code ran a
    // restore here, which meant flipping a preference could rearrange the user's
    // desktop; the spec is explicit that stopping automation only stops future
    // automatic actions.
    if let Some(auto_gather) = request.auto_gather {
        candidate.auto_gather = auto_gather;
    }
    if let Some(auto_restore) = request.auto_restore {
        candidate.auto_restore = auto_restore;
    }
    if let Some(screens) = request.remote_screens {
        candidate.remote_screens = match screens.as_str() {
            "single" => unfy_settings::RemoteScreens::Single,
            "all" => unfy_settings::RemoteScreens::All,
            other => return Err(format!("unknown remote_screens: {other}")),
        };
    }
    if candidate.remote_screens == unfy_settings::RemoteScreens::All {
        candidate.auto_detect = "off".into();
        candidate.auto_gather = false;
        candidate.auto_restore = false;
    }
    if let Some(excluded_apps) = request.excluded_apps {
        candidate.excluded_apps = excluded_apps;
    }
    if let Some(onboarding_done) = request.onboarding_done {
        candidate.onboarding_done = onboarding_done;
    }
    if let Some(onboarding_step) = request.onboarding_step {
        candidate.onboarding_step = onboarding_step.min(4);
    }

    let invalidate_automation = candidate.paused != settings.paused
        || candidate.auto_detect != settings.auto_detect
        || candidate.auto_gather != settings.auto_gather
        || candidate.auto_restore != settings.auto_restore
        || candidate.remote_screens != settings.remote_screens;
    if invalidate_automation {
        app.schedule
            .lock()
            .map_err(|error| error.to_string())?
            .cancel();
    }
    commit_settings(&app.settings_path, &mut engine, &mut settings, candidate)?;
    let autostart = settings.autostart;
    drop(settings);
    drop(engine);

    // OS side effects happen after the document is committed. A registration
    // failure is reported, not turned into "settings not saved".
    if autostart_changed.is_some() {
        if let Err(e) = crate::session::set_autostart(&handle, autostart) {
            // The preference is saved but the OS did not accept it. Say so, and
            // keep the saved value so the next launch can retry.
            note_result(
                &app,
                Some(Message::one(
                    "开机自启未能设置：{error}",
                    "error",
                    e.to_string(),
                )),
            );
        }
    }
    if hotkeys_changed {
        crate::session::install(&handle);
    }

    crate::monitor::apply_sensitivity(&handle);
    sync_tray(&handle);
    settings_dto(&app, &handle)
}

#[tauri::command]
pub fn set_target_display(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
    native_id: String,
) -> Result<(), String> {
    ensure_settings_writable(&app)?;
    let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
    if in_maintenance(&app) {
        return Err("maintenance".into());
    }
    let mut settings = app.settings.lock().map_err(|e| e.to_string())?;

    let displays = engine.backend().topology().map_err(|e| e.to_string())?;
    // Numbered the same way the picker and the on-screen badges are, so the stored
    // number keeps meaning the same thing after the display is unplugged.
    let ordered = unfy_domain::DisplaySnapshot::ordered_by_position(&displays);
    let position = ordered
        .iter()
        .position(|d| d.key.native_id == native_id)
        .ok_or_else(|| "display_not_found".to_string())?;
    let display = ordered[position];

    let mut target = TargetDisplay::from_key(&display.key, display.label.clone());
    target.number = Some(position + 1);
    target.confirmed_topology = engine.topology_signature();
    let mut candidate = settings.clone();
    candidate.target_display = Some(target);
    commit_settings(&app.settings_path, &mut engine, &mut settings, candidate)?;
    app.schedule
        .lock()
        .map_err(|error| error.to_string())?
        .cancel();
    drop(settings);
    drop(engine);
    sync_tray(&handle);
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct PendingWorkDto {
    pub present: bool,
    pub windows: usize,
    pub may_have_moved: bool,
    pub truncated_bytes: u64,
    pub blockers: usize,
    pub transactions: Vec<RecoveryTransactionDto>,
    /// The unresolved windows belong to a transaction this process opened and is
    /// still holding, not to a crashed run. The count is the same; the warning is
    /// not. A successful gather leaves the live layout unresolved by design.
    pub live: bool,
}

#[derive(Debug, Serialize)]
pub struct RecoveryTransactionDto {
    pub id: String,
    pub windows: usize,
}

/// One read-only view shared by status, recovery UI and lifecycle barriers.
pub(crate) fn recovery_status(
    engine: &unfy_engine::Engine<crate::platform::Backend>,
    startup_blocked: bool,
) -> PendingWorkDto {
    let mut status = scan_recovery(
        &engine.config().journal_dir,
        engine.current_transaction(),
        engine.unresolved_window_count(),
    );
    status.blockers += usize::from(startup_blocked);
    status.present |= startup_blocked;
    // `scan_recovery` sees only counts. Two distinctions live in the engine: a
    // transaction this run opened is the layout on screen, and an adopted one only
    // warrants a crash warning when it stopped between submitting a write and
    // reading it back. A journal whose writes all completed is a layout the user
    // closed the app on - normal, and not something to call "unfinished".
    status.live = engine.current_transaction().is_some()
        && (!engine.adopted_transaction() || !engine.interrupted_write());
    status
}

fn journal_paths(directory: &std::path::Path) -> (Vec<std::path::PathBuf>, usize) {
    let mut directories = vec![directory.to_path_buf()];
    let mut files = Vec::new();
    let mut blockers = 0;
    while let Some(current) = directories.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) if current == directory && error.kind() == std::io::ErrorKind::NotFound => {
                continue
            }
            Err(_) => {
                blockers += 1;
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    blockers += 1;
                    continue;
                }
            };
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(_) => {
                    blockers += 1;
                    continue;
                }
            };
            if kind.is_symlink() {
                blockers += 1;
            } else if kind.is_dir() {
                directories.push(entry.path());
            } else if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "journal")
            {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    (files, blockers)
}

fn find_recovery_transaction(
    directory: &std::path::Path,
    transaction: uuid::Uuid,
) -> Result<std::path::PathBuf, String> {
    let (paths, blockers) = journal_paths(directory);
    if blockers > 0 {
        return Err("recovery_required".into());
    }
    let mut matched = None;
    for path in paths {
        if let Ok(unfy_journal::Recovered::Unfinished(recovered)) = unfy_journal::recover(&path) {
            if recovered.transaction == transaction && recovered.has_open_obligation() {
                if matched.is_some() {
                    return Err("recovery_required".into());
                }
                matched = Some(path);
            }
        }
    }
    matched.ok_or_else(|| "recovery_record_missing".into())
}

fn scan_recovery(
    directory: &std::path::Path,
    owned: Option<uuid::Uuid>,
    in_memory: usize,
) -> PendingWorkDto {
    let mut status = PendingWorkDto {
        present: in_memory > 0,
        windows: in_memory,
        may_have_moved: in_memory > 0,
        truncated_bytes: 0,
        blockers: 0,
        transactions: Vec::new(),
        live: false,
    };
    let (paths, blockers) = journal_paths(directory);
    status.blockers = blockers;
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        match unfy_journal::recover(&path) {
            Ok(unfy_journal::Recovered::Clean) => {}
            Ok(unfy_journal::Recovered::Unfinished(transaction)) => {
                if !seen.insert(transaction.transaction) {
                    status.blockers += 1;
                    continue;
                }
                if Some(transaction.transaction) == owned {
                    continue;
                }
                status.truncated_bytes += transaction.truncated_bytes;
                if !transaction.has_open_obligation() {
                    continue;
                }
                let windows = transaction.touched_windows().len();
                status.windows += windows;
                status.may_have_moved |=
                    transaction.may_have_moved_windows() || transaction.has_unresolved_intent();
                status.transactions.push(RecoveryTransactionDto {
                    id: transaction.transaction.to_string(),
                    windows,
                });
            }
            Err(_) => status.blockers += 1,
        }
    }
    status.transactions.sort_by(|a, b| a.id.cmp(&b.id));
    status.present = status.windows > 0 || status.blockers > 0;
    status
}

/// Report whether there is unfinished recovery work.
///
/// Two sources count, and they must not be double-counted:
///
/// 1. The engine adopted a journal at startup and is holding its preimages in
///    memory. It can restore them; the UI must say so.
/// 2. A journal file exists that the engine does not own (for example, written
///    by another user session, or unreadable when the engine started).
///
/// A journal that belongs to the migration currently in flight is neither: it is
/// normal work in progress, and showing a warning banner for it would be wrong.
#[tauri::command]
pub fn pending_work(app: State<'_, AppState>) -> Result<PendingWorkDto, String> {
    let engine = app.engine.lock().map_err(|error| error.to_string())?;
    let mut status = recovery_status(&engine, app.settings_error.is_some());
    let settings = app.settings.lock().map_err(|error| error.to_string())?;
    // Dismissal hides this reminder only; get_status and lifecycle checks always
    // retain the real counts, including unreadable records.
    if settings.recovery_dismissed_until > now_unix_secs() && status.blockers == 0 {
        status.present = false;
    }
    Ok(status)
}

#[tauri::command]
pub async fn restore_recovery_transaction(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
    transaction: String,
) -> Result<RestoreDto, String> {
    let transaction = uuid::Uuid::parse_str(&transaction).map_err(|error| error.to_string())?;
    let admission = WriteAdmission::claim(&app)?;
    let engine = app.engine.clone();
    let settings = app.settings.clone();
    let path = app.settings_path.clone();
    let activity = app.activity.clone();
    let result = tokio::task::spawn_blocking(move || {
        restore_blocking(
            engine,
            settings,
            path,
            activity,
            "window",
            admission,
            Some(transaction),
        )
    })
    .await
    .unwrap_or_else(|error| Err(format!("restore task failed: {error}")));
    note_result(&app, result.as_ref().err().cloned().map(Message::new));
    sync_tray(&handle);
    result
}

#[tauri::command]
pub fn cancel_auto_restore(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
) -> Result<(), String> {
    app.schedule
        .lock()
        .map_err(|error| error.to_string())?
        .cancel_automatic();
    let _ = tauri::Emitter::emit(
        &handle,
        "unfy://progress",
        serde_json::json!({ "phase": "done" }),
    );
    Ok(())
}

/// Hide the recovery banner for a while, without discarding the journal.
///
/// Dismissing is a UI preference, not a decision about the windows: the journal
/// stays on disk and `restore` still works. It expires after a day so a later,
/// unrelated crash is reported again.
#[tauri::command]
pub fn dismiss_pending_work(app: State<'_, AppState>) -> Result<(), String> {
    ensure_settings_writable(&app)?;
    let mut settings = app.settings.lock().map_err(|e| e.to_string())?;
    let mut candidate = settings.clone();
    candidate.recovery_dismissed_until = now_unix_secs() + 24 * 60 * 60;
    candidate
        .save(&app.settings_path)
        .map_err(|e| e.to_string())?;
    *settings = candidate;
    Ok(())
}

/// Clear the dismissal, for example after the user actually restores.
fn clear_recovery_dismissal(settings: &mut unfy_settings::Settings) {
    settings.recovery_dismissed_until = 0;
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Serialize)]
pub struct DiagnosticsDto {
    pub path: String,
    pub bytes: u64,
}

/// Write a redacted diagnostics bundle: versions, topology shape, and setting
/// names — never window titles, paths, or screen content.
#[tauri::command]
pub fn health_check(app: State<'_, AppState>) -> Vec<HealthItemDto> {
    let mut items: Vec<HealthItemDto> = Vec::new();

    // 1. Window control permission. The one failure that makes the product a
    //    no-op while the window still opens normally.
    let permitted = platform::has_window_permission();
    items.push(HealthItemDto {
        id: "permission".into(),
        label: Message::new("窗口控制权限"),
        status: if permitted { "ok" } else { "problem" }.into(),
        detail: if permitted {
            Message::new("可以移动其他应用的窗口。")
        } else {
            // The permission name is itself a catalogued sentence, so it travels as
            // a parameter and the interface translates it on substitution.
            Message::one(
                "缺少「{name}」授权，整理不会生效。",
                "name",
                platform::permission_name(),
            )
        },
    });

    // 2. Displays. Zero is a real fault; one is a legitimate configuration that
    //    makes gathering meaningless, which the user should hear as a fact.
    let displays = app
        .engine
        .lock()
        .ok()
        .and_then(|engine| unfy_platform::DisplayProvider::topology(engine.backend()).ok())
        .unwrap_or_default();
    items.push(HealthItemDto {
        id: "displays".into(),
        label: Message::new("显示器"),
        status: match displays.len() {
            0 => "problem",
            1 => "attention",
            _ => "ok",
        }
        .into(),
        detail: match displays.len() {
            0 => Message::new("没有检测到显示器。"),
            1 => Message::new("只有一块显示器，整理没有可移动的目标。"),
            count => Message::one("检测到 {count} 块显示器。", "count", count.to_string()),
        },
    });

    // 3. Hotkeys. A combination that failed to register is invisible until the
    //    user presses it and nothing happens.
    if let Ok(status) = app.hotkey_status.lock() {
        let failures: Vec<Message> = [&status.gather_error, &status.restore_error]
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        let configured = status.gather.is_some() || status.restore.is_some();
        items.push(HealthItemDto {
            id: "hotkeys".into(),
            label: Message::new("全局快捷键"),
            status: if failures.is_empty() { "ok" } else { "problem" }.into(),
            detail: if let Some(first) = failures.first() {
                // One line, so the first failure only. The settings page lists every
                // registered combination, which is where the rest belong.
                first.clone()
            } else if configured {
                Message::new("已注册，在其他应用中也生效。")
            } else {
                Message::new("未设置。可通过托盘或本窗口触发。")
            },
        });
    }

    // 4. Log directory. If it is unwritable, the next problem will leave no
    //    evidence, and the support flow starts from nothing.
    //
    //    Probed at most once every ten minutes. The probe creates and deletes a
    //    file, and `health_check` is polled every four seconds while the window is
    //    open: running it each time was constant disk churn for a fact that changes
    //    approximately never. A failure is remembered for the same window, so a
    //    recovered disk shows up within ten minutes rather than instantly.
    let dir = unfy_settings::config_dir();
    let writable = {
        const TTL: std::time::Duration = std::time::Duration::from_secs(600);
        let cached = app
            .storage_probe
            .lock()
            .ok()
            .and_then(|guard| *guard)
            .filter(|(at, _)| at.elapsed() < TTL)
            .map(|(_, ok)| ok);
        match cached {
            Some(ok) => ok,
            None => {
                let ok = std::fs::create_dir_all(&dir)
                    .and_then(|_| {
                        let probe = dir.join(".write-probe");
                        std::fs::write(&probe, b"ok")?;
                        std::fs::remove_file(&probe)
                    })
                    .is_ok();
                if let Ok(mut guard) = app.storage_probe.lock() {
                    *guard = Some((std::time::Instant::now(), ok));
                }
                ok
            }
        }
    };
    items.push(HealthItemDto {
        id: "storage".into(),
        label: Message::new("配置目录"),
        status: if writable { "ok" } else { "problem" }.into(),
        detail: if writable {
            Message::one("可写入 {path}", "path", dir.display().to_string())
        } else {
            Message::one(
                "无法写入 {path}：设置和日志都不会保存。",
                "path",
                dir.display().to_string(),
            )
        },
    });

    // 5. An unfinished transaction from a previous run. Not a fault, but the
    //    single most important thing for the user to resolve.
    if let Ok(engine) = app.engine.lock() {
        if engine.state().has_pending_work() {
            items.push(HealthItemDto {
                id: "pending".into(),
                label: Message::new("未完成的整理"),
                status: "attention".into(),
                detail: Message::one(
                    "有 {count} 个窗口的快照等待恢复。",
                    "count",
                    engine.captured_window_count().to_string(),
                ),
            });
        }
    }

    items
}

#[derive(Debug, Serialize)]
pub struct HealthItemDto {
    pub id: String,
    /// The row's title, as a sentence for the interface to word.
    pub label: Message,
    /// `ok`, `attention`, or `problem`.
    pub status: String,
    /// One line explaining the status, in the same form.
    pub detail: Message,
}

#[tauri::command]
pub fn export_diagnostics(app: State<'_, AppState>) -> Result<DiagnosticsDto, String> {
    let report = diagnostics_report(&app)?;
    let dir = unfy_settings::config_dir().join("diagnostics");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("unfy-{}.json", uuid::Uuid::new_v4()));
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, &text).map_err(|e| e.to_string())?;

    Ok(DiagnosticsDto {
        path: path.to_string_lossy().to_string(),
        bytes: text.len() as u64,
    })
}

/// Show exactly what a diagnostics bundle would contain, without writing it.
///
/// Spec P0-15-A: the user must be able to see the scope before exporting. A
/// description of the contents is not the same thing - the only way to be sure no
/// window title slipped in is to read the payload, and this renders the same
/// function the export uses so the two cannot drift.
#[tauri::command]
pub fn preview_diagnostics(app: State<'_, AppState>) -> Result<String, String> {
    let report = diagnostics_report(&app)?;
    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
}

fn diagnostics_report(app: &State<'_, AppState>) -> Result<serde_json::Value, String> {
    let engine = app.engine.lock().map_err(|e| e.to_string())?;
    let settings = app.settings.lock().map_err(|e| e.to_string())?;

    let displays = engine.backend().topology().map_err(|e| e.to_string())?;

    let mut report = serde_json::json!({
        "app_version": env!("CARGO_PKG_VERSION"),
        "platform": platform::name(),
        "supported": platform::is_supported(),
        "state": state_name(engine.state()),
        "display_count": displays.len(),
        "displays": displays.iter().map(|d| serde_json::json!({
            "width": d.bounds.width(),
            "height": d.bounds.height(),
            "scale": d.scale.x,
            "primary": d.primary,
            // The identity is hashed so support can correlate reports without
            // learning the user's monitor model and serial.
            "id_digest": digest(&d.key.native_id),
            "identity_quality": format!("{:?}", d.key.quality).to_lowercase(),
        })).collect::<Vec<_>>(),
        "settings": {
            "layout_mode": format!("{:?}", settings.layout_mode).to_lowercase(),
            "margin": settings.margin,
            "gap": settings.gap,
            "autostart": settings.autostart,
            "restore_delay_secs": settings.restore_delay_secs,
            "has_target": settings.target_display.is_some(),
            "has_gather_hotkey": settings.hotkey_gather.is_some(),
            "has_restore_hotkey": settings.hotkey_restore.is_some(),
            // Detection settings decide whether the app acts on its own; a bug
            // report without them cannot be reasoned about.
            "auto_detect": settings.auto_detect,
            "auto_gather": settings.auto_gather,
            "auto_restore": settings.auto_restore,
        },
    });

    // Which evidence the detector can see right now, and which it concluded.
    // Tool *names* are product names, not user data; they are already visible in
    // the process list, and without them "detection did not fire" is untriageable.
    if let Ok(tracker) = app.detector.lock() {
        if let Some(object) = report.as_object_mut() {
            object.insert(
                "detection".into(),
                serde_json::json!({
                    "active": tracker.is_active(),
                    "evidence": tracker.evidence().map(crate::message::evidence_message),
                    "last_observation": app.last_observation.lock().ok().and_then(|v| v.clone()),
                }),
            );
        }
    }

    // The recent action history. A report saying "it moved my windows" is
    // untriageable without knowing what it actually did and when; the entries
    // carry counts, not window titles.
    if let Ok(log) = app.activity.lock() {
        if let Some(object) = report.as_object_mut() {
            object.insert(
                "activity".into(),
                serde_json::to_value(
                    log.entries()
                        .iter()
                        .rev()
                        .take(50)
                        .map(|e| {
                            serde_json::json!({
                                "at": e.at,
                                "kind": e.kind.code(),
                                "affected": e.affected,
                                "unsettled": e.unsettled,
                                "trigger": e.trigger,
                            })
                        })
                        .collect::<Vec<_>>(),
                )
                .unwrap_or(serde_json::Value::Null),
            );
        }
    }

    // Both guards must go before `pending_work` runs: it takes the engine and settings
    // locks itself, and a `Mutex` is not reentrant - calling it while they are held
    // deadlocked the whole app the first time this was written.
    drop(engine);
    drop(settings);

    // Journal completeness. This is the first question when a user reports that a
    // window did not come back: a torn tail means the record of its original rectangle
    // was never written, and no amount of retrying can fix that.
    if let Ok(pending) = pending_work(app.clone()) {
        if let Some(object) = report.as_object_mut() {
            object.insert(
                "journal".into(),
                serde_json::json!({
                    "unfinished": pending.present,
                    "windows": pending.windows,
                    "may_have_moved": pending.may_have_moved,
                    "truncated_bytes": pending.truncated_bytes,
                }),
            );
        }
    }

    Ok(report)
}

/// Non-reversible digest used to correlate display identity without exposing it.
fn digest(input: &str) -> String {
    // FNV-1a: not cryptographic, but sufficient to avoid storing the raw device
    // path and stable across runs.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Open the OS permission prompt for window control.
///
/// Returns the permission state before the user answers; the UI polls
/// `get_status` afterwards because macOS grants the permission asynchronously.
#[tauri::command]
pub fn request_permission() -> bool {
    platform::request_window_permission()
}

/// Open the OS settings pane for window-control permission.
///
/// macOS only shows its permission prompt once; after that the user has to be
/// taken to the pane. Returns whether the pane was opened.
#[tauri::command]
pub fn open_permission_settings() -> bool {
    platform::open_permission_settings()
}

/// Whether the interface should offer to relaunch with administrator rights.
#[derive(Debug, Serialize)]
pub struct ElevationDto {
    /// This process already runs elevated, so elevated windows are reachable.
    pub elevated: bool,
    /// Elevation exists on this platform at all.
    pub supported: bool,
}

#[tauri::command]
pub fn elevation_status() -> ElevationDto {
    ElevationDto {
        elevated: platform::is_elevated(),
        supported: cfg!(windows),
    }
}

/// Relaunch elevated, then exit this instance.
///
/// The new process starts before this one quits, so the window does not vanish
/// while the user waits for UAC. Exiting only after a successful launch means a
/// declined prompt leaves the app running.
#[tauri::command]
pub fn restart_elevated() -> Result<(), String> {
    if !platform::relaunch_elevated() {
        return Err("elevation_declined".into());
    }
    // The successor holds the single-instance lock from this point, so release
    // the tray and monitor before exiting rather than fighting it for the lock.
    crate::monitor::stop();
    std::process::exit(0);
}

#[derive(Debug, Serialize)]
pub struct UpdateStatusDto {
    pub current_version: String,
    pub auto_update: bool,
    /// False when this build has no updater endpoint or signing key.
    pub configured: bool,
    pub last_outcome: Option<String>,
    pub last_version: Option<String>,
    /// Why the last check failed, when it did.
    pub last_detail: Option<String>,
}

/// Report update state. Does not trigger a network call.
#[tauri::command]
pub fn update_status(
    app: State<'_, AppState>,
    handle: tauri::AppHandle,
) -> Result<UpdateStatusDto, String> {
    let settings = app.settings.lock().map_err(|e| e.to_string())?;
    let last = app.last_update.lock().map(|v| v.clone()).unwrap_or(None);

    Ok(UpdateStatusDto {
        current_version: handle.package_info().version.to_string(),
        auto_update: settings.auto_update,
        // Whether this build can update at all. A development build has an empty
        // signing key and no endpoint, so every check fails; the UI must say
        // "未配置" instead of offering a button that never works.
        configured: crate::update::is_configured(&handle),
        last_outcome: last.as_ref().map(|o| o.0.clone()),
        last_version: last.as_ref().map(|o| o.1.clone()),
        last_detail: last.map(|o| o.2),
    })
}

/// Check for an update now, honouring the recovery barrier.
#[tauri::command]
pub async fn check_update(handle: tauri::AppHandle) -> Result<UpdateCheckDto, String> {
    let outcome = crate::update::check_only(&handle).await;
    let detail = outcome.detail();

    // Record the outcome so the settings page can show the last result after a
    // restart instead of always claiming the check never ran.
    if let Some(state) = handle.try_state::<AppState>() {
        if let Ok(mut last) = state.last_update.lock() {
            *last = Some((
                outcome.as_str().to_string(),
                outcome.version(),
                detail.clone(),
            ));
        }
    }

    Ok(UpdateCheckDto {
        outcome: outcome.as_str().to_string(),
        detail,
    })
}

#[tauri::command]
pub async fn install_update(handle: tauri::AppHandle) -> Result<UpdateCheckDto, String> {
    let outcome = crate::update::check_and_install(handle.clone(), true).await;
    let detail = outcome.detail();
    let state = handle.state::<AppState>();
    if let Ok(mut last) = state.last_update.lock() {
        *last = Some((outcome.as_str().into(), outcome.version(), detail.clone()));
    }
    Ok(UpdateCheckDto {
        outcome: outcome.as_str().into(),
        detail,
    })
}

#[derive(Debug, Serialize)]
pub struct UpdateCheckDto {
    /// `up_to_date`, `deferred`, `installed`, or `failed`.
    pub outcome: String,
    /// The failure reason, empty unless `outcome` is `failed`.
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct ActivityEntryDto {
    pub at: u64,
    pub kind: String,
    pub affected: usize,
    pub unsettled: usize,
    /// What the entry could not say in counts alone. Already in the user's
    /// language when it reaches here.
    pub note: Option<Message>,
    pub trigger: String,
}

/// Forget the activity history.
///
/// Spec P0-15: the history is the user's own record of what this tool did to their
/// windows, so they can erase it without resetting the rest of their configuration.
#[tauri::command]
pub fn clear_activity(app: State<'_, AppState>) -> Result<(), String> {
    let activity = app.activity.clone();
    let mut log = activity.lock().map_err(|e| e.to_string())?;
    log.clear().map_err(|e| e.to_string())
}

/// Recent history, newest first, for the overview.
#[tauri::command]
pub fn activity_log(app: State<'_, AppState>) -> Vec<ActivityEntryDto> {
    let activity = app.activity.clone();
    let entries = match activity.lock() {
        Ok(log) => log.entries().to_vec(),
        Err(_) => return Vec::new(),
    };
    entries
        .into_iter()
        .rev()
        .map(|e| ActivityEntryDto {
            at: e.at,
            kind: e.kind.code().to_string(),
            affected: e.affected,
            unsettled: e.unsettled,
            note: e.note,
            trigger: e.trigger,
        })
        .collect()
}

/// Reset every setting to its default, keeping the chosen target display.
///
/// The target is deliberately preserved: it is the one value the user had to
/// determine by looking at their physical screens ("which one is the remote
/// showing"), and a reset that discards it forces that work again for no reason.
///
/// Everything else returns to the shipped defaults, including exclusions - a
/// "reset" that leaves invisible state behind is worse than no reset at all.
#[tauri::command]
pub fn reset_settings(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
) -> Result<SettingsDto, String> {
    ensure_settings_writable(&app)?;
    let mut engine = app.engine.lock().map_err(|error| error.to_string())?;
    if in_maintenance(&app) {
        return Err("maintenance".into());
    }
    let mut settings = app.settings.lock().map_err(|e| e.to_string())?;
    let preserved_target = settings.target_display.clone();
    let fresh = Settings {
        target_display: preserved_target,
        // The user has already been through the guide; resetting settings is not
        // a reason to show it again.
        onboarding_done: true,
        ..Default::default()
    };
    commit_settings(&app.settings_path, &mut engine, &mut settings, fresh)?;
    drop(settings);
    drop(engine);
    app.schedule
        .lock()
        .map_err(|error| error.to_string())?
        .cancel();
    crate::monitor::apply_sensitivity(&handle);
    crate::session::install(&handle);
    if let Err(e) = crate::session::set_autostart(&handle, false) {
        tracing::warn!(error = %e, "reset could not disable autostart");
        note_result(
            &app,
            Some(Message::one("开机自启未能设置：{error}", "error", e)),
        );
    }
    sync_tray(&handle);

    settings_dto(&app, &handle)
}

/// Where the interface may ask the OS to reveal a folder.
///
/// The old command accepted an arbitrary string and only checked that the
/// canonical path stayed under the config root. That is a filesystem primitive
/// reachable from the webview for no product reason: there are exactly two
/// folders the user ever needs, and naming them removes the whole class of
/// path-traversal and symlink questions.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppFolder {
    /// The configuration directory: settings, journal, activity, logs.
    Config,
    /// The diagnostics directory, where exported reports are written.
    Diagnostics,
}

#[tauri::command]
pub fn open_app_folder(app: State<'_, AppState>, folder: AppFolder) -> Result<bool, String> {
    let _ = app;
    let root = unfy_settings::config_dir();
    let target = match folder {
        AppFolder::Config => root,
        AppFolder::Diagnostics => root.join("diagnostics"),
    };
    Ok(platform::open_directory(&target.to_string_lossy()))
}

/// Dismiss the last error shown on the overview.
///
/// The message describes the most recent failure, not the current state: once the
/// user has read it there is nothing to act on. Without a way to clear it, a fixed
/// problem keeps producing a red line on every visit until the next success
/// happens to overwrite it.
#[tauri::command]
pub fn dismiss_last_error(app: State<'_, AppState>) -> Result<(), String> {
    note_result(&app, None);
    Ok(())
}

/// Tell the backend which language the interface is rendering in.
///
/// The webview owns the rule that resolves `system`, because it reads
/// `navigator.language`; the tray, which is built in Rust, needs the result. Sent
/// on every locale change, including the first render, so the two views cannot end
/// up in different languages.
#[tauri::command]
pub fn set_ui_locale(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
    locale: String,
) -> Result<(), String> {
    let parsed = crate::ui_text::Locale::from_tag(&locale).ok_or_else(|| {
        // An unsupported tag is a bug in the caller rather than a user error; the
        // tray keeps the language it had rather than switching to a guess.
        format!("unsupported locale: {locale}")
    })?;
    let changed = match app.ui_locale.lock() {
        Ok(mut slot) => {
            let changed = *slot != Some(parsed);
            *slot = Some(parsed);
            changed
        }
        Err(e) => return Err(e.to_string()),
    };
    if changed {
        // Worth one line: a user reporting a half-translated interface is asking
        // exactly this, and the tray language is otherwise invisible in the log.
        tracing::info!(?parsed, "interface language reported by the window");
        // Rebuild so the menu is in the new language immediately rather than at the
        // next state change.
        crate::tray::refresh(&handle);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct PauseDto {
    pub paused: bool,
    pub state: &'static str,
}

/// Pause or resume automation while keeping the current layout.
///
/// Pausing keeps the journal and the window positions; resuming does not move
/// anything. Only "restore" puts windows back.
#[tauri::command]
pub fn set_paused(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
    paused: bool,
) -> Result<PauseDto, String> {
    set_paused_inner(&handle, &app, paused)
}

/// The implementation behind the command, callable from the tray with any runtime.
pub fn set_paused_for<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
    app: State<'_, AppState>,
    paused: bool,
) -> Result<PauseDto, String> {
    set_paused_inner(handle, &app, paused)
}

fn set_paused_inner<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
    app: &State<'_, AppState>,
    paused: bool,
) -> Result<PauseDto, String> {
    ensure_settings_writable(app)?;
    app.schedule
        .lock()
        .map_err(|error| error.to_string())?
        .cancel();
    let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
    if in_maintenance(app) {
        return Err("maintenance".into());
    }
    let mut settings = app.settings.lock().map_err(|error| error.to_string())?;
    let mut candidate = settings.clone();
    candidate.paused = paused;
    commit_settings(&app.settings_path, &mut engine, &mut settings, candidate)?;
    let state = state_name(engine.state());
    drop(settings);
    drop(engine);

    sync_tray(handle);
    Ok(PauseDto { paused, state })
}

/// Accept the current window positions as the new baseline.
///
/// This is the third option the exit barrier has always implied but never
/// offered: "restore", "try again", or **keep what is on screen now**. Without it
/// a user whose windows will not go back has no way to quit without leaving an
/// open transaction behind, and the tray keeps refusing to exit.
///
/// Semantically it closes the journal and clears the preimages: from this moment
/// there is nothing to restore, so the next gather captures the current layout as
/// home.
#[tauri::command]
pub fn keep_current_layout(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
) -> Result<(), String> {
    let admission = WriteAdmission::claim(&app)?;
    let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
    admission.check()?;
    let before = engine.captured_window_count();
    engine.keep_current_layout().map_err(|e| e.to_string())?;
    // Keeping a layout does not resume automation or alter the user's settings.
    let state = state_name(engine.state());
    drop(engine);

    if let Ok(mut log) = app.activity.lock() {
        log.record(crate::activity::Entry {
            at: crate::activity::now(),
            kind: crate::activity::Kind::KeepLayout,
            affected: before,
            unsettled: 0,
            note: Some(Message::new("接受了当前布局，不再恢复")),
            trigger: "window".into(),
        });
    }
    if let Ok(mut slot) = app.last_error.lock() {
        *slot = None;
    }
    tracing::info!(state, "kept the current layout; recovery records closed");
    sync_tray(&handle);
    Ok(())
}

/// Result of the last hotkey registration attempt, so the UI can tell the truth
/// instead of showing a shortcut that never got registered.
#[derive(Debug, Serialize, Default, Clone)]
pub struct HotkeyStatusDto {
    pub gather: Option<String>,
    /// Why the combination could not be registered, as a sentence for the
    /// interface to word. `None` when it registered.
    pub gather_error: Option<Message>,
    pub restore: Option<String>,
    pub restore_error: Option<Message>,
}

#[tauri::command]
pub fn hotkey_status(app: State<'_, AppState>) -> Result<HotkeyStatusDto, String> {
    let status = app.hotkey_status.lock().map_err(|e| e.to_string())?;
    Ok(status.clone())
}

/// Flash a numbered badge on every display so the user can identify which screen
/// the remote client is showing.
#[tauri::command]
/// Draw the display numbers.
///
/// Returns `"shown"`, `"already_showing"` or `"unavailable"` rather than a bool:
/// pressing the button twice within the four seconds the badges live is a normal
/// thing to do, and the old `false` told those users their screens had a problem.
pub fn identify_displays(seconds: Option<u64>) -> String {
    match platform::flash_display_labels(seconds.unwrap_or(4)) {
        platform::FlashOutcome::Shown => "shown",
        platform::FlashOutcome::AlreadyShowing => "already_showing",
        platform::FlashOutcome::Unavailable => "unavailable",
    }
    .into()
}

#[derive(Debug, Serialize)]
pub struct DisplayThumbnailDto {
    pub native_id: String,
    pub data_url: String,
    /// True when the display captured as solid black: powered off, on another
    /// input, or a virtual display with nothing attached.
    pub blank: bool,
}

/// Screenshot of each display, for the picker.
///
/// Captured on demand, not at startup: most launches never open the display page,
/// and a screenshot costs a few milliseconds per monitor. The images stay in
/// memory and are never written to disk or included in diagnostics.
#[tauri::command]
pub fn display_thumbnails() -> Vec<DisplayThumbnailDto> {
    platform::display_thumbnails()
        .into_iter()
        .map(|t| DisplayThumbnailDto {
            native_id: t.native_id,
            data_url: t.data_url,
            blank: t.blank,
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct AppDto {
    pub name: String,
    /// True when an exclusion rule already covers this application.
    pub excluded: bool,
}

/// Applications that currently own a window, for the exclusion picker.
///
/// Typing an executable name is a guessing game: users know "OBS", not
/// "obs64.exe". This returns the names the engine would actually match, sorted
/// for display.
#[tauri::command]
pub fn running_applications(app: State<'_, AppState>) -> Vec<AppDto> {
    let excluded = app
        .settings
        .lock()
        .map(|s| s.excluded_apps.clone())
        .unwrap_or_default();

    platform::running_applications()
        .into_iter()
        .map(|name| AppDto {
            excluded: excluded.iter().any(|rule| rule.eq_ignore_ascii_case(&name)),
            name,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(std::path::PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let directory =
                std::env::temp_dir().join(format!("unfy-command-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&directory).unwrap();
            Self(directory)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn unfinished(directory: &std::path::Path, window: u64) -> uuid::Uuid {
        let transaction = uuid::Uuid::new_v4();
        let mut journal = unfy_journal::Journal::begin_named(
            directory.join(format!("{transaction}.journal")),
            transaction,
            "test",
        )
        .unwrap();
        let preimage = unfy_journal::Preimage {
            id: WindowId {
                native_id: window,
                process_id: 1,
                process_start: 1,
                instance: 1,
            },
            display_native_id: "screen".into(),
            display_quality: unfy_domain::IdentityQuality::Strong,
            display_work_area: Rect::new(0, 0, 1920, 1080),
            display_scale: unfy_domain::Scale::IDENTITY,
            visible_rect: Rect::new(50, 50, 550, 450),
            outer_rect: Rect::new(50, 50, 550, 450),
            mode: unfy_domain::WindowMode::Normal,
            topmost: false,
            native_placement: None,
        };
        journal.write_preimage(&preimage).unwrap();
        journal
            .write_intent(&preimage.id, Rect::new(2000, 50, 2500, 450), preimage.mode)
            .unwrap();
        journal.durable().unwrap();
        transaction
    }

    #[test]
    fn recovery_counts_nested_obligations_without_double_counting_owned_transaction() {
        let directory = TestDirectory::new();
        let nested = directory.0.join("imported");
        std::fs::create_dir(&nested).unwrap();
        let owned = unfinished(&directory.0, 1);
        let other = unfinished(&nested, 2);
        let status = scan_recovery(&directory.0, Some(owned), 1);
        assert_eq!(status.windows, 2);
        assert_eq!(status.blockers, 0);
        assert_eq!(
            status
                .transactions
                .iter()
                .map(|tx| tx.id.as_str())
                .collect::<Vec<_>>(),
            vec![other.to_string()]
        );
        assert_eq!(
            find_recovery_transaction(&directory.0, other)
                .unwrap()
                .parent(),
            Some(nested.as_path())
        );
    }

    #[test]
    fn unreadable_directory_and_corrupt_journal_block_lifecycle_instead_of_counting_zero() {
        let directory = TestDirectory::new();
        let corrupt = directory.0.join("bad.journal");
        std::fs::write(&corrupt, b"broken journal").unwrap();
        let status = scan_recovery(&directory.0, None, 0);
        assert!(status.present);
        assert_eq!(status.windows, 0);
        assert_eq!(status.blockers, 1);
        assert_eq!(std::fs::read(&corrupt).unwrap(), b"broken journal");
        assert_eq!(scan_recovery(&corrupt, None, 0).blockers, 1);
    }

    #[test]
    fn recovery_queries_leave_valid_empty_journals_untouched() {
        let directory = TestDirectory::new();
        let id = uuid::Uuid::new_v4();
        let path = directory.0.join(format!("{id}.journal"));
        let mut journal = unfy_journal::Journal::begin_named(&path, id, "test").unwrap();
        journal.durable().unwrap();
        drop(journal);
        let before = std::fs::read(&path).unwrap();
        assert!(!scan_recovery(&directory.0, None, 0).present);
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn failed_settings_save_keeps_runtime_configuration_and_pause() {
        let directory = TestDirectory::new();
        let mut settings = Settings::default();
        let before = settings.clone();
        let config = engine_config_from_settings(&settings);
        let mut engine =
            unfy_engine::Engine::new(unfy_platform::FakeBackend::with_two_displays(), config);
        let mut candidate = settings.clone();
        candidate.layout_mode = unfy_settings::LayoutModeName::Grid;
        candidate.excluded_apps = vec!["editor.exe".into()];
        candidate.paused = true;
        assert!(commit_settings(&directory.0, &mut engine, &mut settings, candidate).is_err());
        assert_eq!(settings, before);
        assert_eq!(
            engine.config().layout.mode,
            unfy_layout::LayoutMode::MoveInto
        );
        assert!(engine.config().excluded_apps.is_empty());
        assert!(!engine.is_paused());
    }

    #[test]
    fn successful_settings_commit_survives_reload_with_the_same_pause_policy() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.json");
        let mut settings = Settings::default();
        let mut engine = unfy_engine::Engine::new(
            unfy_platform::FakeBackend::with_two_displays(),
            engine_config_from_settings(&settings),
        );
        let mut candidate = settings.clone();
        candidate.paused = true;
        commit_settings(&path, &mut engine, &mut settings, candidate).unwrap();
        assert!(engine.is_paused());
        assert!(Settings::load(&path).unwrap().paused);
        let reset = Settings {
            onboarding_done: true,
            ..Default::default()
        };
        commit_settings(&path, &mut engine, &mut settings, reset).unwrap();
        assert!(!engine.is_paused());
        assert!(!Settings::load(path).unwrap().paused);
    }

    #[test]
    fn queued_gather_checks_maintenance_after_acquiring_writer_lock() {
        let directory = TestDirectory::new();
        let config = EngineConfig {
            journal_dir: directory.0.join("journal"),
            ..Default::default()
        };
        let engine = Arc::new(std::sync::Mutex::new(unfy_engine::Engine::new(
            platform::make(),
            config,
        )));
        let maintenance = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let admission = WriteAdmission {
            maintenance: maintenance.clone(),
            token: crate::schedule::Schedule::new().claim_manual(),
            startup_error: None,
        };
        let held = engine.lock().unwrap();
        let worker_engine = engine.clone();
        let activity = Arc::new(std::sync::Mutex::new(crate::activity::Activity::load(
            directory.0.join("activity.jsonl"),
        )));
        let (queued, ready) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            queued.send(()).unwrap();
            migrate_blocking(
                worker_engine,
                Arc::new(std::sync::Mutex::new(Settings::default())),
                activity,
                Arc::new(|_, _| {}),
                "test",
                admission,
            )
        });
        ready.recv().unwrap();
        maintenance.store(true, std::sync::atomic::Ordering::SeqCst);
        drop(held);
        assert_eq!(worker.join().unwrap().unwrap_err(), "maintenance");
        assert_eq!(engine.lock().unwrap().current_transaction(), None);
    }
}
