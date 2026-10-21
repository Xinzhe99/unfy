//! Tauri host for Conflux.
//!
//! The app is a thin shell: it owns the tray icon, the settings window and the
//! global shortcuts, and forwards every action to [`conflux_engine::Engine`].
//! All window decision-making lives in the engine crate so it can be tested
//! without a desktop.

use tauri::Manager;

pub mod commands;
pub mod platform;
pub mod session;
pub mod tray;
pub mod update;

use std::sync::Arc;

use conflux_engine::{Engine, EngineConfig};
use conflux_settings::Settings;

/// Application-wide state shared by commands and the tray.
pub struct AppState {
    /// The engine is behind a mutex because Tauri commands may arrive on any
    /// thread, while the engine is a single-writer state machine.
    pub engine: Arc<std::sync::Mutex<Engine<platform::Backend>>>,
    pub settings: Arc<std::sync::Mutex<Settings>>,
    pub settings_path: std::path::PathBuf,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let settings_path = Settings::default_path();
        let settings = Settings::load(&settings_path).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "settings unreadable, starting from defaults");
            Settings::default()
        });

        let config = EngineConfig {
            layout: conflux_layout::LayoutConfig {
                mode: settings.layout_mode.into(),
                margin: settings.margin,
                gap: settings.gap,
                ..Default::default()
            },
            journal_dir: conflux_settings::config_dir().join("journal"),
            ..Default::default()
        };

        let mut engine = Engine::new(platform::make(), config.clone());
        // Take over any transaction a previous run left behind, before the UI
        // can offer a restore that would otherwise have nothing to restore.
        match engine.recover_latest(&config.journal_dir) {
            Ok(Some(transaction)) => {
                tracing::info!(%transaction, "adopted unfinished transaction")
            }
            Ok(None) => {}
            Err(e) => tracing::error!(error = %e, "could not read previous journal"),
        }

        Self {
            engine: Arc::new(std::sync::Mutex::new(engine)),
            settings: Arc::new(std::sync::Mutex::new(settings)),
            settings_path,
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("conflux=info")),
        )
        .with_target(false)
        .init();

    tauri::Builder::default()
        // Must be the first plugin: a second instance has to hand off and exit
        // before it can touch the journal directory or the global shortcuts.
        // Two writers would break the engine's single-writer invariant, which is
        // what makes "preimage before move" trustworthy.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let _ = (args, _cwd);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // Autostart launches pass `--tray`; a manual launch does not. That
            // is the only reliable way to tell "user double-clicked the app"
            // from "the OS started it at login", which decides whether the
            // settings window is shown.
            Some(vec!["--tray"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::list_displays,
            commands::preview,
            commands::migrate,
            commands::restore,
            commands::rescue_offscreen,
            commands::get_settings,
            commands::save_settings,
            commands::set_target_display,
            commands::pending_work,
            commands::dismiss_pending_work,
            commands::export_diagnostics,
            commands::request_permission,
            commands::open_permission_settings,
            commands::update_status,
            commands::check_update,
            commands::live_transaction,
        ])
        .setup(|app| {
            tray::install(app.handle())?;
            session::install(app.handle())?;

            // A manual launch opens the window; an autostart launch stays in the
            // tray. Users who double-click the app expect to see something.
            let launched_at_login = std::env::args().any(|arg| arg == "--tray");
            if !launched_at_login {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            // Apply the persisted autostart preference on every launch: the
            // registration lives outside the settings file, so the two can drift
            // if the user removes it from Task Manager.
            session::apply_saved_autostart(app.handle());
            update::check_in_background(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the settings window should hide it, not exit: the tray and
            // shortcuts must keep working.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Conflux");
}

/// Convenience for tests and for the headless probe.
pub fn app_state() -> AppState {
    AppState::new()
}
