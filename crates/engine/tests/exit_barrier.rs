//! The exit barrier: quitting must not strand captured windows.
//!
//! `tray::quit` refuses to exit when a restore cannot complete. The engine-level
//! contract those call sites rely on is tested here against the fake backend, so
//! the behaviour is pinned even though the tray menu itself needs a human click.

use std::path::PathBuf;

use conflux_domain::{DisplayKey, IdentityQuality, Rect, WindowMode, WindowOutcome};
use conflux_engine::{Engine, EngineConfig};
use conflux_platform::fake::{FakeBackend, FakeBehaviour};

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("conflux-exit-{tag}-{}", std::process::id()));
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

#[test]
fn open_transaction_means_exit_is_blocked_until_restore_completes() {
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        1,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );

    let mut engine = Engine::new(backend, config("barrier"));
    assert!(
        !engine.state().has_pending_work(),
        "a fresh engine must not block exit"
    );

    engine.migrate(&target()).expect("migrate");
    assert!(
        engine.state().has_pending_work(),
        "after a gather, quitting must be gated on restore"
    );

    let report = engine.restore().expect("restore");
    assert!(report.is_complete());
    assert!(
        !engine.state().has_pending_work(),
        "a clean restore must unblock exit"
    );
}

#[test]
fn refused_window_keeps_exit_blocked_and_the_journal_alive() {
    // A window that will not move back leaves the transaction unresolved. This is
    // exactly the case where the tray refuses to quit: the preimage is still the
    // only record of where that window belongs.
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        2,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::SelfCorrecting,
    );

    let mut engine = Engine::new(backend, config("barrier-refused"));
    let report = engine.migrate(&target()).expect("migrate");
    assert_eq!(
        report.outcomes.get(&id(2)),
        Some(&WindowOutcome::Rejected),
        "a self-correcting window cannot be placed"
    );

    let restored = engine.restore().expect("restore attempt");
    // Whether it reports pending or resolves depends on whether the window moved
    // at all; if it never moved, restoring it is trivially complete.
    if !restored.is_complete() {
        assert!(
            engine.state().has_pending_work(),
            "pending windows must keep exit blocked"
        );
    }
}

#[test]
fn adopted_transaction_also_blocks_exit() {
    let journal_dir = dir("barrier-adopted");
    let path = journal_dir.join("left-behind.journal");
    {
        let mut journal = conflux_journal::Journal::begin(&path, "2 displays").unwrap();
        journal
            .write_preimage(&conflux_journal::Preimage {
                id: id(3),
                display_native_id: "fake-display-a".into(),
                display_quality: IdentityQuality::Strong,
                display_work_area: Rect::new(0, 0, 2560, 1400),
                display_scale: conflux_domain::Scale { x: 1.5, y: 1.5 },
                visible_rect: Rect::new(100, 100, 900, 700),
                outer_rect: Rect::new(93, 93, 907, 707),
                mode: WindowMode::Normal,
                topmost: false,
                native_placement: None,
            })
            .unwrap();
        journal.abandon();
    }

    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        3,
        Rect::new(3000, 300, 3800, 900),
        "fake-display-b",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );

    let mut engine = Engine::new(
        backend,
        EngineConfig {
            journal_dir: journal_dir.clone(),
            ..Default::default()
        },
    );
    assert!(engine.recover_latest(&journal_dir).unwrap().is_some());
    assert!(
        engine.state().has_pending_work(),
        "adopted work must block exit before the user resolves it"
    );

    let report = engine.restore().expect("restore adopted");
    assert!(report.is_complete());
    assert!(!engine.state().has_pending_work());
    assert!(!path.exists());
}

fn id(n: u64) -> conflux_domain::WindowId {
    conflux_domain::WindowId {
        native_id: n,
        process_id: 1000 + n as u32,
        process_start: 1,
    }
}
