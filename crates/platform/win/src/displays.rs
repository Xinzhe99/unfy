//! Display enumeration and identity.
//!
//! Identity uses `DisplayConfigGetDeviceInfo(DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME)`
//! and its `monitorDevicePath`, because `HMONITOR` values are invalidated by
//! `WM_DISPLAYCHANGE` and cannot be persisted. `GetDpiForMonitor` with
//! `MDT_EFFECTIVE_DPI` supplies the user's scale factor; `GetDpiForWindow` cannot
//! be used for this because it reports 96 for DPI-unaware target processes.

use unfy_domain::{DisplayKey, DisplaySnapshot, IdentityQuality, Rect, Scale, WindowId};
use unfy_platform::{DisplayProvider, PlatformError, Result};
use windows::core::BOOL;
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{LPARAM, POINT, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, HDC, HMONITOR, MONITORINFOEXW,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};

use crate::dpi::PerMonitorV2;

/// How long a cached topology is trusted.
///
/// The list has to expire on its own, because nothing in this crate can be told that
/// it changed: there is no window here to receive `WM_DISPLAYCHANGE`, and the app
/// polls `topology()` instead. With a cache that never expired, that poll compared
/// the same value forever - the topology watcher never fired, the rescue for windows
/// stranded by an unplugged monitor never ran, and layouts were planned against a
/// work area that no longer existed.
///
/// Enumeration costs a few hundred microseconds, and the watcher runs every two
/// seconds, so a second of staleness is invisible while still being fresh enough to
/// react before a person notices.
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(1);

/// Cached display topology.
///
/// `HMONITOR` values stop being valid at the next topology change, so entries are
/// dropped wholesale rather than updated.
#[derive(Debug, Default)]
pub struct WinDisplays {
    cache: std::sync::Mutex<Option<Cached>>,
}

#[derive(Debug)]
struct Cached {
    at: std::time::Instant,
    displays: Vec<(isize, DisplaySnapshot)>,
}

impl WinDisplays {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the cached topology.
    ///
    /// For a caller that knows a change happened, to skip the remaining staleness.
    /// Correctness does not depend on anyone calling this.
    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
    }

    fn snapshot(&self) -> Result<Vec<(isize, DisplaySnapshot)>> {
        self.snapshot_at(std::time::Instant::now(), enumerate_monitors)
    }

    /// The cache rule, with the clock and the enumeration injected.
    ///
    /// Testable without displays and without waiting: the bug this replaced was a
    /// cache that never expired, and a test that needs a monitor to be unplugged
    /// would not have been written.
    fn snapshot_at<F>(
        &self,
        now: std::time::Instant,
        enumerate: F,
    ) -> Result<Vec<(isize, DisplaySnapshot)>>
    where
        F: FnOnce() -> Result<Vec<(isize, DisplaySnapshot)>>,
    {
        if let Ok(cache) = self.cache.lock() {
            if let Some(cached) = cache.as_ref() {
                if now.duration_since(cached.at) < CACHE_TTL {
                    return Ok(cached.displays.clone());
                }
            }
        }
        let fresh = enumerate()?;
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(Cached {
                at: now,
                displays: fresh.clone(),
            });
        }
        Ok(fresh)
    }
}

impl DisplayProvider for WinDisplays {
    fn topology(&self) -> Result<Vec<DisplaySnapshot>> {
        // A pre-submit topology check must not reuse an earlier physical layout.
        self.invalidate();
        Ok(self.snapshot()?.into_iter().map(|(_, d)| d).collect())
    }

    fn scale_of(&self, key: &DisplayKey) -> Result<Scale> {
        self.snapshot()?
            .into_iter()
            .find(|(_, d)| d.key == *key)
            .map(|(_, d)| d.scale)
            .ok_or(PlatformError::DisplayGone)
    }

    fn display_for(&self, id: &WindowId) -> Result<DisplayKey> {
        let _guard = PerMonitorV2::enter();
        let hwnd = hwnd_from_id(id)?;
        let (rect, _) = crate::windows_impl::window_rects(hwnd)
            .ok_or_else(|| PlatformError::Os("window geometry unavailable".into()))?;
        self.snapshot()?
            .into_iter()
            .map(|(_, display)| display)
            .min_by(|a, b| {
                let distance = |work: Rect| {
                    let dx = (i64::from(work.left) - i64::from(rect.right))
                        .max(i64::from(rect.left) - i64::from(work.right))
                        .max(0);
                    let dy = (i64::from(work.top) - i64::from(rect.bottom))
                        .max(i64::from(rect.top) - i64::from(work.bottom))
                        .max(0);
                    i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy)
                };
                rect.intersection_area(&b.work_area)
                    .cmp(&rect.intersection_area(&a.work_area))
                    .then_with(|| distance(a.work_area).cmp(&distance(b.work_area)))
                    .then_with(|| a.key.native_id.cmp(&b.key.native_id))
            })
            .map(|display| display.key)
            .ok_or(PlatformError::DisplayGone)
    }
}

/// A property belongs to the window object, not its reusable numeric handle.
/// The value is opaque data: no pointer is dereferenced across processes.
const INSTANCE_PROPERTY: windows::core::PCWSTR = windows::core::w!("Unfy.WindowInstance.v1");

pub(crate) fn window_instance(hwnd: windows::Win32::Foundation::HWND) -> Result<u64> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::UI::WindowsAndMessaging::{GetPropW, SetPropW};
    static ASSIGN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = ASSIGN
        .lock()
        .map_err(|_| PlatformError::IdentityUncertain)?;
    let existing = unsafe { GetPropW(hwnd, INSTANCE_PROPERTY) }.0 as usize as u64;
    if existing != 0 {
        return Ok(existing);
    }
    let guid = unsafe { windows::Win32::System::Com::CoCreateGuid() }
        .map_err(|_| PlatformError::IdentityUncertain)?;
    let token = (guid.to_u128() as usize).max(1);
    unsafe { SetPropW(hwnd, INSTANCE_PROPERTY, Some(HANDLE(token as *mut _))) }
        .map_err(|_| PlatformError::IdentityUncertain)?;
    if unsafe { GetPropW(hwnd, INSTANCE_PROPERTY) }.0 as usize != token {
        return Err(PlatformError::IdentityUncertain);
    }
    Ok(token as u64)
}

pub(crate) fn hwnd_from_id(id: &WindowId) -> Result<windows::Win32::Foundation::HWND> {
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::UI::WindowsAndMessaging::GetPropW;
    let hwnd = windows::Win32::Foundation::HWND(id.native_id as *mut core::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(PlatformError::WindowGone);
    }
    let (pid, start) = process_identity(hwnd);
    if pid != id.process_id || (start != 0 && id.process_start != 0 && start != id.process_start) {
        return Err(PlatformError::WindowGone);
    }
    if start == 0 || id.process_start == 0 || id.instance == 0 {
        return Err(PlatformError::IdentityUncertain);
    }
    let mut own_session = 0;
    let mut target_session = 0;
    unsafe {
        ProcessIdToSessionId(std::process::id(), &mut own_session)
            .and_then(|_| ProcessIdToSessionId(pid, &mut target_session))
    }
    .map_err(|_| PlatformError::IdentityUncertain)?;
    if own_session != target_session {
        return Err(PlatformError::IdentityUncertain);
    }
    let token = unsafe { GetPropW(hwnd, INSTANCE_PROPERTY) }.0 as usize as u64;
    if token != id.instance {
        return Err(PlatformError::IdentityUncertain);
    }
    Ok(hwnd)
}

/// Establish identity for an explicitly scoped HWND without enumerating or
/// attaching metadata to unrelated desktop windows.
pub fn identify_window(native_id: u64, expected_process: u32) -> Result<WindowId> {
    let hwnd = windows::Win32::Foundation::HWND(native_id as *mut core::ffi::c_void);
    let (process_id, process_start) = process_identity(hwnd);
    if process_id != expected_process || process_start == 0 {
        return Err(PlatformError::IdentityUncertain);
    }
    let id = WindowId {
        native_id,
        process_id,
        process_start,
        instance: window_instance(hwnd)?,
    };
    hwnd_from_id(&id)?;
    Ok(id)
}

struct EnumContext {
    out: Vec<(isize, DisplaySnapshot)>,
}

unsafe extern "system" fn enum_proc(
    hmon: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    // SAFETY: `lparam` carries the `&mut EnumContext` created in
    // `enumerate_monitors`, which outlives the synchronous enumeration.
    let ctx = unsafe { &mut *(lparam.0 as *mut EnumContext) };

    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = core::mem::size_of::<MONITORINFOEXW>() as u32;
    if !unsafe { GetMonitorInfoW(hmon, &mut info.monitorInfo) }.as_bool() {
        return TRUE;
    }

    let rc = info.monitorInfo.rcMonitor;
    let work = info.monitorInfo.rcWork;
    let primary = (info.monitorInfo.dwFlags & 1) != 0;

    let mut dpi_x = 0u32;
    let mut dpi_y = 0u32;
    // Zero remains unknown; never fabricate a confirmed 100% scale.
    let _ = unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };

    let device = utf16_field(&info.szDevice);
    let native_id =
        monitor_device_path(hmon).unwrap_or_else(|| format!("hmonitor:{}", hmon.0 as isize));
    let quality = if native_id.starts_with("hmonitor:") {
        IdentityQuality::Weak
    } else {
        IdentityQuality::Strong
    };

    ctx.out.push((
        hmon.0 as isize,
        DisplaySnapshot {
            key: DisplayKey { native_id, quality },
            bounds: Rect::new(rc.left, rc.top, rc.right, rc.bottom),
            work_area: Rect::new(work.left, work.top, work.right, work.bottom),
            scale: Scale::from_dpi(dpi_x, dpi_y),
            physical_size_mm: None,
            primary,
            label: device,
        },
    ));
    TRUE
}

fn utf16_field(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn enumerate_monitors() -> Result<Vec<(isize, DisplaySnapshot)>> {
    let _guard = PerMonitorV2::enter();
    let mut ctx = EnumContext { out: Vec::new() };
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM(&mut ctx as *mut EnumContext as isize),
        )
    };
    if !ok.as_bool() {
        return Err(PlatformError::Os("EnumDisplayMonitors failed".to_string()));
    }
    if ctx.out.is_empty() {
        return Err(PlatformError::Os("no displays reported".to_string()));
    }
    Ok(ctx.out)
}

/// Read the stable device path for the monitor at `hmon`.
///
/// Walks the CCD path list and matches the path whose source rectangle equals
/// the monitor's rectangle. The CCD API has no HMONITOR handle, so geometry is
/// the documented bridge between the two APIs.
fn monitor_device_path(hmon: HMONITOR) -> Option<String> {
    let target_rect = monitor_rect(hmon)?;

    let mut num_paths = 0u32;
    let mut num_modes = 0u32;
    if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut num_paths, &mut num_modes) }
        .is_err()
    {
        return None;
    }

    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); num_paths as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); num_modes as usize];
    if unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut num_paths,
            paths.as_mut_ptr(),
            &mut num_modes,
            modes.as_mut_ptr(),
            None,
        )
    }
    .is_err()
    {
        return None;
    }
    paths.truncate(num_paths as usize);
    modes.truncate(num_modes as usize);
    let mut matched = None;
    for path in &paths {
        let mut name = DISPLAYCONFIG_TARGET_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                size: core::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            },
            ..Default::default()
        };
        if unsafe { DisplayConfigGetDeviceInfo(&mut name.header) } != 0 {
            continue;
        }

        let source_rect = source_mode_rect(path, &modes);

        if source_rect == Some(target_rect) {
            let path_str = utf16_field(&name.monitorDevicePath);
            if !path_str.is_empty() {
                if matched.is_some() {
                    return None;
                }
                matched = Some(path_str);
            }
        }
    }
    matched
}

fn source_mode_rect(
    path: &DISPLAYCONFIG_PATH_INFO,
    modes: &[DISPLAYCONFIG_MODE_INFO],
) -> Option<Rect> {
    use windows::Win32::Devices::Display::DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE;
    let index = unsafe { path.sourceInfo.Anonymous.modeInfoIdx } as usize;
    let info = modes.get(index)?;
    if info.infoType != DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE
        || info.id != path.sourceInfo.id
        || info.adapterId.LowPart != path.sourceInfo.adapterId.LowPart
        || info.adapterId.HighPart != path.sourceInfo.adapterId.HighPart
    {
        return None;
    }
    let mode = unsafe { info.Anonymous.sourceMode };
    Some(Rect::new(
        mode.position.x,
        mode.position.y,
        mode.position
            .x
            .checked_add(i32::try_from(mode.width).ok()?)?,
        mode.position
            .y
            .checked_add(i32::try_from(mode.height).ok()?)?,
    ))
}

fn monitor_rect(hmon: HMONITOR) -> Option<Rect> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = core::mem::size_of::<MONITORINFOEXW>() as u32;
    if !unsafe { GetMonitorInfoW(hmon, &mut info.monitorInfo) }.as_bool() {
        return None;
    }
    let rc = info.monitorInfo.rcMonitor;
    Some(Rect::new(rc.left, rc.top, rc.right, rc.bottom))
}

/// Real process creation time supplements the separate HWND lifetime token.
/// It detects process reuse, but cannot detect HWND reuse within one process.
pub(crate) fn process_identity(hwnd: windows::Win32::Foundation::HWND) -> (u32, u64) {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return (0, 0);
    }

    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return (pid, 0);
    };

    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let ok = unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
    if ok.is_err() {
        return (pid, 0);
    }

    // 100 ns ticks since 1601-01-01. Kept as the raw value: it only has to be
    // comparable and stable for the life of the process, not human-readable.
    let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    (pid, ticks)
}

/// The display under a point (used by the tray menu).
pub fn display_at(displays: &WinDisplays, point: POINT) -> Result<DisplayKey> {
    let _guard = PerMonitorV2::enter();
    let hmon = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    displays
        .snapshot()?
        .into_iter()
        .find(|(raw, _)| *raw == hmon.0 as isize)
        .map(|(_, d)| d.key)
        .ok_or(PlatformError::DisplayGone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    #[test]
    fn ccd_source_mode_requires_matching_index_type_adapter_and_id() {
        use windows::Win32::Devices::Display::{
            DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE, DISPLAYCONFIG_MODE_INFO_TYPE_TARGET,
        };
        let mut path = DISPLAYCONFIG_PATH_INFO::default();
        path.sourceInfo.id = 7;
        path.sourceInfo.adapterId.LowPart = 11;
        path.sourceInfo.Anonymous.modeInfoIdx = 1;
        let mut modes = [DISPLAYCONFIG_MODE_INFO::default(); 2];
        modes[0].id = 7;
        modes[0].infoType = DISPLAYCONFIG_MODE_INFO_TYPE_TARGET;
        modes[1].id = 7;
        modes[1].adapterId.LowPart = 11;
        modes[1].infoType = DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE;
        modes[1].Anonymous.sourceMode =
            windows::Win32::Devices::Display::DISPLAYCONFIG_SOURCE_MODE {
                width: 1920,
                height: 1080,
                ..Default::default()
            };
        assert_eq!(
            source_mode_rect(&path, &modes),
            Some(Rect::new(0, 0, 1920, 1080))
        );
        path.sourceInfo.adapterId.LowPart = 12;
        assert_eq!(source_mode_rect(&path, &modes), None);
        path.sourceInfo.adapterId.LowPart = 11;
        path.sourceInfo.Anonymous.modeInfoIdx = 0;
        assert_eq!(source_mode_rect(&path, &modes), None);
        path.sourceInfo.Anonymous.modeInfoIdx = u32::MAX;
        assert_eq!(source_mode_rect(&path, &modes), None);
    }
    fn display(name: &str, width: i32) -> (isize, DisplaySnapshot) {
        (
            0,
            DisplaySnapshot {
                key: DisplayKey {
                    native_id: name.to_string(),
                    quality: IdentityQuality::Strong,
                },
                label: name.to_string(),
                bounds: Rect {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: 1080,
                },
                work_area: Rect {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: 1040,
                },
                scale: Scale { x: 1.0, y: 1.0 },
                physical_size_mm: None,
                primary: true,
            },
        )
    }

    #[test]
    fn a_fresh_entry_is_reused_without_enumerating_again() {
        let displays = WinDisplays::new();
        let calls = Cell::new(0);
        let now = Instant::now();

        let first = displays
            .snapshot_at(now, || {
                calls.set(calls.get() + 1);
                Ok(vec![display("A", 1920)])
            })
            .expect("first");
        let second = displays
            .snapshot_at(now + Duration::from_millis(500), || {
                calls.set(calls.get() + 1);
                Ok(vec![display("B", 1280)])
            })
            .expect("second");

        assert_eq!(calls.get(), 1, "inside the TTL the list is reused");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].1.key.native_id, first[0].1.key.native_id);
    }

    #[test]
    fn a_stale_entry_is_replaced() {
        // The bug this pins: the cache was only filled when empty, so a monitor
        // that changed resolution - or went away - was reported the same way for the
        // life of the process. The topology watcher polls this value, so it could
        // never see a change, and the rescue for stranded windows never ran.
        let displays = WinDisplays::new();
        let calls = Cell::new(0);
        let now = Instant::now();

        displays
            .snapshot_at(now, || {
                calls.set(calls.get() + 1);
                Ok(vec![display("A", 1920)])
            })
            .expect("first");
        let after_ttl = displays
            .snapshot_at(now + CACHE_TTL + Duration::from_millis(1), || {
                calls.set(calls.get() + 1);
                Ok(vec![display("B", 1280)])
            })
            .expect("second");

        assert_eq!(calls.get(), 2, "past the TTL the list is re-read");
        assert_eq!(after_ttl[0].1.key.native_id, "B");
        assert_eq!(after_ttl[0].1.bounds.width(), 1280);
    }

    #[test]
    fn invalidate_forces_a_re_read() {
        let displays = WinDisplays::new();
        let calls = Cell::new(0);
        let now = Instant::now();
        displays
            .snapshot_at(now, || {
                calls.set(calls.get() + 1);
                Ok(vec![display("A", 1920)])
            })
            .expect("first");
        displays.invalidate();
        displays
            .snapshot_at(now, || {
                calls.set(calls.get() + 1);
                Ok(vec![display("B", 1280)])
            })
            .expect("second");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_failed_read_keeps_the_previous_list() {
        let displays = WinDisplays::new();
        let now = Instant::now();
        displays
            .snapshot_at(now, || Ok(vec![display("A", 1920)]))
            .expect("first");
        let failed = displays.snapshot_at(now + CACHE_TTL + Duration::from_millis(1), || {
            Err(PlatformError::Os("enumeration failed".to_string()))
        });
        assert!(failed.is_err());
        // The next read still works rather than replaying the failure.
        let recovered = displays
            .snapshot_at(now + CACHE_TTL + Duration::from_millis(2), || {
                Ok(vec![display("B", 1280)])
            })
            .expect("third");
        assert_eq!(recovered[0].1.key.native_id, "B");
    }
}
