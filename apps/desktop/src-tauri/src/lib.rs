//! Tauri host for Conflux.
//!
//! The app is a thin shell: it owns the tray icon, the settings window and the
//! global shortcuts, and forwards every action to [`conflux_engine::Engine`].
//! All window decision-making lives in the engine crate so it can be tested
//! without a desktop.

use tauri::Manager;

mod activity;
pub mod commands;
mod monitor;
pub mod platform;
mod schedule;
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
    /// Which global shortcuts actually registered, and why any failed.
    pub hotkey_status: Arc<std::sync::Mutex<commands::HotkeyStatusDto>>,
    /// Outcome of the most recent update check: (outcome, version, detail).
    pub last_update: Arc<std::sync::Mutex<Option<(String, String, String)>>>,
    /// Remote-session detector state. One tracker per process, because the
    /// activation streak must survive across polls.
    pub detector: Arc<std::sync::Mutex<conflux_detect::Tracker>>,
    /// The evidence string for the session that is currently active, if any.
    pub last_remote: Arc<std::sync::Mutex<Option<String>>>,
    /// Last logged evidence summary, so the monitor logs transitions rather than
    /// every poll.
    pub last_observation: Arc<std::sync::Mutex<Option<String>>>,
    /// Set while the user has marked the session by hand. In memory only: a
    /// marker that survived a restart would claim a remote session that ended
    /// with the previous process.
    pub manual_remote: Arc<std::sync::atomic::AtomicBool>,
    /// Owns the cancellable pre-restore wait. A new gather or restore cancels a
    /// pending one, so a stale timer cannot undo a fresh layout.
    pub schedule: Arc<std::sync::Mutex<schedule::Schedule>>,
    /// Append-only history of completed actions, shown on the overview.
    pub activity: Arc<std::sync::Mutex<activity::Activity>>,
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
            // Do not silently overwrite the user's file: it may be recoverable
            // by hand, and the next save would destroy the evidence. Keep a copy
            // next to it and start from defaults.
            tracing::error!(error = %e, "settings unreadable; preserving a copy");
            let backup = settings_path.with_extension("json.invalid");
            if let Err(copy_err) = std::fs::copy(&settings_path, &backup) {
                tracing::warn!(error = %copy_err, "could not preserve the unreadable file");
            } else {
                tracing::info!(path = %backup.display(), "unreadable settings preserved");
            }
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
            restore_delay_secs: settings.restore_delay_secs,
            excluded_apps: settings.excluded_apps.clone(),
            ..Default::default()
        };

        let mut engine = Engine::new(platform::make(), config.clone());
        // A persisted pause must survive a restart: the user paused because they
        // did not want automation acting, and a restart is not consent.
        if settings.paused {
            if let Err(e) = engine.pause() {
                tracing::warn!(error = %e, "could not restore paused state");
            }
        }
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
            hotkey_status: Arc::new(std::sync::Mutex::new(commands::HotkeyStatusDto::default())),
            last_update: Arc::new(std::sync::Mutex::new(None)),
            detector: Arc::new(std::sync::Mutex::new(conflux_detect::Tracker::new(
                conflux_detect::Sensitivity::Off,
            ))),
            last_remote: Arc::new(std::sync::Mutex::new(None)),
            last_observation: Arc::new(std::sync::Mutex::new(None)),
            manual_remote: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            schedule: Arc::new(std::sync::Mutex::new(schedule::Schedule::new())),
            activity: Arc::new(std::sync::Mutex::new(activity::Activity::load(
                conflux_settings::config_dir().join("activity.jsonl"),
            ))),
        }
    }
}

/// Initialise logging to a rotating file under the app's data directory.
///
/// A GUI build has no console on Windows, so the previous `fmt()` writer sent
/// every log line to a stderr nobody could read: the diagnostics export shipped
/// empty, and a failed migration left no trace. The file is the only logging
/// destination that survives a user restarting the app.
///
/// Rotation keeps five files of 2 MiB. Logs contain only state transitions,
/// counts and window handles — never titles, paths or screen content — so a
/// bounded history is safe to keep and to attach to a support request.
fn init_logging() {
    let dir = conflux_settings::config_dir().join("logs");
    let file = std::fs::create_dir_all(&dir).ok().and_then(|()| {
        tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .max_log_files(5)
            .filename_prefix("conflux")
            .filename_suffix("log")
            .build(&dir)
            .ok()
    });

    let Some(file) = file else {
        // Without a writable log directory the app must still run; fall back to
        // stderr so a terminal launch remains debuggable.
        tracing_subscriber::fmt()
            .with_env_filter(log_filter())
            .with_target(false)
            .init();
        return;
    };

    let (non_blocking, guard) = tracing_appender::non_blocking(file);
    // The guard flushes on drop; leaking it keeps the writer alive for the whole
    // process without threading an owned value through Tauri's builder.
    std::mem::forget(guard);

    tracing_subscriber::fmt()
        .with_env_filter(log_filter())
        .with_target(false)
        .with_ansi(false)
        .with_writer(non_blocking)
        .init();
}

/// Record a panic before the process dies.
///
/// A panic inside the engine leaves a journal mid-transaction. On the next
/// launch the recovery banner appears, but without this hook nothing explains
/// *why* it appeared: the log would simply stop. The hook writes the location and
/// message, then defers to the default hook so the process still aborts the way
/// the developer expects.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(
            location = %info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "<unknown>".into()),
            message = %info.payload().downcast_ref::<&str>().copied().unwrap_or("<non-string>"),
            "panic; any open window transaction is left for recovery on next launch"
        );
        previous(info);
    }));
}

fn log_filter() -> tracing_subscriber::EnvFilter {
    // `conflux=info` only matches a target literally named `conflux`. Every
    // crate here is named `conflux_*` (engine, journal, platform_win...), so that
    // filter silenced all of them: the log file existed and stayed two lines
    // long while migrations ran. The directive is per-crate instead.
    tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(concat!(
            "conflux_desktop_lib=info,conflux_engine=info,conflux_journal=info,",
            "conflux_platform=info,conflux_platform_win=info,conflux_platform_mac=info,",
            "conflux_settings=info,conflux_layout=info,warn"
        ))
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    install_panic_hook();

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
            commands::set_paused,
            commands::hotkey_status,
            commands::display_thumbnails,
            commands::running_applications,
            commands::identify_displays,
            commands::activity_log,
            commands::live_transaction,
        ])
        .setup(|app| {
            // A tray failure must not abort startup. Tauri turns a `setup` error
            // into a panic ("Failed to setup app"), which kills the process - and
            // the tray is an optional surface: shortcuts and the window still
            // work without it. The failure is logged and the app continues.
            if let Err(e) = tray::install(app.handle()) {
                tracing::error!(error = %e, "tray unavailable; continuing without it");
            }
            session::install(app.handle());

            // A manual launch opens the window; an autostart launch stays in the
            // tray. Users who double-click the app expect to see something.
            // The window is created visible so Windows gives it its configured
            // geometry; an autostart launch hides it again immediately, before
            // the first frame is presented. Creating it hidden instead left it
            // as a 160x28 stub on Windows.
            let launched_at_login = std::env::args().any(|arg| arg == "--tray");
            if launched_at_login {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            // Apply the persisted autostart preference on every launch: the
            // registration lives outside the settings file, so the two can drift
            // if the user removes it from Task Manager.
            session::apply_saved_autostart(app.handle());
            session::watch_topology(&app.handle().clone());
            update::check_in_background(app.handle().clone());
            monitor::install(app.handle());
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
