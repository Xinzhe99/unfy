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
        .on_menu_event(|app, event| match event.id.as_ref() {
            ID_GATHER => run_and_notify(app, "gather"),
            ID_RESTORE => run_and_notify(app, "restore"),
            ID_RESCUE => run_and_notify(app, "rescue"),
            ID_PAUSE => toggle_pause(app),
            ID_IDENTIFY => {
                let _ = crate::commands::identify_displays(Some(4));
            }
            ID_REMOTE => toggle_remote(app),
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

    let base = match engine_state {
        conflux_domain::EngineState::Idle => "Conflux · 就绪",
        conflux_domain::EngineState::Planning => "Conflux · 正在计划",
        conflux_domain::EngineState::Migrating => "Conflux · 正在整理窗口",
        conflux_domain::EngineState::Active => "Conflux · 布局已启用",
        conflux_domain::EngineState::Restoring => "Conflux · 正在恢复",
        conflux_domain::EngineState::Degraded => "Conflux · 部分受限",
        conflux_domain::EngineState::Error => "Conflux · 需要处理",
    };

    let mut tooltip = if paused {
        format!("{base}（已暂停）")
    } else {
        base.to_string()
    };

    // A tray icon is a dot at the edge of the screen: the state has to be legible
    // on hover without opening anything. The remote line answers the question
    // this app exists for - "is anyone remoting in right now?" - which the engine
    // state alone cannot.
    if let Ok(tracker) = detector.lock() {
        if let Some(evidence) = tracker.evidence() {
            tooltip.push_str(&format!("\n远程会话：{evidence}"));
        }
    }
    tooltip
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
            "evidence": if now_active { "手动标记" } else { "" },
            // The marker only changes the verdict; whether windows move is decided
            // by the automation settings, exactly as for detected evidence.
            "acted": false,
        }),
    );
    refresh(app);
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
    let remote_active = state
        .detector
        .lock()
        .map(|tracker| tracker.is_active())
        .unwrap_or(false);

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
    let remote = MenuItem::with_id(
        app,
        ID_REMOTE,
        if remote_active {
            "结束远程会话并恢复"
        } else {
            "标记为远程会话"
        },
        true,
        None::<&str>,
    )?;
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
            &remote,
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
            note: String::new(),
            trigger: trigger.to_string(),
        });
    };
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

    // Same semantics as the main button: "gather to the target display" while a
    // layout is active means retarget, not "refuse because something is open".
    match engine.retarget(&target) {
        Ok(retarget) => {
            let applied = retarget.migration.applied_count();
            let unsettled = retarget.migration.failed_count() + retarget.migration.unplaced.len();
            drop(engine);
            drop(settings);
            record(
                app,
                crate::activity::Kind::Gather,
                applied,
                unsettled,
                "tray",
            );
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn run_restore<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    // The tray must honour the same delay the window does, or the two entry
    // points would behave differently for the same button label. The wait runs
    // without holding the engine lock.
    let handle = app.clone();
    let state = app.state::<AppState>();
    let result = tauri::async_runtime::block_on(async {
        // Tray restore is an explicit instruction, same as the button.
        crate::commands::restore_after_delay(&handle, &state, false).await
    });
    if let Ok(dto) = &result {
        record(
            app,
            crate::activity::Kind::Restore,
            dto.restored + dto.fallback_visible,
            dto.pending,
            "tray",
        );
    }
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
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn toggle_settings<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        if visible {
            crate::hide_window(&window);
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
    //! Tinting arithmetic. The first implementation used `u16` for the luminance
    //! weights and panicked on the first pixel (`255 * 1000` overflows `u16`);
    //! in release that would have silently wrapped and tinted the icon wrong.

    fn luma(r: u8, g: u8, b: u8) -> u32 {
        (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000
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
