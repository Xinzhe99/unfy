//! Update checking with a recovery barrier.
//!
//! # The rule this module exists to enforce
//!
//! An update must never install while a transaction is open. Installing restarts
//! the process, and the new process has no way to know that windows were captured
//! a moment ago — the journal is still there, but the user has already been
//! dropped into a restarted app with no explanation.
//!
//! So the order is always: stop new migrations → restore → verify the journal is
//! closed → install. If the restore cannot finish, the update is deferred and the
//! UI says why.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::AppState;

/// Result of one update check.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum UpdateOutcome {
    /// No update available.
    UpToDate { current: String },
    /// An update was found and installed; the app will restart.
    Installed { version: String },
    /// An update was found but deferred because windows are still captured.
    Deferred {
        version: String,
        pending_windows: usize,
    },
    /// The check itself failed (offline, bad signature, server error).
    Failed { message: String },
}

impl UpdateOutcome {
    /// Version involved in the outcome, or the running version for a no-op.
    pub fn version(&self) -> String {
        match self {
            UpdateOutcome::UpToDate { current } => current.clone(),
            UpdateOutcome::Installed { version } => version.clone(),
            UpdateOutcome::Deferred { version, .. } => version.clone(),
            UpdateOutcome::Failed { .. } => String::new(),
        }
    }

    /// Why the check failed, for the UI. Empty for every other outcome.
    ///
    /// Returning a bare "failed" left the user with a red line and no cause; the
    /// most common cause is a build with no network access or an update URL that
    /// the platform refuses (non-HTTPS), and both are actionable once named.
    pub fn detail(&self) -> String {
        match self {
            UpdateOutcome::Failed { message } => message.clone(),
            _ => String::new(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            UpdateOutcome::UpToDate { .. } => "up_to_date",
            UpdateOutcome::Installed { .. } => "installed",
            UpdateOutcome::Deferred { .. } => "deferred",
            UpdateOutcome::Failed { .. } => "failed",
        }
    }
}

/// Whether this build has an updater endpoint and signing key.
///
/// Both live in `tauri.conf.json` and are compiled into the binary. Until a
/// release workflow fills them in, every check fails at the transport layer;
/// reporting that as "更新失败" would look like a bug rather than a build that
/// was never configured. The check is derived from the same generated context the
/// plugin reads, so it cannot drift from the real configuration.
pub fn is_configured<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    let config = app.config();
    let updater = &config.plugins.0;
    let Some(settings) = updater.get("updater") else {
        return false;
    };
    let has_key = settings
        .get("pubkey")
        .and_then(|v| v.as_str())
        .is_some_and(|key| !key.trim().is_empty());
    let has_endpoint = settings
        .get("endpoints")
        .and_then(|v| v.as_array())
        .is_some_and(|list| !list.is_empty());
    has_key && has_endpoint
}

/// Check for an update and install it if it is safe to do so.
///
/// `force` skips the "user disabled auto update" preference, not the recovery
/// barrier: a security update still waits for a clean journal.
pub async fn check_and_install(app: AppHandle, force: bool) -> UpdateOutcome {
    if !force {
        let enabled = {
            let state = app.state::<AppState>();
            let settings = state.settings.lock();
            match settings {
                Ok(settings) => settings.auto_update,
                Err(_) => false,
            }
        };
        if !enabled {
            return UpdateOutcome::UpToDate {
                current: app.package_info().version.to_string(),
            };
        }
    }

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(e) => {
            return UpdateOutcome::Failed {
                message: format!("updater unavailable: {e}"),
            }
        }
    };

    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => {
            return UpdateOutcome::UpToDate {
                current: app.package_info().version.to_string(),
            }
        }
        Err(e) => {
            return UpdateOutcome::Failed {
                message: format!("update check failed: {e}"),
            }
        }
    };

    let version = update.version.clone();

    // Recovery barrier: an open transaction must be resolved before we restart.
    // The count includes journals the engine did not adopt, so an unreadable or
    // foreign record blocks the update instead of being silently ignored.
    let pending = pending_windows(&app);
    if pending > 0 {
        let _ = app.emit(
            "unfy://update",
            serde_json::json!({
                "outcome": "deferred",
                "version": version,
                "pending_windows": pending,
            }),
        );
        return UpdateOutcome::Deferred {
            version,
            pending_windows: pending,
        };
    }

    let _ = app.emit(
        "unfy://update",
        serde_json::json!({ "outcome": "downloading", "version": version }),
    );

    // Download first. The barrier is re-taken *after* the download, immediately
    // before the installer runs: the download can take minutes, and a gather that
    // started while it ran must be seen before the process is replaced.
    let bytes = match update.download(|_, _| {}, || {}).await {
        Ok(bytes) => bytes,
        Err(e) => {
            return UpdateOutcome::Failed {
                message: format!("download failed: {e}"),
            };
        }
    };

    let _ = app.emit(
        "unfy://update",
        serde_json::json!({ "outcome": "downloaded", "version": version }),
    );

    if let Err(outcome) = acquire_install_barrier(&app, &version).await {
        return outcome;
    }

    match update.install(bytes) {
        Ok(()) => {
            let _ = app.emit(
                "unfy://update",
                serde_json::json!({ "outcome": "installed", "version": version }),
            );
            tokio::time::sleep(Duration::from_millis(400)).await;
            app.restart();
        }
        Err(e) => {
            release_install_barrier(&app);
            UpdateOutcome::Failed {
                message: format!("install failed: {e}"),
            }
        }
    }
}

/// Close admission, restore every window, and verify there is nothing left.
///
/// Returns `Err(outcome)` when the update must not proceed. Admission is reopened
/// on every failure path, because a deferred update must leave a working app.
async fn acquire_install_barrier(app: &AppHandle, version: &str) -> Result<(), UpdateOutcome> {
    let state = app.state::<AppState>();
    state
        .maintenance
        .store(true, std::sync::atomic::Ordering::SeqCst);
    // Invalidate timers before the restore so a delayed automatic action cannot
    // start behind it.
    if let Ok(mut schedule) = state.schedule.lock() {
        schedule.cancel();
    }

    let report = {
        let engine = state.engine.clone();
        tokio::task::spawn_blocking(move || {
            let mut engine = engine.lock().map_err(|e| e.to_string())?;
            engine.restore().map_err(|e| e.to_string())
        })
        .await
    };

    match report {
        Ok(Ok(report)) if report.is_complete() => {
            // Re-count from disk: the restore only resolved what the engine knew.
            let pending = pending_windows(app);
            if pending == 0 {
                Ok(())
            } else {
                release_install_barrier(app);
                Err(UpdateOutcome::Deferred {
                    version: version.to_string(),
                    pending_windows: pending,
                })
            }
        }
        Ok(Ok(report)) => {
            release_install_barrier(app);
            Err(UpdateOutcome::Deferred {
                version: version.to_string(),
                pending_windows: report.outstanding(),
            })
        }
        Ok(Err(e)) => {
            release_install_barrier(app);
            Err(UpdateOutcome::Failed {
                message: format!("restore before install failed: {e}"),
            })
        }
        Err(e) => {
            release_install_barrier(app);
            Err(UpdateOutcome::Failed {
                message: format!("restore task failed: {e}"),
            })
        }
    }
}

fn release_install_barrier(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state
            .maintenance
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// How many windows still owe a restore.
///
/// Counts unresolved windows in the engine *and* unfinished transactions on disk
/// that the engine does not own. An unreadable journal is treated as pending: the
/// old count only looked at memory, so a crash-adopted file that could not be
/// read let the updater restart the app over an unknown recovery obligation.
fn pending_windows(app: &AppHandle) -> usize {
    let state = app.state::<AppState>();
    let (owned, in_memory) = match state.engine.lock() {
        Ok(engine) => (
            engine.current_transaction(),
            engine.unresolved_window_count(),
        ),
        // A poisoned lock means another thread panicked while holding it. Treat
        // that as "unknown, assume unsafe".
        Err(_) => return 1,
    };

    let mut pending = in_memory;
    let dir = unfy_settings::config_dir().join("journal");
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "journal") {
                continue;
            }
            match unfy_journal::recover(&path) {
                Ok(unfy_journal::Recovered::Unfinished(tx)) => {
                    if Some(tx.transaction) == owned {
                        continue;
                    }
                    pending += tx.touched_windows().len();
                }
                Ok(unfy_journal::Recovered::Clean) => {}
                Err(_) => {
                    // Unreadable is not zero. Blocking the update is recoverable;
                    // restarting over it is not.
                    pending += 1;
                }
            }
        }
    }
    pending
}

/// Spawn a background check at startup. Failures are only logged: an offline
/// machine must not show an error dialog on every launch.
pub fn check_in_background(app: AppHandle) {
    // An unconfigured build has no endpoint and no signing key. Running the check
    // anyway produced a logged failure on every single launch, which trains both
    // the user and the developer to ignore the word "failed" in the log. Skipping
    // is the honest behaviour: there is nothing to check.
    if !is_configured(&app) {
        tracing::debug!("update check skipped: no endpoint or signing key configured");
        return;
    }
    tauri::async_runtime::spawn(async move {
        let outcome = check_and_install(app.clone(), false).await;
        tracing::info!(outcome = outcome.as_str(), "update check finished");

        if let UpdateOutcome::Deferred {
            version,
            pending_windows,
        } = &outcome
        {
            tracing::warn!(
                %version,
                pending_windows,
                "update deferred until windows are restored"
            );
        }
    });
}
