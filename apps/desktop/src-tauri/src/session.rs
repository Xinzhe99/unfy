//! Global shortcut registration and single-instance behaviour.
//!
//! Shortcuts are optional: if a combination is already taken, the app keeps
//! working through the tray and tells the user why the shortcut did not stick,
//! rather than silently failing to register.

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::AppState;

/// Register the shortcuts from settings.
///
/// Called at startup and again whenever settings change. Any previously
/// registered shortcut is released first, otherwise changing a hotkey would
/// leave the old combination still bound.
///
/// The outcome is stored in `AppState::hotkey_status`, not merely logged: the UI
/// must be able to say "this combination is taken by another application"
/// instead of showing a shortcut that never registered.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let (gather, restore) = {
        let state = app.state::<AppState>();
        let settings = match state.settings.lock() {
            Ok(s) => s.clone(),
            Err(_) => return,
        };
        (settings.hotkey_gather, settings.hotkey_restore)
    };

    let plugin = app.global_shortcut();
    // Re-registration must start from a clean slate.
    if let Err(e) = plugin.unregister_all() {
        tracing::warn!(error = %e, "could not clear previous shortcuts");
    }

    let mut status = crate::commands::HotkeyStatusDto::default();

    if let Some(accelerator) = gather {
        status.gather = Some(accelerator.clone());
        match accelerator.parse::<tauri_plugin_global_shortcut::Shortcut>() {
            Ok(shortcut) => {
                let registered = plugin.on_shortcut(shortcut, move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let _ = app.emit_action("hotkey_gather");
                    }
                });
                if let Err(e) = registered {
                    tracing::warn!(%accelerator, error = %e, "gather hotkey unavailable");
                    status.gather_error = Some(format!("{e}"));
                }
            }
            Err(e) => {
                tracing::warn!(%accelerator, error = %e, "gather hotkey is invalid");
                status.gather_error = Some(format!("无效的组合键：{e}"));
            }
        }
    }

    if let Some(accelerator) = restore {
        status.restore = Some(accelerator.clone());
        match accelerator.parse::<tauri_plugin_global_shortcut::Shortcut>() {
            Ok(shortcut) => {
                let registered = plugin.on_shortcut(shortcut, move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let _ = app.emit_action("hotkey_restore");
                    }
                });
                if let Err(e) = registered {
                    tracing::warn!(%accelerator, error = %e, "restore hotkey unavailable");
                    status.restore_error = Some(format!("{e}"));
                }
            }
            Err(e) => {
                tracing::warn!(%accelerator, error = %e, "restore hotkey is invalid");
                status.restore_error = Some(format!("无效的组合键：{e}"));
            }
        }
    }

    if let Ok(mut slot) = app.state::<AppState>().hotkey_status.lock() {
        *slot = status;
    }
}

/// A tiny trait so the shortcut closures do not need to know about the tray.
trait ActionEmitter {
    fn emit_action(&self, action: &str) -> tauri::Result<()>;
}

impl<R: Runtime> ActionEmitter for AppHandle<R> {
    fn emit_action(&self, action: &str) -> tauri::Result<()> {
        use tauri::Emitter;
        match action {
            "hotkey_gather" => {
                let result: std::result::Result<(), String> = (|| {
                    let state = self.state::<AppState>();
                    let settings = state
                        .settings
                        .lock()
                        .map_err(|_| "settings lock".to_string())?;
                    let mut engine = state.engine.lock().map_err(|_| "engine lock".to_string())?;
                    match engine.resolve_target(
                        settings
                            .target_display
                            .as_ref()
                            .map(|t| t.native_id.as_str()),
                    ) {
                        Some(key) => engine.migrate(&key).map(|_| ()).map_err(|e| e.to_string()),
                        None => Err("no displays".to_string()),
                    }
                })();
                if let Err(message) = &result {
                    tracing::warn!(error = %message, "hotkey gather failed");
                }
                self.emit(
                    "conflux://action",
                    serde_json::json!({
                        "action": "gather",
                        "ok": result.is_ok(),
                        "message": result.err(),
                    }),
                )
            }
            "hotkey_restore" => {
                let result: std::result::Result<(), String> = (|| {
                    let state = self.state::<AppState>();
                    let mut engine = state.engine.lock().map_err(|_| "engine lock".to_string())?;
                    engine.restore().map(|_| ()).map_err(|e| e.to_string())
                })();
                self.emit(
                    "conflux://action",
                    serde_json::json!({
                        "action": "restore",
                        "ok": result.is_ok(),
                        "message": result.err(),
                    }),
                )
            }
            _ => Ok(()),
        }
    }
}

/// Apply an autostart preference to the OS.
///
/// Takes the desired value instead of reading settings. The caller that already
/// holds the settings lock must drop it first: `std::sync::Mutex` is not
/// reentrant, so reading settings from here would deadlock the command thread.
///
/// Failures are logged, never fatal: a locked-down machine may forbid the
/// registry write, and the app must still run.
pub fn set_autostart<R: Runtime>(app: &AppHandle<R>, desired: bool) {
    let manager = app.autolaunch();
    // Log the resolved executable too. The plugin registers `current_exe`, and a
    // debug build run from `target\debug` registers that path, not the installed
    // one. Seeing the path is the difference between "the registry write failed"
    // and "it wrote the wrong binary".
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unknown>".into());
    let current = manager.is_enabled().unwrap_or(false);
    tracing::info!(desired, current, %exe, "reconciling autostart");

    if current == desired {
        return;
    }

    let result = if desired {
        manager.enable()
    } else {
        manager.disable()
    };
    match &result {
        Ok(()) => tracing::info!(desired, "autostart registration updated"),
        Err(e) => tracing::warn!(desired, error = %e, "could not change autostart registration"),
    }
}

/// Reconcile the OS autostart registration with the saved preference at startup.
///
/// No settings lock is held at this point, so reading it here is safe.
pub fn apply_saved_autostart<R: Runtime>(app: &AppHandle<R>) {
    let desired = {
        let state = app.state::<AppState>();
        let settings = state.settings.lock();
        match settings {
            Ok(settings) => settings.autostart,
            Err(_) => return,
        }
    };
    set_autostart(app, desired);
}
