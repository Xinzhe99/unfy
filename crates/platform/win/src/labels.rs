//! Temporary numbered badges on every display.
//!
//! # Why this exists
//!
//! The product cannot know which physical screen a remote client is showing —
//! no platform exposes that. So the user has to tell us, and the only reliable
//! way is for them to look at the remote window and read a number off the
//! screen. This module draws that number.
//!
//! # Implementation constraints
//!
//! * Each badge is its own thread with its own message loop. A single thread
//!   cannot own windows on two monitors without pumping messages for both, and a
//!   blocked main thread must never delay or drop the badge.
//! * Windows are `WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOACTIVATE |
//!   WS_EX_TOOLWINDOW` and topmost, so they never intercept a click, never steal
//!   focus, and never appear in the user's alt-tab list.
//! * The badge removes itself after the timeout; the caller does not have to
//!   keep a handle. That matters because the caller is a command handler that
//!   may return long before the badge is dismissed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, DrawTextW, SetBkMode, SetTextColor,
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, HGDIOBJ, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
    MsgWaitForMultipleObjects, PeekMessageW, RegisterClassW, SetLayeredWindowAttributes,
    ShowWindow, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, LWA_ALPHA, MSG, PM_REMOVE, QS_ALLINPUT, SW_SHOWNOACTIVATE,
    WM_PAINT, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

/// Set once a flash is running so two rapid clicks do not stack badges.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Draw a numbered badge on every display for `seconds`.
///
/// Returns `false` when a flash is already running or the display list cannot be
/// read; the caller should treat that as "nothing shown" rather than an error.
pub fn flash(seconds: u64) -> bool {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        return false;
    }

    let displays = match conflux_platform::DisplayProvider::topology(&crate::WinDisplays::new()) {
        Ok(displays) => displays,
        Err(_) => {
            ACTIVE.store(false, Ordering::SeqCst);
            return false;
        }
    };
    if displays.is_empty() {
        ACTIVE.store(false, Ordering::SeqCst);
        return false;
    }

    let seconds = seconds.clamp(1, 30);
    let active = Arc::new(AtomicBool::new(true));

    for (index, display) in displays.iter().enumerate() {
        let bounds = display.bounds;
        let number = index + 1;
        let active = Arc::clone(&active);

        // One thread per display: each needs its own message pump.
        std::thread::spawn(move || {
            run_badge(bounds, number, seconds, active);
        });
    }

    // A supervisor clears the guard once every badge has had time to exit.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(seconds + 1));
        ACTIVE.store(false, Ordering::SeqCst);
    });

    true
}

fn run_badge(
    bounds: conflux_domain::Rect,
    number: usize,
    seconds: u64,
    _active: Arc<AtomicBool>,
) {
    unsafe {
        let instance = match GetModuleHandleW(None) {
            Ok(instance) => instance,
            Err(_) => return,
        };

        let class_name = w!("ConfluxDisplayLabel");
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        // Re-registering the same class on a second flash fails harmlessly; the
        // window can still be created.
        RegisterClassW(&class);

        let width = bounds.width().max(1);
        let height = bounds.height().max(1);

        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name,
            w!("Conflux"),
            WS_POPUP,
            bounds.left,
            bounds.top,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            Some(Box::into_raw(Box::new(Badge { number })) as *mut _),
        ) {
            Ok(hwnd) => hwnd,
            Err(_) => return,
        };

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // A `WS_EX_LAYERED` window with no `SetLayeredWindowAttributes` call is
        // fully transparent: it exists and receives paint messages but shows
        // nothing. Setting alpha *after* the first show avoids the window manager
        // discarding the attribute for a not-yet-visible window.
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 200, LWA_ALPHA);
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, true);
        let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);

        // A message-box timeout would block inside GetMessage; a timer does not,
        // so the deadline is checked around a short `MsgWaitForMultipleObjects`
        // instead of a plain blocking `GetMessage`.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
        let mut msg = MSG::default();
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let wait_ms = u32::try_from(remaining.as_millis().min(100)).unwrap_or(100);
            let result = MsgWaitForMultipleObjects(None, false, wait_ms, QS_ALLINPUT);
            // WAIT_OBJECT_0 means "a message is queued"; WAIT_TIMEOUT means the
            // deadline slice elapsed with nothing to do.
            if result.0 == 0 {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        let _ = DestroyWindow(hwnd);
    }
}

/// Per-window state passed through `CREATESTRUCT.lpCreateParams`.
struct Badge {
    number: usize,
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWLP_USERDATA, WM_NCCREATE,
    };

    match msg {
        // `lpParam` from `CreateWindowExW` arrives here; keep it on the window so
        // `WM_PAINT` can find the badge number without a global map.
        WM_NCCREATE => {
            let create = lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
            if !create.is_null() {
                let params = unsafe { (*create).lpCreateParams } as *mut Badge;
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, params as isize) };
            }
            LRESULT(1)
        }
        WM_PAINT => {
            let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Badge;
            if raw.is_null() {
                return LRESULT(0);
            }
            unsafe { paint(hwnd, &*raw) };
            LRESULT(0)
        }
        WM_TIMER => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe fn paint(hwnd: HWND, badge: &Badge) {
    unsafe {
        let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
        let hdc = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
        if hdc.is_invalid() {
            return;
        }
        // `BeginPaint` clips drawing to the update region. For a freshly shown
        // layered window that region can be empty, in which case the badge paints
        // nothing at all — the failure looked like "the overlay is invisible".
        let _ = windows::Win32::Graphics::Gdi::SelectClipRgn(hdc, None);

        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);

        // The layered-window alpha (200/255) already dims the desktop. Painting
        // another dark rectangle here would double-dim it into an opaque block,
        // so the background is left untouched.
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let cx = rect.left + width / 2;
        let cy = rect.top + height / 2;
        let radius = (width.min(height) / 7).clamp(80, 260);

        // A disc, drawn with both a selected brush and a matching pen so the
        // outline is not left at the default 1px black.
        let disc = CreateSolidBrush(COLORREF(0x0000C8FF));
        let pen = windows::Win32::Graphics::Gdi::CreatePen(
            windows::Win32::Graphics::Gdi::PS_SOLID,
            0,
            COLORREF(0x0000C8FF),
        );
        let old_brush = windows::Win32::Graphics::Gdi::SelectObject(hdc, disc.into());
        let old_pen = windows::Win32::Graphics::Gdi::SelectObject(hdc, pen.into());
        let _ = windows::Win32::Graphics::Gdi::Ellipse(
            hdc,
            cx - radius,
            cy - radius,
            cx + radius,
            cy + radius,
        );
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_pen);
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_brush);
        let _ = DeleteObject(HGDIOBJ(disc.0));
        let _ = DeleteObject(HGDIOBJ(pen.0));

        let _ = SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0x00101010));

        let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
        // Height must be negative for a character-height font; positive values
        // request cell height and render noticeably smaller.
        let font_height = -((radius as f32 * 1.05) as i32);
        let font = windows::Win32::Graphics::Gdi::CreateFontW(
            font_height,
            0,
            0,
            0,
            windows::Win32::Graphics::Gdi::FW_BOLD.0 as i32,
            0,
            0,
            0,
            windows::Win32::Graphics::Gdi::DEFAULT_CHARSET,
            windows::Win32::Graphics::Gdi::OUT_TT_PRECIS,
            windows::Win32::Graphics::Gdi::CLIP_DEFAULT_PRECIS,
            windows::Win32::Graphics::Gdi::CLEARTYPE_QUALITY,
            0,
            windows::core::PCWSTR(face.as_ptr()),
        );
        let previous = windows::Win32::Graphics::Gdi::SelectObject(hdc, font.into());

        let text = badge.number.to_string();
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut text_rect = RECT {
            left: cx - radius,
            top: cy - radius,
            right: cx + radius,
            bottom: cy + radius,
        };
        DrawTextW(
            hdc,
            &mut wide,
            &mut text_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, previous);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
    }
}
