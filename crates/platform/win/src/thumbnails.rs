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
    use conflux_platform::DisplayProvider;

    let displays = WinDisplays::new();
    let Ok(topology) = displays.topology() else {
        return Vec::new();
    };

    topology
        .into_iter()
        .filter_map(|display| {
            let bounds = display.bounds;
            let png = capture_region(bounds.left, bounds.top, bounds.width(), bounds.height())?;
            Some(DisplayThumbnail {
                native_id: display.key.native_id,
                data_url: format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(png)
                ),
            })
        })
        .collect()
}

/// Capture `width x height` at `(x, y)` and scale it down to [`MAX_EDGE`].
fn capture_region(x: i32, y: i32, width: i32, height: i32) -> Option<Vec<u8>> {
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
        let (out_w, out_h) = scaled_size(width, height);
        let step_x = width as f32 / out_w as f32;
        let step_y = height as f32 / out_h as f32;
        let mut rgba = Vec::with_capacity((out_w * out_h * 4) as usize);
        for row in 0..out_h {
            let source_row = ((row as f32 * step_y) as i32).min(height - 1);
            for col in 0..out_w {
                let source_col = ((col as f32 * step_x) as i32).min(width - 1);
                let index = ((source_row * width + source_col) * 4) as usize;
                rgba.extend_from_slice(&[pixels[index + 2], pixels[index + 1], pixels[index], 255]);
            }
        }

        let image = rayless::Rgba {
            width: out_w as u32,
            height: out_h as u32,
            data: rgba,
        };
        image.to_png()
    }
}

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
