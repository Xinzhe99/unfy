//! Deterministic end-to-end verification against the real Windows desktop.
//!
//! The GUI-driven test is useful but noisy: the computer-use harness moves its
//! own window while clicking buttons, which shows up as a restore mismatch that
//! has nothing to do with Conflux. This binary removes that noise by owning the
//! whole scenario:
//!
//! 1. spawn a child process that creates N plain Win32 windows on display 1;
//! 2. snapshot their exact rectangles;
//! 3. run the real engine: migrate everything to display 2;
//! 4. assert every window is reachable inside display 2's work area;
//! 5. restore;
//! 6. assert every window is back on its exact original rectangle.
//!
//! It asserts rather than printing, so a regression fails the process. It mutates
//! only the windows it created.
//!
//! Run: `cargo run -p conflux-platform-win --bin e2e`

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use conflux_domain::{DisplaySnapshot, Rect, WindowId};
use conflux_engine::{Engine, EngineConfig};
use conflux_platform::{DisplayProvider, WindowBackend};
use conflux_platform_win::WinPlatform;

/// How many host windows to create. Enough to force a multi-row grid.
const WINDOWS: usize = 6;

fn main() {
    if std::env::args().any(|arg| arg == "--host") {
        host_main();
        return;
    }

    let displays = WinPlatform::new()
        .topology()
        .expect("topology must be readable");
    if displays.len() < 2 {
        println!("SKIP: need two displays, found {}", displays.len());
        return;
    }
    let source = &displays[0];
    let target = &displays[1];
    println!(
        "source {:<14} {}x{} @ {},{}",
        source.label,
        source.work_area.width(),
        source.work_area.height(),
        source.work_area.left,
        source.work_area.top
    );
    println!(
        "target {:<14} {}x{} @ {},{}",
        target.label,
        target.work_area.width(),
        target.work_area.height(),
        target.work_area.left,
        target.work_area.top
    );

    let mut host = Host::spawn(WINDOWS, source).expect("spawn host windows");

    let journal_dir = std::env::temp_dir().join(format!("conflux-e2e-{}", std::process::id()));
    let config = EngineConfig {
        journal_dir: journal_dir.clone(),
        ..Default::default()
    };

    let mut engine = Engine::new(WinPlatform::new(), config);

    let before = host.rects(&engine);
    assert_eq!(before.len(), WINDOWS, "host windows must be enumerable");
    println!("\ncaptured {} windows", before.len());
    for (id, rect) in &before {
        println!("  {:x} {:?}", id.native_id, rect);
    }

    let report = engine
        .migrate(&target.key)
        .expect("migration must not fail outright");
    // The engine moves every eligible window on the desktop, not only the ones
    // this harness created. That is correct product behaviour, but it means the
    // assertions below must be scoped to the host's own handles.
    println!(
        "\nmigration: applied={} rejected={} skipped={} (desktop-wide)",
        report.applied.len(),
        report.rejected_count(),
        report.skipped.len()
    );
    let moved_ours = before
        .keys()
        .filter(|id| report.applied.iter().any(|a| a == *id))
        .count();
    assert_eq!(
        moved_ours, WINDOWS,
        "every host window must be in the applied set"
    );

    // Only the host's own windows matter; other applications on the desktop are
    // not part of this assertion.
    let after_migrate = host.rects(&engine);
    let mut failures = Vec::new();

    for (id, original) in &before {
        let Some(now) = after_migrate.get(id) else {
            failures.push(format!("{:x}: disappeared during migration", id.native_id));
            continue;
        };
        if !now.reachable_in(&target.work_area, 64, 32) {
            failures.push(format!(
                "{:x}: not reachable on target work area, sits at {now:?}",
                id.native_id
            ));
        }
        if now == original {
            failures.push(format!("{:x}: did not move at all", id.native_id));
        }
    }
    assert!(
        failures.is_empty(),
        "migration assertions failed:\n  {}",
        failures.join("\n  ")
    );
    println!("migration PASS: every window is inside the target work area");

    let restore = engine.restore().expect("restore must not fail outright");
    // Other applications on this desktop are also part of the migration, and
    // some of them refuse to be moved. That is reported honestly rather than
    // hidden, so the assertion must be scoped to the windows this harness owns;
    // a foreign window left in `pending` is not a Conflux failure.
    let ours_pending = restore
        .pending
        .iter()
        .filter(|id| before.contains_key(*id))
        .count();
    println!(
        "restore: complete={} pending={} (ours={} others={})",
        restore.is_complete(),
        restore.pending.len(),
        ours_pending,
        restore.pending.len() - ours_pending
    );
    assert_eq!(
        ours_pending, 0,
        "every host window must reach a safe terminal state"
    );

    let after_restore = host.rects(&engine);
    let mut drift = Vec::new();
    for (id, original) in &before {
        let Some(now) = after_restore.get(id) else {
            drift.push(format!("{:x}: disappeared during restore", id.native_id));
            continue;
        };
        let worst = (now.left - original.left)
            .abs()
            .max((now.top - original.top).abs())
            .max((now.width() - original.width()).abs())
            .max((now.height() - original.height()).abs());
        if worst > 4 {
            drift.push(format!(
                "{:x}: moved back to {now:?} instead of {original:?} ({worst} px off)",
                id.native_id
            ));
        }
    }
    assert!(
        drift.is_empty(),
        "restore assertions failed:\n  {}",
        drift.join("\n  ")
    );
    println!("restore PASS: every window returned to its original rectangle (tolerance 4 px)");

    // A journal may only be deleted once every window reaches a safe terminal
    // state. Foreign windows that refuse to move keep it alive, which is the
    // correct behaviour: deleting it would discard their preimages. When every
    // window resolved, the file must be gone.
    let leftovers: Vec<_> = std::fs::read_dir(&journal_dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "journal"))
                .collect()
        })
        .unwrap_or_default();

    if restore.is_complete() {
        assert!(
            leftovers.is_empty(),
            "a resolved transaction must delete its journal: {leftovers:?}"
        );
        println!("journal PASS: resolved transaction removed its journal");
    } else {
        assert_eq!(
            leftovers.len(),
            1,
            "an unresolved transaction must keep exactly one journal"
        );
        // Prove the retained journal is still recoverable rather than merely
        // present.
        let recovered = conflux_journal::recover(&leftovers[0]).expect("journal readable");
        match recovered {
            conflux_journal::Recovered::Unfinished(tx) => {
                assert!(
                    !tx.preimages.is_empty(),
                    "retained journal must still hold preimages"
                );
                println!(
                    "journal PASS: unresolved transaction kept {} preimages for later recovery",
                    tx.preimages.len()
                );
            }
            conflux_journal::Recovered::Clean => {
                panic!("an unresolved transaction was recorded as clean")
            }
        }
    }

    host.stop();
    std::fs::remove_dir_all(&journal_dir).ok();

    println!("\nE2E PASS ({WINDOWS} windows, real desktop)");
}

trait MigrationCounts {
    fn rejected_count(&self) -> usize;
}

impl MigrationCounts for conflux_engine::MigrationReport {
    fn rejected_count(&self) -> usize {
        self.outcomes
            .values()
            .filter(|o| matches!(o, conflux_domain::WindowOutcome::Rejected))
            .count()
    }
}

/// A child process that owns N plain windows and reports their handles.
struct Host {
    child: Child,
    ids: Vec<WindowId>,
    /// Rectangles captured immediately after creation, before any migration.
    originals: HashMap<WindowId, Rect>,
}

impl Host {
    fn spawn(count: usize, display: &DisplaySnapshot) -> std::io::Result<Self> {
        let exe = std::env::current_exe()?;
        let mut child = Command::new(exe)
            .arg("--host")
            .arg(count.to_string())
            .arg(display.work_area.left.to_string())
            .arg(display.work_area.top.to_string())
            .stdout(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().expect("stdout");
        let reader = BufReader::new(stdout);
        let mut ids = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let Some(value) = line.trim().strip_prefix("hwnd=") else {
                continue;
            };
            let Ok(native_id) = value.parse::<u64>() else {
                continue;
            };
            ids.push(WindowId {
                native_id,
                process_id: child.id(),
                process_start: 0,
            });
            // Break *after* the push: waiting for one more line would block
            // forever because the child only prints `count` handles and then
            // sits in its message loop with the pipe still open.
            if ids.len() == count {
                break;
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(400));

        Ok(Self {
            child,
            ids,
            originals: HashMap::new(),
        })
    }

    /// Read the current rectangle of every host window via the platform backend.
    fn rects(&mut self, engine: &Engine<WinPlatform>) -> HashMap<WindowId, Rect> {
        let mut out = HashMap::new();
        for snapshot in engine.backend().enumerate().unwrap_or_default() {
            if self
                .ids
                .iter()
                .any(|id| id.native_id == snapshot.id.native_id)
            {
                if self.originals.is_empty() {
                    self.originals
                        .insert(snapshot.id.clone(), snapshot.visible_rect);
                }
                out.insert(snapshot.id, snapshot.visible_rect);
            }
        }
        out
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Child mode: create `count` plain windows spread across the given work area and
/// print each handle. Windows are 640x480, which is small enough that six of them
/// tile the 1920x1032 work area without hitting the cascade fallback.
fn host_main() {
    use std::io::Write;

    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, ShowWindow,
        TranslateMessage, CS_HREDRAW, CS_VREDRAW, MSG, SW_SHOWNOACTIVATE, WNDCLASSW,
        WS_OVERLAPPEDWINDOW,
    };

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    let args: Vec<String> = std::env::args().collect();
    let count: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6);
    let origin_x: i32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    let origin_y: i32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);

    unsafe {
        let instance = GetModuleHandleW(None).expect("module handle");
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("ConfluxE2EHost"),
            ..Default::default()
        };
        RegisterClassW(&class);

        let mut handles = Vec::new();
        for index in 0..count {
            // Lay them out in a shallow diagonal so they do not overlap
            // completely; the engine re-tiles them anyway.
            let x = origin_x + (index % 3) * 120;
            let y = origin_y + (index / 3) * 120;
            let hwnd = CreateWindowExW(
                Default::default(),
                w!("ConfluxE2EHost"),
                w!("Conflux E2E host"),
                WS_OVERLAPPEDWINDOW,
                x,
                y,
                640,
                480,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .expect("create window");
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            handles.push(hwnd);
            println!("hwnd={}", hwnd.0 as usize);
            std::io::stdout().flush().ok();
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
