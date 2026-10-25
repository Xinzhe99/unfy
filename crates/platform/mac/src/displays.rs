//! Main-thread AppKit display snapshots in the same desktop points used by AX.
//! Work areas and bounds are published together; absent/stale work areas never
//! silently become whole-display bounds that include the Dock and menu bar.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use core_foundation::base::TCFType;
use core_graphics::display::CGDisplayIsMain;
use unfy_domain::{DisplayKey, DisplaySnapshot, IdentityQuality, Rect, Scale, WindowId};
use unfy_platform::{DisplayProvider, PlatformError, Result};

#[derive(Debug, Default)]
pub struct QuartzDisplays {
    cache: Mutex<Option<Cached>>,
}

#[derive(Debug)]
struct Cached {
    at: Instant,
    displays: Vec<DisplaySnapshot>,
}

// Desktop integration refreshes on the main thread every second, including Dock
// changes. A stalled refresher must stop planning from arbitrarily old geometry.
const CACHE_TTL: Duration = Duration::from_secs(5);

impl QuartzDisplays {
    /// Refresh the entire topology on the main thread and publish it atomically.
    /// False means no usable fresh snapshot was published.
    pub fn prime_work_areas(&self) -> bool {
        let Some(marker) = objc2::MainThreadMarker::new() else {
            return false;
        };
        let fresh = objc2::rc::autoreleasepool(|_| enumerate(marker));
        let Ok(mut cache) = self.cache.lock() else {
            return false;
        };
        match fresh {
            Ok(displays) => {
                *cache = Some(Cached {
                    at: Instant::now(),
                    displays,
                });
                true
            }
            Err(_) => {
                *cache = None;
                false
            }
        }
    }

    fn snapshot(&self) -> Result<Vec<DisplaySnapshot>> {
        if objc2::MainThreadMarker::new().is_some() {
            self.prime_work_areas();
        }
        let cache = self
            .cache
            .lock()
            .map_err(|_| PlatformError::Os("display cache poisoned".into()))?;
        let cached = cache
            .as_ref()
            .filter(|cached| cached.at.elapsed() < CACHE_TTL)
            .ok_or_else(|| {
                PlatformError::Os("main-thread display work areas unavailable or stale".into())
            })?;
        Ok(cached.displays.clone())
    }
}

impl DisplayProvider for QuartzDisplays {
    fn topology(&self) -> Result<Vec<DisplaySnapshot>> {
        self.snapshot()
    }

    fn scale_of(&self, key: &DisplayKey) -> Result<Scale> {
        self.snapshot()?
            .into_iter()
            .find(|display| display.key == *key)
            .map(|display| display.scale)
            .ok_or(PlatformError::DisplayGone)
    }

    fn display_for(&self, _id: &WindowId) -> Result<DisplayKey> {
        // MacPlatform combines the observed AX rectangle with this topology.
        Err(PlatformError::Unsupported)
    }
}

fn enumerate(marker: objc2::MainThreadMarker) -> Result<Vec<DisplaySnapshot>> {
    let screens = objc2_app_kit::NSScreen::screens(marker);
    // NSScreen's first entry is the global menu-bar reference screen. mainScreen
    // instead follows keyboard focus and is not a coordinate-system reference.
    let primary = screens.iter().next().ok_or(PlatformError::DisplayGone)?;
    let reference_height = primary.frame().origin.y + primary.frame().size.height;
    let mut displays = Vec::with_capacity(screens.len());
    for screen in screens.iter() {
        let id = screen_display_id(&screen)
            .ok_or_else(|| PlatformError::Os("NSScreenNumber unavailable".into()))?;
        let frame = screen.frame();
        let visible = screen.visibleFrame();
        let bounds = appkit_rect(
            reference_height,
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        )?;
        let work_area = appkit_rect(
            reference_height,
            visible.origin.x,
            visible.origin.y,
            visible.size.width,
            visible.size.height,
        )?;
        let uuid = uuid_for_display(id);
        let quality = if uuid.is_some() {
            IdentityQuality::Strong
        } else {
            IdentityQuality::Weak
        };
        displays.push(DisplaySnapshot {
            key: DisplayKey {
                native_id: uuid.unwrap_or_else(|| format!("cg:{id}")),
                quality,
            },
            bounds,
            work_area,
            // Both AppKit frames and AX window geometry use points. Retina
            // backing pixels must never rescale these desktop coordinates.
            scale: Scale::IDENTITY,
            physical_size_mm: None,
            primary: unsafe { CGDisplayIsMain(id) != 0 },
            label: screen.localizedName().to_string(),
        });
    }
    Ok(displays)
}

fn appkit_rect(reference_top: f64, x: f64, y: f64, width: f64, height: f64) -> Result<Rect> {
    let top = reference_top - (y + height);
    let edges = [x, top, x + width, top + height];
    if width <= 0.0
        || height <= 0.0
        || edges
            .iter()
            .any(|value| !value.is_finite() || *value < i32::MIN as f64 || *value > i32::MAX as f64)
    {
        return Err(PlatformError::Os("invalid NSScreen geometry".into()));
    }
    Ok(Rect::new(
        edges[0].round() as i32,
        edges[1].round() as i32,
        edges[2].round() as i32,
        edges[3].round() as i32,
    ))
}

fn uuid_for_display(display_id: u32) -> Option<String> {
    use core_foundation::uuid::{CFUUIDGetUUIDBytes, CFUUIDRef, CFUUID};
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> CFUUIDRef;
    }
    let raw = unsafe { CGDisplayCreateUUIDFromDisplayID(display_id) };
    if raw.is_null() {
        return None;
    }
    let uuid = unsafe { CFUUID::wrap_under_create_rule(raw) };
    let bytes = unsafe { CFUUIDGetUUIDBytes(uuid.as_concrete_TypeRef()) };
    Some(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes.byte0, bytes.byte1, bytes.byte2, bytes.byte3, bytes.byte4, bytes.byte5,
        bytes.byte6, bytes.byte7, bytes.byte8, bytes.byte9, bytes.byte10, bytes.byte11,
        bytes.byte12, bytes.byte13, bytes.byte14, bytes.byte15,
    ))
}

fn screen_display_id(screen: &objc2_app_kit::NSScreen) -> Option<u32> {
    use objc2_foundation::{NSNumber, NSString};
    let description = screen.deviceDescription();
    let key = NSString::from_str("NSScreenNumber");
    let value = description.objectForKey(&key)?;
    let number = value.downcast_ref::<NSNumber>()?;
    u32::try_from(number.integerValue()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_coordinates_share_global_reference_and_preserve_work_area() {
        assert_eq!(
            appkit_rect(1080.0, -1920.0, 1080.0, 1920.0, 1200.0).unwrap(),
            Rect::new(-1920, -1200, 0, 0)
        );
        assert_eq!(
            appkit_rect(1080.0, 0.0, -900.0, 1600.0, 900.0).unwrap(),
            Rect::new(0, 1080, 1600, 1980)
        );
        assert_eq!(
            appkit_rect(1080.0, 0.0, 80.0, 1920.0, 975.0).unwrap(),
            Rect::new(0, 25, 1920, 1000)
        );
    }

    #[test]
    fn invalid_screen_geometry_is_not_published() {
        assert!(appkit_rect(1080.0, f64::NAN, 0.0, 1920.0, 1080.0).is_err());
        assert!(appkit_rect(1080.0, 0.0, 0.0, 0.0, 1080.0).is_err());
    }
}
