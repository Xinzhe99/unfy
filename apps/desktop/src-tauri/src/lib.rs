//! Tauri host for Unfy.
//!
//! The app is a thin shell: it owns the tray icon, the settings window and the
//! global shortcuts, and forwards every action to [`unfy_engine::Engine`].
//! All window decision-making lives in the engine crate so it can be tested
//! without a desktop.

use tauri::Manager;

mod activity;
pub mod commands;
mod message;
mod monitor;
pub mod platform;
mod schedule;
pub mod session;
pub mod tray;
pub mod ui_text;
pub mod update;

use std::sync::Arc;

use unfy_engine::Engine;
use unfy_settings::Settings;

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
    pub detector: Arc<std::sync::Mutex<unfy_detect::Tracker>>,
    /// Last logged evidence summary, so the monitor logs transitions rather than
    /// every poll.
    pub last_observation: Arc<std::sync::Mutex<Option<String>>>,
    /// True when the settings file could not be read at startup and the app fell
    /// back to defaults.
    ///
    /// The window has to say so. Without it the first thing the user sees is the
    /// onboarding guide they already completed, every exclusion rule gone and the
    /// shortcuts unset - indistinguishable from the app having thrown their
    /// configuration away. The original file is kept as `settings.json.invalid`,
    /// and the banner is how they learn it exists.
    pub settings_recovered: bool,
    /// When the last automatic gather ran, for the spec's 30-second cooldown.
    ///
    /// A session that flaps - connect, drop, connect - produces a sequence of
    /// `Activated`/`Deactivated` transitions. The detector requires consecutive
    /// polls for weak evidence, which filters a single flaky sample but not a
    /// genuine two-second outage. The cooldown is the second layer: at most one
    /// automatic gather per 30 seconds, whatever the evidence does.
    pub last_auto_gather: Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    /// When the storage probe last ran, as seconds since the process started.
    ///
    /// The probe writes and deletes a file; `health_check` is polled every four
    /// seconds while the window is open, so running it each time meant constant
    /// disk churn for a fact that changes roughly never.
    pub storage_probe: Arc<std::sync::Mutex<Option<(std::time::Instant, bool)>>>,
    /// Owns the cancellable pre-restore wait. A new gather or restore cancels a
    /// pending one, so a stale timer cannot undo a fresh layout.
    pub schedule: Arc<std::sync::Mutex<schedule::Schedule>>,
    /// Append-only history of completed actions, shown on the overview.
    pub activity: Arc<std::sync::Mutex<activity::Activity>>,
    /// The interface language the tray renders in.
    ///
    /// The webview owns the rule that resolves `system` (it reads
    /// `navigator.language`), and reports the result through `set_ui_locale`. Two
    /// independent resolutions could disagree, which would show a Chinese tray
    /// beside an English window.
    pub ui_locale: Arc<std::sync::Mutex<Option<ui_text::Locale>>>,
    /// The most recent operational failure, cleared on the next success.
    ///
    /// Kept here rather than in the engine because not every failure originates
    /// there: a hotkey that fails to register, an update that cannot download, and
    /// a restore that leaves windows behind all matter to the user and none of
    /// them is an `EngineError`. The overview shows this so a problem does not
    /// live only in the log.
    pub last_error: Arc<std::sync::Mutex<Option<crate::message::Message>>>,
    /// Set while the process is shutting down or about to install an update.
    ///
    /// Every write entry point checks it *after* taking the engine lock, so a
    /// request that was already queued cannot start moving windows while the
    /// recovery pass is running. Without this, a hotkey pressed during the exit
    /// restore could begin a new gather behind it.
    pub maintenance: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let settings_path = Settings::default_path();
        let mut settings_recovered = false;
        let settings = Settings::load(&settings_path).unwrap_or_else(|e| {
            settings_recovered = true;
            // Do not silently overwrite the user's file: it may be recoverable
            // by hand, and the next save would destroy the evidence. Keep a copy
            // next to it and start from defaults.
            tracing::error!(error = %e, "settings unreadable; preserving a copy");
            let backup = settings_path.with_extension("json.invalid");
            if let Err(copy_err) = std::fs::copy(&settings_path, &backup) {
                tracing::warn!(error = %copy_err, "could not preserve the unreadable file");
            } else {
                tracing::info!(path = %redact_home(&backup), "unreadable settings preserved");
            }
            Settings::default()
        });

        // One conversion, shared with every later save and reset. Two hardcoded
        // copies of the defaults is how a reset came to write a different layout
        // into the live engine than the one it saved to disk.
        let config = crate::commands::engine_config_from_settings(&settings);

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

        // Read before `settings` is moved into the state below.
        let ui_locale = ui_text::Locale::from_setting(&settings.language);

        Self {
            engine: Arc::new(std::sync::Mutex::new(engine)),
            settings: Arc::new(std::sync::Mutex::new(settings)),
            settings_path,
            hotkey_status: Arc::new(std::sync::Mutex::new(commands::HotkeyStatusDto::default())),
            last_update: Arc::new(std::sync::Mutex::new(None)),
            detector: Arc::new(std::sync::Mutex::new(unfy_detect::Tracker::new(
                unfy_detect::Sensitivity::Off,
            ))),
            last_observation: Arc::new(std::sync::Mutex::new(None)),
            settings_recovered,
            last_auto_gather: Arc::new(std::sync::Mutex::new(None)),
            storage_probe: Arc::new(std::sync::Mutex::new(None)),
            schedule: Arc::new(std::sync::Mutex::new(schedule::Schedule::new())),
            ui_locale: Arc::new(std::sync::Mutex::new(ui_locale)),
            last_error: Arc::new(std::sync::Mutex::new(None)),
            maintenance: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            activity: Arc::new(std::sync::Mutex::new(activity::Activity::load(
                unfy_settings::config_dir().join("activity.jsonl"),
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
/// A path with the user's home directory replaced by `~`.
///
/// The logs are what a user attaches to a support request, and the policy they are
/// written under says no paths. The useful part of these two is which binary and
/// which file, not whose profile directory they sit in - and a profile directory
/// carries the user's name.
pub(crate) fn redact_home(path: &std::path::Path) -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok();
    redact_home_in(path, home.as_deref())
}

/// The same rule with the home directory supplied, so it can be tested without
/// touching the process environment.
fn redact_home_in(path: &std::path::Path, home: Option<&str>) -> String {
    let text = path.display().to_string();
    match home {
        Some(home) if !home.is_empty() => text.replacen(home, "~", 1),
        _ => text,
    }
}

fn init_logging() {
    let dir = unfy_settings::config_dir().join("logs");
    let file = std::fs::create_dir_all(&dir).ok().and_then(|()| {
        tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .max_log_files(5)
            .filename_prefix("unfy")
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
    // `unfy=info` only matches a target literally named `unfy`. Every
    // crate here is named `unfy_*` (engine, journal, platform_win...), so that
    // filter silenced all of them: the log file existed and stayed two lines
    // long while migrations ran. The directive is per-crate instead.
    tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(concat!(
            "unfy_desktop_lib=info,unfy_engine=info,unfy_journal=info,",
            "unfy_platform=info,unfy_platform_win=info,unfy_platform_mac=info,",
            "unfy_settings=info,unfy_layout=info,warn"
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
                show_window(&window);
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
            commands::window_visible,
            commands::get_status,
            commands::identify_displays,
            commands::list_displays,
            commands::preview,
            commands::migrate,
            commands::restore,
            commands::rescue_offscreen,
            commands::get_settings,
            commands::save_settings,
            commands::set_ui_locale,
            commands::set_target_display,
            commands::pending_work,
            commands::dismiss_pending_work,
            commands::export_diagnostics,
            commands::preview_diagnostics,
            commands::request_permission,
            commands::open_permission_settings,
            commands::update_status,
            commands::check_update,
            commands::set_paused,
            commands::hotkey_status,
            commands::display_thumbnails,
            commands::running_applications,
            commands::activity_log,
            commands::clear_activity,
            commands::health_check,
            commands::keep_current_layout,
            commands::dismiss_last_error,
            commands::open_app_folder,
            commands::reset_settings,
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

            // Restore the last size and position. Applied before the autostart
            // hide so a login launch keeps the geometry for when the user opens
            // the window later.
            restore_window_geometry(app.handle());
            apply_window_theme(app.handle());

            // A manual launch opens the window; an autostart launch stays in the
            // tray. Users who double-click the app expect to see something.
            // The window is created visible so Windows gives it its configured
            // geometry; an autostart launch hides it again immediately, before
            // the first frame is presented. Creating it hidden instead left it
            // as a 160x28 stub on Windows.
            let launched_at_login = std::env::args().any(|arg| arg == "--tray");
            if launched_at_login {
                if let Some(window) = app.get_webview_window("main") {
                    hide_window(&window);
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
                // Capture geometry before hiding: the window still has its real
                // size here, and hiding it is what the user just asked for.
                persist_window_geometry(window.app_handle());
                // `window` here is a `Window`, not a `WebviewWindow`; hide and
                // notify inline.
                let _ = window.hide();
                use tauri::Emitter;
                let _ = window.app_handle().emit("unfy://window-hidden", ());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Unfy");
}

/// Hide the window and tell the UI, so it can stop polling.
///
/// `document.visibilityState` does not change when Tauri hides a window: measuring
/// CPU while hidden showed the polling rate unchanged. The frontend therefore
/// cannot rely on the web `visibilitychange` event and needs an explicit signal.
/// Move the settings window back on screen if a display change stranded it.
///
/// Called from the topology watcher, which already runs whenever the monitor set changes.
pub fn rescue_own_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    let rect = (
        position.x,
        position.y,
        size.width as i32,
        size.height as i32,
    );
    let monitors: Vec<MonitorRect> = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width as i32, s.height as i32)
        })
        .collect();

    let Some((x, y)) = rescue_target(rect, &monitors) else {
        return;
    };
    tracing::warn!(
        from_x = rect.0,
        from_y = rect.1,
        x,
        y,
        "settings window was off-screen"
    );
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

/// Match the window frame to the saved theme, before the webview draws anything.
///
/// The frame is drawn by the operating system, and the frontend only reaches
/// `setTheme` after the settings arrive over IPC - so a dark-theme user watched a light
/// title bar until then. Measured on this machine: the first capture after launch shows
/// the title bar as (243,243,243), the next as (32,32,32). The settings are already in
/// hand here, so the gap shrinks to however long Tauri takes to create the window.
fn apply_window_theme(app: &tauri::AppHandle) {
    let theme = {
        let state = app.state::<AppState>();
        let Ok(settings) = state.settings.lock() else {
            return;
        };
        settings.theme
    };
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let value = match theme {
        unfy_settings::ThemeName::Light => Some(tauri::Theme::Light),
        unfy_settings::ThemeName::Dark => Some(tauri::Theme::Dark),
        // `null` hands the decision back to the system, which is what "follow the
        // system" means at this level.
        unfy_settings::ThemeName::System => None,
    };
    let _ = window.set_theme(value);
}

/// Show the settings window and tell the UI it is back.
///
/// Three places reveal this window - the tray's settings item, the tray's left-click
/// toggle, and a second instance handing off - and each used to do its own three calls.
/// Only one of them told the frontend, so a window reopened the other two ways could keep
/// showing stale data. One function means the next caller cannot forget.
pub fn show_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use tauri::Emitter;
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    let _ = window.app_handle().emit("unfy://window-shown", ());
}

pub fn hide_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use tauri::Emitter;
    let _ = window.hide();
    let _ = window.app_handle().emit("unfy://window-hidden", ());
}

/// Apply the persisted window geometry, if it still lands on a visible display.
///
/// A monitor can be unplugged between sessions, and a window restored onto a
/// display that no longer exists opens off-screen with no way to reach it. The
/// saved rect is therefore clamped into the union of the current work areas
/// before it is applied; if no display contains it at all, the configured default
/// is kept instead.
fn restore_window_geometry(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let (saved, was_maximized) = match app.state::<AppState>().settings.lock() {
        Ok(settings) => (settings.window_geometry, settings.window_maximized),
        Err(_) => return,
    };
    let Some((x, y, width, height)) = saved else {
        return;
    };

    // Find the monitor the saved position lands on, and remember its size: the saved
    // geometry can be larger than the screen it is restored onto - a window last used
    // on a 4K display, reopened on a laptop panel - and setting a 3000x1600 window on a
    // 1920x1080 screen leaves parts of it unreachable.
    // Require a meaningful overlap - see `is_reachable`, shared with the topology rescue
    // so the two cannot disagree about what "off-screen" means.
    let host = app.available_monitors().ok().and_then(|monitors| {
        monitors.into_iter().find(|monitor| {
            let pos = monitor.position();
            let size = monitor.size();
            is_reachable(
                (x, y, width, height),
                &[(pos.x, pos.y, size.width as i32, size.height as i32)],
            )
        })
    });

    let Some(monitor) = host else {
        tracing::info!("saved window geometry is off-screen; keeping the default");
        return;
    };

    let bounds = monitor.size();
    let origin = monitor.position();
    // The border the window adds around its content. Measured rather than assumed: it is
    // 16x39 on this machine but differs with theme and DPI.
    let frame = window
        .outer_size()
        .ok()
        .zip(window.inner_size().ok())
        .map(|(outer, inner)| {
            (
                outer.width.saturating_sub(inner.width) as i32,
                outer.height.saturating_sub(inner.height) as i32,
            )
        })
        .unwrap_or((0, 0));
    // The minimum comes from the same config the window was built from, so changing
    // `minWidth` in tauri.conf.json cannot leave this function behind.
    let config = app.config();
    let window_cfg = config.app.windows.first();
    let minimum = (
        window_cfg
            .and_then(|w| w.min_width)
            .map(|v| v as i32)
            .unwrap_or(0),
        window_cfg
            .and_then(|w| w.min_height)
            .map(|v| v as i32)
            .unwrap_or(0),
    );

    let (x, y, width, height) = clamp_to_monitor(
        (x, y, width, height),
        (origin.x, origin.y),
        (bounds.width as i32, bounds.height as i32),
        frame,
        minimum,
    );

    let _ = window.set_size(tauri::PhysicalSize::new(width as u32, height as u32));
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));

    // Maximise after placing, never instead of it: the rectangle above is the size the
    // window returns to when the user un-maximises, and Windows takes the current
    // rectangle as that restore size.
    if was_maximized {
        let _ = window.maximize();
    }
}

/// A monitor's rectangle: `(left, top, width, height)`.
type MonitorRect = (i32, i32, i32, i32);

/// Whether a window would have a draggable strip on some monitor.
///
/// Both the startup restore and the topology rescue ask this, and the rule has to be the
/// same in both places: a window whose title bar is off the top edge cannot be dragged
/// back, and one narrower than the grab margin is not reachable either.
fn is_reachable(rect: (i32, i32, i32, i32), monitors: &[MonitorRect]) -> bool {
    // Height is not part of the test: the question is whether a grabbable strip is on
    // screen, and 40 px of title bar is enough regardless of how tall the window is.
    let (x, y, w, _height) = rect;
    monitors
        .iter()
        .any(|(mx, my, mw, mh)| x + w.min(200) > *mx && x < mx + mw && y + 40 > *my && y < my + mh)
}

/// Where to move a stranded window, or `None` when it is already reachable.
///
/// The engine rescues other applications' windows and deliberately skips this process, so
/// a display unplugged while the settings window sat on it left the window at a coordinate
/// no monitor covers. The tray can show it, but showing does not move it - and a tool whose
/// whole purpose is rescuing stranded windows should not strand itself.
fn rescue_target(rect: (i32, i32, i32, i32), monitors: &[MonitorRect]) -> Option<(i32, i32)> {
    if is_reachable(rect, monitors) {
        return None;
    }
    let (mx, my, mw, mh) = monitors.first().copied()?;
    let (x, y, w, h) = rect;
    // Move it wholly onto the first monitor, the same way the startup restore clamps.
    let (w, h) = (w.min(mw), h.min(mh));
    let x = x.clamp(mx, (mx + mw - w).max(mx));
    let y = y.clamp(my, (my + mh - h).max(my));
    Some((x, y))
}

/// Where a restored window should land, given the saved geometry and the monitor.
///
/// Pure so the four constraints can be tested without a window: the size must fit the
/// monitor's inner area, must not drop below the window's own minimum, and the position
/// must keep the *outer* frame on screen. Each of those was found separately by measuring
/// a running window, which is why they are all in one place now.
///
/// `frame` is the outer-minus-inner difference: the border the window adds around its
/// content. `bounds` is the monitor's size, `origin` its top-left corner.
fn clamp_to_monitor(
    saved: (i32, i32, i32, i32),
    origin: (i32, i32),
    bounds: (i32, i32),
    frame: (i32, i32),
    minimum: (i32, i32),
) -> (i32, i32, i32, i32) {
    let (x, y, width, height) = saved;
    let (ox, oy) = origin;
    let (bw, bh) = bounds;
    let (fw, fh) = frame;
    let (min_w, min_h) = minimum;

    let room_w = bw - fw;
    let room_h = bh - fh;
    // Upper bound first, then the floor: on a monitor smaller than the minimum the two
    // would otherwise fight, and a window slightly too large is the lesser problem.
    let width = width.min(room_w).max(min_w.min(room_w));
    let height = height.min(room_h).max(min_h.min(room_h));

    // `set_position` places the outer top-left, so the limits use the outer size.
    let max_x = ox + bw - (width + fw);
    let max_y = oy + bh - (height + fh);
    let x = x.clamp(ox, max_x.max(ox));
    let y = y.clamp(oy, max_y.max(oy));
    (x, y, width, height)
}

/// Remember the window's size and position.
///
/// Called from the close handler rather than the exit path: closing the window is
/// the moment the user's intent is expressed, and the tray keeps running after it,
/// so the geometry has to be captured while it still exists.
fn persist_window_geometry(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let (Ok(size), Ok(position)) = (window.inner_size(), window.outer_position()) else {
        return;
    };
    let state = app.state::<AppState>();
    let Ok(mut settings) = state.settings.lock() else {
        return;
    };
    let maximized = window.is_maximized().unwrap_or(false);
    let geometry = (
        position.x,
        position.y,
        size.width as i32,
        size.height as i32,
    );
    // While maximised, the reported rectangle is the maximised one. Keeping the previous
    // rectangle means the next launch restores the size the user gets from "restore",
    // which is what Windows itself does with `WINDOWPLACEMENT`.
    if (maximized && settings.window_maximized) || settings.window_geometry == Some(geometry) {
        if settings.window_maximized != maximized {
            settings.window_maximized = maximized;
            if let Err(e) = settings.save(&state.settings_path) {
                tracing::warn!(error = %e, "could not persist window state");
            }
        }
        return;
    }
    if !maximized {
        settings.window_geometry = Some(geometry);
    }
    settings.window_maximized = maximized;
    if let Err(e) = settings.save(&state.settings_path) {
        tracing::warn!(error = %e, "could not persist window geometry");
    }
}

/// Convenience for tests and for the headless probe.
pub fn app_state() -> AppState {
    AppState::new()
}

#[cfg(test)]
mod lock_order {
    //! The two state locks have one legal order.
    //!
    //! `std::sync::Mutex` is not reentrant. The command paths hold the engine while
    //! reading settings, and `get_status` runs on the main thread every four seconds,
    //! so a path that holds the settings *guard* and then asks for the engine
    //! deadlocks against it: the window stops answering and the other thread waits
    //! forever. That is exactly what happened once - `monitor::gather` took the
    //! settings first - and the window froze during an automatic gather.
    //!
    //! Reading the source is unusual for a test, but this rule is about the shape of
    //! the code rather than its behaviour: no single path can show a deadlock, and a
    //! comment asking politely is not a mechanism.

    fn sources() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).expect("read src") {
            let path = entry.expect("entry").path();
            if path.extension().is_some_and(|e| e == "rs") {
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                out.push((name, std::fs::read_to_string(&path).expect("read file")));
            }
        }
        out
    }

    fn function_name(line: &str) -> Option<String> {
        let rest = line.split("fn ").nth(1)?;
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        (!name.is_empty()).then_some(name)
    }

    /// The variable a `let` binds a lock to, when the lock is taken by a plain
    /// `let name = …lock()…` rather than inside a `match`, which clones and drops
    /// the guard within the statement.
    fn bound_guard(line: &str) -> Option<&str> {
        let trimmed = line.trim_start();
        let rest = trimmed.strip_prefix("let ")?;
        if rest.contains("match ") || rest.contains("if let ") {
            return None;
        }
        let name = rest.strip_prefix("mut ").unwrap_or(rest);
        let name = name
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()?;
        (!name.is_empty()).then_some(name)
    }

    /// Indentation of a line, in spaces. `cargo fmt` keeps this consistent, which is
    /// what makes it usable as a scope check.
    fn indent(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    #[test]
    fn settings_are_never_held_while_the_engine_is_taken() {
        let mut offenders = Vec::new();
        let mut checked = 0usize;
        for (name, text) in sources() {
            let mut function = String::from("(top level)");
            // Guards the scanner believes are alive, with the indentation they were
            // bound at. A `}` that closes at or above that level ends their scope.
            let mut held: Vec<(String, usize)> = Vec::new();
            for line in text.lines() {
                if let Some(found) = function_name(line.trim_start()) {
                    function = found;
                    held.clear();
                    continue;
                }
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                if trimmed.starts_with('}') && trimmed.len() <= 2 {
                    let here = indent(line);
                    held.retain(|(_, bound)| *bound <= here);
                }
                if trimmed.contains(".settings.lock()") {
                    if let Some(guard) = bound_guard(line) {
                        held.push((guard.to_string(), indent(line)));
                    }
                }
                for (guard, _) in held.clone() {
                    if line.contains(&format!("drop({guard})")) {
                        held.retain(|(g, _)| g != &guard);
                    }
                }
                if trimmed.contains(".engine.lock()") {
                    checked += 1;
                    if !held.is_empty() {
                        offenders.push(format!("{name}:{function} (holds {held:?})"));
                    }
                }
            }
        }
        // A floor, so the rule cannot quietly stop matching anything.
        assert!(checked > 8, "the scan found only {checked} engine locks");
        assert_eq!(
            offenders,
            Vec::<String>::new(),
            "these hold the settings guard while taking the engine, which deadlocks \n\
             against the status poll: take the engine first"
        );
    }

    /// Which functions take either state lock, by name.
    fn lock_takers(sources: &[(String, String)]) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (file, text) in sources {
            let mut function = String::new();
            for line in text.lines() {
                if let Some(found) = function_name(line.trim_start()) {
                    function = found;
                }
                if !function.is_empty()
                    && (line.contains(".engine.lock()") || line.contains(".settings.lock()"))
                {
                    out.push((file.clone(), function.clone()));
                }
            }
        }
        out
    }

    /// Calling a function that takes a lock, while holding one, deadlocks the same way
    /// a direct second `lock()` does - and the direct check above cannot see it.
    ///
    /// This is the shape that actually bit: `diagnostics_report` held both guards and
    /// called `pending_work`, which takes them itself. The call sits eighty lines below
    /// the guards, so nothing about the line looked wrong.
    #[test]
    fn a_held_guard_is_not_followed_by_a_call_that_locks() {
        let all = sources();
        let takers: Vec<String> = {
            let mut v: Vec<String> = lock_takers(&all).into_iter().map(|(_, f)| f).collect();
            v.sort();
            v.dedup();
            v
        };
        assert!(
            takers.len() > 4,
            "the scan found only {} locking functions",
            takers.len()
        );

        let mut offenders = Vec::new();
        for (name, text) in &all {
            let mut function = String::from("(top level)");
            let mut held: Vec<(String, usize)> = Vec::new();
            for line in text.lines() {
                if let Some(found) = function_name(line.trim_start()) {
                    function = found;
                    held.clear();
                    continue;
                }
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                if trimmed.starts_with('}') && trimmed.len() <= 2 {
                    let here = indent(line);
                    held.retain(|(_, bound)| *bound <= here);
                }
                if trimmed.contains(".settings.lock()") || trimmed.contains(".engine.lock()") {
                    if let Some(guard) = bound_guard(line) {
                        held.push((guard.to_string(), indent(line)));
                    }
                }
                for (guard, _) in held.clone() {
                    if line.contains(&format!("drop({guard})")) {
                        held.retain(|(g, _)| g != &guard);
                    }
                }
                if held.is_empty() {
                    continue;
                }
                // A call to a function that locks, while a guard is alive. Skip the
                // function's own name: recursion is not what this is about, and the
                // `.lock()` lines themselves are handled above.
                for taker in &takers {
                    if taker == &function {
                        continue;
                    }
                    // A call, not a method on something else: `engine.restore()` is the
                    // engine's own method, and `has_pending_work()` merely ends with the
                    // name `pending_work`. Both matched a plain substring test and
                    // produced three false alarms before this.
                    let call = format!("{taker}(");
                    let at = match line.find(&call) {
                        Some(at) => at,
                        None => continue,
                    };
                    let before = line[..at].chars().next_back();
                    if matches!(before, Some('.') | Some('_') | Some(':') | Some('>')) {
                        continue;
                    }
                    offenders.push(format!(
                        "{name}:{function} holds {held:?} then calls {taker}"
                    ));
                }
            }
        }
        offenders.sort();
        offenders.dedup();
        assert_eq!(
            offenders,
            Vec::<String>::new(),
            "these call a locking function while holding a guard: \n{}",
            offenders.join("\n")
        );
    }
}

#[cfg(test)]
mod window_visibility {
    //! Revealing the window must go through one function.
    //!
    //! Three places show it - the tray's settings item, the tray's left-click toggle and
    //! a second instance handing off. Each used to do `show`/`unminimize`/`set_focus`
    //! itself, and when the frontend needed a "you are visible again" event only one of
    //! them was updated, so a window reopened the other two ways could keep showing stale
    //! data. Counting the raw calls is cruder than the lock-order scan and enough here:
    //! the only legal `window.show()` is inside `show_window`.

    fn sources() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).expect("read src") {
            let path = entry.expect("entry").path();
            if path.extension().is_some_and(|e| e == "rs") {
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                out.push((name, std::fs::read_to_string(&path).expect("read file")));
            }
        }
        out
    }

    #[test]
    fn the_window_is_shown_only_through_the_helper() {
        let mut offenders = Vec::new();
        let mut inside_helper = false;
        for (name, text) in sources() {
            for line in text.lines() {
                if line.contains("fn show_window") {
                    inside_helper = name == "lib.rs";
                    continue;
                }
                if inside_helper && line.trim() == "}" {
                    inside_helper = false;
                }
                let trimmed = line.trim_start();
                // Skip comments and the check itself: the doc comment above quotes the
                // call it forbids, and so does this condition.
                if trimmed.starts_with("//") || trimmed.starts_with("if line.contains") {
                    continue;
                }
                if line.contains("window.show()") && !inside_helper {
                    offenders.push(format!("{name}: {}", line.trim()));
                }
            }
        }
        assert_eq!(
            offenders,
            Vec::<String>::new(),
            "these reveal the window without telling the frontend it is back: \n{}",
            offenders.join("\n")
        );
    }
}

#[cfg(test)]
mod geometry {
    //! Window placement constraints.
    //!
    //! Each of these was found separately by measuring a running window across four
    //! rounds - the size overflowing a smaller screen, the frame not being subtracted,
    //! the missing floor, and the position clamp using the inner size. They are pure
    //! arithmetic, so all four can be pinned without a window.

    use super::clamp_to_monitor;

    const SCREEN: ((i32, i32), (i32, i32)) = ((0, 0), (1920, 1080));
    const FRAME: (i32, i32) = (16, 39);
    const MIN: (i32, i32) = (720, 520);

    use super::{is_reachable, rescue_target};

    /// The two monitors on this machine: primary at the origin, a second to its left.
    fn monitors() -> Vec<(i32, i32, i32, i32)> {
        vec![(0, 0, 1920, 1080), (-1920, -78, 1920, 1080)]
    }

    #[test]
    fn a_window_on_an_attached_monitor_is_reachable() {
        assert!(is_reachable((504, 161, 912, 718), &monitors()));
        // The second monitor sits at a negative x.
        assert!(is_reachable((-1500, 100, 912, 718), &monitors()));
    }

    #[test]
    fn a_window_whose_title_bar_is_off_the_top_is_not_reachable() {
        // Nothing to grab: dragging needs the title bar on screen.
        assert!(!is_reachable((-1500, -900, 912, 718), &monitors()));
    }

    #[test]
    fn a_window_on_a_removed_monitor_is_moved_to_the_first_one() {
        // The second monitor was unplugged while the window sat on it.
        let stranded = (-1500, 100, 912, 718);
        assert!(!is_reachable(stranded, &[(0, 0, 1920, 1080)]));
        assert_eq!(
            rescue_target(stranded, &[(0, 0, 1920, 1080)]),
            Some((0, 100))
        );
    }

    #[test]
    fn a_reachable_window_is_left_alone() {
        assert_eq!(rescue_target((504, 161, 912, 718), &monitors()), None);
    }

    #[test]
    fn a_stranded_window_larger_than_the_remaining_screen_is_shrunk_to_fit() {
        let stranded = (-3000, 500, 4000, 2200);
        assert_eq!(rescue_target(stranded, &[(0, 0, 1920, 1080)]), Some((0, 0)));
    }

    #[test]
    fn with_no_monitors_there_is_nowhere_to_move_to() {
        assert_eq!(rescue_target((100, 100, 912, 718), &[]), None);
    }

    #[test]
    fn a_window_larger_than_the_screen_is_shrunk() {
        // A 4K-sized window reopened on a laptop panel.
        let (_, _, w, h) = clamp_to_monitor((100, 100, 4000, 2200), SCREEN.0, SCREEN.1, FRAME, MIN);
        assert_eq!((w, h), (1920 - 16, 1080 - 39));
    }

    #[test]
    fn the_outer_frame_stays_on_screen() {
        // The position clamp has to account for the border: using the inner size left the
        // right edge 16 px past the screen, which is where the measured (16,39) came from.
        let (x, y, w, h) = clamp_to_monitor((100, 100, 4000, 2200), SCREEN.0, SCREEN.1, FRAME, MIN);
        assert_eq!((x, y), (0, 0));
        assert!(x + w + FRAME.0 <= 1920);
        assert!(y + h + FRAME.1 <= 1080);
    }

    #[test]
    fn a_small_window_keeps_its_size_and_position() {
        let got = clamp_to_monitor((504, 161, 896, 679), SCREEN.0, SCREEN.1, FRAME, MIN);
        assert_eq!(got, (504, 161, 896, 679));
    }

    #[test]
    fn the_minimum_wins_over_a_tiny_screen() {
        // 800x600 leaves 784x561 of inner room, below the 720x520 minimum only in width.
        // The floor wins: asking for less than the minimum makes the two constraints
        // fight, and a window slightly too large is the lesser problem.
        let (_, _, w, h) = clamp_to_monitor((0, 0, 100, 100), (0, 0), (800, 600), FRAME, MIN);
        assert_eq!((w, h), (720, 520));
    }

    #[test]
    fn a_window_on_a_second_monitor_uses_that_monitors_origin() {
        // The second screen here starts at x = -1920.
        let (x, y, w, h) = clamp_to_monitor(
            (-3000, -500, 1000, 800),
            (-1920, -78),
            (1920, 1080),
            FRAME,
            MIN,
        );
        assert_eq!((x, y), (-1920, -78));
        assert!(x + w + FRAME.0 <= 0);
        assert!(y + h + FRAME.1 <= -78 + 1080);
    }
}

#[cfg(test)]
mod redaction {
    use super::*;

    #[test]
    fn the_home_directory_becomes_a_tilde() {
        let path = std::path::Path::new(r"C:\Users\ana\AppData\Roaming\Unfy\settings.json");
        assert_eq!(
            redact_home_in(path, Some(r"C:\Users\ana")),
            r"~\AppData\Roaming\Unfy\settings.json",
        );
    }

    #[test]
    fn a_path_outside_home_is_left_alone() {
        // A path that merely starts with the same letters must not be rewritten.
        let path = std::path::Path::new(r"D:\builds\unfy.exe");
        assert_eq!(
            redact_home_in(path, Some(r"C:\Users\ana")),
            r"D:\builds\unfy.exe",
        );
    }

    #[test]
    fn a_missing_home_still_returns_the_path() {
        let path = std::path::Path::new("unfy.exe");
        assert_eq!(redact_home_in(path, None), "unfy.exe");
        assert_eq!(redact_home_in(path, Some("")), "unfy.exe");
    }
}
