//! Tauri commands. This is the entire surface the frontend can reach.
//!
//! Every variant takes and returns plain data: no handles, no paths, no shell.
//! The frontend cannot ask the engine to touch an arbitrary window id — it can
//! only ask for a preview, a migration, a restore, or a setting change.

use std::collections::HashMap;

use conflux_domain::{DisplaySnapshot, IdentityQuality, Rect, WindowId};
use conflux_engine::{EngineState, MigrationReport, PendingWork, Preview, RestoreReport};
use conflux_platform::DisplayProvider;
use conflux_settings::{Settings, TargetDisplay};
use serde::{Deserialize, Serialize};
use tauri::State;

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
    pub last_error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct DisplayDto {
    pub native_id: String,
    pub label: String,
    pub width: i32,
    pub height: i32,
    pub work_width: i32,
    pub work_height: i32,
    pub scale: f64,
    pub primary: bool,
    /// Whether the identity is strong enough to restore onto without asking.
    pub restorable: bool,
    pub is_target: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct TargetDto {
    pub native_id: String,
    pub label: String,
    /// False when the saved target is not present in the current topology.
    pub present: bool,
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
    /// Short label for the UI: pid plus handle, never the window title.
    pub label: String,
    pub rect: RectDto,
    pub clamped: bool,
}

#[derive(Debug, Serialize)]
pub struct PreviewDto {
    pub target_present: bool,
    pub placements: Vec<PlacementDto>,
    pub unplaced: Vec<String>,
    pub skipped: Vec<SkipDto>,
    pub cascaded: bool,
}

#[derive(Debug, Serialize)]
pub struct SkipDto {
    pub window_id: String,
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
    pub cascaded: bool,
    pub state: &'static str,
    /// Process ids of the applications that refused to move, so the user can
    /// recognise "these three apps would not cooperate" instead of a bare count.
    pub refused_pids: Vec<u32>,
}

#[derive(Debug, Serialize)]
pub struct RestoreDto {
    pub restored: usize,
    pub kept_user_change: usize,
    pub fallback_visible: usize,
    pub rejected: usize,
    pub pending: usize,
    pub state: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct SaveSettingsRequest {
    pub layout_mode: Option<String>,
    pub margin: Option<i32>,
    pub gap: Option<i32>,
    pub autostart: Option<bool>,
    pub theme: Option<String>,
    pub hotkey_gather: Option<String>,
    pub hotkey_restore: Option<String>,
    pub restore_delay_secs: Option<u32>,
    pub auto_update: Option<bool>,
    pub telemetry: Option<bool>,
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

fn display_dto(d: &DisplaySnapshot, target: Option<&TargetDisplay>) -> DisplayDto {
    DisplayDto {
        native_id: d.key.native_id.clone(),
        label: d.label.clone(),
        width: d.bounds.width(),
        height: d.bounds.height(),
        work_width: d.work_area.width(),
        work_height: d.work_area.height(),
        scale: d.scale.x,
        primary: d.primary,
        restorable: d.key.quality.is_restorable(),
        is_target: target.is_some_and(|t| t.matches(&d.key)),
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

#[tauri::command]
pub fn get_status(app: State<'_, AppState>) -> Result<StatusDto, String> {
    let engine = app.engine.lock().map_err(|e| e.to_string())?;
    let settings = app.settings.lock().map_err(|e| e.to_string())?;

    let displays = engine.backend().topology().map_err(|e| e.to_string())?;

    let target = settings.target_display.as_ref().map(|t| TargetDto {
        native_id: t.native_id.clone(),
        label: t.label.clone(),
        present: displays.iter().any(|d| t.matches(&d.key)),
    });

    Ok(StatusDto {
        state: state_name(engine.state()),
        platform: platform::name(),
        supported: platform::is_supported(),
        has_permission: platform::has_window_permission(),
        permission_name: platform::permission_name(),
        display_count: displays.len(),
        target,
        pending_windows: 0,
        last_error: None,
    })
}

#[tauri::command]
pub fn list_displays(app: State<'_, AppState>) -> Result<Vec<DisplayDto>, String> {
    let engine = app.engine.lock().map_err(|e| e.to_string())?;
    let settings = app.settings.lock().map_err(|e| e.to_string())?;
    let displays = engine.backend().topology().map_err(|e| e.to_string())?;
    Ok(displays
        .iter()
        .map(|d| display_dto(d, settings.target_display.as_ref()))
        .collect())
}

#[tauri::command]
pub fn preview(app: State<'_, AppState>) -> Result<PreviewDto, String> {
    let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
    let settings = app.settings.lock().map_err(|e| e.to_string())?;

    let target = engine
        .resolve_target(
            settings
                .target_display
                .as_ref()
                .map(|t| t.native_id.as_str()),
        )
        .ok_or_else(|| "no_displays".to_string())?;

    let preview: Preview = engine.preview(&target).map_err(|e| e.to_string())?;

    Ok(PreviewDto {
        target_present: true,
        placements: preview
            .plan
            .placements
            .iter()
            .map(|p| PlacementDto {
                window_id: window_label(&p.id),
                label: window_label(&p.id),
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
                reason: s.reason.code(),
            })
            .collect(),
        cascaded: preview.plan.cascaded,
    })
}

/// Gather every eligible window onto the target display.
///
/// Fails with `already_active` when a transaction is still open. That is not a
/// transient error: the caller must restore first, otherwise the new preimages
/// would record the moved positions as the user's original layout.
#[tauri::command]
pub fn migrate(app: State<'_, AppState>) -> Result<MigrationDto, String> {
    let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
    let settings = app.settings.lock().map_err(|e| e.to_string())?;

    let target = engine
        .resolve_target(
            settings
                .target_display
                .as_ref()
                .map(|t| t.native_id.as_str()),
        )
        .ok_or_else(|| "no_displays".to_string())?;

    // A gather while windows are still captured is refused by the engine; give
    // the user the action that actually helps instead of a raw error string.
    let report: MigrationReport = engine.migrate(&target).map_err(|e| match e {
        conflux_domain::EngineError::TransactionAlreadyOpen => "already_active".to_string(),
        other => other.to_string(),
    })?;
    Ok(MigrationDto {
        transaction: report.transaction.to_string(),
        applied: report.applied_count(),
        rejected: report.failed_count(),
        gone: report
            .outcomes
            .values()
            .filter(|o| **o == conflux_domain::WindowOutcome::Gone)
            .count(),
        skipped: report.skipped.len(),
        unplaced: report.unplaced.len(),
        cascaded: report.cascaded,
        state: state_name(engine.state()),
        refused_pids: report.refused.iter().map(|r| r.id.process_id).collect(),
    })
}

#[tauri::command]
pub fn restore(app: State<'_, AppState>) -> Result<RestoreDto, String> {
    let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
    let report: RestoreReport = engine.restore().map_err(|e| e.to_string())?;
    Ok(RestoreDto {
        restored: report.count(conflux_domain::WindowOutcome::Restored),
        kept_user_change: report.count(conflux_domain::WindowOutcome::KeptUserChange),
        fallback_visible: report.count(conflux_domain::WindowOutcome::FallbackVisible),
        rejected: report.count(conflux_domain::WindowOutcome::Rejected),
        pending: report.pending.len(),
        state: state_name(engine.state()),
    })
}

#[tauri::command]
pub fn rescue_offscreen(app: State<'_, AppState>) -> Result<usize, String> {
    let mut engine = app.engine.lock().map_err(|e| e.to_string())?;
    let rescued = engine.rescue_offscreen().map_err(|e| e.to_string())?;
    Ok(rescued.len())
}

#[tauri::command]
pub fn get_settings(app: State<'_, AppState>) -> Result<SettingsDto, String> {
    let settings = app.settings.lock().map_err(|e| e.to_string())?;
    Ok(SettingsDto::from(&*settings))
}

#[derive(Debug, Serialize)]
pub struct SettingsDto {
    pub layout_mode: String,
    pub margin: i32,
    pub gap: i32,
    pub autostart: bool,
    pub theme: String,
    pub hotkey_gather: Option<String>,
    pub hotkey_restore: Option<String>,
    pub restore_delay_secs: u32,
    pub auto_update: bool,
    pub telemetry: bool,
    pub target: Option<TargetDto>,
}

impl From<&Settings> for SettingsDto {
    fn from(s: &Settings) -> Self {
        Self {
            layout_mode: match s.layout_mode {
                conflux_settings::LayoutModeName::Grid => "grid",
                conflux_settings::LayoutModeName::PrimarySide => "primary_side",
                conflux_settings::LayoutModeName::PreserveOrder => "preserve_order",
            }
            .into(),
            margin: s.margin,
            gap: s.gap,
            autostart: s.autostart,
            theme: match s.theme {
                conflux_settings::ThemeName::System => "system",
                conflux_settings::ThemeName::Light => "light",
                conflux_settings::ThemeName::Dark => "dark",
            }
            .into(),
            hotkey_gather: s.hotkey_gather.clone(),
            hotkey_restore: s.hotkey_restore.clone(),
            restore_delay_secs: s.restore_delay_secs,
            auto_update: s.auto_update,
            telemetry: s.telemetry,
            target: s.target_display.as_ref().map(|t| TargetDto {
                native_id: t.native_id.clone(),
                label: t.label.clone(),
                present: true,
            }),
        }
    }
}

#[tauri::command]
pub fn save_settings(
    handle: tauri::AppHandle,
    app: State<'_, AppState>,
    request: SaveSettingsRequest,
) -> Result<SettingsDto, String> {
    let mut settings = app.settings.lock().map_err(|e| e.to_string())?;

    if let Some(mode) = request.layout_mode {
        settings.layout_mode = match mode.as_str() {
            "grid" => conflux_settings::LayoutModeName::Grid,
            "primary_side" => conflux_settings::LayoutModeName::PrimarySide,
            "preserve_order" => conflux_settings::LayoutModeName::PreserveOrder,
            other => return Err(format!("unknown layout mode: {other}")),
        };
    }
    if let Some(margin) = request.margin {
        settings.margin = margin;
    }
    if let Some(gap) = request.gap {
        settings.gap = gap;
    }
    if let Some(autostart) = request.autostart {
        settings.autostart = autostart;
    }
    let autostart_changed = request.autostart;
    if let Some(theme) = request.theme {
        settings.theme = match theme.as_str() {
            "system" => conflux_settings::ThemeName::System,
            "light" => conflux_settings::ThemeName::Light,
            "dark" => conflux_settings::ThemeName::Dark,
            other => return Err(format!("unknown theme: {other}")),
        };
    }
    if let Some(hotkey) = request.hotkey_gather {
        settings.hotkey_gather = if hotkey.is_empty() {
            None
        } else {
            Some(hotkey)
        };
    }
    if let Some(hotkey) = request.hotkey_restore {
        settings.hotkey_restore = if hotkey.is_empty() {
            None
        } else {
            Some(hotkey)
        };
    }
    if let Some(delay) = request.restore_delay_secs {
        settings.restore_delay_secs = delay;
    }
    if let Some(auto_update) = request.auto_update {
        settings.auto_update = auto_update;
    }
    if let Some(telemetry) = request.telemetry {
        settings.telemetry = telemetry;
    }

    settings.validate().map_err(|e| e.to_string())?;
    settings
        .save(&app.settings_path)
        .map_err(|e| e.to_string())?;

    let dto = SettingsDto::from(&*settings);

    // Drop the settings lock *before* touching the OS registration. The
    // registration path does not read settings, but keeping the guard out of the
    // call makes that independence explicit and prevents a future edit from
    // reintroducing a self-deadlock.
    drop(settings);

    if autostart_changed.is_some() {
        crate::session::set_autostart(&handle, dto.autostart);
    }

    Ok(dto)
}

#[tauri::command]
pub fn set_target_display(app: State<'_, AppState>, native_id: String) -> Result<(), String> {
    let engine = app.engine.lock().map_err(|e| e.to_string())?;
    let mut settings = app.settings.lock().map_err(|e| e.to_string())?;

    let displays = engine.backend().topology().map_err(|e| e.to_string())?;
    let display = displays
        .iter()
        .find(|d| d.key.native_id == native_id)
        .ok_or_else(|| "display_not_found".to_string())?;

    settings.target_display = Some(TargetDisplay::from_key(&display.key, display.label.clone()));
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
    if settings.suppress_recovery {
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
        let windows = if engine.state() == conflux_domain::EngineState::Degraded {
            engine.captured_window_count()
        } else {
            0
        };
        (owned, windows)
    };

    let dir = conflux_settings::config_dir().join("journal");
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
        if let Ok(conflux_journal::Recovered::Unfinished(tx)) = conflux_journal::recover(&path) {
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

#[tauri::command]
pub fn dismiss_pending_work(app: State<'_, AppState>) -> Result<(), String> {
    let mut settings = app.settings.lock().map_err(|e| e.to_string())?;
    settings.suppress_recovery = true;
    settings.save(&app.settings_path).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct DiagnosticsDto {
    pub path: String,
    pub bytes: u64,
}

/// Write a redacted diagnostics bundle: versions, topology shape, and setting
/// names — never window titles, paths, or screen content.
#[tauri::command]
pub fn export_diagnostics(app: State<'_, AppState>) -> Result<DiagnosticsDto, String> {
    let engine = app.engine.lock().map_err(|e| e.to_string())?;
    let settings = app.settings.lock().map_err(|e| e.to_string())?;

    let displays = engine.backend().topology().map_err(|e| e.to_string())?;

    let report = serde_json::json!({
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
        },
    });

    let dir = conflux_settings::config_dir().join("diagnostics");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("conflux-{}.json", uuid::Uuid::new_v4()));
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, &text).map_err(|e| e.to_string())?;

    Ok(DiagnosticsDto {
        path: path.to_string_lossy().to_string(),
        bytes: text.len() as u64,
    })
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

#[allow(dead_code)]
fn unused(_: HashMap<String, String>, _: IdentityQuality) {}

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

/// The transaction the engine is currently running, if any.
///
/// The UI polls this alongside `pending_work` so it can distinguish "this is my
/// own in-flight migration" from "a previous run did not finish".
#[tauri::command]
pub fn live_transaction(app: State<'_, AppState>) -> Result<Option<String>, String> {
    let engine = app.engine.lock().map_err(|e| e.to_string())?;
    Ok(engine.current_transaction().map(|id| id.to_string()))
}

#[derive(Debug, Serialize)]
pub struct UpdateStatusDto {
    pub current_version: String,
    pub auto_update: bool,
    pub last_outcome: Option<String>,
    pub last_version: Option<String>,
}

/// Report update state. Does not trigger a network call.
#[tauri::command]
pub fn update_status(
    app: State<'_, AppState>,
    handle: tauri::AppHandle,
) -> Result<UpdateStatusDto, String> {
    let settings = app.settings.lock().map_err(|e| e.to_string())?;
    Ok(UpdateStatusDto {
        current_version: handle.package_info().version.to_string(),
        auto_update: settings.auto_update,
        last_outcome: None,
        last_version: None,
    })
}

/// Check for an update now, honouring the recovery barrier.
#[tauri::command]
pub async fn check_update(handle: tauri::AppHandle) -> Result<String, String> {
    let outcome = crate::update::check_and_install(handle, true).await;
    Ok(outcome.as_str().to_string())
}
