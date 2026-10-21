//! The first-run guide must be resumable.
//!
//! Hiding and re-showing the window remounts the React component, so the step has
//! to live in settings rather than component state. When it did not, the guide
//! silently restarted at step 1 and the user read the same page again, which
//! looked like a broken button.

use std::path::PathBuf;

use conflux_settings::{Settings, TargetDisplay};

fn temp_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "conflux-onboarding-{tag}-{}-{:?}",
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
            native_id: "primary".into(),
            label: "Primary".into(),
        }),
        ..Default::default()
    };
    assert!(!settings.onboarding_done);
}
