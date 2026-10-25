//! Window enumeration, movement and read-back on Windows.
//!
//! Enumeration retains application facts; eligibility explains conservative skips.
//! Writes are asynchronous and never use activating ShowWindow transitions.

use unfy_domain::{
    DisplayKey, IdentityQuality, Rect, Scale, SkipReason, WindowId, WindowMode, WindowSnapshot,
};
use unfy_platform::{
    Capabilities, ObservedWindow, PlatformError, Result, WindowBackend, WindowPlan,
};
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowPlacement,
    GetWindowRect, IsIconic, IsWindowVisible, IsZoomed, SetWindowLongPtrW, SetWindowPlacement,
    SetWindowPos, GWL_EXSTYLE, GWL_STYLE, GW_OWNER, SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWMAXIMIZED, SW_SHOWMINNOACTIVE,
    SW_SHOWNA, SW_SHOWNOACTIVATE, WINDOWPLACEMENT, WPF_ASYNCWINDOWPLACEMENT, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_MAXIMIZE, WS_THICKFRAME,
};

struct Enumeration {
    foreground: HWND,
    windows: Vec<RawWindow>,
}

use crate::displays::{hwnd_from_id, process_identity, window_instance};
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
        let mut context = Enumeration {
            foreground,
            windows: Vec::new(),
        };
        unsafe {
            EnumWindows(
                Some(enum_proc),
                LPARAM(&mut context as *mut Enumeration as isize),
            )
        }
        .map_err(|e| PlatformError::Os(format!("EnumWindows failed: {e}")))?;
        Ok(context.windows)
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // The callback and its foreground context belong to this enumeration only.
    let context = unsafe { &mut *(lparam.0 as *mut Enumeration) };

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
        } else if is_fullscreen_guess(hwnd, &visible_rect, &outer_rect) {
            WindowMode::Fullscreen
        } else {
            WindowMode::Normal
        };

        let id = WindowId {
            native_id: hwnd.0 as usize as u64,
            process_id: pid,
            process_start: start,
            instance: window_instance(hwnd).unwrap_or(0),
        };
        let native_placement = capture_placement(&id).ok();
        context.windows.push(RawWindow {
            snapshot: WindowSnapshot {
                id,
                app_name: executable_name(pid),
                display: DisplayKey {
                    native_id: String::new(),
                    quality: IdentityQuality::Weak,
                },
                visible_rect,
                outer_rect,
                mode,
                native_placement,
                topmost: ex_style & WS_EX_TOPMOST.0 != 0,
                // Cross-process pointer messages cannot establish reliable limits.
                min_size: None,
                focused: context.foreground == hwnd,
            },
            owner: owner.map(|h| h.0 as isize),
            style_flags: flags,
        });
    }

    TRUE
}

/// Visible and outer rectangles in physical pixels.
pub(crate) fn window_rects(hwnd: HWND) -> Option<(Rect, Rect)> {
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

/// Whether a window looks like it is covering a whole display.
///
/// The previous versions used size alone, first "at least 1280x720" (so a
/// 1024x768 fullscreen window was treated as normal and moved) and then "no frame
/// and at least 640x480" (so every borderless 1600x900 utility window was skipped
/// and could never be gathered). Size is not the signal; covering a display is.
///
/// This asks the window manager which monitor the window is on and compares the
/// window's outer rectangle with that monitor's bounds. A borderless window that
/// does not fill a display is ordinary and must be moved.
///
/// The engine treats `Fullscreen` as "skip", so a false positive is still safe -
/// the window stays put and the result says why - but a false positive here is
/// also a window the product cannot help with.
fn is_fullscreen_guess(hwnd: HWND, visible: &Rect, outer: &Rect) -> bool {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    // Only a window with no frame can be fullscreen; a framed window that happens
    // to fill the screen is just maximized.
    if visible != outer || !has_dwm_geometry(hwnd) {
        return false;
    }

    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return false;
    }
    let mut info = MONITORINFO {
        cbSize: core::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return false;
    }
    let bounds = info.rcMonitor;
    let tolerance = 2;
    (outer.left - bounds.left).abs() <= tolerance
        && (outer.top - bounds.top).abs() <= tolerance
        && (outer.right - bounds.right).abs() <= tolerance
        && (outer.bottom - bounds.bottom).abs() <= tolerance
}

impl WindowBackend for WinWindows {
    fn enumerate(&self) -> Result<Vec<WindowSnapshot>> {
        let mut out = Vec::new();
        for raw in self.enumerate_raw()? {
            if raw.snapshot.id.process_id == self.own_process {
                continue;
            }
            if !raw.style_flags.visible
                || (raw.style_flags.tool_window && !raw.style_flags.has_owner)
            {
                continue;
            }
            out.push(raw.snapshot);
        }
        Ok(out)
    }

    fn snapshot(&self, id: &WindowId) -> Result<WindowSnapshot> {
        let _guard = PerMonitorV2::enter();
        let hwnd = hwnd_from_id(id)?;
        let mut context = Enumeration {
            foreground: unsafe { GetForegroundWindow() },
            windows: Vec::with_capacity(1),
        };
        unsafe {
            let _ = enum_proc(hwnd, LPARAM(&mut context as *mut Enumeration as isize));
        }
        hwnd_from_id(id)?;
        context
            .windows
            .pop()
            .map(|raw| raw.snapshot)
            .ok_or_else(|| PlatformError::Os("window geometry unavailable".into()))
    }

    fn eligibility(&self, id: &WindowId) -> Option<SkipReason> {
        let hwnd = match hwnd_from_id(id) {
            Ok(hwnd) => hwnd,
            Err(_) => return Some(SkipReason::IdentityUncertain),
        };
        if has_owner_group(hwnd) {
            return Some(SkipReason::OwnerGroupUnsupported);
        }
        match on_current_desktop(hwnd) {
            Some(true) => {}
            Some(false) => return Some(SkipReason::OtherDesktop),
            None => return Some(SkipReason::DesktopUnknown),
        }
        let mut cloaked = 0u32;
        if unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut _,
                core::mem::size_of::<u32>() as u32,
            )
        }
        .is_err()
        {
            return Some(SkipReason::GeometryUnknown);
        }
        if cloaked != 0 || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return Some(SkipReason::Invisible);
        }
        if !has_dwm_geometry(hwnd) {
            return Some(SkipReason::GeometryUnknown);
        }
        if let Some((visible, outer)) = window_rects(hwnd) {
            if !unsafe { IsZoomed(hwnd) }.as_bool() && is_fullscreen_guess(hwnd, &visible, &outer) {
                return Some(SkipReason::SuspectedFullscreen);
            }
        }
        None
    }

    fn restore_native_state(&self, id: &WindowId, placement: &[u8], topmost: bool) -> Result<()> {
        let hwnd = hwnd_from_id(id)?;
        if self.eligibility(id).is_some() {
            return Err(PlatformError::Unsupported);
        }
        // All writes preserve Z order. An externally changed topmost group must
        // be retained rather than reordered as an incidental restore side effect.
        let current = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0 != 0;
        if current != topmost {
            return Err(PlatformError::Unsupported);
        }
        let _guard = PerMonitorV2::enter();
        let wp = decode_placement(placement)?;
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        if style & WS_THICKFRAME.0 == 0 {
            if placement_mode(wp.showCmd) != 1
                || unsafe { IsZoomed(hwnd) }.as_bool()
                || unsafe { IsIconic(hwnd) }.as_bool()
            {
                return Err(PlatformError::Unsupported);
            }
            let (bounds, work, _) = decode_placement_geometry(placement)?;
            let normal = workspace_to_screen(wp.rcNormalPosition, bounds, work);
            hwnd_from_id(id)?;
            return unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    normal.left,
                    normal.top,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_ASYNCWINDOWPOS,
                )
            }
            .map_err(map_win_error);
        }
        if wp.showCmd == SW_SHOWMAXIMIZED.0 as u32 {
            let (_, work, scale) = decode_placement_geometry(placement)?;
            return submit_maximized(id, wp, work, scale);
        }
        submit_native_placement(id, wp)
    }

    fn native_placement_matches(&self, actual: &[u8], expected: &[u8]) -> bool {
        let (Ok(actual), Ok(expected)) = (decode_placement(actual), decode_placement(expected))
        else {
            return false;
        };
        let rect = |r: RECT| Rect::new(r.left, r.top, r.right, r.bottom);
        rect(actual.rcNormalPosition) == rect(expected.rcNormalPosition)
            && placement_mode(actual.showCmd) == placement_mode(expected.showCmd)
    }

    fn transform_native_placement(
        &self,
        placement: &[u8],
        source_work: Rect,
        source_scale: Scale,
        target: &unfy_domain::DisplaySnapshot,
    ) -> Result<Vec<u8>> {
        let mut wp = decode_placement(placement)?;
        let (bounds, work, scale) = decode_placement_geometry(placement)?;
        if work != source_work
            || scale != source_scale
            || !scale.is_valid()
            || !target.scale.is_valid()
            || work.is_empty()
            || target.work_area.is_empty()
        {
            return Err(PlatformError::Unsupported);
        }
        let original_normal = workspace_to_screen(wp.rcNormalPosition, bounds, work);
        let normal = unfy_domain::rescale_between(
            original_normal,
            work,
            scale,
            target.work_area,
            target.scale,
        );
        wp.rcNormalPosition = screen_to_workspace(normal, target.bounds, target.work_area);
        wp.ptMinPosition = windows::Win32::Foundation::POINT { x: -1, y: -1 };
        wp.ptMaxPosition = windows::Win32::Foundation::POINT { x: -1, y: -1 };
        wp.flags = WPF_ASYNCWINDOWPLACEMENT;
        Ok(encode_placement(
            &wp,
            (target.bounds, target.work_area, target.scale),
        ))
    }
    fn capabilities(&self, id: &WindowId) -> Capabilities {
        // Native style facts constrain writes; application minimums stay unknown
        // rather than sending a caller-owned pointer through a foreign process.
        let _guard = PerMonitorV2::enter();
        let Ok(hwnd) = hwnd_from_id(id) else {
            return Capabilities::default();
        };
        let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        // A window with no resize border cannot be resized; asking anyway produces
        // a request the application silently corrects, which the engine would then
        // report as a refusal. Reporting the capability honestly lets the planner
        // choose "move only" instead.
        let has_thick_frame = style & WS_THICKFRAME.0 != 0;
        let permitted = self.eligibility(id).is_none();
        Capabilities {
            can_move: permitted,
            can_resize: permitted && has_thick_frame,
            can_read_state: true,
            can_restore_native: ex_style & WS_EX_TOOLWINDOW.0 == 0,
            min_size: None,
        }
    }

    fn apply(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
        self.apply_inner(id, plan)
    }

    fn apply_async(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
        self.apply_inner(id, plan)
    }

    fn read_back(&self, id: &WindowId) -> Result<ObservedWindow> {
        let _guard = PerMonitorV2::enter();
        let hwnd = hwnd_from_id(id)?;
        if !has_dwm_geometry(hwnd) {
            return Err(PlatformError::Os("DWM geometry unavailable".into()));
        }
        let (visible_rect, _) = window_rects(hwnd)
            .ok_or_else(|| PlatformError::Os("window geometry unavailable".into()))?;
        let mode = if unsafe { IsIconic(hwnd) }.as_bool() {
            WindowMode::Minimized
        } else if unsafe { IsZoomed(hwnd) }.as_bool() {
            WindowMode::Maximized
        } else {
            WindowMode::Normal
        };
        hwnd_from_id(id)?;
        Ok(ObservedWindow { visible_rect, mode })
    }
}

fn submit_native_placement(id: &WindowId, mut placement: WINDOWPLACEMENT) -> Result<()> {
    placement.flags |= WPF_ASYNCWINDOWPLACEMENT;
    placement.showCmd = match placement_mode(placement.showCmd) {
        1 => SW_SHOWNOACTIVATE.0 as u32,
        2 => SW_SHOWMINNOACTIVE.0 as u32,
        _ => return Err(PlatformError::Unsupported),
    };
    let hwnd = hwnd_from_id(id)?;
    unsafe { SetWindowPlacement(hwnd, &placement) }.map_err(map_win_error)
}

fn submit_maximized(
    id: &WindowId,
    mut placement: WINDOWPLACEMENT,
    visible: Rect,
    scale: Scale,
) -> Result<()> {
    let hwnd = hwnd_from_id(id)?;
    placement.showCmd = SW_SHOWNA.0 as u32;
    if unsafe { IsZoomed(hwnd) }.as_bool() {
        let source = monitor_geometry_for_window(hwnd)?;
        let inset =
            current_inset(hwnd).ok_or_else(|| PlatformError::Os("DWM frame unavailable".into()))?;
        let outer = Rect::new(
            visible.left - (f64::from(inset.0) * scale.x / source.2.x).round() as i32,
            visible.top - (f64::from(inset.1) * scale.y / source.2.y).round() as i32,
            visible.right + (f64::from(inset.2) * scale.x / source.2.x).round() as i32,
            visible.bottom + (f64::from(inset.3) * scale.y / source.2.y).round() as i32,
        );
        placement.flags |= WPF_ASYNCWINDOWPLACEMENT;
        hwnd_from_id(id)?;
        unsafe { SetWindowPlacement(hwnd, &placement) }.map_err(map_win_error)?;
        let hwnd = hwnd_from_id(id)?;
        return unsafe {
            SetWindowPos(
                hwnd,
                None,
                outer.left,
                outer.top,
                outer.width(),
                outer.height(),
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_ASYNCWINDOWPOS,
            )
        }
        .map_err(map_win_error);
    }

    let id = id.clone();
    queue_native_change(move || {
        let _guard = PerMonitorV2::enter();
        let hwnd = hwnd_from_id(&id)?;
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        if style & WS_THICKFRAME.0 == 0 {
            return Err(PlatformError::Unsupported);
        }
        // ShowWindow(SW_MAXIMIZE) and SC_MAXIMIZE activate the window. Changing
        // its frame state preserves focus, but sends synchronous style messages.
        hwnd_from_id(&id)?;
        if unsafe { SetWindowLongPtrW(hwnd, GWL_STYLE, (style | WS_MAXIMIZE.0) as isize) } == 0 {
            return Err(map_win_error(windows::core::Error::from_thread()));
        }
        if !unsafe { IsZoomed(hwnd) }.as_bool() {
            return Err(PlatformError::Unsupported);
        }
        placement.flags &= !WPF_ASYNCWINDOWPLACEMENT;
        hwnd_from_id(&id)?;
        unsafe { SetWindowPlacement(hwnd, &placement) }.map_err(map_win_error)?;
        let hwnd = hwnd_from_id(&id)?;
        unsafe {
            SetWindowPos(
                hwnd,
                None,
                visible.left,
                visible.top,
                visible.width(),
                visible.height(),
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED,
            )
        }
        .map_err(map_win_error)?;
        adjust_visible_rect(&id, visible, true)
    })
}

fn queue_native_change(work: impl FnOnce() -> Result<()> + Send + 'static) -> Result<()> {
    // A foreign window may hang in a style/state message. Keep that wait off the
    // engine thread; its bounded read-back retains the journal if work is unresolved.
    std::thread::Builder::new()
        .spawn(move || {
            if let Err(error) = work() {
                tracing::debug!(%error, "native window state change did not complete");
            }
        })
        .map(|_| ())
        .map_err(|error| PlatformError::Os(error.to_string()))
}

// Only called by the native-change worker after the new frame state has settled.
fn adjust_visible_rect(id: &WindowId, visible: Rect, resize: bool) -> Result<()> {
    let hwnd = hwnd_from_id(id)?;
    let inset =
        current_inset(hwnd).ok_or_else(|| PlatformError::Os("DWM frame unavailable".into()))?;
    let outer = Rect::new(
        visible.left - inset.0,
        visible.top - inset.1,
        visible.right + inset.2,
        visible.bottom + inset.3,
    );
    let mut flags = SWP_NOACTIVATE | SWP_NOZORDER;
    if !resize {
        flags |= SWP_NOSIZE;
    }
    let hwnd = hwnd_from_id(id)?;
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            outer.left,
            outer.top,
            outer.width(),
            outer.height(),
            flags,
        )
    }
    .map_err(map_win_error)
}

impl WinWindows {
    fn apply_inner(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
        let _guard = PerMonitorV2::enter();
        let hwnd = hwnd_from_id(id)?;
        if self.eligibility(id).is_some() {
            return Err(PlatformError::Unsupported);
        }
        let was_zoomed = unsafe { IsZoomed(hwnd) }.as_bool();
        let source = monitor_geometry_for_window(hwnd)?;
        let target = monitor_geometry_for_rect(plan.visible_rect)?;
        let old_inset =
            current_inset(hwnd).ok_or_else(|| PlatformError::Os("DWM frame unavailable".into()))?;
        let sx = target.2.x / source.2.x;
        let sy = target.2.y / source.2.y;
        let inset = (
            (f64::from(old_inset.0) * sx).round() as i32,
            (f64::from(old_inset.1) * sy).round() as i32,
            (f64::from(old_inset.2) * sx).round() as i32,
            (f64::from(old_inset.3) * sy).round() as i32,
        );
        if was_zoomed || plan.mode != WindowMode::Normal {
            let mut wp = native_placement(hwnd)?;
            let outer = if plan.mode == WindowMode::Maximized {
                // WINDOWPLACEMENT normal coordinates are workspace-relative for
                // application windows. Transform the saved normal position, not
                // the maximized visible frame, before choosing the target monitor.
                let normal = workspace_to_screen(wp.rcNormalPosition, source.0, source.1);
                unfy_domain::rescale_between(
                    normal,
                    source.1,
                    if plan.resize {
                        source.2
                    } else {
                        Scale::IDENTITY
                    },
                    target.1,
                    if plan.resize {
                        target.2
                    } else {
                        Scale::IDENTITY
                    },
                )
            } else {
                let normal = workspace_to_screen(wp.rcNormalPosition, source.0, source.1);
                let left = plan.visible_rect.left - inset.0;
                let top = plan.visible_rect.top - inset.1;
                if plan.resize {
                    Rect::new(
                        left,
                        top,
                        plan.visible_rect.right + inset.2,
                        plan.visible_rect.bottom + inset.3,
                    )
                } else {
                    Rect::new(left, top, left + normal.width(), top + normal.height())
                }
            };
            wp.rcNormalPosition = screen_to_workspace(outer, target.0, target.1);
            wp.flags = WPF_ASYNCWINDOWPLACEMENT;
            wp.showCmd = match plan.mode {
                WindowMode::Maximized => SW_SHOWMAXIMIZED.0 as u32,
                WindowMode::Minimized => SW_SHOWMINNOACTIVE.0 as u32,
                WindowMode::Normal => SW_SHOWNOACTIVATE.0 as u32,
                WindowMode::Fullscreen => return Err(PlatformError::Unsupported),
            };
            if plan.mode == WindowMode::Maximized {
                return submit_maximized(id, wp, plan.visible_rect, target.2);
            }
            if was_zoomed && plan.mode == WindowMode::Normal {
                let id = id.clone();
                let visible = plan.visible_rect;
                let resize = plan.resize;
                return queue_native_change(move || {
                    let _guard = PerMonitorV2::enter();
                    let hwnd = hwnd_from_id(&id)?;
                    wp.flags &= !WPF_ASYNCWINDOWPLACEMENT;
                    unsafe { SetWindowPlacement(hwnd, &wp) }.map_err(map_win_error)?;
                    if unsafe { IsZoomed(hwnd) }.as_bool() {
                        return Err(PlatformError::Unsupported);
                    }
                    adjust_visible_rect(&id, visible, resize)
                });
            }
            return submit_native_placement(id, wp);
        }
        let outer = Rect::new(
            plan.visible_rect.left - inset.0,
            plan.visible_rect.top - inset.1,
            plan.visible_rect.right + inset.2,
            plan.visible_rect.bottom + inset.3,
        );
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        let mut flags = SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS;
        if !plan.resize || style & WS_THICKFRAME.0 == 0 {
            flags |= SWP_NOSIZE;
        }
        hwnd_from_id(id)?;
        unsafe {
            SetWindowPos(
                hwnd,
                None,
                outer.left,
                outer.top,
                outer.width(),
                outer.height(),
                flags,
            )
        }
        .map_err(map_win_error)
    }
}

fn native_placement(hwnd: HWND) -> Result<WINDOWPLACEMENT> {
    let mut wp = WINDOWPLACEMENT {
        length: core::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe { GetWindowPlacement(hwnd, &mut wp) }.map_err(map_win_error)?;
    Ok(wp)
}

fn has_dwm_geometry(hwnd: HWND) -> bool {
    let mut rect = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut _,
            core::mem::size_of::<RECT>() as u32,
        )
    }
    .is_ok()
}

fn has_owner_group(hwnd: HWND) -> bool {
    // A tool window is not part of a dialog group. Applications create them for
    // console hosts, IME plumbing and UI framework popups; they are owned windows
    // in the Win32 sense but moving the main window does not strand them, and
    // treating them as a group made Windows Terminal, cmd and DeskBox permanently
    // un-gatherable.
    if is_tool_window(hwnd) {
        return false;
    }
    if unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok_and(|owner| !owner.is_invalid()) {
        return true;
    }
    struct Query {
        owner: HWND,
        found: bool,
    }
    unsafe extern "system" fn find_owned(hwnd: HWND, data: LPARAM) -> BOOL {
        let query = unsafe { &mut *(data.0 as *mut Query) };
        // Hidden IME/helper windows and tool windows do not form a visible group.
        if unsafe { IsWindowVisible(hwnd) }.as_bool()
            && !is_tool_window(hwnd)
            && unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok_and(|owner| owner == query.owner)
        {
            query.found = true;
        }
        TRUE
    }
    let mut query = Query {
        owner: hwnd,
        found: false,
    };
    unsafe { EnumWindows(Some(find_owned), LPARAM(&mut query as *mut Query as isize)) }.is_err()
        || query.found
}

fn is_tool_window(hwnd: HWND) -> bool {
    let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    ex_style & WS_EX_TOOLWINDOW.0 != 0
}

fn on_current_desktop(hwnd: HWND) -> Option<bool> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let result = (|| {
        let manager: IVirtualDesktopManager =
            unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_INPROC_SERVER) }.ok()?;
        unsafe { manager.IsWindowOnCurrentVirtualDesktop(hwnd) }
            .ok()
            .map(|value| value.as_bool())
    })();
    if initialized {
        unsafe {
            CoUninitialize();
        }
    }
    result
}

fn monitor_geometry_for_window(hwnd: HWND) -> Result<(Rect, Rect, Scale)> {
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
    monitor_geometry(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) })
}

fn monitor_geometry_for_rect(rect: Rect) -> Result<(Rect, Rect, Scale)> {
    use windows::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONEAREST};
    let native = RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    let geometry = monitor_geometry(unsafe { MonitorFromRect(&native, MONITOR_DEFAULTTONEAREST) })?;
    if rect.intersection_area(&geometry.1) == 0 {
        return Err(PlatformError::DisplayGone);
    }
    Ok(geometry)
}

fn monitor_geometry(
    monitor: windows::Win32::Graphics::Gdi::HMONITOR,
) -> Result<(Rect, Rect, Scale)> {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    let mut info = MONITORINFO {
        cbSize: core::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return Err(PlatformError::DisplayGone);
    }
    let (mut x, mut y) = (0, 0);
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) }
        .map_err(map_win_error)?;
    let scale = Scale::from_dpi(x, y);
    if !scale.is_valid() {
        return Err(PlatformError::DisplayGone);
    }
    let convert = |r: RECT| Rect::new(r.left, r.top, r.right, r.bottom);
    Ok((convert(info.rcMonitor), convert(info.rcWork), scale))
}

fn workspace_to_screen(rect: RECT, bounds: Rect, work: Rect) -> Rect {
    let (dx, dy) = (work.left - bounds.left, work.top - bounds.top);
    Rect::new(
        rect.left + dx,
        rect.top + dy,
        rect.right + dx,
        rect.bottom + dy,
    )
}

fn screen_to_workspace(rect: Rect, bounds: Rect, work: Rect) -> RECT {
    let (dx, dy) = (work.left - bounds.left, work.top - bounds.top);
    RECT {
        left: rect.left - dx,
        top: rect.top - dy,
        right: rect.right - dx,
        bottom: rect.bottom - dy,
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
    let wp = native_placement(hwnd)?;
    let bytes = encode_placement(&wp, monitor_geometry_for_window(hwnd)?);
    hwnd_from_id(id)?;
    Ok(bytes)
}

const PLACEMENT_BYTES: usize = core::mem::size_of::<WINDOWPLACEMENT>();
const GEOMETRY_BYTES: usize = 52;

fn encode_placement(wp: &WINDOWPLACEMENT, geometry: (Rect, Rect, Scale)) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(PLACEMENT_BYTES + GEOMETRY_BYTES);
    let native =
        unsafe { core::slice::from_raw_parts(wp as *const _ as *const u8, PLACEMENT_BYTES) };
    bytes.extend_from_slice(native);
    bytes.extend_from_slice(b"UWP1");
    for rect in [geometry.0, geometry.1] {
        for value in [rect.left, rect.top, rect.right, rect.bottom] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&geometry.2.x.to_le_bytes());
    bytes.extend_from_slice(&geometry.2.y.to_le_bytes());
    bytes
}

fn decode_placement(bytes: &[u8]) -> Result<WINDOWPLACEMENT> {
    if bytes.len() != PLACEMENT_BYTES
        && (bytes.len() != PLACEMENT_BYTES + GEOMETRY_BYTES
            || &bytes[PLACEMENT_BYTES..PLACEMENT_BYTES + 4] != b"UWP1")
    {
        return Err(PlatformError::Unsupported);
    }
    let wp = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const WINDOWPLACEMENT) };
    if wp.length != PLACEMENT_BYTES as u32 {
        return Err(PlatformError::Unsupported);
    }
    Ok(wp)
}

fn decode_placement_geometry(bytes: &[u8]) -> Result<(Rect, Rect, Scale)> {
    decode_placement(bytes)?;
    if bytes.len() != PLACEMENT_BYTES + GEOMETRY_BYTES {
        return Err(PlatformError::Unsupported);
    }
    let data = &bytes[PLACEMENT_BYTES + 4..];
    let integer = |offset: usize| i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
    let rect = |offset| {
        Rect::new(
            integer(offset),
            integer(offset + 4),
            integer(offset + 8),
            integer(offset + 12),
        )
    };
    let scale = Scale {
        x: f64::from_le_bytes(data[32..40].try_into().unwrap()),
        y: f64::from_le_bytes(data[40..48].try_into().unwrap()),
    };
    Ok((rect(0), rect(16), scale))
}

fn placement_mode(show: u32) -> u32 {
    match show {
        1 | 4 | 5 | 8 | 9 => 1,
        2 | 6 | 7 | 11 => 2,
        other => other,
    }
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
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_ASYNCWINDOWPOS,
        )
    }
    .map_err(map_win_error)
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transformed_restore_uses_captured_normal_position_and_workspace_offset() {
        let bounds = Rect::new(0, 0, 1920, 1080);
        let work = Rect::new(40, 0, 1920, 1040);
        let scale = Scale { x: 1.5, y: 1.5 };
        let wp = WINDOWPLACEMENT {
            length: PLACEMENT_BYTES as u32,
            showCmd: SW_SHOWMAXIMIZED.0 as u32,
            rcNormalPosition: RECT {
                left: 100,
                top: 100,
                right: 1300,
                bottom: 1000,
            },
            ..Default::default()
        };
        let blob = encode_placement(&wp, (bounds, work, scale));
        let target = unfy_domain::DisplaySnapshot {
            key: DisplayKey {
                native_id: "target".into(),
                quality: IdentityQuality::Strong,
            },
            bounds,
            work_area: Rect::new(0, 0, 1920, 1040),
            scale: Scale::IDENTITY,
            physical_size_mm: None,
            primary: true,
            label: String::new(),
        };
        let backend = WinWindows::new();
        let transformed = backend
            .transform_native_placement(&blob, work, scale, &target)
            .unwrap();
        let restored = decode_placement(&transformed).unwrap();
        assert_eq!(
            workspace_to_screen(restored.rcNormalPosition, bounds, target.work_area),
            Rect::new(102, 100, 902, 700)
        );
        assert_eq!(restored.showCmd, SW_SHOWMAXIMIZED.0 as u32);
        assert!(matches!(
            backend.transform_native_placement(&blob[..PLACEMENT_BYTES], work, scale, &target),
            Err(PlatformError::Unsupported)
        ));
    }

    #[test]
    fn native_comparison_rejects_hidden_normal_position_drift() {
        let area = Rect::new(0, 0, 1920, 1080);
        let mut wp = WINDOWPLACEMENT {
            length: PLACEMENT_BYTES as u32,
            showCmd: 1,
            rcNormalPosition: RECT {
                left: 100,
                top: 100,
                right: 900,
                bottom: 700,
            },
            ..Default::default()
        };
        let original = encode_placement(&wp, (area, area, Scale::IDENTITY));
        wp.flags = WPF_ASYNCWINDOWPLACEMENT;
        wp.showCmd = SW_SHOWNOACTIVATE.0 as u32;
        let backend = WinWindows::new();
        assert!(backend.native_placement_matches(
            &encode_placement(&wp, (area, area, Scale::IDENTITY)),
            &original
        ));
        wp.rcNormalPosition.left += 10;
        assert!(!backend.native_placement_matches(
            &encode_placement(&wp, (area, area, Scale::IDENTITY)),
            &original
        ));
    }
    #[test]
    fn native_workspace_coordinates_round_trip_with_top_and_left_taskbars() {
        let bounds = Rect::new(-1920, -1080, 0, 0);
        let work = Rect::new(-1872, -1040, 0, 0);
        let screen = Rect::new(-1800, -1000, -1000, -400);
        let native = screen_to_workspace(screen, bounds, work);
        assert_eq!(native.left, -1848);
        assert_eq!(native.top, -1040);
        assert_eq!(workspace_to_screen(native, bounds, work), screen);
    }
}
