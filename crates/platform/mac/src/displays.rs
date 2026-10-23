//! Display enumeration and identity for macOS.
//!
//! Identity uses `CGDisplayCreateUUIDFromDisplayID`, the only documented stable
//! identifier on this platform. Serial numbers are unreliable: many monitors
//! report `0`, and the vendor/model numbers are properties of the *model*, not of
//! this physical unit.
//!
//! Scale comes from `CGDisplayPixelsWide` vs `CGDisplayModeCopyPixelEncoding`-era
//! logic being deprecated, so instead we take the ratio of the display's pixel
//! width to the width reported by `NSScreen.frame` in points. That is the same
//! number the Accessibility API uses.

#![cfg(target_os = "macos")]

use unfy_domain::{DisplayKey, DisplaySnapshot, IdentityQuality, Rect, Scale, WindowId};
use unfy_platform::{DisplayProvider, PlatformError, Result};
use core_graphics::display::{CGDisplay, CGDisplayBounds, CGDisplayIsMain, CGGetActiveDisplayList};

/// Quartz-backed display provider.
///
/// `CGDisplayBounds` is the whole display including the menu bar, while window
/// placement must respect the work area. macOS exposes work areas only through
/// `NSScreen.visibleFrame`, which AppKit restricts to the main thread. The app
/// therefore calls [`QuartzDisplays::prime_work_areas`] from `setup` (which runs
/// on the main thread) and again on every display reconfiguration; `enumerate`
/// uses the primed values and falls back to full bounds when they are missing.
#[derive(Debug, Default)]
pub struct QuartzDisplays {
    cache: std::sync::Mutex<Vec<(u32, DisplaySnapshot)>>,
    work_areas: std::sync::Mutex<std::collections::HashMap<u32, Rect>>,
}

impl QuartzDisplays {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
        if let Ok(mut work) = self.work_areas.lock() {
            work.clear();
        }
    }

    /// Record the current work area of every display.
    ///
    /// Must be called on the main thread; returns `false` when it was not, so
    /// the caller can retry on the main thread instead of silently using whole
    /// display bounds.
    pub fn prime_work_areas(&self) -> bool {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSScreen;

        let Some(marker) = MainThreadMarker::new() else {
            return false;
        };
        let screens = NSScreen::screens(marker);
        let main_height = NSScreen::mainScreen(marker)
            .map(|s| s.frame())
            .map(|f| f.size.height)
            .unwrap_or(0.0);

        let mut work = std::collections::HashMap::new();
        for screen in screens.iter() {
            let visible = screen.visibleFrame();
            let top = main_height - (visible.origin.y + visible.size.height);
            // `NSScreen` does not expose the `CGDirectDisplayID` directly. The
            // documented bridge is `NSScreenNumber` in `deviceDescription`;
            // when it is missing the display simply keeps its full bounds.
            let Some(display_id) = screen_display_id(&screen) else {
                continue;
            };
            work.insert(
                display_id,
                Rect::new(
                    visible.origin.x.round() as i32,
                    top.round() as i32,
                    (visible.origin.x + visible.size.width).round() as i32,
                    (top + visible.size.height).round() as i32,
                ),
            );
        }

        if let Ok(mut cache) = self.work_areas.lock() {
            *cache = work;
        }
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
        true
    }

    fn work_area_for(&self, id: u32, bounds: Rect) -> Rect {
        self.work_areas
            .lock()
            .ok()
            .and_then(|m| m.get(&id).copied())
            .unwrap_or(bounds)
    }

    fn snapshot(&self) -> Result<Vec<(u32, DisplaySnapshot)>> {
        if let Ok(cache) = self.cache.lock() {
            if !cache.is_empty() {
                return Ok(cache.clone());
            }
        }
        let fresh = enumerate(self)?;
        if let Ok(mut cache) = self.cache.lock() {
            *cache = fresh.clone();
        }
        Ok(fresh)
    }
}

impl DisplayProvider for QuartzDisplays {
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

    fn display_for(&self, _id: &WindowId) -> Result<DisplayKey> {
        // The AX backend fills `WindowSnapshot::display` during enumeration by
        // matching the window rectangle against display bounds; looking it up
        // here would require re-querying the window, which the engine does not
        // need.
        Err(PlatformError::Unsupported)
    }
}

fn enumerate(owner: &QuartzDisplays) -> Result<Vec<(u32, DisplaySnapshot)>> {
    let mut count: u32 = 0;
    let mut ids = [0u32; 16];

    let err = unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut count) };
    if err != 0 {
        return Err(PlatformError::Os(format!(
            "CGGetActiveDisplayList failed: {err}"
        )));
    }

    let mut out = Vec::new();
    for id in ids.iter().take(count as usize) {
        let display = CGDisplay::new(*id);
        let bounds = unsafe { CGDisplayBounds(*id) };
        let rect = Rect::new(
            bounds.origin.x.round() as i32,
            bounds.origin.y.round() as i32,
            (bounds.origin.x + bounds.size.width).round() as i32,
            (bounds.origin.y + bounds.size.height).round() as i32,
        );

        let pixel_width = display.pixels_wide() as i32;
        let scale_x = if rect.width() > 0 {
            f64::from(pixel_width) / f64::from(rect.width())
        } else {
            1.0
        };
        // CGDisplayBounds is already in points, so the ratio gives the backing
        // scale (1.0 or 2.0 on Retina).
        let scale = Scale {
            x: if scale_x > 0.0 { scale_x } else { 1.0 },
            y: if scale_x > 0.0 { scale_x } else { 1.0 },
        };

        let native_id = uuid_for_display(*id).unwrap_or_else(|| format!("cg:{id}"));
        let quality = if native_id.starts_with("cg:") {
            IdentityQuality::Weak
        } else {
            IdentityQuality::Strong
        };

        // The menu bar is 25-37 points tall depending on macOS version. The
        // authoritative work area comes from `NSScreen.visibleFrame`, primed on
        // the main thread; when priming has not run yet the display keeps its
        // full bounds rather than pretending the menu bar does not exist.
        out.push((
            *id,
            DisplaySnapshot {
                key: DisplayKey { native_id, quality },
                bounds: rect,
                work_area: owner.work_area_for(*id, rect),
                scale,
                physical_size_mm: None,
                primary: unsafe { CGDisplayIsMain(*id) != 0 },
                label: format!("Display {}", id),
            },
        ));
    }
    Ok(out)
}

/// Stable per-display identifier.
fn uuid_for_display(display_id: u32) -> Option<String> {
    // `CGDisplayCreateUUIDFromDisplayID` lives in ColorSync and is available from
    // macOS 10.13. It returns a CFUUID that is stable across reboots and port
    // changes for the same physical display.
    #[link(name = "ColorSync", kind = "framework")]
    extern "C" {
        fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> core_foundation::base::CFTypeRef;
    }
    let uuid = unsafe { CGDisplayCreateUUIDFromDisplayID(display_id) };
    if uuid.is_null() {
        return None;
    }
    // CFUUID -> string via CFStringCreateFromExternalRepresentation is fiddly;
    // storing the CFUUID's byte representation is enough for identity.
    let bytes = unsafe { core_foundation_sys::uuid::CFUUIDGetUUIDBytes(uuid as _) };
    unsafe { core_foundation_sys::base::CFRelease(uuid as _) };
    Some(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes.byte0,
        bytes.byte1,
        bytes.byte2,
        bytes.byte3,
        bytes.byte4,
        bytes.byte5,
        bytes.byte6,
        bytes.byte7,
        bytes.byte8,
        bytes.byte9,
        bytes.byte10,
        bytes.byte11,
        bytes.byte12,
        bytes.byte13,
        bytes.byte14,
        bytes.byte15,
    ))
}

/// The `CGDirectDisplayID` behind an `NSScreen`.
///
/// `NSScreenNumber` in `deviceDescription` is the documented bridge between
/// AppKit and Quartz display identifiers.
#[cfg(target_os = "macos")]
fn screen_display_id(screen: &objc2_app_kit::NSScreen) -> Option<u32> {
    use objc2_foundation::NSDictionary;
    use objc2_foundation::NSNumber;
    use objc2_foundation::NSString;

    let description: objc2::rc::Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> =
        screen.deviceDescription();
    let key = NSString::from_str("NSScreenNumber");
    let value = description.objectForKey(&key)?;
    let number = value.downcast_ref::<NSNumber>()?;
    u32::try_from(number.integerValue()).ok()
}
