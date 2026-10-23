//! Screenshots of each display, for the display picker.
//!
//! # Why this exists
//!
//! Choosing "which monitor is the remote client showing" from a list of
//! resolutions is guesswork: two identical monitors produce two identical rows.
//! A real thumbnail with a number drawn on it is the only representation a user
//! can match against what they see in the remote window.
//!
//! # Privacy
//!
//! The image is captured locally, scaled down, and returned only in memory to the
//! app's own window. It is never written to disk, never included in the
//! diagnostics export, and never uploaded. The user is looking at their own
//! desktop, so this reveals nothing they cannot already see — but the constraint
//! is stated here because "screenshot" is otherwise a red flag in a privacy
//! review.

use base64::Engine as _;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    SRCCOPY,
};

use crate::WinDisplays;

/// One display's thumbnail, encoded as a PNG data URL.
#[derive(Debug, Clone)]
pub struct DisplayThumbnail {
    pub native_id: String,
    /// `data:image/png;base64,...`, ready for an `<img src>`.
    pub data_url: String,
    /// True when the capture came back entirely black.
    ///
    /// A monitor that is powered off, on another input, or a disconnected virtual
    /// display still enumerates and captures - as solid black. The picker shows a
    /// black card with a number on it, which reads as a broken preview unless it
    /// says what it is.
    pub blank: bool,
}

/// Longest edge of the returned thumbnail, in pixels.
///
/// 256 is enough to recognise a wallpaper and window shapes. Larger sizes made
/// the base64 payload large enough that sending every display's image over the
/// IPC bridge on each page load was noticeable.
const MAX_EDGE: i32 = 256;

/// Capture a thumbnail of every display.
///
/// Returns an empty vector when screen capture is unavailable (for example a
/// locked session); the UI then falls back to the geometric map rather than
/// showing broken images.
pub fn display_thumbnails() -> Vec<DisplayThumbnail> {
    use unfy_platform::DisplayProvider;

    let displays = WinDisplays::new();
    let Ok(topology) = displays.topology() else {
        return Vec::new();
    };

    topology
        .into_iter()
        .filter_map(|display| {
            let bounds = display.bounds;
            let captured =
                capture_region(bounds.left, bounds.top, bounds.width(), bounds.height())?;
            Some(DisplayThumbnail {
                native_id: display.key.native_id,
                data_url: format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(captured.png)
                ),
                blank: captured.blank,
            })
        })
        .collect()
}

/// A captured frame, scaled down, with the verdict on whether it holds anything.
struct Captured {
    png: Vec<u8>,
    blank: bool,
}

/// Capture `width x height` at `(x, y)` and scale it down to [`MAX_EDGE`].
fn capture_region(x: i32, y: i32, width: i32, height: i32) -> Option<Captured> {
    if width <= 0 || height <= 0 {
        return None;
    }

    // SAFETY: every handle acquired here is released on the same path; the raw
    // pointer arithmetic is bounded by the dimensions read from the screen DC.
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let memory = CreateCompatibleDC(Some(screen));
        if memory.is_invalid() {
            ReleaseDC(None, screen);
            return None;
        }

        let bitmap = CreateCompatibleBitmap(screen, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            return None;
        }
        let previous = SelectObject(memory, bitmap.into());

        let copied = BitBlt(memory, 0, 0, width, height, Some(screen), x, y, SRCCOPY).is_ok();

        let mut pixels: Vec<u8> = Vec::new();
        if copied {
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: core::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    // Negative height requests top-down rows, matching PNG's order.
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            pixels.resize((width as usize) * (height as usize) * 4, 0);
            let scanned = GetDIBits(
                memory,
                bitmap,
                0,
                height as u32,
                Some(pixels.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            );
            if scanned == 0 {
                pixels.clear();
            }
        }

        let _ = SelectObject(memory, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);

        if pixels.is_empty() {
            return None;
        }

        // BGRA -> RGBA, then downscale by integer step and encode as PNG.
        let (rgba, out_w, out_h, blank) = downscale_bgra(&pixels, width, height);
        let image = rayless::Rgba {
            width: out_w as u32,
            height: out_h as u32,
            data: rgba,
        };
        Some(Captured {
            png: image.to_png()?,
            blank,
        })
    }
}

/// Downscale a top-down BGRA frame to RGBA, and report whether it shows anything.
///
/// Separate from the capture so the "is this display showing nothing" rule can be
/// tested without a screen: it decides whether the picker explains a black card or
/// leaves the user wondering.
fn downscale_bgra(bgra: &[u8], width: i32, height: i32) -> (Vec<u8>, i32, i32, bool) {
    let (out_w, out_h) = scaled_size(width, height);
    let step_x = width as f32 / out_w as f32;
    let step_y = height as f32 / out_h as f32;
    let mut rgba = Vec::with_capacity((out_w * out_h * 4) as usize);
    // Every sampled pixel being black means the display is showing nothing.
    // Anything at all - a taskbar, a wallpaper, a window - clears the flag, so a
    // dark theme is never mistaken for a dead monitor.
    let mut blank = true;
    for row in 0..out_h {
        let source_row = ((row as f32 * step_y) as i32).min(height - 1);
        for col in 0..out_w {
            let source_col = ((col as f32 * step_x) as i32).min(width - 1);
            let index = ((source_row * width + source_col) * 4) as usize;
            let (b, g, r) = (bgra[index], bgra[index + 1], bgra[index + 2]);
            if blank && pixel_shows_content(r, g, b) {
                blank = false;
            }
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    (rgba, out_w, out_h, blank)
}

/// Whether one pixel shows anything at all.
fn pixel_shows_content(r: u8, g: u8, b: u8) -> bool {
    r >= NEAR_BLACK || g >= NEAR_BLACK || b >= NEAR_BLACK
}

/// Channel value at or above which a pixel counts as displaying something.
///
/// Eight, not zero: a powered-off panel and a Virtual Display Driver with no
/// streaming client both capture as 0, while video compression artefacts around
/// black text sit above this.
const NEAR_BLACK: u8 = 8;

/// Fit within [`MAX_EDGE`] while preserving the aspect ratio.
fn scaled_size(width: i32, height: i32) -> (i32, i32) {
    if width <= MAX_EDGE && height <= MAX_EDGE {
        return (width, height);
    }
    let ratio = (MAX_EDGE as f32 / width as f32).min(MAX_EDGE as f32 / height as f32);
    (
        ((width as f32 * ratio) as i32).max(1),
        ((height as f32 * ratio) as i32).max(1),
    )
}

/// Minimal RGBA -> PNG encoder, so the crate does not pull in a full image
/// library for one call.
mod rayless {
    pub struct Rgba {
        pub width: u32,
        pub height: u32,
        pub data: Vec<u8>,
    }

    impl Rgba {
        pub fn to_png(&self) -> Option<Vec<u8>> {
            let mut raw = Vec::with_capacity(self.data.len() + self.height as usize);
            for row in 0..self.height as usize {
                raw.push(0); // filter: none
                let start = row * self.width as usize * 4;
                raw.extend_from_slice(&self.data[start..start + self.width as usize * 4]);
            }

            let mut out = Vec::new();
            out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);

            let mut ihdr = Vec::new();
            ihdr.extend_from_slice(&self.width.to_be_bytes());
            ihdr.extend_from_slice(&self.height.to_be_bytes());
            ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
            write_chunk(&mut out, b"IHDR", &ihdr);

            let mut idat = Vec::new();
            zlib_store(&raw, &mut idat);
            write_chunk(&mut out, b"IDAT", &idat);
            write_chunk(&mut out, b"IEND", &[]);
            Some(out)
        }
    }

    fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc = Crc32::new();
        crc.update(kind);
        crc.update(data);
        out.extend_from_slice(&crc.finish().to_be_bytes());
    }

    /// Compress with real deflate.
    ///
    /// A stored (uncompressed) stream was the first implementation, and it made
    /// each 320 px thumbnail ~300 KiB: sending a handful of them over the IPC
    /// bridge on every page load was visibly slow.
    fn zlib_store(data: &[u8], out: &mut Vec<u8>) {
        use std::io::Write;

        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        if encoder.write_all(data).is_ok() {
            if let Ok(compressed) = encoder.finish() {
                out.extend_from_slice(&compressed);
                return;
            }
        }
        // Compression failure must not lose the image; fall back to storing.
        out.push(0x78);
        out.push(0x01);
        for (index, block) in data.chunks(65_535).enumerate() {
            let last = (index + 1) * 65_535 >= data.len();
            out.push(if last { 1 } else { 0 });
            let len = block.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(block);
        }
    }

    struct Crc32 {
        table: [u32; 256],
        value: u32,
    }

    impl Crc32 {
        fn new() -> Self {
            let mut table = [0u32; 256];
            for (index, slot) in table.iter_mut().enumerate() {
                let mut c = index as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        0xedb8_8320 ^ (c >> 1)
                    } else {
                        c >> 1
                    };
                }
                *slot = c;
            }
            Self {
                table,
                value: 0xffff_ffff,
            }
        }

        fn update(&mut self, data: &[u8]) {
            for byte in data {
                self.value = self.table[((self.value ^ u32::from(*byte)) & 0xff) as usize]
                    ^ (self.value >> 8);
            }
        }

        fn finish(&self) -> u32 {
            self.value ^ 0xffff_ffff
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A top-down BGRA frame from a list of RGB triples.
    fn frame(pixels: &[(u8, u8, u8)], width: i32, height: i32) -> Vec<u8> {
        assert_eq!(pixels.len() as i32, width * height);
        let mut out = Vec::new();
        for (r, g, b) in pixels {
            out.extend_from_slice(&[*b, *g, *r, 255]);
        }
        out
    }

    #[test]
    fn a_black_frame_is_reported_as_blank() {
        let bgra = frame(&[(0, 0, 0); 4], 2, 2);
        let (rgba, w, h, blank) = downscale_bgra(&bgra, 2, 2);
        assert!(blank);
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgba.len(), 2 * 2 * 4);
        // Alpha is forced opaque: a fully transparent thumbnail renders as a hole.
        assert_eq!(&rgba[3..4], &[255]);
    }

    #[test]
    fn one_lit_pixel_is_enough_to_disqualify_blank() {
        let mut pixels = vec![(0u8, 0u8, 0u8); 4];
        pixels[3] = (255, 255, 255);
        let (_, _, _, blank) = downscale_bgra(&frame(&pixels, 2, 2), 2, 2);
        assert!(!blank, "a taskbar or a cursor means the display is on");
    }

    #[test]
    fn a_single_channel_above_the_threshold_counts_as_content() {
        // Video compression leaves near-black pixels around dark text; only a
        // truly dark frame is treated as "nothing is showing".
        let (_, _, _, just_under) = downscale_bgra(&frame(&[(7, 7, 7); 4], 2, 2), 2, 2);
        assert!(just_under, "below the threshold is still blank");
        let (_, _, _, at_threshold) = downscale_bgra(&frame(&[(8, 0, 0); 4], 2, 2), 2, 2);
        assert!(!at_threshold, "at the threshold is content");
    }

    #[test]
    fn channels_are_reordered_from_bgra_to_rgba() {
        // Getting this backwards tints every thumbnail blue, which is subtle enough
        // to ship.
        let (rgba, _, _, _) = downscale_bgra(&frame(&[(10, 20, 30)], 1, 1), 1, 1);
        assert_eq!(rgba, vec![10, 20, 30, 255]);
    }

    #[test]
    fn a_large_frame_is_scaled_to_the_max_edge() {
        let bgra = frame(&[(200, 100, 50); 1920 * 1080], 1920, 1080);
        let (rgba, w, h, blank) = downscale_bgra(&bgra, 1920, 1080);
        assert!(!blank);
        // The longest edge is fitted, so a 16:9 frame is 256 wide and 144 tall.
        assert_eq!(w, MAX_EDGE);
        assert_eq!(h, 144);
        assert_eq!(rgba.len() as i32, w * h * 4);
    }

    #[test]
    fn a_small_frame_keeps_its_size() {
        let (_, w, h, _) = downscale_bgra(&frame(&[(1, 2, 3)], 1, 1), 1, 1);
        assert_eq!((w, h), (1, 1));
    }
}
