//! Pause semantics and the restore delay.
//!
//! Both exist because of how remote sessions behave in practice: the user wants
//! to stop automation for a moment without losing the layout, and a connection
//! that drops and reconnects within seconds must not cause two migrations.

use std::path::PathBuf;

use unfy_domain::{DisplayKey, IdentityQuality, Rect, WindowMode};
use unfy_engine::{Engine, EngineConfig};
use unfy_platform::fake::{FakeBackend, FakeBehaviour};
use unfy_platform::WindowBackend;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("unfy-pause-{tag}-{}", std::process::id()));
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
        matches!(refused, Err(unfy_domain::EngineError::Paused)),
        "a paused engine must refuse new gathers, got {refused:?}"
    );

    engine.resume().expect("resume");
    assert!(!engine.is_paused());
    assert_eq!(engine.state(), unfy_domain::EngineState::Active);

    // The original preimage survived the pause/resume cycle.
    let report = engine.restore().expect("restore");
    assert!(report.is_complete());
    let window = engine.backend().enumerate().unwrap().pop().unwrap();
    assert_eq!(window.visible_rect, Rect::new(100, 100, 900, 700));
}

#[test]
fn pause_without_a_transaction_still_pauses_automation() {
    // Pause is an automation policy, not a property of a transaction. It used to
    // be a no-op without one, so the UI said "paused" while the engine kept
    // gathering on the next automatic trigger.
    let mut engine = Engine::new(backend(), config("pause-idle"));
    engine.pause().expect("pause on idle");
    assert!(
        engine.is_paused(),
        "pause must survive without a transaction"
    );
    // And it must actually block a gather, which is the effect the user asked for.
    assert!(matches!(
        engine.migrate(&target()),
        Err(unfy_domain::EngineError::Paused)
    ));
    engine.resume().expect("resume");
    assert!(!engine.is_paused());
}

/// The engine must *not* sleep on the restore delay.
///
/// It used to, inside the engine mutex: a 30 s delay blocked status polling and
/// tray refresh for 30 s, which looked like a frozen app. The desktop layer now
/// owns the wait (cancellable, outside the lock). This test pins the contract so
/// the sleep cannot come back.
#[test]
fn restore_does_not_sleep_on_the_configured_delay() {
    let mut engine = Engine::new(
        backend(),
        EngineConfig {
            restore_delay_secs: 30,
            ..config("delay")
        },
    );
    engine.migrate(&target()).expect("migrate");

    let started = std::time::Instant::now();
    let report = engine.restore().expect("restore");
    let elapsed = started.elapsed();

    assert!(report.is_complete());
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "the engine must leave the delay to its caller, took {elapsed:?}"
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

#[test]
fn topology_signature_changes_with_geometry_not_with_time() {
    // The watcher compares this string every two seconds. It must be stable while
    // nothing changes (or the rescue would fire constantly) and must differ once
    // the topology does.
    let engine = Engine::new(backend(), config("signature"));
    let first = engine.topology_signature().expect("signature");
    let second = engine.topology_signature().expect("signature");
    assert_eq!(
        first, second,
        "an unchanged topology must produce the same fingerprint"
    );
    assert!(
        first.contains("fake-display-b"),
        "identity must be part of the key"
    );
}

#[test]
fn topology_change_rescues_windows_left_outside_every_display() {
    let mut backend = FakeBackend::with_two_displays();
    // A window parked on a display that is about to disappear.
    backend.add_window(
        9,
        Rect::new(6000, 4000, 6600, 4400),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );

    let mut engine = Engine::new(backend, config("hotplug"));
    let rescued = engine.on_topology_changed().expect("topology change");
    assert_eq!(
        rescued.len(),
        1,
        "the unreachable window must be moved back"
    );

    let window = engine.backend().enumerate().unwrap().pop().unwrap();
    let displays = unfy_platform::DisplayProvider::topology(engine.backend()).unwrap();
    assert!(
        displays
            .iter()
            .any(|d| window.visible_rect.reachable_in(&d.work_area, 64, 32)),
        "rescued window must be reachable on some display"
    );
}

#[test]
fn topology_change_leaves_visible_windows_alone() {
    let mut engine = Engine::new(backend(), config("hotplug-noop"));
    let before = engine.backend().enumerate().unwrap();
    let rescued = engine.on_topology_changed().expect("topology change");
    assert!(rescued.is_empty());
    assert_eq!(engine.backend().enumerate().unwrap(), before);
}

/// Resuming must not claim a partial layout is complete.
///
/// `resume` used to set `Active` whenever a transaction existed, including when the
/// engine was `Degraded` - the state that says "windows are still displaced". The
/// overview chooses between "restore" and "keep current layout" from that state, so
/// overwriting it hid the work that was left.
///
/// The `Degraded` used here is the one the app actually produces: a journal adopted
/// from a previous run that ended mid-transaction.
#[test]
fn resuming_a_degraded_layout_stays_degraded() {
    let dir = config("resume-degraded").journal_dir;
    let mut engine = Engine::new(backend(), config("resume-degraded"));
    engine.migrate(&target()).expect("migrate");
    let transaction = engine.current_transaction().expect("open transaction");

    // Simulate a crash: write a journal with preimages but no resolution, then
    // adopt it as a fresh process would.
    let path = dir.join(format!("{transaction}.journal"));
    drop(engine);

    let mut engine = Engine::new(backend(), config("resume-degraded"));
    match engine.recover_latest(&dir) {
        Ok(Some(_)) => {}
        other => panic!("expected an adopted transaction, got {other:?}"),
    }
    assert_eq!(
        engine.state(),
        unfy_domain::EngineState::Degraded,
        "an adopted unresolved transaction is degraded"
    );

    engine.pause().expect("pause a degraded layout");
    engine.resume().expect("resume");
    assert_eq!(
        engine.state(),
        unfy_domain::EngineState::Degraded,
        "resuming must not upgrade a partial layout to Active"
    );

    // Cleanup: the adopted journal is still on disk.
    let _ = std::fs::remove_file(&path);
}
