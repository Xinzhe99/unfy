//! Manual smoke probe for the Windows backend.
//!
//! Two modes:
//!
//! * `probe` — prints the display topology and every candidate window, then
//!   exits. Read-only, safe to run any time.
//! * `probe --move` — spawns itself as `--host` (a child process that owns a
//!   plain Win32 window), moves that *other process's* window across displays,
//!   reads it back, and kills the child. This is the cross-process move that the
//!   whole product depends on, exercised without touching the user's windows.
//!
//! `probe --host` is the child side and is not meant to be run by hand.

use std::io::Write;

use conflux_domain::{rescale_between, WindowMode};
use conflux_platform::{DisplayProvider, WindowBackend, WindowPlan};
use conflux_platform_win::{WinDisplays, WinPlatform, WinWindows};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--host") {
        run_host();
        return;
    }

    if args.iter().any(|a| a == "--mins") {
        dump_min_sizes();
        return;
    }

    if args.iter().any(|a| a == "--dump") {
        dump_positions();
        return;
    }

    let should_move = args.iter().any(|a| a == "--move");

    let displays = WinDisplays::new();
    let topology = displays.topology().expect("topology");
    println!("displays: {}", topology.len());
    for d in &topology {
        println!(
            "  {:<14} primary={:<5} scale={:.2} bounds={}x{} work={}x{} id={}",
            d.label,
            d.primary,
            d.scale.x,
            d.bounds.width(),
            d.bounds.height(),
            d.work_area.width(),
            d.work_area.height(),
            &d.key.native_id[..d.key.native_id.len().min(48)],
        );
    }

    let windows = WinWindows::new();
    let raw = windows.enumerate_raw().expect("enumerate");
    let visible = raw.iter().filter(|w| w.style_flags.visible).count();
    let tool = raw.iter().filter(|w| w.style_flags.tool_window).count();
    let cloaked = raw.iter().filter(|w| w.style_flags.cloaked).count();
    let owned = raw.iter().filter(|w| w.owner.is_some()).count();
    let minimized = raw
        .iter()
        .filter(|w| w.snapshot.mode == WindowMode::Minimized)
        .count();
    let candidates: Vec<_> = raw
        .iter()
        .filter(|w| {
            w.style_flags.visible
                && !w.style_flags.tool_window
                && !w.style_flags.cloaked
                && w.snapshot.mode != WindowMode::Minimized
                && w.owner.is_none()
        })
        .collect();

    println!(
        "\nwindows: {} total | visible={visible} tool={tool} cloaked={cloaked} owned={owned} minimized={minimized}",
        raw.len()
    );
    println!("candidates after filters: {}", candidates.len());
    for w in candidates.iter().take(8) {
        let (l, t, r, b) = w.snapshot.border_inset();
        println!(
            "  hwnd={:#x} pid={} mode={:?} visible={}x{}+{}+{} inset=({l},{t},{r},{b})",
            w.snapshot.id.native_id,
            w.snapshot.id.process_id,
            w.snapshot.mode,
            w.snapshot.visible_rect.width(),
            w.snapshot.visible_rect.height(),
            w.snapshot.visible_rect.left,
            w.snapshot.visible_rect.top,
        );
    }

    if !should_move {
        println!("\n(pass --move to spawn a host window and exercise a cross-process move)");
        return;
    }

    cross_process_move(&displays, &windows);
}

/// Spawn `--host`, move its window across displays, read back, then kill it.
fn cross_process_move(displays: &WinDisplays, windows: &WinWindows) {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let topology = displays.topology().expect("topology");
    if topology.len() < 2 {
        println!("need two displays to exercise a move");
        return;
    }
    let (source, target) = (&topology[0], &topology[1]);

    let exe = std::env::current_exe().expect("current exe");
    let mut child = Command::new(exe)
        .arg("--host")
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn host");

    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read hwnd");
    let hwnd_value = line
        .trim()
        .strip_prefix("hwnd=")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| {
            println!("host did not report an hwnd: {line:?}");
            std::process::exit(1);
        });

    println!("\nhost window hwnd={hwnd_value:#x} pid={}", child.id());

    // Give the child a moment to finish showing the window.
    std::thread::sleep(std::time::Duration::from_millis(300));

    let snapshot = windows
        .enumerate_raw()
        .expect("enumerate")
        .into_iter()
        .find(|w| w.snapshot.id.native_id == hwnd_value)
        .map(|w| w.snapshot);

    let Some(snapshot) = snapshot else {
        println!("FAIL: host window not visible to EnumWindows");
        let _ = child.kill();
        let _ = child.wait();
        return;
    };

    println!(
        "  before: visible {}x{}+{}+{} outer {}x{}+{}+{} mode={:?}",
        snapshot.visible_rect.width(),
        snapshot.visible_rect.height(),
        snapshot.visible_rect.left,
        snapshot.visible_rect.top,
        snapshot.outer_rect.width(),
        snapshot.outer_rect.height(),
        snapshot.outer_rect.left,
        snapshot.outer_rect.top,
        snapshot.mode
    );

    let plan_rect = rescale_between(
        snapshot.visible_rect,
        source.work_area,
        source.scale,
        target.work_area,
        target.scale,
    );
    let plan = WindowPlan {
        visible_rect: plan_rect,
        mode: WindowMode::Normal,
    };
    println!("  requested: {plan_rect:?}");

    let outcome = windows.apply(&snapshot.id, &plan).and_then(|()| {
        std::thread::sleep(std::time::Duration::from_millis(200));
        windows.read_back(&snapshot.id)
    });

    match outcome {
        Ok(observed) => {
            let delta = [
                (observed.visible_rect.left - plan_rect.left).abs(),
                (observed.visible_rect.top - plan_rect.top).abs(),
                (observed.visible_rect.width() - plan_rect.width()).abs(),
                (observed.visible_rect.height() - plan_rect.height()).abs(),
            ];
            let worst = delta.iter().copied().max().unwrap_or(0);
            println!(
                "  observed: {}x{}+{}+{} delta={delta:?}",
                observed.visible_rect.width(),
                observed.visible_rect.height(),
                observed.visible_rect.left,
                observed.visible_rect.top
            );
            if worst <= 4 {
                println!("  PASS: cross-process move within 4 px");
            } else {
                println!("  FAIL: drifted {worst} px");
            }

            // Restore and verify the round trip.
            let restore = WindowPlan {
                visible_rect: snapshot.visible_rect,
                mode: snapshot.mode,
            };
            let restored = windows
                .apply(&snapshot.id, &restore)
                .and_then(|()| windows.read_back(&snapshot.id));
            match restored {
                Ok(o) => {
                    let back = [
                        (o.visible_rect.left - snapshot.visible_rect.left).abs(),
                        (o.visible_rect.top - snapshot.visible_rect.top).abs(),
                        (o.visible_rect.width() - snapshot.visible_rect.width()).abs(),
                        (o.visible_rect.height() - snapshot.visible_rect.height()).abs(),
                    ];
                    let worst = back.iter().copied().max().unwrap_or(0);
                    println!(
                        "  restore delta={back:?} -> {}",
                        if worst <= 4 { "PASS" } else { "FAIL" }
                    );
                }
                Err(e) => println!("  restore failed: {e}"),
            }
        }
        Err(e) => println!("  FAIL: {e}"),
    }

    let _ = child.kill();
    let _ = child.wait();
}

/// Child mode: create a plain window, print its handle, and pump messages.
fn run_host() {
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, ShowWindow,
        TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, MSG, SW_SHOW, WNDCLASSW,
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

    unsafe {
        let instance = GetModuleHandleW(None).expect("module handle");
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("ConfluxProbeHost"),
            ..Default::default()
        };
        RegisterClassW(&class);

        let hwnd = CreateWindowExW(
            Default::default(),
            w!("ConfluxProbeHost"),
            w!("Conflux probe host"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            640,
            480,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .expect("create window");

        let _ = ShowWindow(hwnd, SW_SHOW);
        println!("hwnd={}", hwnd.0 as usize);
        std::io::stdout().flush().ok();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Print every visible top-level window as one JSON object per line.
///
/// Print each candidate window's own minimum size.
///
/// The layout refuses to build a grid when the windows' minimums cannot tile the
/// work area, and falls back to a cascade instead. That decision is only as good as
/// the minimums, which come from `WM_GETMINMAXINFO` - an answer the application
/// controls, and one that can come back as the window's current size.
fn dump_min_sizes() {
    use conflux_platform::WindowBackend as _;

    let platform = WinPlatform::new();
    let raw = WinWindows::new().enumerate_raw().expect("enumerate");
    for w in raw {
        if !w.style_flags.visible || w.style_flags.tool_window || w.style_flags.cloaked {
            continue;
        }
        let id = &w.snapshot.id;
        let caps = platform.capabilities(id);
        let rect = w.snapshot.visible_rect;
        let min = caps
            .min_size
            .map(|m| format!("{}x{}", m.width, m.height))
            .unwrap_or_else(|| "unknown".to_string());
        println!(
            "{{\"app\":\"{}\",\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"min\":\"{}\"}}",
            w.snapshot.app_name,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            min
        );
    }
}

/// Used by the end-to-end verification to compare window positions before and
/// after a migration, so a restore can be checked for pixel fidelity rather than
/// "the app said it worked".
fn dump_positions() {
    let windows = WinWindows::new();
    let raw = windows.enumerate_raw().expect("enumerate");
    for w in raw {
        if !w.style_flags.visible || w.style_flags.tool_window || w.style_flags.cloaked {
            continue;
        }
        println!(
            "{{\"hwnd\":{},\"pid\":{},\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"mode\":\"{:?}\"}}",
            w.snapshot.id.native_id,
            w.snapshot.id.process_id,
            w.snapshot.visible_rect.left,
            w.snapshot.visible_rect.top,
            w.snapshot.visible_rect.width(),
            w.snapshot.visible_rect.height(),
            w.snapshot.mode,
        );
    }
}
