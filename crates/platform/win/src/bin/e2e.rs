//! Deterministic end-to-end verification against the real Windows desktop.
//!
//! The GUI-driven test is useful but noisy: the computer-use harness moves its
//! own window while clicking buttons, which shows up as a restore mismatch that
//! has nothing to do with Unfy. This binary removes that noise by owning the
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
//! only the windows it created: the engine runs behind [`Scoped`], a backend
//! wrapper that hides every other window on the desktop. Without that wrapper a
//! run on a real desktop moved the user's own applications - the harness claimed
//! "only the windows it created" while the engine enumerated everything.
//!
//! A scope is not a product feature. It exists so a destructive test can run on
//! the machine a person is using.
//!
//! Run: `cargo run -p unfy-platform-win --bin e2e`

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use unfy_domain::{DisplaySnapshot, Rect, WindowId, WindowSnapshot};
use unfy_engine::{Engine, EngineConfig};
use unfy_platform::{
    Capabilities, DisplayProvider, ObservedWindow, PlatformError, Result as PlatformResult,
    WindowBackend, WindowPlan,
};
use unfy_platform_win::WinPlatform;

/// Default number of host windows. Enough to force a multi-row grid.
const DEFAULT_WINDOWS: usize = 6;

fn main() {
    if std::env::args().any(|arg| arg == "--host") {
        host_main();
        return;
    }

    // `--stress N` raises the window count to exercise the capacity/cascade path
    // and to measure the timing the product spec budgets for.
    let args: Vec<String> = std::env::args().collect();
    let windows = args
        .iter()
        .position(|a| a == "--stress")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(DEFAULT_WINDOWS);
    let stress = windows > DEFAULT_WINDOWS;

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

    let mut host = Host::spawn(windows, source).expect("spawn host windows");

    let journal_dir = std::env::temp_dir().join(format!("unfy-e2e-{}", std::process::id()));
    let config = EngineConfig {
        journal_dir: journal_dir.clone(),
        ..Default::default()
    };

    let scope = Scope::new(host.ids.clone());
    let mut engine = Engine::new(Scoped::new(WinPlatform::new(), scope), config);

    let before = host.rects(&engine);
    assert_eq!(before.len(), windows, "host windows must be enumerable");
    println!("\ncaptured {} windows", before.len());
    for (id, rect) in &before {
        println!("  {:x} {:?}", id.native_id, rect);
    }

    let enumerate_started = std::time::Instant::now();
    let _ = engine.backend().enumerate().expect("enumerate");
    let enumerate_ms = enumerate_started.elapsed().as_millis();

    let preview_started = std::time::Instant::now();
    let _ = engine.preview(&target.key).expect("preview");
    let preview_ms = preview_started.elapsed().as_millis();

    let started = std::time::Instant::now();
    let report = engine
        .migrate(&target.key)
        .expect("migration must not fail outright");
    let migrate_ms = started.elapsed().as_millis();
    println!(
        "\ntiming: enumerate {enumerate_ms} ms, preview {preview_ms} ms, migrate {migrate_ms} ms ({windows} host windows)"
    );
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
    // Above the target display's capacity the planner deliberately leaves the
    // overflow in place rather than producing unusable slivers, so at 100
    // windows on 1920x1032 only the placed ones can be asserted. The invariant
    // that still holds for every window: it did not end up invisible.
    let placed = windows - report.unplaced.len();
    assert_eq!(
        moved_ours, placed,
        "every *placed* host window must be in the applied set ({} of {windows} placed)",
        placed
    );
    println!(
        "placed {placed} of {windows} host windows; {} left in place by capacity",
        report.unplaced.len()
    );

    // Only the host's own windows matter; other applications on the desktop are
    // not part of this assertion.
    let after_migrate = host.rects(&engine);
    let mut failures = Vec::new();

    // Windows the planner left in place (capacity overflow) were never claimed
    // to be reachable on the target; only the placed ones are asserted.
    let unplaced: std::collections::HashSet<u64> =
        report.unplaced.iter().map(|id| id.native_id).collect();

    for (id, original) in &before {
        let Some(now) = after_migrate.get(id) else {
            failures.push(format!("{:x}: disappeared during migration", id.native_id));
            continue;
        };
        if unplaced.contains(&id.native_id) {
            continue;
        }
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

    // The default mode must not touch a window that is already usable on the
    // target. Retargeting onto the same display is the cheapest way to prove it:
    // every window is already there, so `unchanged` must account for all of them
    // and nothing may be written.
    let before_noop = host.rects(&engine);
    let noop = engine
        .retarget(&target.key)
        .expect("a retarget onto the same display must not fail");
    assert_eq!(
        noop.migration.unchanged, windows,
        "windows already on the target must not be rewritten"
    );
    assert_eq!(
        noop.migration.applied.len(),
        0,
        "no window should have moved on an idempotent retarget"
    );
    let after_noop = host.rects(&engine);
    assert_eq!(
        before_noop, after_noop,
        "an idempotent retarget must not change any rectangle"
    );
    println!("idempotence PASS: {windows} windows already on target were left alone");

    let restore_started = std::time::Instant::now();
    let restore = engine.restore().expect("restore must not fail outright");
    let restore_ms = restore_started.elapsed().as_millis();
    println!("restore wall time: {restore_ms} ms");
    // Other applications on this desktop are also part of the migration, and
    // some of them refuse to be moved. That is reported honestly rather than
    // hidden, so the assertion must be scoped to the windows this harness owns;
    // a foreign window left in `pending` is not a Unfy failure.
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
    // state. An unresolved window keeps it alive, which is the correct behaviour:
    // deleting it would discard the preimage. When everything resolved, the file
    // must be gone.
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
        let recovered = unfy_journal::recover(&leftovers[0]).expect("journal readable");
        assert!(
            matches!(recovered, unfy_journal::Recovered::Unfinished(_)),
            "an unresolved transaction must not be recorded as clean"
        );
        println!("journal PASS: unresolved transaction kept for later recovery");
    }

    host.stop();
    std::fs::remove_dir_all(&journal_dir).ok();

    if stress {
        // The spec budgets p95 < 300 ms for the first submission pass and <= 1 s
        // for visible stability at 100 windows. This harness measures one run,
        // not a p95, so it reports rather than asserts; a gross regression still
        // shows up as an obviously wrong number.
        println!("\nstress: {windows} windows, migration {migrate_ms} ms, restore {restore_ms} ms");
        println!("note: single run, not a p95; use repeated runs for a distribution");
    }

    println!("\nE2E PASS ({windows} windows, real desktop)");
}

trait MigrationCounts {
    fn rejected_count(&self) -> usize;
}

impl MigrationCounts for unfy_engine::MigrationReport {
    fn rejected_count(&self) -> usize {
        self.outcomes
            .values()
            .filter(|o| matches!(o, unfy_domain::WindowOutcome::Rejected))
            .count()
    }
}

/// A backend that exposes only the windows this harness created.
///
/// Reads pass through for scoped windows; everything else is filtered out of
/// `enumerate`, and any write aimed at an unscoped id is refused before it reaches
/// the OS. The point is not to test the filter - the engine already filters - but
/// to make a destructive desktop test safe to run while a person is using the
/// machine.
struct Scoped {
    inner: WinPlatform,
    ids: Vec<WindowId>,
}

impl Scoped {
    fn new(inner: WinPlatform, scope: Scope) -> Self {
        Self {
            inner,
            ids: scope.ids,
        }
    }

    fn in_scope(&self, id: &WindowId) -> bool {
        self.ids
            .iter()
            .any(|candidate| candidate.native_id == id.native_id)
    }
}

struct Scope {
    ids: Vec<WindowId>,
}

impl Scope {
    fn new(ids: Vec<WindowId>) -> Self {
        Self { ids }
    }
}

impl DisplayProvider for Scoped {
    fn topology(&self) -> PlatformResult<Vec<DisplaySnapshot>> {
        self.inner.topology()
    }

    fn scale_of(&self, key: &unfy_domain::DisplayKey) -> PlatformResult<unfy_domain::Scale> {
        self.inner.scale_of(key)
    }

    fn display_for(&self, id: &WindowId) -> PlatformResult<unfy_domain::DisplayKey> {
        if !self.in_scope(id) {
            return Err(PlatformError::WindowGone);
        }
        self.inner.display_for(id)
    }
}

impl WindowBackend for Scoped {
    fn enumerate(&self) -> PlatformResult<Vec<WindowSnapshot>> {
        let mut windows = self.inner.enumerate()?;
        windows.retain(|window| self.in_scope(&window.id));
        Ok(windows)
    }

    fn capabilities(&self, id: &WindowId) -> Capabilities {
        if !self.in_scope(id) {
            return Capabilities::default();
        }
        self.inner.capabilities(id)
    }

    fn apply(&self, id: &WindowId, plan: &WindowPlan) -> PlatformResult<()> {
        if !self.in_scope(id) {
            return Err(PlatformError::WindowGone);
        }
        self.inner.apply(id, plan)
    }

    fn apply_async(&self, id: &WindowId, plan: &WindowPlan) -> PlatformResult<()> {
        if !self.in_scope(id) {
            return Err(PlatformError::WindowGone);
        }
        self.inner.apply_async(id, plan)
    }

    fn read_back(&self, id: &WindowId) -> PlatformResult<ObservedWindow> {
        if !self.in_scope(id) {
            return Err(PlatformError::WindowGone);
        }
        self.inner.read_back(id)
    }

    fn restore_native(&self, id: &WindowId, placement: &[u8]) -> PlatformResult<()> {
        if !self.in_scope(id) {
            return Err(PlatformError::WindowGone);
        }
        self.inner.restore_native(id, placement)
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
    fn rects(&mut self, engine: &Engine<Scoped>) -> HashMap<WindowId, Rect> {
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
            lpszClassName: w!("UnfyE2EHost"),
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
                w!("UnfyE2EHost"),
                w!("Unfy E2E host"),
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
