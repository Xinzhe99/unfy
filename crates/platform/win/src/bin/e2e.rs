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
use unfy_platform_win::{identify_window, WinPlatform};

/// Default number of host windows. Enough to force a multi-row grid.
const DEFAULT_WINDOWS: usize = 6;

fn main() {
    let identity_args: Vec<String> = std::env::args().collect();
    if identity_args.get(1).is_some_and(|arg| arg == "--identity") {
        let id = WindowId {
            native_id: identity_args[2].parse().expect("native id"),
            process_id: identity_args[3].parse().expect("process id"),
            process_start: identity_args[4].parse().expect("process creation time"),
            instance: identity_args[5].parse().expect("window instance"),
        };
        WinPlatform::new()
            .read_back(&id)
            .expect("persisted identity must be readable in a new process");
        return;
    }
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

    for id in &host.ids {
        let status = Command::new(std::env::current_exe().expect("executable"))
            .args([
                "--identity".to_string(),
                id.native_id.to_string(),
                id.process_id.to_string(),
                id.process_start.to_string(),
                id.instance.to_string(),
            ])
            .status()
            .expect("identity observer");
        assert!(
            status.success(),
            "cross-process window token must survive backend restart"
        );
        let mut stale = id.clone();
        stale.instance ^= 1;
        assert!(
            matches!(
                engine.backend().inner.read_back(&stale),
                Err(PlatformError::IdentityUncertain)
            ),
            "same HWND and process with wrong lifetime token must be rejected"
        );
    }
    let before = host.rects(&engine);
    let before_states: HashMap<_, _> = engine
        .backend()
        .enumerate()
        .expect("initial states")
        .into_iter()
        .map(|snapshot| (snapshot.id.clone(), snapshot))
        .collect();
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

    let foreground = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    let started = std::time::Instant::now();
    let report = engine
        .migrate(&target.key)
        .expect("migration must not fail outright");
    let migrate_ms = started.elapsed().as_millis();
    assert_eq!(
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() },
        foreground,
        "gather must not activate a host window"
    );
    println!(
        "\ntiming: enumerate {enumerate_ms} ms, preview {preview_ms} ms, migrate {migrate_ms} ms ({windows} host windows)"
    );
    // Every enumerated and written ID is the complete identity of a host window.
    println!(
        "\nmigration: applied={} rejected={} skipped={} (scoped host only)",
        report.applied.len(),
        report.rejected_count(),
        report.skipped.len()
    );
    let moved_ours = before
        .keys()
        .filter(|id| report.applied.iter().any(|a| a == *id))
        .count();
    // MoveInto allows overlap; every host window must be planned.
    assert!(
        report.unplaced.is_empty(),
        "default gather must not impose grid capacity"
    );
    let placed = windows - report.unplaced.len();
    assert_eq!(
        moved_ours, placed,
        "every placed host window must be applied ({placed} of {windows}); skipped={:?}; outcomes={:?}",
        report.skipped, report.outcomes
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
    for snapshot in engine.backend().enumerate().expect("migrated states") {
        assert_eq!(
            snapshot.mode, before_states[&snapshot.id].mode,
            "default gather must preserve window mode"
        );
    }

    // The default mode must not touch a window that is already usable on the
    // target. Retargeting onto the same display is the cheapest way to prove it:
    // every window is already there, so `unchanged` must account for all of them
    // and nothing may be written.
    let before_noop = host.rects(&engine);
    let writes_before_noop = engine.backend().writes.get();
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
    assert_eq!(
        engine.backend().writes.get(),
        writes_before_noop,
        "unchanged means zero native submissions, including restore"
    );
    println!("idempotence PASS: {windows} windows already on target were left alone");

    let restore_started = std::time::Instant::now();
    let restore = engine.restore().expect("restore must not fail outright");
    let restore_ms = restore_started.elapsed().as_millis();
    println!("restore wall time: {restore_ms} ms");
    assert!(
        restore.pending.iter().all(|id| before.contains_key(id)),
        "restore must never acquire responsibility for an unscoped window"
    );
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
    for snapshot in engine.backend().enumerate().expect("restored states") {
        let original = &before_states[&snapshot.id];
        assert_eq!(snapshot.mode, original.mode, "restore must restore mode");
        assert_eq!(
            snapshot.topmost, original.topmost,
            "restore must preserve topmost group"
        );
        if let (Some(actual), Some(expected)) =
            (&snapshot.native_placement, &original.native_placement)
        {
            assert!(
                engine
                    .backend()
                    .inner
                    .native_placement_matches(actual, expected),
                "restore must restore the hidden normal rectangle as well as visible geometry"
            );
        }
    }
    assert_eq!(
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() },
        foreground,
        "restore must not activate a host window"
    );

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

    if let Some(maximized) = before_states
        .values()
        .find(|snapshot| snapshot.mode == unfy_domain::WindowMode::Maximized)
    {
        let mut grid = Engine::new(
            Scoped::new(WinPlatform::new(), Scope::new(vec![maximized.id.clone()])),
            EngineConfig {
                journal_dir: journal_dir.join("maximized-grid"),
                layout: unfy_layout::LayoutConfig {
                    mode: unfy_layout::LayoutMode::Grid,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let tiled = grid.migrate(&target.key).expect("grid gather");
        assert_eq!(
            tiled.applied,
            vec![maximized.id.clone()],
            "maximized window must fit its grid cell"
        );
        assert_eq!(
            grid.backend().read_back(&maximized.id).unwrap().mode,
            unfy_domain::WindowMode::Normal
        );
        assert_eq!(
            unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() },
            foreground
        );
        assert!(grid.restore().expect("grid restore").is_complete());
        let restored = grid
            .backend()
            .enumerate()
            .unwrap()
            .pop()
            .expect("restored maximized host");
        assert_eq!(restored.mode, maximized.mode);
        assert_eq!(restored.visible_rect, maximized.visible_rect);
        assert!(grid.backend().inner.native_placement_matches(
            restored.native_placement.as_ref().unwrap(),
            maximized.native_placement.as_ref().unwrap()
        ));
        assert_eq!(
            unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() },
            foreground
        );
        println!("grid mode PASS: maximized window normalized and restored without activation");
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
    writes: std::cell::Cell<usize>,
}

impl Scoped {
    fn new(inner: WinPlatform, scope: Scope) -> Self {
        Self {
            inner,
            ids: scope.ids,
            writes: std::cell::Cell::new(0),
        }
    }

    fn in_scope(&self, id: &WindowId) -> bool {
        self.ids.contains(id)
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
        self.ids.iter().map(|id| self.inner.snapshot(id)).collect()
    }

    fn snapshot(&self, id: &WindowId) -> PlatformResult<WindowSnapshot> {
        if !self.in_scope(id) {
            return Err(PlatformError::IdentityUncertain);
        }
        self.inner.snapshot(id)
    }

    fn eligibility(&self, id: &WindowId) -> Option<unfy_domain::SkipReason> {
        if !self.in_scope(id) {
            return Some(unfy_domain::SkipReason::IdentityUncertain);
        }
        self.inner.eligibility(id)
    }

    fn restore_native_state(
        &self,
        id: &WindowId,
        placement: &[u8],
        topmost: bool,
    ) -> PlatformResult<()> {
        if !self.in_scope(id) {
            return Err(PlatformError::IdentityUncertain);
        }
        self.writes.set(self.writes.get() + 1);
        self.inner.restore_native_state(id, placement, topmost)
    }

    fn native_placement_matches(&self, actual: &[u8], expected: &[u8]) -> bool {
        self.inner.native_placement_matches(actual, expected)
    }

    fn transform_native_placement(
        &self,
        placement: &[u8],
        source_work: Rect,
        source_scale: unfy_domain::Scale,
        target: &DisplaySnapshot,
    ) -> PlatformResult<Vec<u8>> {
        self.inner
            .transform_native_placement(placement, source_work, source_scale, target)
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
        self.writes.set(self.writes.get() + 1);
        assert!(
            self.inner.capabilities(id).can_resize || !plan.resize,
            "fixed-size windows must receive a move-only plan"
        );
        self.inner.apply(id, plan)
    }

    fn apply_async(&self, id: &WindowId, plan: &WindowPlan) -> PlatformResult<()> {
        if !self.in_scope(id) {
            return Err(PlatformError::WindowGone);
        }
        self.writes.set(self.writes.get() + 1);
        assert!(
            self.inner.capabilities(id).can_resize || !plan.resize,
            "fixed-size windows must receive a move-only plan"
        );
        self.inner.apply_async(id, plan)
    }

    fn read_back(&self, id: &WindowId) -> PlatformResult<ObservedWindow> {
        if !self.in_scope(id) {
            return Err(PlatformError::WindowGone);
        }
        self.inner.read_back(id)
    }
}

/// A child process that owns N plain windows and reports their handles.
struct Host {
    child: Child,
    ids: Vec<WindowId>,
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
            ids.push(
                identify_window(native_id, child.id()).unwrap_or_else(|error| {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("host identity must be provable: {error}");
                }),
            );
            // Break *after* the push: waiting for one more line would block
            // forever because the child only prints `count` handles and then
            // sits in its message loop with the pipe still open.
            if ids.len() == count {
                break;
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(400));

        Ok(Self { child, ids })
    }

    /// Read the current rectangle of every host window via the platform backend.
    fn rects(&mut self, engine: &Engine<Scoped>) -> HashMap<WindowId, Rect> {
        let mut out = HashMap::new();
        for snapshot in engine.backend().enumerate().expect("scoped enumeration") {
            assert!(
                self.ids.contains(&snapshot.id),
                "foreign identity entered scope"
            );
            out.insert(snapshot.id, snapshot.visible_rect);
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
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
        GetMessageW, GetPropW, RegisterClassW, ShowWindow, TranslateMessage, CS_HREDRAW,
        CS_VREDRAW, MSG, SW_SHOWNA, SW_SHOWNOACTIVATE, WNDCLASSW, WS_MAXIMIZE, WS_MAXIMIZEBOX,
        WS_OVERLAPPEDWINDOW, WS_THICKFRAME,
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
            let fixed = index == 1;
            let style = if fixed {
                WS_OVERLAPPEDWINDOW & !WS_THICKFRAME & !WS_MAXIMIZEBOX
            } else if index == 2 {
                WS_OVERLAPPEDWINDOW | WS_MAXIMIZE
            } else {
                WS_OVERLAPPEDWINDOW
            };
            let (width, height) = if fixed { (180, 120) } else { (640, 480) };
            let create = || {
                CreateWindowExW(
                    Default::default(),
                    w!("UnfyE2EHost"),
                    w!("Unfy E2E host"),
                    style,
                    x,
                    y,
                    width,
                    height,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )
                .expect("create window")
            };
            let mut hwnd = create();
            if index == 0 {
                let old = identify_window(hwnd.0 as usize as u64, std::process::id())
                    .expect("assign first lifetime");
                assert_ne!(old.instance, 0);
                DestroyWindow(hwnd).expect("destroy lifetime probe");
                hwnd = create();
                assert!(
                    GetPropW(hwnd, w!("Unfy.WindowInstance.v1")).is_invalid(),
                    "a newly created window must not inherit a destroyed window's property"
                );
                let new = identify_window(hwnd.0 as usize as u64, std::process::id())
                    .expect("assign second lifetime");
                assert_ne!(
                    old.instance, new.instance,
                    "each window lifetime gets its own token"
                );
                assert!(
                    WinPlatform::new().read_back(&old).is_err(),
                    "destroyed identity must not resolve"
                );
                println!("identity lifetime PASS: destroyed property not inherited");
            }
            let foreground = GetForegroundWindow();
            let _ = ShowWindow(
                hwnd,
                if index == 2 {
                    SW_SHOWNA
                } else {
                    SW_SHOWNOACTIVATE
                },
            );
            assert_eq!(
                GetForegroundWindow(),
                foreground,
                "host setup must not steal focus"
            );
            if index == 0 {
                // Keep a hidden owned helper alive even on systems without an IME.
                // It must not exclude its otherwise movable owner from the gather.
                let _hidden_owned = CreateWindowExW(
                    Default::default(),
                    w!("UnfyE2EHost"),
                    w!("Unfy hidden helper"),
                    WS_OVERLAPPEDWINDOW,
                    x,
                    y,
                    80,
                    80,
                    Some(hwnd),
                    None,
                    Some(instance.into()),
                    None,
                )
                .expect("hidden owned helper");
            }
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
