//! Tray icon and its menu.
//!
//! The tray is the primary surface: the user should be able to gather, restore,
//! rescue and pause without ever opening the settings window. The state label
//! always comes from the engine, never from a cached guess.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::AppState;
use conflux_platform::DisplayProvider;

/// Menu item ids. Kept as constants so the handler and the builder cannot drift.
pub const ID_GATHER: &str = "gather";
pub const ID_RESTORE: &str = "restore";
pub const ID_RESCUE: &str = "rescue";
pub const ID_SETTINGS: &str = "settings";
pub const ID_QUIT: &str = "quit";

/// Install the tray icon. Called once from `setup`.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let gather = MenuItem::with_id(app, ID_GATHER, "整理到目标屏", true, None::<&str>)?;
    let restore = MenuItem::with_id(app, ID_RESTORE, "恢复本次布局", true, None::<&str>)?;
    let rescue = MenuItem::with_id(app, ID_RESCUE, "救援越界窗口", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let settings = MenuItem::with_id(app, ID_SETTINGS, "设置…", true, None::<&str>)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, ID_QUIT, "退出", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &gather,
            &restore,
            &rescue,
            &separator,
            &settings,
            &separator2,
            &quit_item,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("conflux")
        .tooltip("Conflux · 一键把窗口收拢到一块屏")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            ID_GATHER => run_and_notify(app, "gather"),
            ID_RESTORE => run_and_notify(app, "restore"),
            ID_RESCUE => run_and_notify(app, "rescue"),
            ID_SETTINGS => show_settings(app),
            ID_QUIT => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                toggle_settings(app);
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    let _ = builder.build(app)?;
    Ok(())
}

/// Run an engine action from the tray and surface the outcome.
///
/// Failures are emitted as events rather than panicking: a tray click must never
/// take down the process that owns the recovery journal.
fn run_and_notify<R: Runtime>(app: &AppHandle<R>, action: &str) {
    let result = match action {
        "gather" => run_gather(app),
        "restore" => run_restore(app),
        "rescue" => run_rescue(app),
        _ => Err("unknown action".to_string()),
    };

    if let Err(message) = &result {
        tracing::warn!(action, error = %message, "tray action failed");
    }
    let _ = app.emit(
        "conflux://action",
        serde_json::json!({
            "action": action,
            "ok": result.is_ok(),
            "message": result.err(),
        }),
    );
}

fn run_gather<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let mut engine = state.engine.lock().map_err(|e| e.to_string())?;

    let displays = engine.backend().topology().map_err(|e| e.to_string())?;
    let target = match resolve_target(&settings, &displays) {
        Some(key) => key,
        None => {
            show_settings(app);
            return Err("请先在设置中选择目标显示器".into());
        }
    };

    engine
        .migrate(&target)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn run_restore<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut engine = state.engine.lock().map_err(|e| e.to_string())?;
    engine.restore().map(|_| ()).map_err(|e| e.to_string())
}

fn run_rescue<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut engine = state.engine.lock().map_err(|e| e.to_string())?;
    engine
        .rescue_offscreen()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn resolve_target(
    settings: &conflux_settings::Settings,
    displays: &[conflux_domain::DisplaySnapshot],
) -> Option<conflux_domain::DisplayKey> {
    if let Some(target) = &settings.target_display {
        if let Some(found) = displays.iter().find(|d| target.matches(&d.key)) {
            return Some(found.key.clone());
        }
    }
    displays
        .iter()
        .find(|d| d.primary)
        .or_else(|| displays.first())
        .map(|d| d.key.clone())
}

fn show_settings<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn toggle_settings<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        if visible {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

/// Exit only after the recovery barrier.
///
/// A failure to restore must not silently drop windows, so the process stays
/// alive and the error is surfaced. The user can retry from the tray.
fn quit<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let blocked = match state.engine.lock() {
        Ok(mut engine) => {
            // If anything is still captured, try to put it back first.
            if engine.state().has_pending_work() {
                match engine.restore() {
                    Ok(report) if report.is_complete() => false,
                    Ok(report) => {
                        tracing::warn!(pending = report.pending.len(), "restore incomplete");
                        true
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "restore failed during exit");
                        true
                    }
                }
            } else {
                false
            }
        }
        Err(_) => true,
    };

    if blocked {
        let _ = app.emit(
            "conflux://exit-blocked",
            serde_json::json!({ "reason": "restore_incomplete" }),
        );
        show_settings(app);
        return;
    }
    app.exit(0);
}
