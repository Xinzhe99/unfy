//! Pause semantics and the restore delay.
//!
//! Both exist because of how remote sessions behave in practice: the user wants
//! to stop automation for a moment without losing the layout, and a connection
//! that drops and reconnects within seconds must not cause two migrations.

use std::path::PathBuf;

use conflux_domain::{DisplayKey, IdentityQuality, Rect, WindowMode};
use conflux_engine::{Engine, EngineConfig};
use conflux_platform::fake::{FakeBackend, FakeBehaviour};
use conflux_platform::WindowBackend;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("conflux-pause-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn config(tag: &str) -> EngineConfig {
    EngineConfig {
        journal_dir: dir(tag),
        ..Default::default()
    }
}

fn target() -> DisplayKey {
    DisplayKey {
        native_id: "fake-display-b".into(),
        quality: IdentityQuality::Strong,
    }
}

fn backend() -> FakeBackend {
    let mut b = FakeBackend::with_two_displays();
    b.add_window(
        1,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    b
}

#[test]
fn pause_keeps_the_layout_and_blocks_new_gathers() {
    let mut engine = Engine::new(backend(), config("pause"));
    engine.migrate(&target()).expect("migrate");
    assert_eq!(engine.captured_window_count(), 1);

    engine.pause().expect("pause");
    assert!(engine.is_paused());
    // Pausing must not move anything or discard the journal.
    assert_eq!(engine.captured_window_count(), 1);

    let refused = engine.migrate(&target());
    assert!(
        matches!(refused, Err(conflux_domain::EngineError::Paused)),
        "a paused engine must refuse new gathers, got {refused:?}"
    );

    engine.resume().expect("resume");
    assert!(!engine.is_paused());
    assert_eq!(engine.state(), conflux_domain::EngineState::Active);

    // The original preimage survived the pause/resume cycle.
    let report = engine.restore().expect("restore");
    assert!(report.is_complete());
    let window = engine.backend().enumerate().unwrap().pop().unwrap();
    assert_eq!(window.visible_rect, Rect::new(100, 100, 900, 700));
}

#[test]
fn pause_without_a_transaction_is_a_noop() {
    let mut engine = Engine::new(backend(), config("pause-idle"));
    engine.pause().expect("pause on idle");
    // Nothing was captured, so there is no state to protect.
    assert!(!engine.is_paused());
}

#[test]
fn restore_respects_the_configured_delay() {
    let mut engine = Engine::new(
        backend(),
        EngineConfig {
            restore_delay_secs: 1,
            ..config("delay")
        },
    );
    engine.migrate(&target()).expect("migrate");

    let started = std::time::Instant::now();
    let report = engine.restore().expect("restore");
    let elapsed = started.elapsed();

    assert!(report.is_complete());
    assert!(
        elapsed >= std::time::Duration::from_millis(900),
        "restore must wait for the configured delay, took {elapsed:?}"
    );
}

#[test]
fn zero_delay_restores_immediately() {
    let mut engine = Engine::new(backend(), config("no-delay"));
    engine.migrate(&target()).expect("migrate");

    let started = std::time::Instant::now();
    engine.restore().expect("restore");
    assert!(
        started.elapsed() < std::time::Duration::from_millis(500),
        "the default delay is zero and must not sleep"
    );
}
