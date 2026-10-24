//! Tauri commands. This is the entire surface the frontend can reach.
//!
//! Every variant takes and returns plain data: no handles, no paths, no shell.
//! The frontend cannot ask the engine to touch an arbitrary window id — it can
//! only ask for a preview, a migration, a restore, or a setting change.

use std::sync::Arc;

use unfy_domain::{DisplaySnapshot, Rect, WindowId};
use unfy_engine::{
    EngineConfig, EngineState, MigrationReport, PendingWork, Preview, RestoreReport,
};
use unfy_platform::{DisplayProvider, WindowBackend};
use unfy_settings::{Settings, TargetDisplay};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::message::Message;
use crate::{platform, AppState};

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

    let displays = engine.backend().topology().map_err(|e| e.to_string())?;

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
            number: live.or(t.number),
        }
    });

    // Windows still held by an unfinished transaction, from the same source the
    // recovery banner uses. Computed before the DTO so one query serves both.
    let pending_windows = {
        let owned = engine.current_transaction();
        let mut count = if engine.state().has_pending_work() {
            engine.captured_window_count()
        } else {
            0
        };
        // A journal the engine does not own (another session, or an unreadable one
        // at startup) still counts: the windows may be displaced.
        let dir = unfy_settings::config_dir().join("journal");
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "journal") {
                    if let Ok(unfy_journal::Recovered::Unfinished(tx)) =
                        unfy_journal::recover(&path)
                    {
                        if Some(tx.transaction) != owned {
                            count = count.max(tx.preimages.len());
                        }
                    }
                }
            }
        }
        count
    };
    let last_error = app.last_error.lock().ok().and_then(|guard| guard.clone());

    Ok(StatusDto {
        state: state_name(engine.state()),
        platform: platform::name(),
        supported: platform::is_supported(),
        has_permission: platform::has_window_permission(),
        permission_name: platform::permission_name(),
        display_count: displays.len(),
        target,
        // Counted from the journal, not invented. The tray and the exit barrier
        // both depend on this number, and a hardcoded zero made a degraded state
        // look idle in every surface that read the status.
        pending_windows,
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
    if let Ok(mut schedule) = app.schedule.lock() {
        schedule.cancel();
    }

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
        migrate_blocking(engine_state, settings, activity, emit, trigger)
    })
    .await
    .map_err(|e| format!("migration task failed: {e}"))?;

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
) -> Result<MigrationDto, String> {
    let mut engine = engine.lock().map_err(|e| e.to_string())?;
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

    // A gather while windows are still captured is refused by the engine; give
    // the user the action that actually helps instead of a raw error string.
    // `retarget` rather than `migrate`: the tray and the button both say
    // "整理到目标屏", and pressing it while a layout is already active means
    // "move everything to the display I just picked". Refusing with
    // `already_active` made the label a lie. `retarget` restores first, so the
    // original preimages survive.
    let retarget = engine.retarget(&target).map_err(|e| match e {
        unfy_domain::EngineError::Paused => "paused".to_string(),
        unfy_domain::EngineError::RestoreIncomplete { pending } => {
            format!("restore_incomplete:{pending}")
        }
        other => other.to_string(),
    })?;
    let report: MigrationReport = retarget.migration;
    let restored_first = 0usize;

    // Record before returning: the user's question ("what did it just do?") is
    // asked immediately after the action, not after a later poll.
    if let Ok(mut log) = activity.lock() {
        log.record(crate::activity::Entry {
            at: crate::activity::now(),
            kind: crate::activity::Kind::Gather,
            affected: report.applied_count(),
            unsettled: report.unfinished() + report.unplaced.len(),
            note: if restored_first > 0 {
                Some(Message::one(
                    "先恢复了 {count} 个窗口",
                    "count",
                    restored_first.to_string(),
                ))
            } else {
                None
            },
            trigger: trigger.into(),
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
        refused_apps: {
            // Look the names up while the engine lock is still held; the report
            // only carries ids.
            let names: std::collections::HashMap<u32, String> = engine
                .backend()
                .enumerate()
                .unwrap_or_default()
                .into_iter()
                .map(|w| (w.id.process_id, w.app_name))
                .collect();
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

    // Claim the schedule even for a zero delay: a zero-delay restore must also
    // cancel any older pending one, otherwise two restores run back to back.
    let token = {
        let mut schedule = app.schedule.lock().map_err(|e| e.to_string())?;
        schedule.claim()
    };

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
        restore_blocking(engine_state, settings, settings_path, activity, trigger)
    })
    .await
    .map_err(|e| format!("restore task failed: {e}"))?;
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
) -> Result<RestoreDto, String> {
    let mut engine = engine.lock().map_err(|e| e.to_string())?;
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
    if complete {
        if let Ok(mut guard) = settings.lock() {
            clear_recovery_dismissal(&mut guard);
            // Restoring ends the cycle, so a paused engine has nothing left to
            // pause. Leaving the flag set would make the next launch start
            // paused with no transaction, which looks like automation is broken.
            guard.paused = false;
            let _ = guard.save(&settings_path);
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
    })
}

#[tauri::command]
pub fn rescue_offscreen(app: State<'_, AppState>) -> Result<usize, String> {
    let rescued = {
        let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
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
        let settings = app.settings.lock().map_err(|e| e.to_string())?;
        SettingsDto::from(&*settings)
    };
    let _ = handle;
    if let Ok(tracker) = app.detector.lock() {
        dto.remote_active = tracker.is_active();
        dto.remote_evidence = tracker.evidence().map(crate::message::evidence_message);
    }
    // `From<&Settings>` cannot see the topology, so the target's presence and live
    // number are resolved here. Without this the settings payload claimed the
    // target was present even after the monitor was unplugged.
    if let Some(target) = dto.target.as_mut() {
        let live = app
            .engine
            .lock()
            .ok()
            .and_then(|engine| unfy_platform::DisplayProvider::topology(engine.backend()).ok())
            .map(|displays| {
                let ordered = unfy_domain::DisplaySnapshot::ordered_by_position(&displays);
                ordered
                    .iter()
                    .position(|d| d.key.native_id == target.native_id)
                    .map(|index| index + 1)
            });
        match live {
            Some(Some(number)) => {
                target.present = true;
                target.number = Some(number);
            }
            Some(None) => target.present = false,
            // Topology unavailable: leave the saved values rather than claiming
            // the display disappeared.
            None => {}
        }
    }
    Ok(dto)
}

#[derive(Debug, Serialize)]
pub struct SettingsDto {
    pub layout_mode: String,
    pub margin: i32,
    pub gap: i32,
    pub autostart: bool,
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
    /// Applications that refused the last gather, for the "exclude" shortcut.
    pub target: Option<TargetDto>,
    /// What the detector currently believes, and why. Independent of whether
    /// automation is enabled: the overview shows the evidence either way.
    pub remote_active: bool,
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
                present: true,
                number: t.number,
            }),
        }
    }
}

/// Resolve the display the user confirmed, or explain what is missing.
fn resolve_confirmed_target(
    engine: &unfy_engine::Engine<crate::platform::Backend>,
    settings: &Settings,
) -> Result<unfy_domain::DisplayKey, String> {
    let Some(saved) = settings.target_display.as_ref() else {
        return Err("no_target".to_string());
    };
    engine
        .resolve_target(Some(saved.native_id.as_str()))
        .ok_or_else(|| "target_missing".to_string())
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

/// Apply the persisted settings to a running engine.
///
/// Returns the engine's refusal (an open transaction) instead of swallowing it:
/// the caller has to tell the user that the new values start with the next
/// gather, not now.
fn publish_settings_to_engine(
    app: &State<'_, AppState>,
    settings: &Settings,
) -> Result<(), String> {
    let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
    let mut config = engine_config_from_settings(settings);
    // Keep the live progress hook: it belongs to the caller, not to the document.
    config.on_progress = engine.config().on_progress.clone();
    engine.update_config(config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_settings(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
    request: SaveSettingsRequest,
) -> Result<SettingsDto, String> {
    // Read before taking the settings lock: the engine lock order is
    // engine-then-settings elsewhere, and reversing it here would invite a
    // deadlock the first time both paths run concurrently.
    let engine_has_open_transaction = app
        .engine
        .lock()
        .map(|engine| engine.current_transaction().is_some())
        .unwrap_or(false);

    let mut settings = app.settings.lock().map_err(|e| e.to_string())?;

    // Merge into a copy. A rejected value must leave the live settings - and the
    // file - exactly as they were; mutating in place and returning an error
    // halfway through would keep the fields that happened to be applied first.
    let mut candidate = settings.clone();

    if let Some(mode) = request.layout_mode {
        candidate.layout_mode = match mode.as_str() {
            "grid" => unfy_settings::LayoutModeName::Grid,
            // The interface's name for the default mode, plus the two historical
            // spellings a stale window or a hand-edited file may still send.
            "default" | "preserve_order" | "primary_side" => {
                unfy_settings::LayoutModeName::MoveInto
            }
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
    let _ = engine_has_open_transaction;
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
    if let Some(excluded_apps) = request.excluded_apps {
        candidate.excluded_apps = excluded_apps;
    }
    if let Some(onboarding_done) = request.onboarding_done {
        candidate.onboarding_done = onboarding_done;
    }
    if let Some(onboarding_step) = request.onboarding_step {
        candidate.onboarding_step = onboarding_step.min(4);
    }

    // Validate and persist the candidate before it becomes live.
    candidate.validate().map_err(|e| e.to_string())?;
    candidate
        .save(&app.settings_path)
        .map_err(|e| e.to_string())?;
    *settings = candidate;
    let dto = SettingsDto::from(&*settings);
    let snapshot = settings.clone();
    drop(settings);

    // OS side effects happen after the document is committed. A registration
    // failure is reported, not turned into "settings not saved".
    if autostart_changed.is_some() {
        crate::session::set_autostart(&handle, dto.autostart);
    }
    if hotkeys_changed {
        crate::session::install(&handle);
    }

    if let Err(e) = publish_settings_to_engine(&app, &snapshot) {
        tracing::warn!(error = %e, "settings saved but not applied to the live engine");
        note_result(
            &app,
            Some(Message::new(
                "设置已保存，但当前整理还在进行中，新的布局会在下次整理时生效。",
            )),
        );
    }

    crate::monitor::apply_sensitivity(&handle);
    sync_tray(&handle);
    Ok(dto)
}

#[tauri::command]
pub fn set_target_display(app: State<'_, AppState>, native_id: String) -> Result<(), String> {
    let engine = app.engine.lock().map_err(|e| e.to_string())?;
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
    settings.target_display = Some(target);
    settings.save(&app.settings_path).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct PendingWorkDto {
    pub present: bool,
    pub windows: usize,
    pub may_have_moved: bool,
    pub truncated_bytes: u64,
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
    let settings = app.settings.lock().map_err(|e| e.to_string())?;
    // A dismissal expires. A permanent flag would mean the user dismisses one
    // crash banner and never learns about a later, different crash.
    if settings.recovery_dismissed_until > now_unix_secs() {
        return Ok(PendingWorkDto {
            present: false,
            windows: 0,
            may_have_moved: false,
            truncated_bytes: 0,
        });
    }
    drop(settings);

    let (owned, adopted_windows) = {
        let engine = app.engine.lock().map_err(|e| e.to_string())?;
        let owned = engine.current_transaction();
        let windows = if engine.state() == unfy_domain::EngineState::Degraded {
            engine.captured_window_count()
        } else {
            0
        };
        (owned, windows)
    };

    let dir = unfy_settings::config_dir().join("journal");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(PendingWorkDto {
            present: adopted_windows > 0,
            windows: adopted_windows,
            may_have_moved: adopted_windows > 0,
            truncated_bytes: 0,
        });
    };

    let mut best_windows = adopted_windows;
    let mut may_have_moved = adopted_windows > 0;
    let mut truncated_bytes = 0u64;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e == "journal") {
            continue;
        }
        if let Ok(unfy_journal::Recovered::Unfinished(tx)) = unfy_journal::recover(&path) {
            if Some(tx.transaction) == owned {
                // The engine already owns this one; counted above.
                continue;
            }
            if tx.preimages.is_empty() {
                // Nothing captured, nothing movable: clean it up rather than
                // alarming the user about work that cannot exist.
                std::fs::remove_file(&path).ok();
                continue;
            }
            let work = PendingWork::from_recovered(&tx);
            if work.windows > best_windows {
                best_windows = work.windows;
            }
            may_have_moved |= work.may_have_moved;
            truncated_bytes = truncated_bytes.max(work.truncated_bytes);
        }
    }

    Ok(PendingWorkDto {
        present: best_windows > 0,
        windows: best_windows,
        may_have_moved,
        truncated_bytes,
    })
}

/// Hide the recovery banner for a while, without discarding the journal.
///
/// Dismissing is a UI preference, not a decision about the windows: the journal
/// stays on disk and `restore` still works. It expires after a day so a later,
/// unrelated crash is reported again.
#[tauri::command]
pub fn dismiss_pending_work(app: State<'_, AppState>) -> Result<(), String> {
    let mut settings = app.settings.lock().map_err(|e| e.to_string())?;
    settings.recovery_dismissed_until = now_unix_secs() + 24 * 60 * 60;
    settings.save(&app.settings_path).map_err(|e| e.to_string())
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
    let outcome = crate::update::check_and_install(handle.clone(), true).await;
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
    let mut settings = app.settings.lock().map_err(|e| e.to_string())?;
    let preserved_target = settings.target_display.clone();
    let fresh = Settings {
        target_display: preserved_target,
        // The user has already been through the guide; resetting settings is not
        // a reason to show it again.
        onboarding_done: true,
        ..Default::default()
    };
    fresh.save(&app.settings_path).map_err(|e| e.to_string())?;
    *settings = fresh.clone();
    drop(settings);

    // The engine reads layout and exclusions from its own config, and the
    // detector reads its level from settings; without these the reset would only
    // take effect at the next launch.
    if let Err(e) = publish_settings_to_engine(&app, &fresh) {
        tracing::warn!(error = %e, "reset could not be applied to the live engine");
    }
    crate::monitor::apply_sensitivity(&handle);
    crate::session::install(&handle);
    crate::session::set_autostart(&handle, false);
    sync_tray(&handle);

    settings_dto(&app, &handle)
}

/// Move the target display to the next one in positional order, keeping the
/// original baseline.
///
/// Spec P0-04-B: cycling must not overwrite the preimages, so a later restore
/// returns to the layout the user started from rather than to an intermediate one.
/// `retarget` already guarantees that, which is why this delegates instead of
/// duplicating the gather logic.
#[tauri::command]
pub async fn cycle_target<R: tauri::Runtime>(
    handle: tauri::AppHandle<R>,
    app: State<'_, AppState>,
) -> Result<TargetDto, String> {
    {
        let engine = app.engine.lock().map_err(|e| e.to_string())?;
        let mut settings = app.settings.lock().map_err(|e| e.to_string())?;
        let displays = engine.backend().topology().map_err(|e| e.to_string())?;
        if displays.len() < 2 {
            return Err("single_display".to_string());
        }
        let ordered = unfy_domain::DisplaySnapshot::ordered_by_position(&displays);
        let current = settings
            .target_display
            .as_ref()
            .and_then(|t| ordered.iter().position(|d| t.matches(&d.key)));
        let next = match current {
            Some(index) => (index + 1) % ordered.len(),
            // No target yet: start at the first, which is also what the picker
            // would show as "显示器 1".
            None => 0,
        };
        let display = ordered[next];
        let mut target = TargetDisplay::from_key(&display.key, display.label.clone());
        target.number = Some(next + 1);
        settings.target_display = Some(target.clone());
        settings
            .save(&app.settings_path)
            .map_err(|e| e.to_string())?;
    }

    // Re-gather onto the new display, keeping the original baseline.
    let engine_state = app.engine.clone();
    let settings_state = app.settings.clone();
    let activity = app.activity.clone();
    let progress_emitter = handle.clone();
    let emit: Arc<dyn Fn(usize, usize) + Send + Sync> = Arc::new(move |done, total| {
        let _ = tauri::Emitter::emit(
            &progress_emitter,
            "unfy://progress",
            serde_json::json!({ "phase": "migrating", "done": done, "total": total }),
        );
    });
    let _ = tokio::task::spawn_blocking(move || {
        migrate_blocking(engine_state, settings_state, activity, emit, "window")
    })
    .await;
    // Close the progress cycle. The gather and restore paths complete their own
    // events; this one only ever announced "migrating", so anything watching the
    // stream saw a batch that began and never ended. The button happens to reset
    // through `run()`'s finally, which is why this was invisible, but the event
    // stream should not depend on which call site the user came through.
    let _ = tauri::Emitter::emit(
        &handle,
        "unfy://progress",
        serde_json::json!({ "phase": "done" }),
    );

    {
        let engine = app.engine.lock().map_err(|e| e.to_string())?;
        let settings = app.settings.lock().map_err(|e| e.to_string())?;
        let displays = engine.backend().topology().unwrap_or_default();
        let ordered = unfy_domain::DisplaySnapshot::ordered_by_position(&displays);
        let target = settings.target_display.as_ref().map(|t| {
            let live = ordered
                .iter()
                .position(|d| t.matches(&d.key))
                .map(|i| i + 1);
            TargetDto {
                native_id: t.native_id.clone(),
                label: t.label.clone(),
                present: live.is_some(),
                number: live.or(t.number),
            }
        });
        target.ok_or_else(|| "no_target".to_string())
    }
}

/// Reveal a directory in the OS file manager.
///
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
    /// True when resuming also ran the restore that the pause had deferred.
    pub restored_deferred: bool,
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
    // One implementation for the window, the tray and the shortcut. Pausing used
    // to be applied by the engine in one path and by a settings flag in another,
    // which produced "the UI says paused, the engine keeps gathering".
    let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
    if paused {
        engine.pause().map_err(|e| e.to_string())?;
    } else {
        engine.resume().map_err(|e| e.to_string())?;
    }
    let paused = engine.is_paused();
    let state = state_name(engine.state());
    drop(engine);

    // Persist the same value that is now live. A failure is reported: silently
    // keeping a preference that the next launch will not honour is worse than an
    // error the user can see.
    {
        let mut settings = app.settings.lock().map_err(|e| e.to_string())?;
        let previous = settings.paused;
        settings.paused = paused;
        if let Err(e) = settings.save(&app.settings_path) {
            settings.paused = previous;
            return Err(format!("could not save the paused preference: {e}"));
        }
    }

    sync_tray(handle);
    Ok(PauseDto {
        paused,
        state,
        restored_deferred: false,
    })
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
pub fn keep_current_layout(app: State<'_, AppState>) -> Result<(), String> {
    let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
    let before = engine.captured_window_count();
    engine.keep_current_layout().map_err(|e| e.to_string())?;
    // The engine already cleared its pause; mirror that rather than clearing a
    // separate flag and hoping the two agree.
    let paused = engine.is_paused();
    let state = state_name(engine.state());
    drop(engine);

    if let Ok(mut settings) = app.settings.lock() {
        settings.paused = paused;
        if let Err(e) = settings.save(&app.settings_path) {
            tracing::warn!(error = %e, "kept the layout but could not persist the pause flag");
        }
    }
    // Accepting the current layout is the same decision as cancelling a pending
    // automatic restore: the windows stay where they are.
    if let Ok(mut schedule) = app.schedule.lock() {
        schedule.cancel();
    }
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
