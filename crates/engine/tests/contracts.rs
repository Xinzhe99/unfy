//! Regression tests for the usability-review contracts.
//!
//! Each test here pins one behaviour that was broken in the shipped build and
//! would be invisible in a screenshot: a read that mutated state, a commit that
//! touched an unprepared window, a restore that overwrote a user's change, or a
//! baseline that moved. They are deliberately narrow - one assertion group per
//! contract - so a future refactor cannot quietly satisfy the shape while losing
//! the meaning.

use std::collections::HashMap;
use std::path::PathBuf;

use unfy_domain::{
    DisplayKey, EngineState, IdentityQuality, Rect, Scale, WindowId, WindowMode, WindowOutcome,
};
use unfy_engine::{Engine, EngineConfig};
use unfy_platform::fake::{FakeBackend, FakeBehaviour};
use unfy_platform::{DisplayProvider, WindowBackend};

/// A directory unique to this test.
///
/// The tag alone is not enough: a test that calls this twice (once for the
/// config, once for a fixture path) would otherwise have the second call delete
/// the first call's file.
fn journal_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "unfy-contract-{tag}-{}-{serial}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("journal dir");
    dir
}

fn config(tag: &str) -> EngineConfig {
    EngineConfig {
        journal_dir: journal_dir(tag),
        ..Default::default()
    }
}

fn target() -> DisplayKey {
    DisplayKey {
        native_id: "fake-display-b".into(),
        quality: IdentityQuality::Strong,
    }
}

fn other_display() -> DisplayKey {
    DisplayKey {
        native_id: "fake-display-a".into(),
        quality: IdentityQuality::Strong,
    }
}

fn scenario() -> FakeBackend {
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        1,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    backend.add_window(
        2,
        Rect::new(200, 300, 1000, 900),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    backend
}

fn work_area(engine: &Engine<FakeBackend>, key: &DisplayKey) -> Rect {
    engine
        .backend()
        .topology()
        .unwrap()
        .into_iter()
        .find(|d| &d.key == key)
        .unwrap()
        .work_area
}

/// A read must not change the state a restore depends on.
#[test]
fn preview_keeps_the_transaction_and_state() {
    let mut engine = Engine::new(scenario(), config("preview-state"));
    engine.migrate(&target()).expect("migrate");
    assert_eq!(engine.state(), EngineState::Active);
    let pending_before = engine.unresolved_window_count();
    assert_eq!(pending_before, 2);

    // The exact sequence the UI performs: open a preview, then poll it.
    engine.preview(&target()).expect("preview while active");
    assert_eq!(
        engine.state(),
        EngineState::Active,
        "preview must not demote an active layout to idle"
    );
    assert_eq!(
        engine.unresolved_window_count(),
        pending_before,
        "preview must not change the number of windows that owe a restore"
    );

    // And the restore is still possible afterwards.
    let restored = engine.restore().expect("restore");
    assert!(restored.is_complete());
}

/// A preview failure must not erase an error state either.
#[test]
fn failed_preview_does_not_hide_an_error_state() {
    let mut engine = Engine::new(scenario(), config("error-state"));
    let path = engine.config().journal_dir.join("broken.journal");
    std::fs::write(&path, b"\xFF\xFF\xFF\xFFgarbage").unwrap();
    assert!(engine.recover(&path).is_err());
    assert_eq!(engine.state(), EngineState::Error);

    // `Error` refuses writes, so the preview has nothing to do; it must also not
    // turn the state into `Idle`, which would offer a gather that cannot run.
    let _ = engine.preview(&target());
    assert_eq!(engine.state(), EngineState::Error);
}

/// A window whose preimage cannot be captured must receive zero platform writes,
/// while the others still complete.
#[test]
fn a_window_that_cannot_be_snapshotted_is_never_written() {
    let mut backend = scenario();
    // This window refuses even the capability probe, so the engine cannot read its
    // state. It must be reported, not moved.
    backend.add_window(
        9,
        Rect::new(150, 150, 550, 550),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Denied,
    );

    let mut engine = Engine::new(backend, config("snapshot"));
    let report = engine.migrate(&target()).expect("migrate");

    let denied = WindowId {
        native_id: 9,
        process_id: 1009,
        process_start: 1,
    };
    assert!(
        !report.outcomes.contains_key(&denied)
            || report.outcomes.get(&denied) != Some(&WindowOutcome::Applied),
        "a denied window must never be reported as applied"
    );
    assert_eq!(report.applied_count(), 2, "the healthy windows still move");

    // The denied window is still where it was.
    let after = engine.backend().enumerate().unwrap();
    let denied_now = after.iter().find(|w| w.id == denied).unwrap();
    assert_eq!(denied_now.visible_rect, Rect::new(150, 150, 550, 550));
}

/// Windows already usable on the target are not written to at all.
#[test]
fn windows_already_on_the_target_get_zero_writes() {
    let mut backend = FakeBackend::with_two_displays();
    let target_work = backend
        .topology()
        .unwrap()
        .into_iter()
        .find(|d| d.key == target())
        .unwrap()
        .work_area;
    // A window comfortably inside the target's work area.
    backend.add_window(
        1,
        Rect::new(
            target_work.left + 40,
            target_work.top + 40,
            target_work.left + 640,
            target_work.top + 440,
        ),
        "fake-display-b",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    backend.add_window(
        2,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );

    let mut engine = Engine::new(backend, config("already-there"));
    let report = engine.migrate(&target()).expect("migrate");
    assert_eq!(
        report.unchanged, 1,
        "the target window must not be rewritten"
    );
    assert_eq!(report.applied_count(), 1);

    // It is also not part of the recovery obligation: nothing was moved.
    let restored = engine.restore().expect("restore");
    assert_eq!(restored.count(WindowOutcome::Restored), 1);
}

/// A default gather preserves the window's size instead of folding the desktop.
#[test]
fn default_gather_preserves_size() {
    let mut backend = FakeBackend::with_two_displays();
    let original = Rect::new(300, 300, 1500, 1200);
    backend.add_window(
        1,
        original,
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );

    let mut engine = Engine::new(backend, config("preserve-size"));
    engine.migrate(&target()).expect("migrate");

    let window = engine.backend().enumerate().unwrap().pop().unwrap();
    // Source is 150 %, target is 100 %: the apparent size is preserved, so the
    // physical rectangle becomes two thirds. What must NOT happen is the old
    // desktop-wide compression, which produced a size unrelated to the window.
    assert_eq!(
        window.visible_rect.width(),
        800,
        "a 1200 px window on a 150 % display is 800 px at 100 %"
    );
    assert_eq!(window.visible_rect.height(), 600);
    assert!(window
        .visible_rect
        .reachable_in(&work_area(&engine, &target()), 64, 32));
}

/// Retargeting keeps the first preimage and never closes the transaction.
#[test]
fn retarget_keeps_the_first_baseline() {
    let backend = scenario();
    let before: HashMap<u64, Rect> = backend
        .enumerate()
        .unwrap()
        .into_iter()
        .map(|w| (w.id.native_id, w.visible_rect))
        .collect();

    let mut engine = Engine::new(backend, config("retarget-baseline"));
    engine.migrate(&target()).expect("first gather");
    let transaction = engine.current_transaction().expect("transaction");

    engine.retarget(&other_display()).expect("retarget");
    assert_eq!(
        engine.current_transaction(),
        Some(transaction),
        "retarget must append to the same transaction, not start a new baseline"
    );

    let restored = engine.restore().expect("restore");
    assert!(restored.is_complete());
    for window in engine.backend().enumerate().unwrap() {
        assert_eq!(
            window.visible_rect, before[&window.id.native_id],
            "retarget lost the original position of {:?}",
            window.id
        );
    }
}

/// A mode-only change by the user is an external change and must be preserved.
#[test]
fn a_user_maximizing_a_window_is_preserved_on_restore() {
    let mut backend = FakeBackend::with_two_displays();
    let id = backend.add_window(
        1,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    let mut engine = Engine::new(backend, config("mode-change"));
    engine.migrate(&target()).expect("migrate");

    // The user maximizes the window we just placed. Its rectangle may be the same
    // on some applications, so the mode is the only evidence of the change.
    engine.backend().set_mode(&id, WindowMode::Maximized);

    let report = engine.restore().expect("restore");
    assert_eq!(
        report.outcomes.get(&id),
        Some(&WindowOutcome::KeptUserChange),
        "maximizing after a gather is a user change, not something to undo"
    );
}

/// A window whose original display is gone is visible but still owes its position.
#[test]
fn a_window_from_a_missing_display_keeps_its_obligation() {
    let backend = scenario();
    let original_displays = backend.displays();
    let mut engine = Engine::new(backend, config("missing"));

    engine.migrate(&target()).expect("migrate");

    // The source display disappears while the transaction is open.
    engine.backend().set_displays(
        original_displays
            .iter()
            .filter(|d| d.key != other_display())
            .cloned()
            .collect(),
    );

    let report = engine.restore().expect("restore");
    assert!(
        !report.awaiting_original.is_empty(),
        "a missing source display must keep the original position on record"
    );
    assert_eq!(
        engine.state(),
        EngineState::Degraded,
        "an outstanding obligation is not a clean idle state"
    );
    assert!(!report.is_complete());

    // The journal survives, so a later run can finish the job.
    let dir = engine.config().journal_dir.clone();
    let journals = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "journal"))
        .count();
    assert_eq!(journals, 1, "the recovery record must survive");

    // Once the display is back, the restore finishes and the record is closed.
    engine.backend().set_displays(original_displays);
    let second = engine.restore().expect("second restore");
    assert!(second.is_complete());
    assert_eq!(engine.state(), EngineState::Idle);
    let journals = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "journal"))
        .count();
    assert_eq!(journals, 0, "a completed restore must remove its journal");
}

/// A preimage recorded before identity was provable must not be written.
///
/// Windows recycles `HWND` values. A record from an older build carries
/// `process_start = 0`, which cannot prove the handle still belongs to the same
/// window; writing its rectangle could move an unrelated application. The record
/// must stay pending and be reported, not silently "restored".
#[test]
fn a_record_without_process_identity_is_never_written() {
    let dir = journal_dir("legacy-identity");
    let path = dir.join("legacy.journal");

    let backend = scenario();
    // The record names the real window but cannot prove it: this is what a file
    // written by a build without creation-time support looks like.
    let id = WindowId {
        native_id: 1,
        process_id: 1001,
        process_start: 0,
    };
    let original = Rect::new(100, 100, 900, 700);

    {
        let mut journal = unfy_journal::Journal::begin(&path, "legacy").unwrap();
        journal
            .write_preimage(&unfy_journal::Preimage {
                id: id.clone(),
                display_native_id: "fake-display-a".into(),
                display_quality: IdentityQuality::Strong,
                display_work_area: Rect::new(0, 0, 2560, 1400),
                display_scale: Scale { x: 1.5, y: 1.5 },
                visible_rect: original,
                outer_rect: original,
                mode: WindowMode::Normal,
                topmost: false,
                native_placement: None,
            })
            .unwrap();
        journal.durable().unwrap();
        journal.abandon();
    }

    let mut engine = Engine::new(backend, config("legacy-identity"));
    engine.recover(&path).expect("adopt the legacy record");
    assert_eq!(engine.state(), EngineState::Degraded);

    let report = engine.restore().expect("restore must not fail outright");
    assert_eq!(
        report.outcomes.get(&id),
        Some(&WindowOutcome::Unknown),
        "an unprovable identity must be reported, not treated as restored"
    );
    assert!(
        report.pending.contains(&id),
        "the obligation must stay open so the user can decide"
    );
    assert_eq!(engine.state(), EngineState::Degraded);

    // And nothing was moved.
    let window = engine
        .backend()
        .enumerate()
        .unwrap()
        .into_iter()
        .find(|w| w.id.native_id == id.native_id)
        .unwrap();
    assert_eq!(window.visible_rect, original);

    // The explicit "keep current layout" path is how the user discards it.
    engine.keep_current_layout().expect("discard explicitly");
    assert_eq!(engine.state(), EngineState::Idle);
    assert_eq!(engine.unresolved_window_count(), 0);
}
