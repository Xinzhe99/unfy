//! Accessibility window access. All retained AX references live and die on one
//! worker thread; callers exchange only Rust values. Each IPC call has a timeout.

use std::ffi::c_void;
use std::sync::{mpsc, OnceLock};

use core_foundation::array::CFArray;
use core_foundation::base::{CFGetTypeID, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use unfy_domain::{DisplayKey, IdentityQuality, Rect, Size, WindowId, WindowMode, WindowSnapshot};
use unfy_platform::{
    Capabilities, ObservedWindow, PlatformError, Result, WindowBackend, WindowPlan,
};

const AX_SUCCESS: i32 = 0;
const AX_ERROR_ATTRIBUTE_UNSUPPORTED: i32 = -25205;
const AX_ERROR_NO_VALUE: i32 = -25212;
const AX_ERROR_API_DISABLED: i32 = -25211;
const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;
const AX_VALUE_CG_POINT: u32 = 1;
const AX_VALUE_CG_SIZE: u32 = 2;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementGetTypeID() -> usize;
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
    fn AXValueGetTypeID() -> usize;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, out: *mut c_void) -> u8;
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
}

#[repr(C)]
#[derive(Default)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Default)]
struct CGSize {
    width: f64,
    height: f64,
}

pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

pub fn request_trust() -> bool {
    let options = CFDictionary::from_CFType_pairs(&[(
        CFString::from_static_string("AXTrustedCheckOptionPrompt"),
        CFBoolean::true_value(),
    )]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_CFTypeRef()) != 0 }
}

type Reply<T> = mpsc::Sender<Result<T>>;
enum Request {
    Enumerate(Reply<Vec<WindowSnapshot>>),
    Snapshot(WindowId, Reply<WindowSnapshot>),
    Capabilities(WindowId, Reply<Capabilities>),
    Apply(WindowId, WindowPlan, Reply<()>),
    ReadBack(WindowId, Reply<ObservedWindow>),
}

/// This handle is Send + Sync without transferring CF or Objective-C objects.
#[derive(Debug, Default)]
pub struct AccessibilityBackend {
    worker: OnceLock<Result<mpsc::Sender<Request>>>,
}

impl AccessibilityBackend {
    fn request<T>(&self, make: impl FnOnce(Reply<T>) -> Request) -> Result<T> {
        let worker = self
            .worker
            .get_or_init(|| {
                let (sender, receiver) = mpsc::channel();
                std::thread::Builder::new()
                    .name("unfy-accessibility".into())
                    .spawn(move || {
                        let mut state = AxState::default();
                        while let Ok(request) = receiver.recv() {
                            objc2::rc::autoreleasepool(|_| match request {
                                Request::Enumerate(reply) => {
                                    let _ = reply.send(state.enumerate());
                                }
                                Request::Snapshot(id, reply) => {
                                    let _ = reply.send(state.snapshot(&id));
                                }
                                Request::Capabilities(id, reply) => {
                                    let _ = reply.send(state.capabilities(&id));
                                }
                                Request::Apply(id, plan, reply) => {
                                    let _ = reply.send(state.apply(&id, &plan));
                                }
                                Request::ReadBack(id, reply) => {
                                    let _ = reply.send(state.read_back(&id));
                                }
                            });
                        }
                        // Dropping state here releases every retained reference on its owner thread.
                    })
                    .map_err(|error| PlatformError::Os(format!("start AX worker: {error}")))?;
                Ok(sender)
            })
            .as_ref()
            .map_err(Clone::clone)?;
        let (reply, result) = mpsc::channel();
        worker
            .send(make(reply))
            .map_err(|_| PlatformError::Os("AX worker stopped".into()))?;
        // Do not abandon a submitted write: its completion must remain attributable.
        result
            .recv()
            .map_err(|_| PlatformError::Os("AX worker stopped before reply".into()))?
    }
}

impl WindowBackend for AccessibilityBackend {
    fn enumerate(&self) -> Result<Vec<WindowSnapshot>> {
        self.request(Request::Enumerate)
    }
    fn snapshot(&self, id: &WindowId) -> Result<WindowSnapshot> {
        self.request(|reply| Request::Snapshot(id.clone(), reply))
    }
    fn capabilities(&self, id: &WindowId) -> Capabilities {
        self.request(|reply| Request::Capabilities(id.clone(), reply))
            .unwrap_or_default()
    }
    fn apply(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
        self.request(|reply| Request::Apply(id.clone(), *plan, reply))
    }
    fn read_back(&self, id: &WindowId) -> Result<ObservedWindow> {
        self.request(|reply| Request::ReadBack(id.clone(), reply))
    }
}

struct AxWindow {
    element: CFType,
    id: WindowId,
}

struct AxState {
    windows: Vec<AxWindow>,
    next_id: u64,
    instance: u64,
}

impl Default for AxState {
    fn default() -> Self {
        let uuid = core_foundation::uuid::CFUUID::new();
        let bytes =
            unsafe { core_foundation::uuid::CFUUIDGetUUIDBytes(uuid.as_concrete_TypeRef()) };
        let instance = u64::from_be_bytes([
            bytes.byte0,
            bytes.byte1,
            bytes.byte2,
            bytes.byte3,
            bytes.byte4,
            bytes.byte5,
            bytes.byte10,
            bytes.byte11,
        ]);
        Self {
            windows: Vec::new(),
            next_id: 0,
            instance,
        }
    }
}

impl AxState {
    fn enumerate(&mut self) -> Result<Vec<WindowSnapshot>> {
        if !is_trusted() {
            return Err(PlatformError::PermissionDenied);
        }
        if self.instance == 0 {
            return Err(PlatformError::IdentityUncertain);
        }
        let mut snapshots = Vec::new();
        let apps = objc2_app_kit::NSWorkspace::sharedWorkspace().runningApplications();
        for app in apps.iter() {
            let pid = app.processIdentifier();
            if pid <= 0 || pid as u32 == std::process::id() {
                continue;
            }
            // Missing launch time cannot establish continuity across PID reuse.
            let Some(launch) = app.launchDate() else {
                continue;
            };
            let started = launch.timeIntervalSince1970() * 10_000_000.0;
            if !started.is_finite() || started <= 0.0 {
                continue;
            }
            let process_start = started as u64;
            let application = owned(
                unsafe { AXUIElementCreateApplication(pid) },
                "create AX application",
            )?;
            let windows = match copy_attribute(&application, "AXWindows") {
                Ok(value) => value,
                Err(PlatformError::Unsupported) => continue,
                Err(error) => return Err(error),
            };
            let array = windows
                .downcast_into::<CFArray>()
                .ok_or_else(|| invalid_type("AXWindows"))?;
            let app_name = app
                .localizedName()
                .map(|name| name.to_string())
                .unwrap_or_default();
            for element in array.iter() {
                let raw = *element;
                if raw.is_null() || unsafe { CFGetTypeID(raw) != AXUIElementGetTypeID() } {
                    continue;
                }
                let element = unsafe { CFType::wrap_under_get_rule(raw) };
                // CFEqual compares AX object identity, unlike the address of a new wrapper.
                let id = if let Some(window) = self.windows.iter().find(|window| {
                    window.id.process_id == pid as u32
                        && window.id.process_start == process_start
                        && window.element == element
                }) {
                    window.id.clone()
                } else {
                    self.next_id = self
                        .next_id
                        .checked_add(1)
                        .ok_or_else(|| PlatformError::Os("AX logical ID exhausted".into()))?;
                    let id = WindowId {
                        native_id: self.next_id,
                        process_id: pid as u32,
                        process_start,
                        instance: self.instance,
                    };
                    self.windows.push(AxWindow {
                        element: element.clone(),
                        id: id.clone(),
                    });
                    id
                };
                match read_snapshot(&element, id, app_name.clone()) {
                    Ok(snapshot) => snapshots.push(snapshot),
                    Err(PlatformError::Unsupported) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        // Keep retained identities when an app is temporarily inaccessible or on another Space.
        Ok(snapshots)
    }

    fn window(&self, id: &WindowId) -> Result<&CFType> {
        if id.instance == 0 || id.instance != self.instance {
            return Err(PlatformError::IdentityUncertain);
        }
        if !is_trusted() {
            return Err(PlatformError::PermissionDenied);
        }
        let window = self
            .windows
            .iter()
            .find(|window| window.id == *id)
            .ok_or(PlatformError::IdentityUncertain)?;
        let app = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(
            id.process_id as i32,
        )
        .ok_or(PlatformError::IdentityUncertain)?;
        let launch = app.launchDate().ok_or(PlatformError::IdentityUncertain)?;
        if (launch.timeIntervalSince1970() * 10_000_000.0) as u64 != id.process_start {
            return Err(PlatformError::IdentityUncertain);
        }
        Ok(&window.element)
    }

    fn snapshot(&self, id: &WindowId) -> Result<WindowSnapshot> {
        let element = self.window(id)?;
        let app = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(
            id.process_id as i32,
        )
        .ok_or(PlatformError::IdentityUncertain)?;
        let app_name = app
            .localizedName()
            .map(|name| name.to_string())
            .unwrap_or_default();
        read_snapshot(element, id.clone(), app_name)
    }

    fn capabilities(&self, id: &WindowId) -> Result<Capabilities> {
        let element = self.window(id)?;
        Ok(Capabilities {
            can_move: is_settable(element, "AXPosition")?,
            can_resize: is_settable(element, "AXSize")?,
            can_read_state: read_observed(element).is_ok(),
            can_restore_native: false,
            min_size: optional_size(element, "AXMinSize")?,
        })
    }

    fn apply(&self, id: &WindowId, plan: &WindowPlan) -> Result<()> {
        let element = self.window(id)?;
        if copy_string(element, "AXSubrole")? != "AXStandardWindow" {
            return Err(PlatformError::Unsupported);
        }
        let observed = read_observed(element)?;
        if observed.mode != WindowMode::Normal || plan.mode != WindowMode::Normal {
            return Err(PlatformError::Unsupported);
        }
        if !is_settable(element, "AXPosition")? {
            return Err(PlatformError::Unsupported);
        }
        let resize = plan.resize;
        if resize && !is_settable(element, "AXSize")? {
            return Err(PlatformError::Unsupported);
        }
        if resize {
            set_size(
                element,
                plan.visible_rect.width() as f64,
                plan.visible_rect.height() as f64,
            )?;
        }
        set_position(
            element,
            plan.visible_rect.left as f64,
            plan.visible_rect.top as f64,
        )?;
        if resize {
            set_size(
                element,
                plan.visible_rect.width() as f64,
                plan.visible_rect.height() as f64,
            )?;
        }
        Ok(())
    }

    fn read_back(&self, id: &WindowId) -> Result<ObservedWindow> {
        read_observed(self.window(id)?)
    }
}

fn read_snapshot(element: &CFType, id: WindowId, app_name: String) -> Result<WindowSnapshot> {
    // A missing role is unknown, never evidence of a standard application window.
    if copy_string(element, "AXSubrole")? != "AXStandardWindow" {
        return Err(PlatformError::Unsupported);
    }
    let observed = read_observed(element)?;
    Ok(WindowSnapshot {
        id,
        app_name,
        display: DisplayKey {
            native_id: String::new(),
            quality: IdentityQuality::Weak,
        },
        visible_rect: observed.visible_rect,
        outer_rect: observed.visible_rect,
        mode: observed.mode,
        native_placement: None,
        topmost: false,
        min_size: optional_size(element, "AXMinSize")?,
        focused: optional_bool(element, "AXFocused")?
            .or(optional_bool(element, "AXMain")?)
            .unwrap_or(false),
    })
}

fn read_observed(element: &CFType) -> Result<ObservedWindow> {
    let position = copy_point(element, "AXPosition")?;
    let size = copy_size(element, "AXSize")?;
    let minimized = copy_bool(element, "AXMinimized")?;
    // Missing or unreadable state cannot establish that this window is safe to move.
    let fullscreen = copy_bool(element, "AXFullScreen")?;
    let edges = [
        position.x,
        position.y,
        position.x + size.width,
        position.y + size.height,
    ];
    if size.width <= 0.0
        || size.height <= 0.0
        || size.width > i32::MAX as f64
        || size.height > i32::MAX as f64
        || edges
            .iter()
            .any(|v| !v.is_finite() || *v < i32::MIN as f64 || *v > i32::MAX as f64)
        || edges[2].round() - edges[0].round() > i32::MAX as f64
        || edges[3].round() - edges[1].round() > i32::MAX as f64
    {
        return Err(PlatformError::Os("invalid AX window geometry".into()));
    }
    if edges[2].round() <= edges[0].round() || edges[3].round() <= edges[1].round() {
        return Err(PlatformError::Os("invalid AX window geometry".into()));
    }
    Ok(ObservedWindow {
        visible_rect: Rect::new(
            edges[0].round() as i32,
            edges[1].round() as i32,
            edges[2].round() as i32,
            edges[3].round() as i32,
        ),
        mode: if minimized {
            WindowMode::Minimized
        } else if fullscreen {
            WindowMode::Fullscreen
        } else {
            WindowMode::Normal
        },
    })
}

fn owned(raw: CFTypeRef, operation: &str) -> Result<CFType> {
    if raw.is_null() {
        return Err(PlatformError::Os(format!("{operation} returned null")));
    }
    Ok(unsafe { CFType::wrap_under_create_rule(raw) })
}

fn ax_result(code: i32, operation: &str) -> Result<()> {
    match code {
        AX_SUCCESS => Ok(()),
        AX_ERROR_API_DISABLED => Err(PlatformError::PermissionDenied),
        AX_ERROR_ATTRIBUTE_UNSUPPORTED | AX_ERROR_NO_VALUE => Err(PlatformError::Unsupported),
        // Invalid element and other AX errors do not establish that a journal window is gone.
        _ => Err(PlatformError::Os(format!("{operation}: AX error {code}"))),
    }
}

fn ipc_result(code: i32, operation: &str, elapsed: std::time::Duration) -> Result<()> {
    // CannotComplete also covers messaging failures before the timeout. Preserve
    // those as OS errors; only an exhausted deadline is classified as TimedOut.
    if code == AX_ERROR_CANNOT_COMPLETE && elapsed >= std::time::Duration::from_millis(400) {
        return Err(PlatformError::TimedOut);
    }
    ax_result(code, operation)
}

fn invalid_type(name: &str) -> PlatformError {
    PlatformError::Os(format!("invalid AX type for {name}"))
}

fn set_timeout(element: &CFType) -> Result<()> {
    ax_result(
        unsafe { AXUIElementSetMessagingTimeout(element.as_CFTypeRef(), 0.4) },
        "set AX timeout",
    )
}

fn copy_attribute(element: &CFType, name: &str) -> Result<CFType> {
    set_timeout(element)?;
    let attribute = CFString::new(name);
    let mut value = std::ptr::null();
    let started = std::time::Instant::now();
    let result = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_CFTypeRef(),
            attribute.as_concrete_TypeRef(),
            &mut value,
        )
    };
    // Own any returned reference even when AX reports an error.
    let value = if value.is_null() {
        None
    } else {
        Some(unsafe { CFType::wrap_under_create_rule(value) })
    };
    ipc_result(result, name, started.elapsed())?;
    value.ok_or_else(|| invalid_type(name))
}

fn copy_point(element: &CFType, name: &str) -> Result<CGPoint> {
    let value = copy_attribute(element, name)?;
    let mut point = CGPoint::default();
    if unsafe { CFGetTypeID(value.as_CFTypeRef()) != AXValueGetTypeID() }
        || unsafe {
            AXValueGetValue(
                value.as_CFTypeRef(),
                AX_VALUE_CG_POINT,
                &mut point as *mut _ as *mut c_void,
            )
        } == 0
    {
        return Err(invalid_type(name));
    }
    Ok(point)
}

fn copy_size(element: &CFType, name: &str) -> Result<CGSize> {
    let value = copy_attribute(element, name)?;
    let mut size = CGSize::default();
    if unsafe { CFGetTypeID(value.as_CFTypeRef()) != AXValueGetTypeID() }
        || unsafe {
            AXValueGetValue(
                value.as_CFTypeRef(),
                AX_VALUE_CG_SIZE,
                &mut size as *mut _ as *mut c_void,
            )
        } == 0
    {
        return Err(invalid_type(name));
    }
    Ok(size)
}

fn optional_size(element: &CFType, name: &str) -> Result<Option<Size>> {
    match copy_size(element, name) {
        Ok(size)
            if size.width.is_finite()
                && size.height.is_finite()
                && size.width > 0.0
                && size.height > 0.0
                && size.width <= i32::MAX as f64
                && size.height <= i32::MAX as f64 =>
        {
            Ok(Some(Size::new(
                size.width.ceil() as i32,
                size.height.ceil() as i32,
            )))
        }
        Ok(_) => Err(invalid_type(name)),
        Err(PlatformError::Unsupported) => Ok(None),
        Err(error) => Err(error),
    }
}

fn copy_bool(element: &CFType, name: &str) -> Result<bool> {
    let value = copy_attribute(element, name)?;
    bool_value(&value, name)
}

fn bool_value(value: &CFType, name: &str) -> Result<bool> {
    if let Some(boolean) = value.downcast::<CFBoolean>() {
        return Ok(boolean.into());
    }
    if let Some(number) = value.downcast::<CFNumber>() {
        return number
            .to_i64()
            .map(|value| value != 0)
            .ok_or_else(|| invalid_type(name));
    }
    Err(invalid_type(name))
}

fn optional_bool(element: &CFType, name: &str) -> Result<Option<bool>> {
    match copy_bool(element, name) {
        Ok(value) => Ok(Some(value)),
        Err(PlatformError::Unsupported) => Ok(None),
        Err(error) => Err(error),
    }
}

fn copy_string(element: &CFType, name: &str) -> Result<String> {
    copy_attribute(element, name)?
        .downcast_into::<CFString>()
        .map(|value| value.to_string())
        .ok_or_else(|| invalid_type(name))
}

fn is_settable(element: &CFType, name: &str) -> Result<bool> {
    set_timeout(element)?;
    let attribute = CFString::new(name);
    let mut settable = 0;
    let started = std::time::Instant::now();
    let result = unsafe {
        AXUIElementIsAttributeSettable(
            element.as_CFTypeRef(),
            attribute.as_concrete_TypeRef(),
            &mut settable,
        )
    };
    ipc_result(result, name, started.elapsed())?;
    Ok(settable != 0)
}

fn set_position(element: &CFType, x: f64, y: f64) -> Result<()> {
    let point = CGPoint { x, y };
    let value = owned(
        unsafe { AXValueCreate(AX_VALUE_CG_POINT, &point as *const _ as *const c_void) },
        "create AX point",
    )?;
    write_attribute(element, "AXPosition", &value)
}

fn set_size(element: &CFType, width: f64, height: f64) -> Result<()> {
    let size = CGSize { width, height };
    let value = owned(
        unsafe { AXValueCreate(AX_VALUE_CG_SIZE, &size as *const _ as *const c_void) },
        "create AX size",
    )?;
    write_attribute(element, "AXSize", &value)
}

fn write_attribute(element: &CFType, name: &str, value: &CFType) -> Result<()> {
    set_timeout(element)?;
    let attribute = CFString::new(name);
    let started = std::time::Instant::now();
    let result = unsafe {
        AXUIElementSetAttributeValue(
            element.as_CFTypeRef(),
            attribute.as_concrete_TypeRef(),
            value.as_CFTypeRef(),
        )
    };
    ipc_result(result, name, started.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previous_runtime_identity_remains_unresolved() {
        let state = AxState::default();
        let previous = WindowId {
            native_id: 1,
            process_id: 1,
            process_start: 1,
            instance: state.instance.wrapping_add(1),
        };
        assert!(matches!(
            state.window(&previous),
            Err(PlatformError::IdentityUncertain)
        ));
        let legacy = WindowId {
            instance: 0,
            ..previous
        };
        assert!(matches!(
            state.window(&legacy),
            Err(PlatformError::IdentityUncertain)
        ));
        assert!(matches!(
            ax_result(-25202, "read"),
            Err(PlatformError::Os(_))
        ));
        assert_eq!(
            ax_result(AX_ERROR_API_DISABLED, "read"),
            Err(PlatformError::PermissionDenied)
        );
        assert!(matches!(
            ax_result(AX_ERROR_CANNOT_COMPLETE, "read"),
            Err(PlatformError::Os(_))
        ));
        assert_eq!(
            ipc_result(
                AX_ERROR_CANNOT_COMPLETE,
                "read",
                std::time::Duration::from_millis(400)
            ),
            Err(PlatformError::TimedOut)
        );
    }

    #[test]
    fn boolean_attributes_reject_unrelated_cf_types() {
        assert_eq!(
            bool_value(&CFBoolean::true_value().as_CFType(), "state"),
            Ok(true)
        );
        assert_eq!(
            bool_value(&CFBoolean::false_value().as_CFType(), "state"),
            Ok(false)
        );
        assert_eq!(
            bool_value(&CFNumber::from(1_i32).as_CFType(), "state"),
            Ok(true)
        );
        assert!(matches!(
            bool_value(&CFString::new("false").as_CFType(), "state"),
            Err(PlatformError::Os(_))
        ));
    }
}
