//! Display enumeration and identity.
//!
//! Identity uses `DisplayConfigGetDeviceInfo(DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME)`
//! and its `monitorDevicePath`, because `HMONITOR` values are invalidated by
//! `WM_DISPLAYCHANGE` and cannot be persisted. `GetDpiForMonitor` with
//! `MDT_EFFECTIVE_DPI` supplies the user's scale factor; `GetDpiForWindow` cannot
//! be used for this because it reports 96 for DPI-unaware target processes.

use conflux_domain::{DisplayKey, DisplaySnapshot, IdentityQuality, Rect, Scale, WindowId};
use conflux_platform::{DisplayProvider, PlatformError, Result};
use windows::core::BOOL;
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{LPARAM, POINT, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HDC, HMONITOR,
    MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};

use crate::dpi::PerMonitorV2;

/// Cached display topology.
///
/// `HMONITOR` values are only valid until the next topology change, so the cache
/// is keyed by the raw value and rebuilt wholesale on [`WinDisplays::invalidate`].
#[derive(Debug, Default)]
pub struct WinDisplays {
    cache: std::sync::Mutex<Vec<(isize, DisplaySnapshot)>>,
}

impl WinDisplays {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the cached topology. Call on `WM_DISPLAYCHANGE` or `WM_DPICHANGED`.
    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    fn snapshot(&self) -> Result<Vec<(isize, DisplaySnapshot)>> {
        if let Ok(cache) = self.cache.lock() {
            if !cache.is_empty() {
                return Ok(cache.clone());
            }
        }
        let fresh = enumerate_monitors()?;
        if let Ok(mut cache) = self.cache.lock() {
            *cache = fresh.clone();
        }
        Ok(fresh)
    }
}

impl DisplayProvider for WinDisplays {
    fn topology(&self) -> Result<Vec<DisplaySnapshot>> {
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
        let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        self.snapshot()?
            .into_iter()
            .find(|(raw, _)| *raw == hmon.0 as isize)
            .map(|(_, d)| d.key)
            .ok_or(PlatformError::DisplayGone)
    }
}

pub(crate) fn hwnd_from_id(id: &WindowId) -> Result<windows::Win32::Foundation::HWND> {
    let hwnd = windows::Win32::Foundation::HWND(id.native_id as *mut core::ffi::c_void);
    if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        Ok(hwnd)
    } else {
        Err(PlatformError::WindowGone)
    }
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

    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
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

        // The source mode lives in the mode array; find it by source id.
        let source_rect = modes.iter().find(|m| m.id == path.sourceInfo.id).map(|m| {
            let mode = unsafe { m.Anonymous.sourceMode };
            Rect::new(
                mode.position.x,
                mode.position.y,
                mode.position.x + mode.width as i32,
                mode.position.y + mode.height as i32,
            )
        });

        if source_rect == Some(target_rect) {
            let path_str = utf16_field(&name.monitorDevicePath);
            if !path_str.is_empty() {
                return Some(path_str);
            }
        }
    }
    None
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

/// Process id and creation time for a window, used to disambiguate recycled
/// handles. Creation time is `0` until the task that needs it lands; the pid
/// alone still catches the common "different app, same HWND value" case.
pub(crate) fn process_identity(hwnd: windows::Win32::Foundation::HWND) -> (u32, u64) {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (pid, 0)
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
