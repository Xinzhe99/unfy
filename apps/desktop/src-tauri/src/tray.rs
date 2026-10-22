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
/// Cycle the target display and re-gather onto it (spec P0-04-B).
pub const ID_CYCLE: &str = "cycle";
/// Manual remote-mode toggle.
///
/// Detection cannot cover every remote tool, and the verification work found
/// that "just ask the user" is the only universally reliable signal. This item is
/// that escape hatch: one click marks the machine as remoted in, and one click
/// ends it and restores.
pub const ID_REMOTE: &str = "remote";
pub const ID_SETTINGS: &str = "settings";
pub const ID_QUIT: &str = "quit";

/// Install the tray icon. Called once from `setup`.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build_menu(app)?;

    let mut builder = TrayIconBuilder::with_id("conflux")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
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

/// Run a tray menu item.
///
/// Named rather than inline so the actions can be driven without a click. The menu
/// is a primary surface - it is the entire interface when the window is hidden - and
/// on Windows 11 the icons live in a XAML overflow flyout that cannot be opened by
/// automation, so nothing else exercises it.
pub fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        ID_GATHER => run_and_notify(app, "gather"),
        ID_RESTORE => run_and_notify(app, "restore"),
        ID_RESCUE => run_and_notify(app, "rescue"),
        ID_PAUSE => toggle_pause(app),
        ID_IDENTIFY => {
            let _ = crate::commands::identify_displays(Some(4));
        }
        ID_CYCLE => cycle_target(app),
        ID_REMOTE => toggle_remote(app),
        ID_SETTINGS => show_settings(app),
        ID_QUIT => quit(app),
        _ => {}
    }
}

/// Rebuild the tray icon, menu and tooltip.
///
/// Called after every action, because the enabled set and the labels depend on
/// whether windows are currently gathered.
///
/// # Callers must hold no state guard
///
/// `build_menu` takes the engine and detector locks, and a `Mutex` is not
/// reentrant: calling this while holding the engine or the settings guard deadlocks
/// the calling thread against itself. The visible result is not a stale menu - the
/// window stops answering, because the next status poll waits for a lock that will
/// never be released.
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let Some(tray) = app.tray_by_id("conflux") else {
        return;
    };
    if let Ok(menu) = build_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tooltip(app)));
    if let Some(icon) = state_icon(app) {
        let _ = tray.set_icon(Some(icon));
    }
}

/// Recolour the tray icon so the state is readable without hovering.
///
/// The tooltip already carries the state, but a tray icon is a dot at the edge of
/// the screen: the user has to hover to read anything. A tinted icon answers
/// "is there a layout active right now?" at a glance, which is the question this
/// app is used for dozens of times a day.
///
/// Tinting is done on the embedded icon bytes at runtime rather than shipping
/// pre-rendered variants, so the palette stays in one place.
fn state_icon<R: Runtime>(app: &AppHandle<R>) -> Option<tauri::image::Image<'static>> {
    let state = {
        let state = app.state::<AppState>();
        let engine = state.engine.lock().ok()?;
        (engine.state(), engine.is_paused())
    };

    // Same palette as the window's status dots, so the tray and the overview never
    // disagree about what a colour means. The values mirror the `oklch` tokens in
    // `styles.css` (the tray works on raw pixels and cannot read CSS):
    //
    //   --color-success  idle, nothing pending
    //   --color-active   a layout is in effect, or a gather is running
    //   --color-warning  paused
    //   --color-danger   error or degraded
    let (r, g, b) = match state {
        // Idle and healthy: green, matching the green "就绪" dot in the window.
        (conflux_domain::EngineState::Idle, false) => (38u8, 123u8, 76u8),
        // A live layout, and the working states that lead to one: blue.
        (conflux_domain::EngineState::Active, false)
        | (conflux_domain::EngineState::Migrating, _)
        | (conflux_domain::EngineState::Restoring, _)
        | (conflux_domain::EngineState::Planning, _) => (56u8, 110u8, 200u8),
        // Needs attention: amber when paused or degraded, red when broken.
        (conflux_domain::EngineState::Error, _) => (175u8, 52u8, 51u8),
        // Remaining combinations: degraded, or paused in any state.
        _ => (150u8, 102u8, 38u8),
    };

    let source = app.default_window_icon()?;
    let (width, height) = (source.width(), source.height());
    let rgba = source.rgba();
    let mut tinted = Vec::with_capacity(rgba.len());

    for pixel in rgba.chunks_exact(4) {
        let alpha = pixel[3];
        if alpha == 0 {
            // Transparent padding stays transparent, or the icon becomes a square.
            tinted.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        // Keep the artwork's luminance and apply the state colour, so the mark
        // stays recognisable while the hue changes.
        //
        // The weights are applied in u32: 255 * (299 + 587 + 114) is 255,000,
        // which overflows u16 and panicked the tray refresh on the very first
        // pixel. `u8::from`-style conversions would not have caught it.
        let luma =
            (u32::from(pixel[0]) * 299 + u32::from(pixel[1]) * 587 + u32::from(pixel[2]) * 114)
                / 1000;
        let mix = |channel: u8| -> u8 {
            let scaled = u32::from(channel) * luma / 255;
            u8::try_from((scaled + u32::from(channel)) / 2).unwrap_or(channel)
        };
        tinted.extend_from_slice(&[mix(r), mix(g), mix(b), alpha]);
    }

    tauri::image::Image::new_owned(tinted, width, height).into()
}

/// The tray sentences for the language the interface is using.
///
/// Falls back to the source language while the locale is unresolved, which is the
/// window before the webview reports it; guessing an English-speaking user from
/// the operating system here would risk a Chinese window with an English tray.
fn locale_text<R: Runtime>(app: &AppHandle<R>) -> &'static crate::ui_text::TrayText {
    let locale = app
        .state::<AppState>()
        .ui_locale
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .unwrap_or(crate::ui_text::Locale::Zh);
    crate::ui_text::tray(locale)
}

fn locale_remote_prefix<R: Runtime>(app: &AppHandle<R>) -> &'static str {
    locale_text(app).remote_prefix
}

fn tooltip<R: Runtime>(app: &AppHandle<R>) -> String {
    let state = app.state::<AppState>();
    // Clone the Arcs so the locks are not held across the whole function: the
    // detector lock is taken below, and nesting the two invites a future
    // deadlock the moment either path writes history.
    let engine = state.engine.clone();
    let detector = state.detector.clone();

    let (engine_state, paused) = match engine.lock() {
        Ok(engine) => (engine.state(), engine.is_paused()),
        Err(_) => return "Conflux".into(),
    };

    let text = locale_text(app);
    let base = match engine_state {
        conflux_domain::EngineState::Idle => text.state_idle,
        conflux_domain::EngineState::Planning => text.state_planning,
        conflux_domain::EngineState::Migrating => text.state_migrating,
        conflux_domain::EngineState::Active => text.state_active,
        conflux_domain::EngineState::Restoring => text.state_restoring,
        conflux_domain::EngineState::Degraded => text.state_degraded,
        conflux_domain::EngineState::Error => text.state_error,
    };

    let mut tooltip = if paused {
        format!("{base}{}", text.paused_suffix)
    } else {
        base.to_string()
    };

    // A tray icon is a dot at the edge of the screen: the state has to be legible
    // on hover without opening anything. The remote line answers the question
    // this app exists for - "is anyone remoting in right now?" - which the engine
    // state alone cannot.
    if let Ok(tracker) = detector.lock() {
        if let Some(evidence) = tracker.evidence() {
            let sentence = crate::ui_text::evidence(text, evidence.key(), evidence.param());
            tooltip.push('\n');
            tooltip.push_str(locale_remote_prefix(app));
            tooltip.push_str(&sentence);
        }
    }
    tooltip
}

/// Cycle the target display from the tray.
fn cycle_target<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let state = app.state::<AppState>();
    let result = tauri::async_runtime::block_on(crate::commands::cycle_target(handle, state));
    if let Err(message) = &result {
        tracing::warn!(error = %message, "cycle target failed");
    }
    refresh(app);
}

/// Mark the machine as remoted in, or end that and restore.
///
/// Public so the command layer can offer the same action from the window: the
/// tray alone means a user who has hidden the tray icon has no way to declare a
/// session.
pub fn toggle_remote<R: Runtime>(app: &AppHandle<R>) {
    // One implementation, shared with the window button. The command also runs a
    // poll immediately, so the transition and any automation happen now instead of
    // up to two seconds later.
    let state = app.state::<AppState>();
    let now_active = !state
        .detector
        .lock()
        .map(|tracker| tracker.is_active())
        .unwrap_or(false);
    state
        .manual_remote
        .store(now_active, std::sync::atomic::Ordering::SeqCst);
    crate::monitor::tick(app);

    let _ = app.emit(
        "conflux://remote",
        serde_json::json!({
            "active": now_active,
            "evidence": if now_active {
                serde_json::json!(crate::message::evidence_message(
                    &conflux_detect::Evidence::ManualMarker
                ))
            } else {
                serde_json::json!(null)
            },
            // The marker only changes the verdict; whether windows move is decided
            // by the automation settings, exactly as for detected evidence.
            "acted": false,
        }),
    );
    refresh(app);
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let state = app.state::<AppState>();
    let (engine_state, paused, pending, display_count) = match state.engine.lock() {
        Ok(engine) => (
            engine.state(),
            engine.is_paused(),
            engine.captured_window_count(),
            // Read here so the cycle item can be enabled only when cycling means
            // something. A topology query is a few microseconds and the menu is
            // rebuilt only on state changes.
            conflux_platform::DisplayProvider::topology(engine.backend())
                .map(|d| d.len())
                .unwrap_or(0),
        ),
        Err(_) => (conflux_domain::EngineState::Error, false, 0, 0),
    };

    let has_layout = engine_state.has_pending_work() || pending > 0;
    let remote_active = state
        .detector
        .lock()
        .map(|tracker| tracker.is_active())
        .unwrap_or(false);

    let text = locale_text(app);
    let gather_label = if has_layout {
        text.re_gather
    } else {
        text.gather
    };

    let enabled = menu_enabled(paused, has_layout, display_count);
    let gather = MenuItem::with_id(app, ID_GATHER, gather_label, enabled.gather, None::<&str>)?;
    let restore = MenuItem::with_id(app, ID_RESTORE, text.restore, enabled.restore, None::<&str>)?;
    let rescue = MenuItem::with_id(app, ID_RESCUE, text.rescue, true, None::<&str>)?;
    let pause =
        CheckMenuItem::with_id(app, ID_PAUSE, text.pause, enabled.pause, paused, None::<&str>)?;
    let identify = MenuItem::with_id(app, ID_IDENTIFY, text.identify, true, None::<&str>)?;
    let cycle = MenuItem::with_id(app, ID_CYCLE, text.cycle, enabled.cycle, None::<&str>)?;
    let remote = MenuItem::with_id(
        app,
        ID_REMOTE,
        if remote_active {
            text.remote_end
        } else {
            text.remote_mark
        },
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let settings = MenuItem::with_id(app, ID_SETTINGS, text.settings, true, None::<&str>)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, ID_QUIT, text.quit, true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &gather,
            &restore,
            &rescue,
            &pause,
            &identify,
            &cycle,
            &remote,
            &separator,
            &settings,
            &separator2,
            &quit_item,
        ],
    )
}

/// Which menu items are enabled, given the state they depend on.
///
/// Split out because the tray itself cannot be driven by the accessibility harness -
/// the icon is not exposed as a control - so the enable rules were the one part of the
/// tray nobody could check. They are the part that matters: an item enabled in the
/// wrong state either does nothing or does the wrong thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuEnabled {
    pub gather: bool,
    pub restore: bool,
    pub pause: bool,
    pub cycle: bool,
}

pub fn menu_enabled(paused: bool, has_layout: bool, display_count: usize) -> MenuEnabled {
    MenuEnabled {
        gather: !paused,
        restore: has_layout,
        pause: has_layout,
        // Only meaningful with more than one display; disabled rather than hidden, so
        // the item does not appear and disappear between setups.
        cycle: display_count > 1,
    }
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

/// Append one line to the history so the overview can answer "what did it do?".
pub fn record<R: Runtime>(
    app: &AppHandle<R>,
    kind: crate::activity::Kind,
    affected: usize,
    unsettled: usize,
    trigger: &str,
) {
    // Clone the Arc out of the state before locking: a `State<'_, _>` guard makes
    // the borrow checker treat the mutex guard as borrowed from a temporary.
    let activity = app.state::<AppState>().activity.clone();
    {
        let mut log = match activity.lock() {
            Ok(log) => log,
            Err(_) => return,
        };
        log.record(crate::activity::Entry {
            at: crate::activity::now(),
            kind,
            affected,
            unsettled,
            note: None,
            trigger: trigger.to_string(),
        });
    };
}

fn run_gather<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    // Same implementation as the window button and the shortcut, told what invoked
    // it. This used to be a third copy that resolved the target and called the
    // engine itself, so a tray gather skipped everything the command does: the
    // refused-application list, `last_error`, and the progress events.
    let state = app.state::<AppState>();
    let result =
        tauri::async_runtime::block_on(crate::commands::migrate_with_trigger(app, &state, "tray"));
    match result {
        Ok(_) => Ok(()),
        Err(error) => {
            // No display to gather onto is the one failure the settings window can
            // fix, so open it. Every other error is reported by the caller.
            if error == "no_displays" {
                show_settings(app);
            }
            Err(error)
        }
    }
}

fn run_restore<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    // The tray must honour the same delay the window does, or the two entry
    // points would behave differently for the same button label. The wait runs
    // without holding the engine lock.
    let state = app.state::<AppState>();
    // The shared path records the history entry with the trigger, so the tray does
    // not append a second one.
    let result = tauri::async_runtime::block_on(crate::commands::restore_with_trigger(
        app, &state, false, "tray",
    ));
    result.map(|_| ())
}

/// Restore without the configured delay, for paths where the user is actively
/// waiting (exit).
fn restore_now<R: Runtime>(app: &AppHandle<R>) -> Result<conflux_engine::RestoreReport, String> {
    let state = app.state::<AppState>();
    let mut engine = state.engine.lock().map_err(|e| e.to_string())?;
    engine.restore().map_err(|e| e.to_string())
}

fn run_rescue<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let count = {
        let state = app.state::<AppState>();
        let mut engine = state.engine.lock().map_err(|e| e.to_string())?;
        engine
            .rescue_offscreen()
            .map(|rescued| rescued.len())
            .map_err(|e| e.to_string())?
    };
    if count > 0 {
        record(app, crate::activity::Kind::Rescue, count, 0, "tray");
    }
    Ok(())
}

fn show_settings<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        crate::show_window(&window);
    }
}

fn toggle_settings<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        if visible {
            crate::hide_window(&window);
        } else {
            crate::show_window(&window);
        }
    }
}

/// Exit only after the recovery barrier.
///
/// A failure to restore must not silently drop windows, so the process stays
/// alive and the error is surfaced. The user can retry from the tray.
fn quit<R: Runtime>(app: &AppHandle<R>) {
    // A pending delayed restore is irrelevant now: the user is quitting, and the
    // exit restore below runs immediately.
    if let Ok(mut schedule) = app.state::<AppState>().schedule.lock() {
        schedule.cancel();
    }

    // The exit path ignores the configured delay: the user is waiting for the
    // process to end, not for a timer.
    let blocked = match restore_now(app) {
        // Nothing was captured, or everything went back: exit.
        Ok(report) if report.is_complete() => false,
        Ok(report) => {
            tracing::warn!(pending = report.pending.len(), "restore incomplete");
            true
        }
        Err(e) => {
            tracing::error!(error = %e, "restore failed during exit");
            true
        }
    };

    if blocked {
        let _ = app.emit(
            "conflux://exit-blocked",
            serde_json::json!({ "reason": "restore_incomplete" }),
        );
        show_settings(app);
        return;
    }
    // Stop the detector loop before the process tears down: it holds no lock
    // across `exit`, but a sleeping thread that wakes during shutdown would emit
    // events into a dying runtime and log a spurious error.
    crate::monitor::stop();
    app.exit(0);
}

#[cfg(test)]
mod icon_tests {
    //! Tinting arithmetic and menu enable rules.
    //!
    //! The first tinting implementation used `u16` for the luminance weights and
    //! panicked on the first pixel (`255 * 1000` overflows `u16`); in release that
    //! would have silently wrapped and tinted the icon wrong.
    //!
    //! The menu rules live here because the tray icon is not exposed to the
    //! accessibility harness, so clicking the menu is the one part of the app that
    //! cannot be driven automatically.

    use super::menu_enabled;

    fn luma(r: u8, g: u8, b: u8) -> u32 {
        (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000
    }

    #[test]
    fn restore_and_pause_need_a_layout_to_act_on() {
        // Without a layout there is nothing to restore and nothing to pause; an enabled
        // item would be a button that does nothing.
        let idle = menu_enabled(false, false, 2);
        assert!(!idle.restore);
        assert!(!idle.pause);

        let gathered = menu_enabled(false, true, 2);
        assert!(gathered.restore);
        assert!(gathered.pause);
    }

    #[test]
    fn pausing_disables_gather_but_not_restore() {
        // Paused means "stop moving windows", not "stranded where you are": restoring
        // must stay available, which is the whole point of pausing before a restore.
        let paused = menu_enabled(true, true, 2);
        assert!(!paused.gather);
        assert!(paused.restore);
        assert!(paused.pause);
    }

    #[test]
    fn cycling_needs_a_second_display() {
        assert!(!menu_enabled(false, false, 0).cycle);
        assert!(!menu_enabled(false, false, 1).cycle);
        assert!(menu_enabled(false, false, 2).cycle);
    }

    #[test]
    fn luminance_stays_in_range_for_every_extreme() {
        assert_eq!(luma(255, 255, 255), 255);
        assert_eq!(luma(0, 0, 0), 0);
        // The value that overflowed: pure white through the full weight.
        assert!(luma(255, 255, 255) <= 255);
    }

    #[test]
    fn tint_never_exceeds_eight_bits() {
        for channel in [0u8, 1, 127, 254, 255] {
            for l in [0u32, 128, 255] {
                let scaled = channel as u32 * l / 255;
                let mixed = (scaled + channel as u32) / 2;
                assert!(mixed <= 255, "channel {channel} luma {l} produced {mixed}");
            }
        }
    }
}
