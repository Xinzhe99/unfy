//! Window enumeration, movement and read-back on Windows.
//!
//! Design notes that matter more than the code:
//!
//! * `enumerate` over-reports and the engine filters. This module only applies
//!   the hard exclusions that require Win32 knowledge (own process, invisible,
//!   tool windows, cloaked) and reports the rest.
//! * `apply` never claims success. It submits the request and lets the caller
//!   `read_back`.
//! * Maximized windows are moved by restoring, repositioning and re-maximizing,
//!   because `SetWindowPos` on a maximized Chromium window is overwritten by its
//!   `OnWindowPosChanging` handler.

use conflux_domain::{DisplayKey, IdentityQuality, Rect, WindowId, WindowMode, WindowSnapshot};
use conflux_platform::{
    Capabilities, ObservedWindow, PlatformError, Result, WindowBackend, WindowPlan,
};
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowPlacement,
    GetWindowRect, IsIconic, IsWindowVisible, IsZoomed, SetWindowPlacement, SetWindowPos,
    ShowWindow, GWL_EXSTYLE, GW_OWNER, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOSIZE,
    SWP_NOZORDER, SW_MAXIMIZE, SW_RESTORE, WINDOWPLACEMENT, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

/// Foreground window handle for the enumeration currently in progress.
///
/// `EnumWindows` takes a plain C callback with no user-data slot, so the value the
/// callback needs is passed through a process-global instead of a parameter. It is
/// written once before each enumeration and only read while that enumeration runs.
static FOREGROUND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

use crate::displays::{hwnd_from_id, process_identity};
use crate::dpi::PerMonitorV2;

/// Window backend for the current process on Windows.
#[derive(Debug)]
pub struct WinWindows {
    own_process: u32,
}

impl WinWindows {
    pub fn new() -> Self {
        Self {
            own_process: std::process::id(),
        }
    }
}

impl Default for WinWindows {
    fn default() -> Self {
        Self::new()
    }
}

/// A candidate window found by enumeration, before engine filtering.
#[derive(Debug, Clone)]
pub struct RawWindow {
    pub snapshot: WindowSnapshot,
    pub owner: Option<isize>,
    pub style_flags: WindowStyleFlags,
}

/// The style facts the engine needs but cannot see from a snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowStyleFlags {
    pub tool_window: bool,
    pub has_owner: bool,
    pub cloaked: bool,
    pub zoomed: bool,
    pub iconic: bool,
    pub visible: bool,
}

impl WinWindows {
    /// Enumerate every top-level window with its style flags.
    ///
    /// Exposed separately from the trait method so the UI can explain *why* a
    /// window was skipped instead of silently dropping it.
    pub fn enumerate_raw(&self) -> Result<Vec<RawWindow>> {
        let _guard = PerMonitorV2::enter();
        // Captured once per enumeration: `GetForegroundWindow` is a global query,
        // and calling it from inside the `EnumWindows` callback for every window
        // would both be wasteful and risk seeing a changed foreground mid-scan.
        let foreground = unsafe { GetForegroundWindow() };
        FOREGROUND.store(foreground.0 as usize, std::sync::atomic::Ordering::Relaxed);
        let mut out: Vec<RawWindow> = Vec::new();
        unsafe {
            EnumWindows(
                Some(enum_proc),
                LPARAM(&mut out as *mut Vec<RawWindow> as isize),
            )
        }
        .map_err(|e| PlatformError::Os(format!("EnumWindows failed: {e}")))?;
        Ok(out)
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `&mut Vec<RawWindow>` from `enumerate_raw`, valid
    // for the duration of the synchronous enumeration.
    let out = unsafe { &mut *(lparam.0 as *mut Vec<RawWindow>) };

    let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) }.ok();
    let cloaked = {
        let mut value = 0u32;
        let hr = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut value as *mut u32 as *mut _,
                core::mem::size_of::<u32>() as u32,
            )
        };
        hr.is_ok() && value != 0
    };

    let flags = WindowStyleFlags {
        tool_window: ex_style & WS_EX_TOOLWINDOW.0 != 0,
        has_owner: owner.is_some_and(|h| !h.is_invalid()),
        cloaked,
        zoomed: unsafe { IsZoomed(hwnd) }.as_bool(),
        iconic: unsafe { IsIconic(hwnd) }.as_bool(),
        visible: unsafe { IsWindowVisible(hwnd) }.as_bool(),
    };

    if let Some((visible_rect, outer_rect)) = window_rects(hwnd) {
        let (pid, start) = process_identity(hwnd);
        let mode = if flags.iconic {
            WindowMode::Minimized
        } else if flags.zoomed {
            WindowMode::Maximized
        } else if is_fullscreen_guess(&visible_rect, &outer_rect) {
            WindowMode::Fullscreen
        } else {
            WindowMode::Normal
        };

        out.push(RawWindow {
            snapshot: WindowSnapshot {
                id: WindowId {
                    native_id: hwnd.0 as usize as u64,
                    process_id: pid,
                    process_start: start,
                },
                app_name: executable_name(pid),
                display: DisplayKey {
                    native_id: String::new(),
                    quality: IdentityQuality::Weak,
                },
                visible_rect,
                outer_rect,
                mode,
                native_placement: None,
                topmost: ex_style & WS_EX_TOPMOST.0 != 0,
                // Deliberately left unknown here. Asking every window costs a
                // cross-process `SendMessageTimeout`, and enumeration runs on
                // every preview and every migration: measured at +119 ms for eight
                // windows. The engine asks only for the windows it is about to
                // place, through `capabilities`.
                min_size: None,
                focused: FOREGROUND.load(std::sync::atomic::Ordering::Relaxed) == hwnd.0 as usize,
            },
            owner: owner.map(|h| h.0 as isize),
            style_flags: flags,
        });
    }

    TRUE
}

/// Visible and outer rectangles in physical pixels.
fn window_rects(hwnd: HWND) -> Option<(Rect, Rect)> {
    let mut outer = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut outer) }.ok()?;

    let mut visible = RECT::default();
    let hr = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut visible as *mut RECT as *mut _,
            core::mem::size_of::<RECT>() as u32,
        )
    };
    // DWM is unavailable for some windows (e.g. during a session switch); fall
    // back to the outer rectangle rather than dropping the window.
    let visible = if hr.is_ok() { visible } else { outer };

    Some((
        Rect::new(visible.left, visible.top, visible.right, visible.bottom),
        Rect::new(outer.left, outer.top, outer.right, outer.bottom),
    ))
}

/// A cheap fullscreen heuristic: the visible rectangle equals the outer
/// rectangle and is at least 720p, which is what a borderless fullscreen window
/// looks like. The engine treats `Fullscreen` as "skip", so a false positive is
/// harmless.
fn is_fullscreen_guess(visible: &Rect, outer: &Rect) -> bool {
    visible == outer && visible.width() >= 1280 && visible.height() >= 720
}

impl WindowBackend for WinWindows {
    fn enumerate(&self) -> Result<Vec<WindowSnapshot>> {
        let mut out = Vec::new();
        for raw in self.enumerate_raw()? {
            if raw.snapshot.id.process_id == self.own_process {
                continue;
            }
            if !raw.style_flags.visible || raw.style_flags.tool_window || raw.style_flags.cloaked {
                continue;
            }
            out.push(raw.snapshot);
        }
        Ok(out)
    }

    fn capabilities(&self, id: &WindowId) -> Capabilities {
        // The minimum size comes from `WM_GETMINMAXINFO`, which the target window
        // answers in its own display's pixels. Without this guard the thread is
        // system-aware, Windows virtualises the reply, and the layout believes a
        // window can be squeezed smaller than it will actually go - the same class of
        // bug the other guards exist for. Untested here: both monitors on this machine
        // are at 100 %, where the two coordinate spaces coincide.
        let _guard = PerMonitorV2::enter();
        let Ok(hwnd) = hwnd_from_id(id) else {
            return Capabilities::default();
        };
        let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        Capabilities {
            can_move: true,
            can_resize: true,
            can_read_state: true,
            can_restore_native: ex_style & WS_EX_TOOLWINDOW.0 == 0,
            // Asked here, not during enumeration: the engine calls `capabilities`
            // once per window it is about to place, while enumeration runs on
            // every preview and would pay the round trip for every window on the
            // desktop.
            min_size: track_min_size(hwnd),
        }
    }

    fn apply(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
        self.apply_inner(id, plan, false)
    }

    fn apply_async(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
        self.apply_inner(id, plan, true)
    }

    fn read_back(&self, id: &WindowId) -> Result<ObservedWindow> {
        let _guard = PerMonitorV2::enter();
        let hwnd = hwnd_from_id(id)?;
        let (visible_rect, _) = window_rects(hwnd).ok_or(PlatformError::WindowGone)?;
        let mode = if unsafe { IsIconic(hwnd) }.as_bool() {
            WindowMode::Minimized
        } else if unsafe { IsZoomed(hwnd) }.as_bool() {
            WindowMode::Maximized
        } else {
            WindowMode::Normal
        };
        Ok(ObservedWindow { visible_rect, mode })
    }

    fn restore_native(&self, id: &WindowId, placement: &[u8]) -> Result<()> {
        if placement.len() != core::mem::size_of::<WINDOWPLACEMENT>() {
            return Err(PlatformError::Unsupported);
        }
        let hwnd = hwnd_from_id(id)?;
        let mut wp: WINDOWPLACEMENT = unsafe { core::mem::zeroed() };
        unsafe {
            core::ptr::copy_nonoverlapping(
                placement.as_ptr(),
                &mut wp as *mut WINDOWPLACEMENT as *mut u8,
                placement.len(),
            );
        }
        unsafe { SetWindowPlacement(hwnd, &wp) }.map_err(map_win_error)
    }
}

impl WinWindows {
    fn apply_inner(&self, id: &WindowId, plan: &WindowPlan, asynchronous: bool) -> Result<()> {
        let _guard = PerMonitorV2::enter();
        let hwnd = hwnd_from_id(id)?;

        // A maximized window ignores the size part of SetWindowPos, and Chromium
        // rewrites the whole rectangle. Restore first so the move applies.
        //
        // It is deliberately *not* re-maximized afterwards: a maximized window
        // covers the whole target display and would hide every other window we
        // just placed there. The plan decides the end state, and the plan uses
        // `Normal` for tiled slots. Restoring the original mode is the restore
        // path's job, using the preimage.
        let was_zoomed = unsafe { IsZoomed(hwnd) }.as_bool();
        if was_zoomed && plan.mode != WindowMode::Maximized {
            let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
        }

        // `SetWindowPos` takes the outer rectangle, so grow the requested visible
        // rect by the inset the window currently has. Using the *current* inset
        // (rather than a constant) keeps themed windows stable.
        let inset = current_inset(hwnd).unwrap_or((0, 0, 0, 0));
        let outer = Rect::new(
            plan.visible_rect.left - inset.0,
            plan.visible_rect.top - inset.1,
            plan.visible_rect.right + inset.2,
            plan.visible_rect.bottom + inset.3,
        );

        unsafe {
            SetWindowPos(
                hwnd,
                None,
                outer.left,
                outer.top,
                outer.width(),
                outer.height(),
                if asynchronous {
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS
                } else {
                    SWP_NOZORDER | SWP_NOACTIVATE
                },
            )
        }
        .map_err(map_win_error)?;

        if plan.mode == WindowMode::Maximized {
            let _ = unsafe { ShowWindow(hwnd, SW_MAXIMIZE) };
        }
        Ok(())
    }
}

/// How far the outer rectangle extends past the visible one, per side.
///
/// Matches `WindowSnapshot::border_inset`; `SetWindowPos` takes the outer
/// rectangle, so `apply` expands the requested visible rect by these amounts.
fn current_inset(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    let (visible, outer) = window_rects(hwnd)?;
    Some((
        visible.left - outer.left,
        visible.top - outer.top,
        outer.right - visible.right,
        outer.bottom - visible.bottom,
    ))
}

/// Capture the native placement blob so the engine can restore it later.
///
/// `WINDOWPLACEMENT.rcNormalPosition` is in **workspace** coordinates and must
/// only ever be handed back to `SetWindowPlacement`, never to `SetWindowPos`.
pub fn capture_placement(id: &WindowId) -> Result<Vec<u8>> {
    let _guard = PerMonitorV2::enter();
    let hwnd = hwnd_from_id(id)?;
    let mut wp = WINDOWPLACEMENT {
        length: core::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe { GetWindowPlacement(hwnd, &mut wp) }.map_err(map_win_error)?;
    let mut bytes = vec![0u8; core::mem::size_of::<WINDOWPLACEMENT>()];
    unsafe {
        core::ptr::copy_nonoverlapping(
            &wp as *const WINDOWPLACEMENT as *const u8,
            bytes.as_mut_ptr(),
            bytes.len(),
        );
    }
    Ok(bytes)
}

fn map_win_error(err: windows::core::Error) -> PlatformError {
    // 5 = ERROR_ACCESS_DENIED, raised by UIPI when the target runs at a higher
    // integrity level than this process.
    if err.code().0 as u32 & 0xFFFF == 5 {
        PlatformError::PermissionDenied
    } else {
        PlatformError::Os(format!("{err}"))
    }
}

/// Touch a window without moving it, so the shell re-evaluates its z-order.
pub fn nudge_window(id: &WindowId) -> Result<()> {
    let hwnd = hwnd_from_id(id)?;
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        )
    }
    .map_err(map_win_error)
}

/// The smallest size a window accepts, as it reports to the window manager.
///
/// Windows applications answer `WM_GETMINMAXINFO` with their own limits; asking
/// is the only way to learn them, since the value lives in application code and
/// not in any style bit.
///
/// Two details make this safe:
///
/// * `SendMessageTimeout` with a short timeout. A hung application must not block
///   enumeration - the enumeration runs on every migration and every preview.
/// * `SMTO_ABORTIFHUNG` so a window that is not pumping messages is skipped rather
///   than waited on.
///
/// A zero or absurd answer is reported as `None`: some applications answer with
/// zeros because they only fill the fields they care about, and treating 0x0 as a
/// real minimum would collapse every slot to nothing.
fn track_min_size(hwnd: HWND) -> Option<conflux_domain::Size> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, MINMAXINFO, SMTO_ABORTIFHUNG, WM_GETMINMAXINFO,
    };

    let mut info = MINMAXINFO::default();
    let mut result: usize = 0;
    let ok = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_GETMINMAXINFO,
            WPARAM(0),
            LPARAM(&mut info as *mut MINMAXINFO as isize),
            SMTO_ABORTIFHUNG,
            // Two frames at 60 Hz. Long enough for a healthy application to reply,
            // short enough that a busy one cannot stall a hundred-window gather.
            33,
            Some(&mut result),
        )
    };
    if ok.0 == 0 {
        return None;
    }

    let width = info.ptMinTrackSize.x;
    let height = info.ptMinTrackSize.y;
    // 1..=16384: anything outside that is a placeholder or a sentinel, not a size
    // a user could work with.
    if !(1..=16384).contains(&width) || !(1..=16384).contains(&height) {
        return None;
    }
    Some(conflux_domain::Size::new(width, height))
}

/// File name of the executable running in `pid`.
///
/// Only the base name is returned. The full path can contain a user name, and
/// this value ends up in exclusion rules and the UI, so anything above the file
/// name is deliberately dropped.
fn executable_name(pid: u32) -> String {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return String::new();
    }
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        // Elevated processes deny even limited queries; that window is skipped
        // anyway, so an empty name is the honest answer.
        return String::new();
    };

    let mut buffer = [0u16; 1024];
    let mut size = buffer.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
    if queried.is_err() || size == 0 {
        return String::new();
    }

    let full = String::from_utf16_lossy(&buffer[..size as usize]);
    full.rsplit(['\\', '/']).next().unwrap_or(&full).to_string()
}
