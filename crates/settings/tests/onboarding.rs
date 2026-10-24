//! The first-run guide must be resumable.
//!
//! Hiding and re-showing the window remounts the React component, so the step has
//! to live in settings rather than component state. When it did not, the guide
//! silently restarted at step 1 and the user read the same page again, which
//! looked like a broken button.

use std::path::PathBuf;

use unfy_settings::{Settings, TargetDisplay};

fn temp_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "unfy-onboarding-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("settings.json")
}

#[test]
fn fresh_install_starts_the_guide_at_step_zero() {
    let settings = Settings::default();
    assert!(!settings.onboarding_done);
    assert_eq!(settings.onboarding_step, 0);
}

#[test]
fn step_survives_a_restart() {
    let path = temp_path("resume");

    let settings = Settings {
        onboarding_step: 1,
        ..Default::default()
    };
    settings.save(&path).unwrap();

    let reloaded = Settings::load(&path).unwrap();
    assert_eq!(reloaded.onboarding_step, 1);
    assert!(!reloaded.onboarding_done);
}

#[test]
fn completion_is_recorded_alongside_the_step() {
    let path = temp_path("done");

    let settings = Settings {
        onboarding_done: true,
        onboarding_step: 2,
        ..Default::default()
    };
    settings.save(&path).unwrap();

    let reloaded = Settings::load(&path).unwrap();
    assert!(reloaded.onboarding_done);
    assert_eq!(reloaded.onboarding_step, 2);
}

#[test]
fn a_chosen_target_does_not_imply_completion() {
    // A user may deliberately keep the primary display as the target, so the
    // guide must be completed explicitly rather than inferred from a target.
    let settings = Settings {
        target_display: Some(TargetDisplay {
            number: None,
            native_id: "primary".into(),
            label: "Primary".into(),
        }),
        ..Default::default()
    };
    assert!(!settings.onboarding_done);
}

/// Window geometry must round-trip and must reject nonsense.
///
/// A window restored to an off-screen position or a 1x1 size is unrecoverable
/// without hand-editing JSON, so the range check at the boundary matters more
/// than it looks.
#[test]
fn window_geometry_round_trips_and_is_range_checked() {
    let path = temp_path("geometry");

    let settings = unfy_settings::Settings::default();
    assert_eq!(settings.window_geometry, None);
    let settings = unfy_settings::Settings {
        window_geometry: Some((200, 120, 1040, 800)),
        ..Default::default()
    };
    settings.save(&path).expect("save");
    assert_eq!(
        unfy_settings::Settings::load(&path)
            .unwrap()
            .window_geometry,
        Some((200, 120, 1040, 800))
    );

    for bad in [
        (0, 0, 10, 800),         // far too narrow
        (0, 0, 1040, 100),       // far too short
        (0, 0, 9000, 800),       // wider than any display
        (999_999, 0, 1040, 800), // unreachable position
    ] {
        let settings = unfy_settings::Settings {
            window_geometry: Some(bad),
            ..Default::default()
        };
        assert!(
            settings.save(&path).is_err(),
            "geometry {bad:?} should be rejected"
        );
    }
}

/// Reset must keep the one value the user cannot recover by guessing.
///
/// The target display is chosen by looking at the physical screens; discarding it
/// on reset forces that work again. Everything else - rules, hotkeys, automation -
/// must go, because a "reset" that leaves invisible state behind is not a reset.
#[test]
fn resetting_keeps_the_target_and_clears_everything_else() {
    let path = temp_path("reset");
    let mut settings = unfy_settings::Settings {
        target_display: Some(TargetDisplay {
            native_id: "DISPLAY#AAA".into(),
            label: "\\\\.\\DISPLAY1".into(),
            number: Some(2),
        }),
        excluded_apps: vec!["msedge.exe".into()],
        hotkey_gather: Some("CmdOrCtrl+Alt+G".into()),
        autostart: true,
        auto_gather: true,
        auto_restore: true,
        auto_detect: "verified".into(),
        restore_delay_secs: 30,
        theme: unfy_settings::ThemeName::Dark,
        onboarding_done: true,
        onboarding_step: 3,
        ..Default::default()
    };
    settings.layout_mode = unfy_settings::LayoutModeName::Grid;
    settings.margin = 40;
    settings.save(&path).expect("save");

    // What the command does: defaults, target preserved, guide not repeated.
    let fresh = unfy_settings::Settings {
        target_display: settings.target_display.clone(),
        onboarding_done: true,
        ..Default::default()
    };

    assert_eq!(
        fresh.target_display.as_ref().map(|t| t.native_id.as_str()),
        Some("DISPLAY#AAA")
    );
    assert_eq!(
        fresh.target_display.as_ref().and_then(|t| t.number),
        Some(2)
    );
    assert!(fresh.excluded_apps.is_empty());
    assert!(fresh.hotkey_gather.is_none());
    assert!(!fresh.autostart);
    assert!(!fresh.auto_gather);
    assert!(!fresh.auto_restore);
    assert_eq!(fresh.auto_detect, "off", "reset must not enable automation");
    // Assert against the shipped default rather than a literal: the point is
    // "reset restores defaults", not "the default is zero".
    assert_eq!(
        fresh.restore_delay_secs,
        unfy_settings::Settings::default().restore_delay_secs
    );
    // Layout mode has a literal here on purpose: "reset restores the default"
    // must mean the move-into mode, and silently drifting to a grid-on-reset
    // regression is what this line is for.
    assert_eq!(fresh.layout_mode, unfy_settings::LayoutModeName::MoveInto);
    assert_eq!(fresh.margin, 12);
    assert!(fresh.onboarding_done, "the guide must not reappear");
}
