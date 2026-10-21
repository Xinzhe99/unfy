//! Tray icon and its menu.
//!
//! The tray is the primary surface: the user should be able to gather, restore,
//! rescue and pause without ever opening the settings window. The state label
//! always comes from the engine, never from a cached guess.
//!
//! # Why the menu is rebuilt
//!
//! Menu items are enabled/disabled and their labels change with engine state
//! ("整理到目标屏" makes no sense while a layout is already active). Tauri does
//! not support mutating a `CheckMenuItem` reliably across platforms, so the whole
//! menu is replaced whenever the state changes. That is cheap and it cannot drift
//! from reality.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::AppState;

/// Menu item ids. Kept as constants so the handler and the builder cannot drift.
pub const ID_GATHER: &str = "gather";
pub const ID_RESTORE: &str = "restore";
pub const ID_RESCUE: &str = "rescue";
pub const ID_PAUSE: &str = "pause";
pub const ID_IDENTIFY: &str = "identify";
pub const ID_SETTINGS: &str = "settings";
pub const ID_QUIT: &str = "quit";

/// Install the tray icon. Called once from `setup`.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build_menu(app)?;

    let mut builder = TrayIconBuilder::with_id("conflux")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            ID_GATHER => run_and_notify(app, "gather"),
            ID_RESTORE => run_and_notify(app, "restore"),
            ID_RESCUE => run_and_notify(app, "rescue"),
            ID_PAUSE => toggle_pause(app),
            ID_IDENTIFY => {
                let _ = crate::commands::identify_displays(Some(4));
            }
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

    builder.build(app)?;
    refresh(app);
    Ok(())
}

/// Rebuild the menu and tooltip from the current engine state.
///
/// Called after every action, because the enabled set and the labels depend on
/// whether windows are currently gathered.
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let Some(tray) = app.tray_by_id("conflux") else {
        return;
    };
    if let Ok(menu) = build_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tooltip(app)));
}

fn tooltip<R: Runtime>(app: &AppHandle<R>) -> String {
    let state = app.state::<AppState>();
    let Ok(engine) = state.engine.lock() else {
        return "Conflux".into();
    };
    let base = match engine.state() {
        conflux_domain::EngineState::Idle => "Conflux · 就绪",
        conflux_domain::EngineState::Planning => "Conflux · 正在计划",
        conflux_domain::EngineState::Migrating => "Conflux · 正在整理窗口",
        conflux_domain::EngineState::Active => "Conflux · 布局已启用",
        conflux_domain::EngineState::Restoring => "Conflux · 正在恢复",
        conflux_domain::EngineState::Degraded => "Conflux · 部分受限",
        conflux_domain::EngineState::Error => "Conflux · 需要处理",
    };
    if engine.is_paused() {
        format!("{base}（已暂停）")
    } else {
        base.to_string()
    }
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let state = app.state::<AppState>();
    let (engine_state, paused, pending) = match state.engine.lock() {
        Ok(engine) => (
            engine.state(),
            engine.is_paused(),
            engine.captured_window_count(),
        ),
        Err(_) => (conflux_domain::EngineState::Error, false, 0),
    };

    let has_layout = engine_state.has_pending_work() || pending > 0;

    let gather_label = if has_layout {
        "重新整理到目标屏"
    } else {
        "整理到目标屏"
    };

    let gather = MenuItem::with_id(app, ID_GATHER, gather_label, !paused, None::<&str>)?;
    let restore = MenuItem::with_id(app, ID_RESTORE, "恢复本次布局", has_layout, None::<&str>)?;
    let rescue = MenuItem::with_id(app, ID_RESCUE, "救援越界窗口", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(
        app,
        ID_PAUSE,
        "暂停自动整理",
        has_layout,
        paused,
        None::<&str>,
    )?;
    let identify = MenuItem::with_id(app, ID_IDENTIFY, "识别屏幕编号", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let settings = MenuItem::with_id(app, ID_SETTINGS, "设置…", true, None::<&str>)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, ID_QUIT, "退出", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &gather,
            &restore,
            &rescue,
            &pause,
            &identify,
            &separator,
            &settings,
            &separator2,
            &quit_item,
        ],
    )
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
    refresh(app);
}

fn toggle_pause<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let now_paused = match state.engine.lock() {
        Ok(mut engine) => {
            if engine.is_paused() {
                engine.resume().is_ok() && false
            } else {
                engine.pause().is_ok() && true
            }
        }
        Err(_) => return,
    };

    if let Ok(mut settings) = state.settings.lock() {
        settings.paused = now_paused;
        let _ = settings.save(&state.settings_path);
    }

    let _ = app.emit(
        "conflux://action",
        serde_json::json!({ "action": "pause", "ok": true, "paused": now_paused }),
    );
    refresh(app);
}

fn run_gather<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let mut engine = state.engine.lock().map_err(|e| e.to_string())?;

    let Some(target) = engine.resolve_target(
        settings
            .target_display
            .as_ref()
            .map(|t| t.native_id.as_str()),
    ) else {
        show_settings(app);
        return Err("没有可用显示器".into());
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

fn show_settings<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
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
            let _ = window.unminimize();
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
