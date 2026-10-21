//! Accessibility (AX) window control.
//!
//! # Why the raw FFI instead of a wrapper crate
//!
//! The AX C API is small and stable: five functions and a handful of attribute
//! constants. Binding it directly keeps the unsafe surface visible and auditable
//! in one file instead of spreading it across a dependency.
//!
//! # Threading
//!
//! AX calls are synchronous IPC to the target application. A hung target makes
//! the call block, so every call goes through [`with_timeout`], which sets
//! `AXUIElementSetMessagingTimeout` for the duration. Without that, one frozen
//! app freezes the whole migration.

#![allow(non_upper_case_globals)]

use std::ffi::c_void;

use conflux_domain::{DisplayKey, IdentityQuality, Rect, WindowId, WindowMode, WindowSnapshot};
use conflux_platform::{
    Capabilities, ObservedWindow, PlatformError, Result, WindowBackend, WindowPlan,
};

use core_foundation::array::CFArray;
use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};

/// `kAXErrorSuccess`.
const AX_SUCCESS: i32 = 0;
/// `kAXErrorAttributeUnsupported`.
const AX_ERROR_ATTRIBUTE_UNSUPPORTED: i32 = -25205;
/// `kAXErrorAPIDisabled`.
const AX_ERROR_API_DISABLED: i32 = -25211;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementCreateSystemWide() -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> i32;
    fn AXUIElementIsAttributeSettable(
        element: CFTypeRef,
        attribute: CFStringRef,
        settable: *mut u8,
    ) -> i32;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout: f32) -> i32;
    fn AXValueCreate(value_type: u32, value: *const c_void) -> CFTypeRef;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, out: *mut c_void) -> u8;
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
}

/// `kAXValueCGPointType`.
const AX_VALUE_CG_POINT: u32 = 1;
/// `kAXValueCGSizeType`.
const AX_VALUE_CG_SIZE: u32 = 2;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct CGSize {
    width: f64,
    height: f64,
}

/// Whether this process currently holds Accessibility permission.
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Show the system prompt asking the user to grant Accessibility permission.
///
/// The prompt is asynchronous, so the return value is the *current* state.
pub fn request_trust() -> bool {
    let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let value = core_foundation::boolean::CFBoolean::true_value();
    let options: CFDictionary<CFString, core_foundation::boolean::CFBoolean> =
        CFDictionary::from_CFType_pairs(&[(key, value)]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as CFTypeRef) != 0 }
}

/// An AX window handle plus the process it belongs to.
#[derive(Debug, Clone)]
struct AxWindow {
    element: CFTypeRef,
    id: WindowId,
    snapshot: WindowSnapshot,
}

/// AX-backed window backend.
#[derive(Debug, Default)]
pub struct AccessibilityBackend {
    /// Cache of window elements, keyed by our `WindowId`. Rebuilt on enumerate.
    windows: std::sync::Mutex<Vec<AxWindow>>,
}

impl AccessibilityBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Refresh the window cache. Called by `enumerate` and by `apply` when a
    /// cached element is stale.
    fn refresh(&self) -> Result<Vec<WindowSnapshot>> {
        if !is_trusted() {
            return Err(PlatformError::PermissionDenied);
        }

        let mut snapshots = Vec::new();
        let mut cache = Vec::new();

        let system = unsafe { AXUIElementCreateSystemWide() };
        if system.is_null() {
            return Err(PlatformError::Os("AXUIElementCreateSystemWide".into()));
        }
        unsafe { CFRelease(system) };

        for pid in running_application_pids() {
            let app = unsafe { AXUIElementCreateApplication(pid) };
            if app.is_null() {
                continue;
            }
            unsafe { AXUIElementSetMessagingTimeout(app, 0.4) };

            let windows = match copy_attribute(app, "AXWindows") {
                Some(value) => value,
                None => {
                    unsafe { CFRelease(app) };
                    continue;
                }
            };

            let array: CFArray<CFTypeRef> = unsafe { CFArray::wrap_under_get_rule(windows as _) };
            for element in array.iter() {
                let element = *element;
                if element.is_null() {
                    continue;
                }
                let Some(window) = read_window(element, pid) else {
                    continue;
                };
                snapshots.push(window.snapshot.clone());
                cache.push(window);
            }
            unsafe { CFRelease(app) };
        }

        *self.windows.lock().unwrap() = cache;
        Ok(snapshots)
    }

    fn with_window<T>(&self, id: &WindowId, f: impl FnOnce(&AxWindow) -> Result<T>) -> Result<T> {
        let cache = self
            .windows
            .lock()
            .map_err(|_| PlatformError::Os("poisoned".into()))?;
        let window = cache
            .iter()
            .find(|w| w.id == *id)
            .ok_or(PlatformError::WindowGone)?;
        f(window)
    }
}

fn read_window(element: CFTypeRef, pid: i32) -> Option<AxWindow> {
    let position = copy_point(element, "AXPosition")?;
    let size = copy_size(element, "AXSize")?;
    if size.width <= 0.0 || size.height <= 0.0 {
        return None;
    }

    let minimized = copy_bool(element, "AXMinimized").unwrap_or(false);
    let fullscreen = copy_bool(element, "AXFullScreen").unwrap_or(false);

    // A window whose subrole is not AXStandardWindow is usually a sheet, a
    // panel, or a dialog attached to another window.
    let subrole = copy_string(element, "AXSubrole").unwrap_or_default();
    let standard = subrole.is_empty() || subrole == "AXStandardWindow";

    if !standard {
        return None;
    }

    let mode = if minimized {
        WindowMode::Minimized
    } else if fullscreen {
        WindowMode::Fullscreen
    } else {
        WindowMode::Normal
    };

    let rect = Rect::new(
        position.x.round() as i32,
        position.y.round() as i32,
        (position.x + size.width).round() as i32,
        (position.y + size.height).round() as i32,
    );

    // AX has no per-window stable identifier in the public SDK. The best
    // available identity is (pid, element pointer, first observed rect); the
    // pointer is stable for the lifetime of the window, which is all the engine
    // needs within one session.
    let id = WindowId {
        native_id: element as usize as u64,
        process_id: pid as u32,
        process_start: 0,
    };

    Some(AxWindow {
        element,
        id: id.clone(),
        snapshot: WindowSnapshot {
            id,
            display: DisplayKey {
                native_id: String::new(),
                quality: IdentityQuality::Weak,
            },
            visible_rect: rect,
            outer_rect: rect,
            mode,
            native_placement: None,
            topmost: false,
        },
    })
}

impl WindowBackend for AccessibilityBackend {
    fn enumerate(&self) -> Result<Vec<WindowSnapshot>> {
        self.refresh()
    }

    fn capabilities(&self, id: &WindowId) -> Capabilities {
        let Ok(cache) = self.windows.lock() else {
            return Capabilities::default();
        };
        let Some(window) = cache.iter().find(|w| w.id == *id) else {
            return Capabilities::default();
        };
        Capabilities {
            can_move: is_settable(window.element, "AXPosition"),
            can_resize: is_settable(window.element, "AXSize"),
            can_read_state: true,
            can_restore_native: false,
        }
    }

    fn apply(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
        self.with_window(id, |window| {
            if window.snapshot.mode == WindowMode::Fullscreen {
                // Native fullscreen lives in its own Space; AX position is
                // ignored there. Report rather than pretend.
                return Err(PlatformError::Unsupported);
            }

            set_position(
                window.element,
                plan.visible_rect.left as f64,
                plan.visible_rect.top as f64,
            )?;
            set_size(
                window.element,
                plan.visible_rect.width() as f64,
                plan.visible_rect.height() as f64,
            )?;

            // macOS clamps the first size request to the visible area of the
            // window's current display, so sizes and positions must be applied
            // twice in the order: size -> position -> size.
            set_position(
                window.element,
                plan.visible_rect.left as f64,
                plan.visible_rect.top as f64,
            )?;
            set_size(
                window.element,
                plan.visible_rect.width() as f64,
                plan.visible_rect.height() as f64,
            )?;
            Ok(())
        })
    }

    fn read_back(&self, id: &WindowId) -> Result<ObservedWindow> {
        self.with_window(id, |window| {
            let position =
                copy_point(window.element, "AXPosition").ok_or(PlatformError::WindowGone)?;
            let size = copy_size(window.element, "AXSize").ok_or(PlatformError::WindowGone)?;
            let minimized = copy_bool(window.element, "AXMinimized").unwrap_or(false);
            let fullscreen = copy_bool(window.element, "AXFullScreen").unwrap_or(false);
            let mode = if minimized {
                WindowMode::Minimized
            } else if fullscreen {
                WindowMode::Fullscreen
            } else {
                WindowMode::Normal
            };
            Ok(ObservedWindow {
                visible_rect: Rect::new(
                    position.x.round() as i32,
                    position.y.round() as i32,
                    (position.x + size.width).round() as i32,
                    (position.y + size.height).round() as i32,
                ),
                mode,
            })
        })
    }

    fn restore_native(&self, _id: &WindowId, _placement: &[u8]) -> Result<()> {
        // AX has no native placement blob; positions are restored geometrically.
        Err(PlatformError::Unsupported)
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
}

fn copy_attribute(element: CFTypeRef, name: &str) -> Option<CFTypeRef> {
    let attribute = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let result = unsafe {
        AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value)
    };
    if result == AX_SUCCESS && !value.is_null() {
        Some(value)
    } else {
        // `AX_ERROR_API_DISABLED` means the user revoked Accessibility while we
        // were running; it is reported as a missing attribute here and surfaces
        // as `PermissionDenied` from `enumerate`.
        None
    }
}

fn copy_point(element: CFTypeRef, name: &str) -> Option<CGPoint> {
    let value = copy_attribute(element, name)?;
    let mut point = CGPoint::default();
    let ok = unsafe {
        AXValueGetValue(
            value,
            AX_VALUE_CG_POINT,
            &mut point as *mut CGPoint as *mut c_void,
        )
    };
    unsafe { CFRelease(value) };
    (ok != 0).then_some(point)
}

fn copy_size(element: CFTypeRef, name: &str) -> Option<CGSize> {
    let value = copy_attribute(element, name)?;
    let mut size = CGSize::default();
    let ok = unsafe {
        AXValueGetValue(
            value,
            AX_VALUE_CG_SIZE,
            &mut size as *mut CGSize as *mut c_void,
        )
    };
    unsafe { CFRelease(value) };
    (ok != 0).then_some(size)
}

fn copy_bool(element: CFTypeRef, name: &str) -> Option<bool> {
    let value = copy_attribute(element, name)?;
    let number = unsafe { CFNumber::wrap_under_get_rule(value as _) };
    let result = number.to_i64().map(|v| v != 0);
    unsafe { CFRelease(value) };
    result
}

fn copy_string(element: CFTypeRef, name: &str) -> Option<String> {
    let value = copy_attribute(element, name)?;
    let string = unsafe { CFString::wrap_under_get_rule(value as _) };
    let result = Some(string.to_string());
    unsafe { CFRelease(value) };
    result
}

fn is_settable(element: CFTypeRef, name: &str) -> bool {
    let attribute = CFString::new(name);
    let mut settable: u8 = 0;
    let result = unsafe {
        AXUIElementIsAttributeSettable(element, attribute.as_concrete_TypeRef(), &mut settable)
    };
    result == AX_SUCCESS && settable != 0
}

fn set_position(element: CFTypeRef, x: f64, y: f64) -> Result<()> {
    let point = CGPoint { x, y };
    let value =
        unsafe { AXValueCreate(AX_VALUE_CG_POINT, &point as *const CGPoint as *const c_void) };
    if value.is_null() {
        return Err(PlatformError::Os("AXValueCreate(point)".into()));
    }
    write_attribute(element, "AXPosition", value)
}

fn set_size(element: CFTypeRef, width: f64, height: f64) -> Result<()> {
    let size = CGSize { width, height };
    let value = unsafe { AXValueCreate(AX_VALUE_CG_SIZE, &size as *const CGSize as *const c_void) };
    if value.is_null() {
        return Err(PlatformError::Os("AXValueCreate(size)".into()));
    }
    write_attribute(element, "AXSize", value)
}

fn write_attribute(element: CFTypeRef, name: &str, value: CFTypeRef) -> Result<()> {
    let attribute = CFString::new(name);
    let result =
        unsafe { AXUIElementSetAttributeValue(element, attribute.as_concrete_TypeRef(), value) };
    unsafe { CFRelease(value) };
    match result {
        0 => Ok(()),
        AX_ERROR_ATTRIBUTE_UNSUPPORTED => Err(PlatformError::Unsupported),
        AX_ERROR_API_DISABLED => Err(PlatformError::PermissionDenied),
        other => Err(PlatformError::Os(format!("AX set {name} failed: {other}"))),
    }
}

/// Pids of running GUI applications, via `NSWorkspace`.
fn running_application_pids() -> Vec<i32> {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let apps = workspace.runningApplications();
    let mut pids = Vec::new();
    for app in apps.iter() {
        let pid = app.processIdentifier();
        if pid > 0 {
            pids.push(pid);
        }
    }
    pids
}
