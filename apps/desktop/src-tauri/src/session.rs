//! Global shortcut registration and single-instance behaviour.
//!
//! Shortcuts are optional: if a combination is already taken, the app keeps
//! working through the tray and tells the user why the shortcut did not stick,
//! rather than silently failing to register.

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::AppState;
use conflux_platform::DisplayProvider;

/// Register the shortcuts from settings. Called from `setup` and again whenever
/// settings change.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let (gather, restore) = {
        let state = app.state::<AppState>();
        let settings = match state.settings.lock() {
            Ok(s) => s.clone(),
            Err(_) => return Ok(()),
        };
        (settings.hotkey_gather, settings.hotkey_restore)
    };

    let plugin = app.global_shortcut();

    if let Some(accelerator) = gather {
        match accelerator.parse::<tauri_plugin_global_shortcut::Shortcut>() {
            Ok(shortcut) => {
                let registered = plugin.on_shortcut(shortcut, move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let _ = app.emit_action("hotkey_gather");
                    }
                });
                if let Err(e) = registered {
                    tracing::warn!(%accelerator, error = %e, "gather hotkey unavailable");
                }
            }
            Err(e) => tracing::warn!(%accelerator, error = %e, "gather hotkey is invalid"),
        }
    }

    if let Some(accelerator) = restore {
        match accelerator.parse::<tauri_plugin_global_shortcut::Shortcut>() {
            Ok(shortcut) => {
                let registered = plugin.on_shortcut(shortcut, move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let _ = app.emit_action("hotkey_restore");
                    }
                });
                if let Err(e) = registered {
                    tracing::warn!(%accelerator, error = %e, "restore hotkey unavailable");
                }
            }
            Err(e) => tracing::warn!(%accelerator, error = %e, "restore hotkey is invalid"),
        }
    }

    Ok(())
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
                    let displays = engine.backend().topology().map_err(|e| e.to_string())?;
                    let target = settings
                        .target_display
                        .as_ref()
                        .and_then(|t| displays.iter().find(|d| t.matches(&d.key)))
                        .or_else(|| displays.iter().find(|d| d.primary))
                        .or_else(|| displays.first())
                        .map(|d| d.key.clone());
                    match target {
                        Some(key) => engine.migrate(&key).map(|_| ()).map_err(|e| e.to_string()),
                        None => Err("no target display".to_string()),
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

/// Reconcile autostart from a borrowed state reference (used by the settings
/// command, which already holds the state lock).
pub fn apply_autostart_handle<R: Runtime>(app: &AppHandle<R>) {
    apply_autostart(app);
}

/// Reconcile the OS autostart registration with the saved preference.
///
/// Failures are logged, never fatal: a locked-down machine may forbid the
/// registry write, and the app must still run.
pub fn apply_autostart<R: Runtime>(app: &AppHandle<R>) {
    let desired = {
        let state = app.state::<AppState>();
        let settings = state.settings.lock();
        match settings {
            Ok(settings) => settings.autostart,
            Err(_) => return,
        }
    };

    let manager = app.autolaunch();
    let current = manager.is_enabled().unwrap_or(false);
    if current == desired {
        return;
    }

    let result = if desired {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(e) = result {
        tracing::warn!(desired, error = %e, "could not change autostart registration");
    }
}
