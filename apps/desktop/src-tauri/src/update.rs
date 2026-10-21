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
    let pending = pending_windows(&app);
    if pending > 0 {
        let _ = app.emit(
            "conflux://update",
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
        "conflux://update",
        serde_json::json!({ "outcome": "downloading", "version": version }),
    );

    match update
        .download_and_install(
            |chunk, total| {
                let _ = (chunk, total);
            },
            || {},
        )
        .await
    {
        Ok(()) => {
            let _ = app.emit(
                "conflux://update",
                serde_json::json!({ "outcome": "installed", "version": version }),
            );
            // Give the event a moment to reach the UI before restarting.
            tokio::time::sleep(Duration::from_millis(400)).await;
            app.restart();
        }
        Err(e) => UpdateOutcome::Failed {
            message: format!("install failed: {e}"),
        },
    }
}

/// How many windows are currently captured by an open transaction.
fn pending_windows(app: &AppHandle) -> usize {
    let state = app.state::<AppState>();
    let engine = state.engine.lock();
    match engine {
        Ok(engine) => engine.captured_window_count(),
        // A poisoned lock means another thread panicked while holding it. Treat
        // that as "unknown, assume unsafe": deferring an update is harmless,
        // restarting over a live transaction is not.
        Err(_) => 1,
    }
}

/// Spawn a background check at startup. Failures are only logged: an offline
/// machine must not show an error dialog on every launch.
pub fn check_in_background(app: AppHandle) {
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
