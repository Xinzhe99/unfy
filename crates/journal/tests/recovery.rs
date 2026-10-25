//! Crash and corruption tests for the journal.
//!
//! These are the tests that decide whether the product is allowed to move a
//! window at all: if any of them fails, the "write preimage before touching a
//! window" guarantee is not real.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use unfy_domain::{IdentityQuality, Rect, WindowId, WindowMode, WindowOutcome};
use unfy_journal::{
    recover, AppliedResult, Journal, ObservedRect, Preimage, Recovered, RecoveredTransaction,
};

fn temp_journal() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("unfy-journal-test-{}", uuid_like()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join("journal.bin")
}

/// Small unique suffix without pulling `uuid` into the test's public surface.
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos}-{:?}", std::thread::current().id())
}

fn preimage(native_id: u64) -> Preimage {
    Preimage {
        id: WindowId {
            native_id,
            process_id: 100,
            process_start: 1,
            instance: 1,
        },
        display_native_id: "display-a".into(),
        display_quality: IdentityQuality::Strong,
        display_work_area: Rect::new(0, 0, 2560, 1400),
        display_scale: unfy_domain::Scale { x: 1.5, y: 1.5 },
        visible_rect: Rect::new(10, 20, 810, 620),
        outer_rect: Rect::new(3, 13, 817, 627),
        mode: WindowMode::Normal,
        topmost: false,
        native_placement: None,
    }
}

#[test]
fn missing_file_is_clean() {
    let path = temp_journal();
    assert_eq!(recover(&path).unwrap(), Recovered::Clean);
}

#[test]
fn closed_transaction_is_clean() {
    let path = temp_journal();
    let mut journal = Journal::begin(&path, "1 display").unwrap();
    journal.write_preimage(&preimage(1)).unwrap();
    journal
        .write_resolved(&preimage(1).id, WindowOutcome::KeptUserChange)
        .unwrap();
    journal.close().unwrap();

    assert_eq!(recover(&path).unwrap(), Recovered::Clean);
}

#[test]
fn unfinished_transaction_is_reported_with_preimages() {
    let path = temp_journal();
    let mut journal = Journal::begin(&path, "2 displays").unwrap();
    journal.write_preimage(&preimage(1)).unwrap();
    journal.write_preimage(&preimage(2)).unwrap();
    let transaction = journal.transaction();
    journal.abandon();

    match recover(&path).unwrap() {
        Recovered::Unfinished(tx) => {
            assert_eq!(tx.transaction, transaction);
            assert_eq!(tx.preimages.len(), 2);
            assert_eq!(tx.truncated_bytes, 0);
            assert!(!tx.may_have_moved_windows());
        }
        Recovered::Clean => panic!("unfinished journal must not be reported clean"),
    }
}

#[test]
fn applied_result_marks_windows_as_possibly_moved() {
    let path = temp_journal();
    let mut journal = Journal::begin(&path, "2 displays").unwrap();
    journal.write_preimage(&preimage(1)).unwrap();
    journal
        .write_intent(
            &preimage(1).id,
            Rect::new(0, 0, 400, 300),
            WindowMode::Normal,
        )
        .unwrap();
    journal
        .write_result(&AppliedResult {
            id: preimage(1).id,
            outcome: WindowOutcome::Applied,
            observed: Some(ObservedRect {
                visible_rect: Rect::new(0, 0, 400, 300),
                mode: WindowMode::Normal,
            }),
        })
        .unwrap();
    journal.abandon();

    let Recovered::Unfinished(tx) = recover(&path).unwrap() else {
        panic!("expected unfinished");
    };
    assert!(tx.may_have_moved_windows());
    assert_eq!(tx.results.len(), 1);
    assert!(tx.preimage_for(&preimage(1).id).is_some());
}

#[test]
fn intent_without_result_still_allows_recovery() {
    // The dangerous window: the platform call may have happened, but the process
    // died before recording the outcome. Recovery must treat the window as
    // possibly moved even though no Result record exists.
    let path = temp_journal();
    let mut journal = Journal::begin(&path, "2 displays").unwrap();
    journal.write_preimage(&preimage(7)).unwrap();
    journal
        .write_intent(
            &preimage(7).id,
            Rect::new(0, 0, 400, 300),
            WindowMode::Normal,
        )
        .unwrap();
    journal.abandon();

    let Recovered::Unfinished(tx) = recover(&path).unwrap() else {
        panic!("expected unfinished");
    };
    assert_eq!(tx.preimages.len(), 1);
    assert!(tx.results.is_empty());
    // No result means we cannot prove it did *not* move, so the engine must
    // offer recovery rather than assume it is untouched.
    assert!(tx.has_unresolved_intent());
}

#[test]
fn truncated_tail_drops_only_the_torn_record() {
    let path = temp_journal();
    let mut journal = Journal::begin(&path, "2 displays").unwrap();
    journal.write_preimage(&preimage(1)).unwrap();
    journal.write_preimage(&preimage(2)).unwrap();
    journal.abandon();

    // Simulate a torn append: chop the last 6 bytes of the final record.
    let bytes = std::fs::read(&path).unwrap();
    let cut = bytes.len() - 6;
    std::fs::write(&path, &bytes[..cut]).unwrap();

    let Recovered::Unfinished(tx) = recover(&path).unwrap() else {
        panic!("expected unfinished");
    };
    // The whole torn record is discarded, not partially accepted. That is safe:
    // a torn preimage means the window was never moved, because the write
    // barrier is what authorises the move.
    assert_eq!(tx.preimages.len(), 1);
    assert_eq!(tx.preimages[0].id, preimage(1).id);
    assert!(tx.truncated_bytes > 6, "the entire torn record is dropped");
    assert!(!path.with_extension("corrupt").exists());
    assert_eq!(std::fs::read(&path).unwrap(), bytes[..cut]);
}

#[test]
fn garbage_tail_is_blocked_without_changing_the_file() {
    let path = temp_journal();
    let mut journal = Journal::begin(&path, "2 displays").unwrap();
    journal.write_preimage(&preimage(1)).unwrap();
    journal.write_preimage(&preimage(2)).unwrap();
    journal.abandon();

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&[0xFF; 32]).unwrap();
    drop(file);

    let before = std::fs::read(&path).unwrap();
    assert!(recover(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!path.with_extension("corrupt").exists());
}

#[test]
fn begin_refuses_to_overwrite_a_previous_transaction() {
    let path = temp_journal();
    {
        let mut journal = Journal::begin(&path, "old").unwrap();
        journal.write_preimage(&preimage(1)).unwrap();
        journal.abandon();
    }
    let before = std::fs::read(&path).unwrap();
    assert!(Journal::begin(&path, "new").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn close_and_remove_leaves_no_file() {
    let path = temp_journal();
    let journal = Journal::begin(&path, "1 display").unwrap();
    journal.close_and_remove().unwrap();
    assert!(!path.exists());
    assert_eq!(recover(&path).unwrap(), Recovered::Clean);
}

#[test]
fn recovered_transaction_helper_behaviour() {
    let tx = RecoveredTransaction {
        transaction: uuid::Uuid::nil(),
        schema_version: 1,
        preimages: vec![preimage(1)],
        intents: Vec::new(),
        results: vec![AppliedResult {
            id: preimage(2).id,
            outcome: WindowOutcome::Rejected,
            observed: None,
        }],
        resolved: Vec::new(),
        truncated_bytes: 0,
    };
    assert!(!tx.may_have_moved_windows());
    assert!(tx.preimage_for(&preimage(1).id).is_some());
    assert!(tx.preimage_for(&preimage(2).id).is_none());
}

/// Kill a real child process mid-write and verify the journal is still usable.
///
/// This is the only test that proves the durability claim against an actual
/// process termination rather than a simulated file edit.
#[test]
fn kill_child_mid_write_leaves_recoverable_journal() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let path = temp_journal();
    let exe = env!("CARGO_BIN_EXE_crash_writer");

    for delay_ms in [5u64, 15, 40, 120] {
        let mut child = Command::new(exe)
            .arg(&path)
            .arg("1000000")
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn crash writer");

        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("ready line");
        assert_eq!(line.trim(), "ready");

        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        child.kill().expect("kill");
        child.wait().expect("wait");

        let recovered = recover(&path).expect("recovery must not fail");
        match recovered {
            Recovered::Unfinished(tx) => {
                // Whatever survived must be a prefix with no duplicate or
                // out-of-order ids: a torn tail can only drop records.
                let ids: Vec<u64> = tx.preimages.iter().map(|p| p.id.native_id).collect();
                let mut sorted = ids.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(ids, sorted, "preimages must be a dense prefix");
                for (i, id) in ids.iter().enumerate() {
                    assert_eq!(*id, i as u64, "preimage order must be preserved");
                }
            }
            Recovered::Clean => {
                // Possible only if the kill landed before `Begin` was flushed,
                // which the `ready` handshake makes very unlikely. Accept it but
                // do not let it mask a real bug silently.
                eprintln!("note: kill at {delay_ms}ms produced a clean journal");
            }
        }

        std::fs::remove_file(&path).ok();
        std::fs::remove_file(path.with_extension("corrupt")).ok();
    }
}

#[test]
fn a_later_intent_is_not_satisfied_by_an_earlier_result() {
    let path = temp_journal();
    let preimage = preimage(1);
    let mut journal = Journal::begin(&path, "repeated write").unwrap();
    journal.write_preimage(&preimage).unwrap();
    journal
        .write_intent(&preimage.id, preimage.visible_rect, WindowMode::Normal)
        .unwrap();
    journal
        .write_result(&AppliedResult {
            id: preimage.id.clone(),
            outcome: WindowOutcome::Applied,
            observed: Some(ObservedRect {
                visible_rect: preimage.visible_rect,
                mode: WindowMode::Normal,
            }),
        })
        .unwrap();
    journal
        .write_intent(
            &preimage.id,
            Rect::new(50, 50, 850, 650),
            WindowMode::Normal,
        )
        .unwrap();
    journal.durable().unwrap();
    journal.abandon();
    let Recovered::Unfinished(tx) = recover(&path).unwrap() else {
        panic!("unfinished");
    };
    assert!(tx.has_unresolved_intent());
    assert!(tx.may_have_moved_windows());
    assert!(tx.results.is_empty());
    assert_eq!(tx.intents[0].write_seq, 2);
}

#[test]
fn reopen_validates_identity_before_truncating_a_short_tail() {
    let path = temp_journal();
    let journal = Journal::begin(&path, "reopen").unwrap();
    let id = journal.transaction();
    journal.abandon();
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&[1, 2, 3])
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(Journal::reopen(&path, uuid::Uuid::new_v4()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut journal = Journal::reopen(&path, id).unwrap();
    journal.close().unwrap();
    assert_eq!(recover(&path).unwrap(), Recovered::Clean);
}

#[test]
fn closed_with_unresolved_windows_or_a_short_suffix_is_corrupt() {
    let path = temp_journal();
    let mut journal = Journal::begin(&path, "unfinished close").unwrap();
    journal.write_preimage(&preimage(1)).unwrap();
    journal.close().unwrap();
    assert!(recover(&path).is_err());

    let path = temp_journal();
    let mut journal = Journal::begin(&path, "closed suffix").unwrap();
    journal.close().unwrap();
    drop(journal);
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&[1])
        .unwrap();
    assert!(recover(&path).is_err());
}

#[test]
fn stale_sequenced_result_cannot_resolve_a_new_write() {
    let path = temp_journal();
    let mut journal = Journal::begin(&path, "stale result").unwrap();
    let transaction = journal.transaction();
    let preimage = preimage(1);
    journal.write_preimage(&preimage).unwrap();
    journal
        .write_intent(&preimage.id, preimage.visible_rect, WindowMode::Normal)
        .unwrap();
    journal
        .write_intent(&preimage.id, preimage.visible_rect, WindowMode::Maximized)
        .unwrap();
    journal.abandon();
    let record = unfy_journal::Record::Result {
        transaction,
        write_seq: 1,
        result: AppliedResult {
            id: preimage.id,
            outcome: WindowOutcome::Applied,
            observed: None,
        },
    };
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&record.encode().unwrap())
        .unwrap();
    assert!(recover(&path).is_err());
}

#[test]
fn a_resolved_window_cannot_be_reopened_by_an_intent() {
    let path = temp_journal();
    let mut journal = Journal::begin(&path, "resolved window").unwrap();
    let preimage = preimage(1);
    journal.write_preimage(&preimage).unwrap();
    journal
        .write_resolved(&preimage.id, WindowOutcome::KeptUserChange)
        .unwrap();
    journal
        .write_intent(&preimage.id, preimage.visible_rect, WindowMode::Normal)
        .unwrap();
    journal.abandon();
    assert!(recover(&path).is_err());
}

#[test]
fn empty_existing_file_is_an_unknown_recovery_blocker() {
    let path = temp_journal();
    std::fs::write(&path, []).unwrap();
    assert!(recover(&path).is_err());
}

#[test]
fn future_schema_is_a_typed_blocker() {
    let path = temp_journal();
    let version = unfy_journal::SCHEMA_VERSION + 1;
    let begin = unfy_journal::Record::Begin {
        schema_version: version,
        transaction: uuid::Uuid::new_v4(),
        display_summary: "future".into(),
        created_unix_ms: 0,
    };
    std::fs::write(&path, begin.encode().unwrap()).unwrap();
    assert_eq!(
        recover(&path),
        Err(unfy_journal::JournalError::SchemaMismatch(version))
    );
}
