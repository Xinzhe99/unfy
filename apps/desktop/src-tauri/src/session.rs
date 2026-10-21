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
            // Both actions run on a worker thread. The shortcut callback runs on
            // the event loop, and a restore can legitimately sleep for the user's
            // configured delay (up to 300 s) or block on a busy application's
            // message pump. Doing that inline froze every subsequent shortcut.
            "hotkey_gather" => {
                let app = self.clone();
                std::thread::spawn(move || {
                    // Counters are filled inside the engine scope and read after
                    // the locks are dropped, so the history write never happens
                    // while holding the engine mutex.
                    let applied_count = std::sync::atomic::AtomicUsize::new(0);
                    let unsettled_count = std::sync::atomic::AtomicUsize::new(0);
                    let result: std::result::Result<(), String> = (|| {
                        let state = app.state::<AppState>();
                        let settings = state
                            .settings
                            .lock()
                            .map_err(|_| "settings lock".to_string())?;
                        let mut engine =
                            state.engine.lock().map_err(|_| "engine lock".to_string())?;
                        match engine.resolve_target(
                            settings
                                .target_display
                                .as_ref()
                                .map(|t| t.native_id.as_str()),
                        ) {
                            // Retarget, not migrate: pressing the shortcut while a
                            // layout is active means "redo it here", which is what
                            // the user sees happen when it works.
                            Some(key) => engine
                                .retarget(&key)
                                .map(|retarget| {
                                    let applied = retarget.migration.applied_count();
                                    let unsettled = retarget.migration.failed_count()
                                        + retarget.migration.unplaced.len();
                                    // Record inside the closure so the counts are
                                    // available; the guard is released right after.
                                    applied_count
                                        .store(applied, std::sync::atomic::Ordering::Relaxed);
                                    unsettled_count
                                        .store(unsettled, std::sync::atomic::Ordering::Relaxed);
                                })
                                .map_err(|e| e.to_string()),
                            None => Err("no displays".to_string()),
                        }
                    })();
                    if let Err(message) = &result {
                        tracing::warn!(error = %message, "hotkey gather failed");
                    }
                    if result.is_ok() {
                        crate::tray::record(
                            &app,
                            crate::activity::Kind::Gather,
                            applied_count.load(std::sync::atomic::Ordering::Relaxed),
                            unsettled_count.load(std::sync::atomic::Ordering::Relaxed),
                            "hotkey",
                        );
                    }
                    crate::tray::refresh(&app);
                    let _ = app.emit(
                        "conflux://action",
                        serde_json::json!({
                            "action": "gather",
                            "ok": result.is_ok(),
                            "message": result.err(),
                        }),
                    );
                });
                Ok(())
            }
            "hotkey_restore" => {
                let app = self.clone();
                std::thread::spawn(move || {
                    // One restore path for all three surfaces. Calling
                    // `engine.restore()` here bypassed the configured delay, so the
                    // shortcut fired immediately while the tray and the button both
                    // waited - the same action behaving differently depending on
                    // how it was invoked.
                    let state = app.state::<AppState>();
                    let outcome = tauri::async_runtime::block_on(
                        crate::commands::restore_after_delay(&app, &state, true),
                    );
                    match &outcome {
                        Ok(report) => {
                            crate::tray::record(
                                &app,
                                crate::activity::Kind::Restore,
                                report.restored + report.fallback_visible,
                                report.pending,
                                "hotkey",
                            );
                        }
                        Err(message) => {
                            // `restore_cancelled` means a newer request took over;
                            // that is not a failure worth alarming the user about.
                            if message != "restore_cancelled" {
                                tracing::warn!(error = %message, "hotkey restore failed");
                            }
                        }
                    }
                    crate::tray::refresh(&app);
                    let _ = app.emit(
                        "conflux://action",
                        serde_json::json!({
                            "action": "restore",
                            "ok": outcome.is_ok(),
                            "message": outcome.err(),
                        }),
                    );
                });
                Ok(())
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

/// Watch for display topology changes and rescue unreachable windows.
///
/// A window sitting on a monitor that was just unplugged is invisible and
/// unreachable: the user cannot drag it back because they cannot see it. That is
/// the worst failure this product can leave behind, so it is detected and
/// repaired without waiting for the user to notice.
///
/// Polling is used rather than a window message because the tray-only process has
/// no window of its own to receive `WM_DISPLAYCHANGE`, and the message would not
/// be delivered to a hidden window on every platform. Two seconds costs one
/// topology query — a handful of microseconds — and keeps the reaction time below
/// what a user would notice.
pub fn watch_topology<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut last = {
            let state = handle.state::<AppState>();
            let engine = state.engine.lock();
            match engine {
                Ok(engine) => engine.topology_signature(),
                Err(_) => None,
            }
        };

        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));

            let state = handle.state::<AppState>();
            let Ok(mut engine) = state.engine.lock() else {
                continue;
            };
            let current = engine.topology_signature();
            if current.is_none() || current == last {
                continue;
            }
            // First observation after startup: nothing to compare against yet.
            if last.is_none() {
                last = current;
                continue;
            }
            last = current;

            match engine.on_topology_changed() {
                Ok(rescued) => {
                    let count = rescued.len();
                    let captured = engine.captured_window_count();
                    let state = engine.state();
                    drop(engine);

                    if count > 0 {
                        tracing::warn!(count, "display topology changed; rescued windows");
                    } else {
                        tracing::info!("display topology changed; nothing needed rescuing");
                    }

                    use tauri::Emitter;
                    let _ = handle.emit(
                        "conflux://topology-changed",
                        serde_json::json!({
                            "rescued": count,
                            // A layout that survived a topology change is still
                            // aimed at the old display list. Telling the UI lets
                            // it offer "整理到目标屏" again, which is what the
                            // user wants when the monitor comes back or moves.
                            "needs_regather": captured > 0
                                && state == conflux_domain::EngineState::Active,
                            "captured": captured,
                        }),
                    );
                    crate::tray::refresh(&handle);
                }
                Err(e) => tracing::error!(error = %e, "topology rescue failed"),
            }
        }
    });
}
