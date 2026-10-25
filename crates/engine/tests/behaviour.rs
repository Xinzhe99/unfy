//! Engine behaviour tests against the fake platform.
//!
//! These cover the paths that must never regress: the journal barrier, per-window
//! retry, user-change preservation, and off-screen rescue.

use std::collections::HashMap;
use std::path::PathBuf;

use unfy_domain::{
    DisplayKey, DisplaySnapshot, EngineState, IdentityQuality, Rect, Scale, WindowId, WindowMode,
    WindowOutcome,
};
use unfy_engine::{Engine, EngineConfig, PendingWork};
use unfy_journal::Recovered;
use unfy_platform::fake::{FakeBackend, FakeBehaviour};
use unfy_platform::{DisplayProvider, WindowBackend};

fn journal_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("unfy-engine-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("journal dir");
    dir
}

fn config(tag: &str) -> EngineConfig {
    EngineConfig {
        journal_dir: journal_dir(tag),
        ..Default::default()
    }
}

fn target_key() -> DisplayKey {
    DisplayKey {
        native_id: "fake-display-b".into(),
        quality: IdentityQuality::Strong,
    }
}

/// A fake with three windows on display A (150 %) and a target display B (100 %).
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
    backend.add_window(
        3,
        Rect::new(300, 500, 1100, 1100),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    backend
}

#[test]
fn preview_never_moves_windows() {
    let backend = scenario();
    let before = backend.enumerate().unwrap();
    let engine = Engine::new(backend, config("preview"));

    let preview = engine.preview(&target_key()).expect("preview");
    assert_eq!(preview.eligible.len(), 3);
    assert_eq!(preview.plan.placements.len(), 3);
    assert_eq!(engine.state(), EngineState::Idle);

    let after = engine.backend().enumerate().unwrap();
    assert_eq!(before, after, "preview must not touch any window");
}

#[test]
fn migrate_moves_every_window_onto_the_target_display() {
    let backend = scenario();
    let mut engine = Engine::new(backend, config("migrate"));

    let report = engine.migrate(&target_key()).expect("migrate");
    assert_eq!(report.applied.len(), 3);
    assert_eq!(report.failed_count(), 0);
    assert_eq!(engine.state(), EngineState::Active);

    let target_work = engine
        .backend()
        .topology()
        .unwrap()
        .into_iter()
        .find(|d| d.key == target_key())
        .unwrap()
        .work_area;

    for window in engine.backend().enumerate().unwrap() {
        assert!(
            window.visible_rect.reachable_in(&target_work, 64, 32),
            "window {:?} landed at {:?}, outside {:?}",
            window.id,
            window.visible_rect,
            target_work
        );
    }
}

#[test]
fn restore_returns_every_window_to_its_preimage() {
    let backend = scenario();
    let before: HashMap<u64, Rect> = backend
        .enumerate()
        .unwrap()
        .into_iter()
        .map(|w| (w.id.native_id, w.visible_rect))
        .collect();

    let mut engine = Engine::new(backend, config("restore"));
    engine.migrate(&target_key()).expect("migrate");
    let report = engine.restore().expect("restore");

    assert!(report.is_complete());
    assert_eq!(report.count(WindowOutcome::Restored), 3);
    assert_eq!(engine.state(), EngineState::Idle);

    for window in engine.backend().enumerate().unwrap() {
        let original = before[&window.id.native_id];
        assert_eq!(
            window.visible_rect, original,
            "window {} was not restored",
            window.id.native_id
        );
    }
}

#[test]
fn denied_window_is_rejected_but_others_still_move() {
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        1,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Denied,
    );
    backend.add_window(
        2,
        Rect::new(200, 300, 1000, 900),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );

    let mut engine = Engine::new(backend, config("denied"));
    let report = engine.migrate(&target_key()).expect("migrate");

    // The denied window never reaches the planner because capabilities report it
    // as immovable, so it is skipped rather than retried.
    assert!(report.skipped.iter().any(|s| s.id.native_id == 1));
    assert_eq!(report.outcomes.get(&id(2)), Some(&WindowOutcome::Applied));
    assert_eq!(engine.state(), EngineState::Active);
}

#[test]
fn self_correcting_window_remains_unknown_without_an_overlapping_retry() {
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        9,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::SelfCorrecting,
    );

    let mut engine = Engine::new(backend, config("selfcorrect"));
    let report = engine.migrate(&target_key()).expect("migrate");

    assert_eq!(report.outcomes.get(&id(9)), Some(&WindowOutcome::Unknown));
    assert_eq!(report.failed_count(), 1);
    assert_eq!(engine.state(), EngineState::Degraded);
}

#[test]
fn an_unreported_minimum_remains_pending_without_false_success() {
    let mut backend = FakeBackend::with_two_displays();
    // 2200 wide cannot fit the 1920-wide target, so the planner scales it down;
    // the application's 2000-pixel minimum then overrides that scale. The original
    // rectangle is above the minimum, which is what makes restore possible at all.
    backend.add_window(
        4,
        Rect::new(100, 100, 2300, 1000),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::MinSize(2000, 600),
    );

    let mut engine = Engine::new(backend, config("minsize"));
    let report = engine.migrate(&target_key()).expect("migrate");

    // The application enforces a minimum the backend could not discover. A
    // submitted request is not proof that the planned size was accepted.
    assert_eq!(report.outcomes.get(&id(4)), Some(&WindowOutcome::Unknown));
    assert!(report.applied.is_empty());
    assert_eq!(engine.unresolved_window_count(), 1);
    assert!(engine.current_transaction().is_some());

    // Restore must still reach the original position. It used to re-submit the
    // unconfirmed gather request on every attempt, so a window that cannot fit its
    // cell stayed unresolved forever and the journal never closed.
    let restore = engine.restore().expect("restore after a refused gather");
    assert!(
        restore.is_complete(),
        "an unconfirmed gather request must not block restore: {:?}",
        restore.pending
    );
    assert_eq!(
        engine.backend().read_back(&id(4)).unwrap().visible_rect,
        Rect::new(100, 100, 2300, 1000)
    );
    assert_eq!(engine.state(), EngineState::Idle);
    assert!(engine.current_transaction().is_none());
}

#[test]
fn vanished_window_is_reported_gone_not_failed() {
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        5,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Vanishes,
    );

    let mut engine = Engine::new(backend, config("vanish"));
    let report = engine.migrate(&target_key()).expect("migrate");
    let outcome = report.outcomes.get(&id(5)).copied().expect("outcome");
    assert!(matches!(
        outcome,
        WindowOutcome::Gone | WindowOutcome::Unknown | WindowOutcome::Rejected
    ));
}

#[test]
fn user_move_during_session_is_preserved_on_restore() {
    let backend = scenario();
    let mut engine = Engine::new(backend, config("userchange"));
    engine.migrate(&target_key()).expect("migrate");

    // Simulate the user dragging window 2 somewhere visible on the target.
    let moved = Rect::new(3000, 200, 3600, 700);
    engine
        .backend()
        .apply(
            &id(2),
            &unfy_platform::WindowPlan {
                visible_rect: moved,
                mode: WindowMode::Normal,
                resize: true,
            },
        )
        .expect("user move");

    let report = engine.restore().expect("restore");
    assert_eq!(
        report.outcomes.get(&id(2)),
        Some(&WindowOutcome::KeptUserChange)
    );
    let window = engine
        .backend()
        .enumerate()
        .unwrap()
        .into_iter()
        .find(|w| w.id.native_id == 2)
        .unwrap();
    assert_eq!(window.visible_rect, moved, "user position must win");
}

#[test]
fn offscreen_window_is_rescued_without_a_transaction() {
    let mut backend = FakeBackend::with_two_displays();
    // A window far outside both displays, as happens after unplugging a monitor.
    backend.add_window(
        6,
        Rect::new(9000, 5000, 9600, 5400),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );

    let mut engine = Engine::new(backend, config("rescue"));
    let rescued = engine.rescue_offscreen().expect("rescue");
    assert_eq!(rescued.len(), 1);

    let work_area = engine
        .backend()
        .topology()
        .unwrap()
        .into_iter()
        .find(|d| d.primary)
        .unwrap()
        .work_area;
    let window = engine.backend().enumerate().unwrap().pop().unwrap();
    assert!(window.visible_rect.reachable_in(&work_area, 64, 32));
}

#[test]
fn visible_window_is_not_rescued() {
    let backend = scenario();
    let mut engine = Engine::new(backend, config("norescue"));
    let rescued = engine.rescue_offscreen().expect("rescue");
    assert!(rescued.is_empty(), "visible windows must not be touched");
}

#[test]
fn journal_is_removed_after_a_complete_restore() {
    let dir = journal_dir("journal-removed");
    let backend = scenario();
    let mut engine = Engine::new(
        backend,
        EngineConfig {
            journal_dir: dir.clone(),
            ..Default::default()
        },
    );

    engine.migrate(&target_key()).expect("migrate");
    let files_during: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "journal"))
        .collect();
    assert_eq!(files_during.len(), 1, "one journal per transaction");

    engine.restore().expect("restore");

    let files_after: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "journal"))
        .collect();
    assert!(files_after.is_empty(), "restore must clean up its journal");
}

#[test]
fn keep_current_layout_closes_the_journal_without_moving_windows() {
    let dir = journal_dir("keep");
    let backend = scenario();
    let mut engine = Engine::new(
        backend,
        EngineConfig {
            journal_dir: dir.clone(),
            ..Default::default()
        },
    );

    engine.migrate(&target_key()).expect("migrate");
    let after_migrate = engine.backend().enumerate().unwrap();
    engine.keep_current_layout().expect("keep");

    assert_eq!(engine.state(), EngineState::Idle);
    assert_eq!(engine.backend().enumerate().unwrap(), after_migrate);

    let remaining = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "journal"))
        .count();
    assert_eq!(remaining, 0);
}

#[test]
fn unknown_target_display_is_an_error_not_a_guess() {
    let backend = scenario();
    let engine = Engine::new(backend, config("badtarget"));
    let bogus = DisplayKey {
        native_id: "display-that-does-not-exist".into(),
        quality: IdentityQuality::Strong,
    };
    let err = engine.preview(&bogus).unwrap_err();
    assert!(matches!(
        err,
        unfy_domain::EngineError::TargetDisplayMissing
    ));
}

#[test]
fn restore_with_nothing_pending_is_a_noop() {
    let backend = scenario();
    let mut engine = Engine::new(backend, config("noop"));
    let report = engine.restore().expect("restore");
    assert!(report.is_complete());
    assert!(report.outcomes.is_empty());
}

#[test]
fn recovery_reports_unfinished_transaction_as_pending_work() {
    let dir = journal_dir("pending");
    let path = dir.join("abandoned.journal");
    {
        let mut journal = unfy_journal::Journal::begin(&path, "2 displays").unwrap();
        journal
            .write_preimage(&unfy_journal::Preimage {
                id: id(1),
                display_native_id: "fake-display-a".into(),
                display_quality: IdentityQuality::Strong,
                display_work_area: Rect::new(0, 0, 2560, 1400),
                display_scale: Scale { x: 1.5, y: 1.5 },
                visible_rect: Rect::new(100, 100, 900, 700),
                outer_rect: Rect::new(93, 93, 907, 707),
                mode: WindowMode::Normal,
                topmost: false,
                native_placement: None,
            })
            .unwrap();
        journal.abandon();
    }

    let backend = scenario();
    let mut engine = Engine::new(backend, config("pending-engine"));
    let recovered = engine.recover(&path).expect("recover");
    let Recovered::Unfinished(tx) = recovered else {
        panic!("expected unfinished");
    };
    let pending = PendingWork::from_recovered(&tx);
    assert_eq!(pending.windows, 1);
    assert_eq!(engine.state(), EngineState::Degraded);
}

fn id(n: u64) -> WindowId {
    WindowId {
        native_id: n,
        process_id: 1000 + n as u32,
        process_start: 1,
        instance: 1,
    }
}

#[allow(dead_code)]
fn display(key: &str, primary: bool) -> DisplaySnapshot {
    DisplaySnapshot {
        key: DisplayKey {
            native_id: key.into(),
            quality: IdentityQuality::Strong,
        },
        bounds: Rect::new(0, 0, 1920, 1080),
        work_area: Rect::new(0, 0, 1920, 1040),
        scale: Scale::IDENTITY,
        physical_size_mm: None,
        primary,
        label: key.into(),
    }
}

#[test]
fn adopted_transaction_can_actually_be_restored() {
    // The failure this guards against: a previous run is killed mid-migration,
    // the next launch reports "unfinished work", and the restore button does
    // nothing because the engine never loaded the preimages.
    let dir = journal_dir("adopt");
    let path = dir.join("died.journal");

    let original = Rect::new(100, 100, 900, 700);
    {
        let mut journal = unfy_journal::Journal::begin(&path, "2 displays").unwrap();
        journal
            .write_preimage(&unfy_journal::Preimage {
                id: id(1),
                display_native_id: "fake-display-a".into(),
                display_quality: IdentityQuality::Strong,
                display_work_area: Rect::new(0, 0, 2560, 1400),
                display_scale: Scale { x: 1.5, y: 1.5 },
                visible_rect: original,
                outer_rect: Rect::new(93, 93, 907, 707),
                mode: WindowMode::Normal,
                topmost: false,
                native_placement: None,
            })
            .unwrap();
        journal
            .write_intent(&id(1), Rect::new(3000, 300, 3800, 900), WindowMode::Normal)
            .unwrap();
        journal.durable().unwrap();
        // No `close`: this is what a kill leaves behind.
        journal.abandon();
    }

    // The window is somewhere else now, as if the previous run had moved it.
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        1,
        Rect::new(3000, 300, 3800, 900),
        "fake-display-b",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );

    let mut engine = Engine::new(backend, config("adopt-engine"));
    let adopted = engine
        .recover_latest(&dir)
        .expect("recover latest")
        .expect("a transaction should be adopted");
    assert_eq!(engine.state(), EngineState::Degraded);

    let report = engine.restore().expect("restore adopted transaction");
    assert!(report.is_complete(), "adopted restore must finish");
    assert_eq!(report.transaction, Some(adopted));
    assert_eq!(report.count(WindowOutcome::Restored), 1);

    let window = engine.backend().enumerate().unwrap().pop().unwrap();
    assert_eq!(window.visible_rect, original, "preimage must be honoured");
    assert_eq!(engine.state(), EngineState::Idle);
    assert!(!path.exists(), "journal must be cleaned up after restore");
}

#[test]
fn journal_without_preimages_is_discarded_instead_of_blocking() {
    // A crash between `Begin` and the first preimage leaves a transaction with
    // nothing in it. It must not block the user forever.
    let dir = journal_dir("empty-tx");
    let path = dir.join("empty.journal");
    {
        let journal = unfy_journal::Journal::begin(&path, "2 displays").unwrap();
        journal.abandon();
    }

    let backend = scenario();
    let mut engine = Engine::new(backend, config("empty-tx-engine"));
    let adopted = engine.recover_latest(&dir).expect("recover");
    assert!(adopted.is_none());
    assert_eq!(engine.state(), EngineState::Idle);
    assert!(!path.exists());
}

#[test]
fn second_migration_keeps_the_original_preimage() {
    // Real scenario: the user gathers windows, then notices the remote is
    // showing the other display and switches the target. The restore must still
    // return windows to where they were *before the first* gather, not to where
    // they were after it.
    let backend = scenario();
    let original = backend
        .enumerate()
        .unwrap()
        .into_iter()
        .map(|w| (w.id.native_id, w.visible_rect))
        .collect::<HashMap<_, _>>();

    let mut engine = Engine::new(backend, config("retarget"));
    let first = engine.migrate(&target_key()).expect("first migrate");
    assert_eq!(first.applied.len(), 3);

    // Switching the target while windows are captured must be refused: a second
    // gather would capture the post-move rectangles as if they were home.
    let primary = engine
        .backend()
        .topology()
        .unwrap()
        .into_iter()
        .find(|d| d.primary)
        .unwrap()
        .key;
    let refused = engine.migrate(&primary);
    assert!(
        matches!(
            refused,
            Err(unfy_domain::EngineError::TransactionAlreadyOpen)
        ),
        "a second gather while Active must be refused, got {refused:?}"
    );

    // Restore, then gather again with the new target. The windows are already
    // home, so the move-in default writes nothing; that is the contract, not a
    // failure. The round trip still returns them to the first baseline.
    engine.restore().expect("restore");
    let second = engine
        .migrate(&primary)
        .expect("second migrate after restore");
    assert_eq!(
        second.unchanged, 3,
        "windows already on the target must not be written to"
    );

    let report = engine.restore().expect("restore");
    assert!(report.is_complete());

    for window in engine.backend().enumerate().unwrap() {
        let before_first = original[&window.id.native_id];
        assert_eq!(
            window.visible_rect, before_first,
            "window {} must return to its position before the *first* gather",
            window.id.native_id
        );
    }
}

#[test]
fn excluded_app_is_never_moved() {
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
    backend.set_app_name(2, "obs64.exe");

    let mut engine = Engine::new(
        backend,
        EngineConfig {
            excluded_apps: vec!["OBS64.EXE".into()],
            ..config("exclude")
        },
    );

    let report = engine.migrate(&target_key()).expect("migrate");
    assert_eq!(report.outcomes.get(&id(1)), Some(&WindowOutcome::Applied));
    assert!(
        !report.outcomes.contains_key(&id(2)),
        "the excluded window must not be touched at all"
    );
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.id.native_id == 2 && s.reason == unfy_domain::SkipReason::ExcludedApp),
        "the excluded window must be reported with its reason"
    );

    // And it must still be where it started.
    let window = engine
        .backend()
        .enumerate()
        .unwrap()
        .into_iter()
        .find(|w| w.id.native_id == 2)
        .unwrap();
    assert_eq!(window.visible_rect, Rect::new(200, 300, 1000, 900));
}

#[test]
fn an_exclusion_blocks_gathering_but_not_restoring() {
    // The two halves of the rule, pinned together so a future change cannot keep
    // one and drop the other. An exclusion means "do not move my windows" - it does
    // not mean "leave them where a gather put them". Restoring returns the window to
    // the rectangle it had before, which is the opposite of moving it, and refusing
    // to restore an excluded application would strand it on the target display
    // forever.
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        1,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    backend.set_app_name(1, "obs64.exe");

    let mut engine = Engine::new(
        backend,
        EngineConfig {
            excluded_apps: vec!["obs64.exe".into()],
            ..config("exclude-then-restore")
        },
    );

    // Gathering leaves it exactly where it was.
    let report = engine.migrate(&target_key()).expect("migrate");
    assert!(
        !report.outcomes.contains_key(&id(1)),
        "an excluded window must not be gathered"
    );
    let window = engine
        .backend()
        .enumerate()
        .unwrap()
        .into_iter()
        .find(|w| w.id.native_id == 1)
        .unwrap();
    assert_eq!(window.visible_rect, Rect::new(100, 100, 900, 700));

    // And restoring is not blocked by the exclusion. A second, unexcluded window
    // makes that observable: gather moves it, restore brings it back, and the
    // excluded window is still where it started - proving restore ran over the
    // whole set rather than skipping the excluded entry.
    let mut engine = {
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
            Rect::new(300, 300, 1100, 900),
            "fake-display-a",
            WindowMode::Normal,
            FakeBehaviour::Compliant,
        );
        backend.set_app_name(1, "obs64.exe");
        Engine::new(
            backend,
            EngineConfig {
                excluded_apps: vec!["obs64.exe".into()],
                ..config("exclude-then-restore-2")
            },
        )
    };

    engine.migrate(&target_key()).expect("migrate");
    let gathered = engine
        .backend()
        .enumerate()
        .unwrap()
        .into_iter()
        .find(|w| w.id.native_id == 2)
        .unwrap();
    assert_ne!(
        gathered.visible_rect,
        Rect::new(300, 300, 1100, 900),
        "the unexcluded window must have moved, or this proves nothing"
    );

    let restored = engine.restore().expect("restore");
    assert_eq!(
        restored.outcomes.get(&id(2)),
        Some(&WindowOutcome::Restored),
        "restore must still bring the unexcluded window home"
    );
    for (native, expected) in [
        (1u64, Rect::new(100, 100, 900, 700)),
        (2, Rect::new(300, 300, 1100, 900)),
    ] {
        let window = engine
            .backend()
            .enumerate()
            .unwrap()
            .into_iter()
            .find(|w| w.id.native_id == native)
            .unwrap();
        assert_eq!(window.visible_rect, expected, "window {native} is not home");
    }
}

#[test]
fn rescue_leaves_excluded_applications_alone() {
    // Rescue exists for windows stranded off every display. An exclusion rule says
    // "never move this application", and there is no clause for stranded windows -
    // the exclusions page tells the user these applications will not be moved, full
    // stop. Rescue ignored the rule, so the one command a user reaches for when
    // something is wrong would drag the applications they had protected.
    let mut backend = FakeBackend::with_two_displays();
    // Fully outside both displays (which span 0..4480).
    backend.add_window(
        1,
        Rect::new(9000, 9000, 9600, 9600),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    backend.add_window(
        2,
        Rect::new(9200, 9000, 9800, 9600),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    backend.set_app_name(2, "obs64.exe");

    let mut engine = Engine::new(
        backend,
        EngineConfig {
            excluded_apps: vec!["OBS64.EXE".into()],
            ..config("rescue-excluded")
        },
    );

    let rescued = engine.rescue_offscreen().expect("rescue");
    let ids: Vec<u64> = rescued.iter().map(|(id, _)| id.native_id).collect();
    assert_eq!(ids, vec![1], "only the unexcluded window may be rescued");

    let window = engine
        .backend()
        .enumerate()
        .unwrap()
        .into_iter()
        .find(|w| w.id.native_id == 2)
        .unwrap();
    assert_eq!(
        window.visible_rect,
        Rect::new(9200, 9000, 9800, 9600),
        "the excluded window must stay where it was"
    );
}

#[test]
fn exclusion_is_case_insensitive_and_ignores_unknown_names() {
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        1,
        Rect::new(100, 100, 900, 700),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    backend.set_app_name(1, "Spotify.exe");

    let mut engine = Engine::new(
        backend,
        EngineConfig {
            excluded_apps: vec!["spotify.EXE".into(), "never-running.exe".into()],
            ..config("exclude-case")
        },
    );
    let report = engine.migrate(&target_key()).expect("migrate");
    assert!(report.applied.is_empty());
}

#[test]
fn multiple_unfinished_journals_require_explicit_selection() {
    // Two crashes can leave two open transactions. Deleting the older one because
    // a newer file exists would discard the only record of where its windows were.
    let dir = journal_dir("stale");
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let _ = std::fs::remove_file(entry.path());
    }

    let make = |name: &str, n: u64| {
        let path = dir.join(name);
        let mut journal = unfy_journal::Journal::begin(&path, "2 displays").unwrap();
        journal
            .write_preimage(&unfy_journal::Preimage {
                id: WindowId {
                    native_id: n,
                    process_id: 1000 + n as u32,
                    process_start: 1,
                    instance: 1,
                },
                display_native_id: "fake-display-a".into(),
                display_quality: IdentityQuality::Strong,
                display_work_area: Rect::new(0, 0, 2560, 1400),
                display_scale: Scale { x: 1.5, y: 1.5 },
                visible_rect: Rect::new(100, 100, 900, 700),
                outer_rect: Rect::new(93, 93, 907, 707),
                mode: WindowMode::Normal,
                topmost: false,
                native_placement: None,
            })
            .unwrap();
        journal.abandon();
        std::thread::sleep(std::time::Duration::from_millis(20));
        path
    };

    let old_a = make("a.journal", 1);
    let old_b = make("b.journal", 2);
    let newest = make("c.journal", 3);

    let backend = scenario();
    let mut engine = Engine::new(backend, config("stale-engine"));
    assert!(engine.recover_latest(&dir).is_err());
    assert!(engine.current_transaction().is_none());
    assert!(
        old_a.exists(),
        "an older unfinished journal must not be deleted"
    );
    assert!(
        old_b.exists(),
        "an older unfinished journal must not be deleted"
    );
    assert!(newest.exists(), "the adopted journal must be kept");
    assert_eq!(engine.state(), EngineState::Error);
}

#[test]
fn adopted_journal_is_deleted_when_it_has_nothing_to_restore() {
    // A crash between `Begin` and the first preimage left an empty transaction
    // that used to block every future gather with "unfinished work" and could
    // never be resolved.
    let dir = journal_dir("empty-adopt");
    let path = dir.join("empty.journal");
    {
        let journal = unfy_journal::Journal::begin(&path, "test").unwrap();
        journal.abandon();
    }

    let backend = scenario();
    let mut engine = Engine::new(backend, config("empty-adopt-engine"));
    assert!(engine.recover_latest(&dir).expect("recover").is_none());
    assert_eq!(engine.state(), EngineState::Idle);
    assert!(!path.exists(), "an empty transaction must be discarded");
}

/// A retarget must not redefine "home".
///
/// The failure this pins: `restore` then `migrate` as two user actions would
/// record the restored positions as the new preimages the moment the second
/// gather started. Doing it inside the engine gives the caller one operation,
/// and the preimages the user started with must survive it.
#[test]
fn retarget_keeps_the_original_preimages() {
    let backend = scenario();
    let before: HashMap<u64, Rect> = backend
        .enumerate()
        .unwrap()
        .into_iter()
        .map(|w| (w.id.native_id, w.visible_rect))
        .collect();

    let mut engine = Engine::new(backend, config("retarget"));

    // Gather onto display B, then change the target to display A while the
    // windows are still captured.
    let first = engine.retarget(&target_key()).expect("first retarget");
    assert_eq!(first.migration.applied_count(), 3);

    let display_a = DisplayKey {
        native_id: "fake-display-a".into(),
        quality: IdentityQuality::Strong,
    };
    let second = engine.retarget(&display_a).expect("second retarget");
    assert_eq!(second.migration.applied_count(), 3);
    assert_eq!(engine.state(), EngineState::Active);

    // And the final restore still knows where the windows came from.
    let restored = engine.restore().expect("restore");
    assert!(restored.is_complete());
    assert_eq!(restored.count(WindowOutcome::Restored), 3);

    for window in engine.backend().enumerate().unwrap() {
        assert_eq!(
            window.visible_rect, before[&window.id.native_id],
            "retarget lost the original position of {:?}",
            window.id
        );
    }
}

/// After "keep current layout", the current positions become home.
///
/// The user accepted the arrangement, so a later restore must return to *that*,
/// not to the layout from before the gather they rejected. Getting this wrong
/// would silently move windows to a place the user had already said no to.
#[test]
fn keeping_the_layout_makes_it_the_new_baseline() {
    let backend = scenario();
    let mut engine = Engine::new(backend, config("keep-baseline"));

    engine.migrate(&target_key()).expect("first gather");
    let accepted: HashMap<u64, Rect> = engine
        .backend()
        .enumerate()
        .unwrap()
        .into_iter()
        .map(|w| (w.id.native_id, w.visible_rect))
        .collect();

    engine.keep_current_layout().expect("keep");
    assert_eq!(engine.captured_window_count(), 0);

    // Gather again, then restore: the target is the kept layout.
    engine.migrate(&target_key()).expect("second gather");
    let restored = engine.restore().expect("restore");
    assert!(restored.is_complete());

    for window in engine.backend().enumerate().unwrap() {
        assert_eq!(
            window.visible_rect, accepted[&window.id.native_id],
            "restore after keep must return to the accepted layout for {:?}",
            window.id
        );
    }
}

#[test]
fn retarget_on_idle_engine_is_a_plain_gather() {
    let backend = scenario();
    let mut engine = Engine::new(backend, config("retarget-idle"));

    let report = engine.retarget(&target_key()).expect("retarget");
    assert_eq!(report.migration.applied_count(), 3);
}

/// The progress hook must fire once per submitted window and end at (total, total).
///
/// Without it the UI shows an indeterminate spinner for the whole gather. On an
/// idle desktop that is 200 ms; with one application holding its message queue it
/// can be seconds, and an indeterminate spinner that long reads as a hang.
#[test]
fn migrate_reports_progress_for_every_window() {
    use std::sync::{Arc, Mutex};

    let backend = scenario();
    let seen: Arc<Mutex<Vec<(usize, usize)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();

    let mut engine = Engine::new(
        backend,
        EngineConfig {
            on_progress: Some(Arc::new(move |done, total| {
                sink.lock().unwrap().push((done, total));
            })),
            ..config("progress")
        },
    );

    engine.migrate(&target_key()).expect("migrate");

    let events = seen.lock().unwrap().clone();
    assert_eq!(
        events.len(),
        3,
        "one event per submitted window: {events:?}"
    );
    assert_eq!(events.first(), Some(&(1, 3)));
    assert_eq!(
        events.last(),
        Some(&(3, 3)),
        "the last event must reach the total, or the UI never shows completion"
    );
    // Monotonic: a counter that jumps backwards looks like a restart.
    assert!(events.windows(2).all(|w| w[0].0 < w[1].0), "{events:?}");
}

#[test]
fn no_progress_hook_installed_is_not_an_error() {
    // The default config has no hook; migrating must still work.
    let mut engine = Engine::new(scenario(), config("no-progress"));
    let report = engine.migrate(&target_key()).expect("migrate");
    assert_eq!(report.applied_count(), 3);
}

/// Focus must reach the planner.
///
/// `EligibleWindow.focused` was hardcoded to `false` while the grid uses it to
/// order cells. The layout had its own unit test with `focused = true` set by
/// hand, so the broken wiring between the platform snapshot and the planner was
/// invisible.
#[test]
fn focused_window_is_placed_in_the_first_grid_cell() {
    let mut backend = scenario();
    backend.focus(2);

    let engine = Engine::new(
        backend,
        EngineConfig {
            layout: unfy_layout::LayoutConfig {
                mode: unfy_layout::LayoutMode::Grid,
                ..Default::default()
            },
            ..config("focus")
        },
    );

    let preview = engine.preview(&target_key()).expect("preview");
    let work = engine
        .backend()
        .topology()
        .unwrap()
        .into_iter()
        .find(|d| d.key == target_key())
        .unwrap()
        .work_area;

    let focused = preview
        .plan
        .placements
        .iter()
        .find(|p| p.id.native_id == 2)
        .expect("the focused window is in the plan");
    assert!(
        focused.rect.reachable_in(&work, 64, 32),
        "the focused window must be inside the work area"
    );
    // The first cell is the top-left one; nothing else may be further up-left.
    for other in &preview.plan.placements {
        assert!(
            (focused.rect.top, focused.rect.left) <= (other.rect.top, other.rect.left),
            "focused window {:?} is not first; {:?} is",
            focused.rect,
            other.rect
        );
    }
}

/// A window's own minimum size must reach the planner.
///
/// Verified against a live desktop, the values differ enormously per application
/// (Notepad reports 332x202, an IDE-class window 616x408). A single global
/// fallback let large applications be shrunk into a slot their chrome cannot fit
/// in, which shows up as overlapping windows.
#[test]
fn a_large_window_minimum_is_respected() {
    let mut backend = scenario();
    // Make one window insist on a size that a narrow slot cannot provide.
    backend.set_min_size(1, unfy_domain::Size::new(900, 700));

    let engine = Engine::new(backend, config("min-size"));
    let preview = engine.preview(&target_key()).expect("preview");

    let placement = preview
        .plan
        .placements
        .iter()
        .find(|p| p.id.native_id == 1)
        .expect("window 1 is planned");
    assert!(
        placement.rect.width() >= 600 && placement.rect.height() >= 467,
        "the slot must satisfy the window's minimum, got {}x{}",
        placement.rect.width(),
        placement.rect.height()
    );
    // The 900x700 minimum is expressed at source scale 150%; at 100% it
    // becomes 600x467. The default mode must honor it even though the window fits.
    assert!(
        placement.clamped_to_min,
        "a minimum that dominates the layout must be reported as clamped"
    );
}

/// A failed journal write must move the engine to `Error`, not leave it busy.
///
/// Before this, `migrate` returned the write error via `?` while the state had
/// already been set to `Migrating`. Every later call then failed with
/// `InvalidTransition` - gather, restore, retarget - so the app was stuck until it
/// was restarted, and the UI reported "migrating" for an operation that had
/// already aborted.
#[test]
fn a_failed_journal_write_enters_the_error_state() {
    // Occupy the journal path with a file so creating the directory fails.
    let blocker = std::env::temp_dir().join(format!("unfy-journal-blocker-{}", std::process::id()));
    std::fs::write(&blocker, b"not a directory").expect("write blocker");

    let mut engine = Engine::new(
        scenario(),
        EngineConfig {
            // A path whose parent is a file cannot be created.
            journal_dir: blocker.join("journal"),
            ..Default::default()
        },
    );

    let result = engine.migrate(&target_key());
    assert!(result.is_err(), "the gather must fail without a journal");

    assert_eq!(
        engine.state(),
        EngineState::Error,
        "a failed durable write is not a transient condition"
    );

    assert!(
        engine.rescue_offscreen().is_err(),
        "rescue also requires a durable journal"
    );
    // But new gathers must be refused rather than attempted on a broken journal.
    assert!(engine.migrate(&target_key()).is_err());

    let _ = std::fs::remove_file(&blocker);
}

/// A failed preview must not leave the engine unable to act.
///
/// `preview` set the state to `Planning`, then returned the platform error with
/// `?`. Until this was fixed that left the engine in `Planning` forever, and every
/// later call - gather, restore, retarget, another preview - was refused with
/// `InvalidTransition`. A single transient topology error would wedge the app.
#[test]
fn a_failed_preview_returns_to_idle() {
    use unfy_domain::DisplayKey;

    let mut engine = Engine::new(scenario(), config("preview-fail"));

    // A display that does not exist makes `build_preview` fail after the state has
    // already been set to `Planning`.
    let missing = DisplayKey {
        native_id: "display-that-is-gone".into(),
        quality: IdentityQuality::Strong,
    };
    let result = engine.preview(&missing);
    assert!(result.is_err(), "a missing target must fail the preview");

    assert_eq!(
        engine.state(),
        EngineState::Idle,
        "a failed read-only preview must not leave the engine busy"
    );

    // The engine must still work.
    let preview = engine.preview(&target_key()).expect("preview still works");
    assert_eq!(preview.eligible.len(), 3);
    assert_eq!(
        engine.migrate(&target_key()).map(|r| r.applied_count()),
        Ok(3),
        "gathering must still be possible after a failed preview"
    );
}

/// An uncertain partial move retains its preimage and does not submit an
/// unjournaled size compensation over a possibly outstanding platform request.
#[test]
fn a_partial_move_remains_pending_for_recovery() {
    let mut backend = FakeBackend::with_two_displays();
    let before = Rect::new(100, 100, 700, 600);
    let id = backend.add_window(
        21,
        before,
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::SizeOnly,
    );

    // What the window actually looks like before anything is asked of it.
    let original = backend.read_back(&id).expect("read back").visible_rect;
    assert_eq!(original, before);

    let mut engine = Engine::new(backend, config("sizeonly"));
    let report = engine.migrate(&target_key()).expect("migrate");

    assert_eq!(
        report.outcomes.get(&id),
        Some(&WindowOutcome::Unknown),
        "a window that stays put was not placed"
    );

    assert_eq!(engine.unresolved_window_count(), 1);
    assert!(engine.current_transaction().is_some());
    assert!(engine.retarget(&target_key()).is_err());
}

#[test]
fn a_window_straddling_two_displays_is_moved_fully_onto_the_target() {
    // Displays span 0..2560 (A) and 2560..4480 (B); the target is B. This window
    // is 1000 wide with 400 px on A and 600 px on B. Leaving it where it is would
    // keep 400 px outside the screen the remote client shows, so a straddling
    // window is not "already on the target": the whole window is moved in.
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        7,
        Rect::new(2160, 200, 3160, 800),
        "fake-display-a",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    let mut engine = Engine::new(backend, config("straddle"));
    let report = engine.migrate(&target_key()).expect("migrate");

    assert_eq!(report.outcomes.get(&id(7)), Some(&WindowOutcome::Applied));
    let placed = engine.backend().read_back(&id(7)).unwrap().visible_rect;
    let target_work = Rect::new(2560, 0, 4480, 1040);
    assert_eq!(placed.width(), 1000, "a straddling window keeps its size");
    assert_eq!(placed.height(), 600);
    assert!(
        placed.left >= target_work.left && placed.right <= target_work.right,
        "no part of the window may stay on the other display: {placed:?}"
    );
}

#[test]
fn a_straddling_window_already_inside_the_target_is_left_alone() {
    // Same window after a gather: fully on B. A second gather must not write it
    // again, which is what makes repeated gathering idempotent.
    let mut backend = FakeBackend::with_two_displays();
    backend.add_window(
        7,
        Rect::new(2600, 200, 3600, 800),
        "fake-display-b",
        WindowMode::Normal,
        FakeBehaviour::Compliant,
    );
    let mut engine = Engine::new(backend, config("straddle-idempotent"));
    let report = engine.migrate(&target_key()).expect("migrate");

    assert!(
        !report.outcomes.contains_key(&id(7)),
        "a window already inside the target must receive zero writes: {:?}",
        report.outcomes
    );
    assert_eq!(
        engine.backend().read_back(&id(7)).unwrap().visible_rect,
        Rect::new(2600, 200, 3600, 800)
    );
}
