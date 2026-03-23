//! LCD framebuffer reconstruction from pixel trace data.
//!
//! Reads the `pix` field from trace entries and reconstructs 160×144
//! 2-bit grayscale frames. Each character in the `pix` string is a
//! pixel value ('0'-'3'). Pixels are pushed left-to-right per scanline,
//! with scanline boundaries detected from `ly` changes.

use crate::column_store::ColumnStore;

pub const LCD_WIDTH: usize = 160;
pub const LCD_HEIGHT: usize = 144;

/// A single reconstructed LCD frame (160×144, 2-bit pixels).
pub struct Frame {
    /// Row-major pixel data. `pixels[y * LCD_WIDTH + x]` is the pixel
    /// value (0-3) at position (x, y).
    pub pixels: Vec<u8>,
    /// Frame index (0-based).
    pub index: usize,
    /// Entry index in the trace where this frame starts.
    pub start_entry: usize,
    /// Entry index where this frame ends (exclusive).
    pub end_entry: usize,
}

impl Frame {
    fn new(index: usize, start_entry: usize) -> Self {
        Self {
            pixels: vec![0; LCD_WIDTH * LCD_HEIGHT],
            index,
            start_entry,
            end_entry: start_entry,
        }
    }

    /// Return raw RGBA pixel data (160×144×4 bytes). DMG palette: 0=white, 3=black.
    pub fn to_rgba(&self) -> Vec<u8> {
        const PALETTE: [u8; 4] = [0xFF, 0xAA, 0x55, 0x00]; // DMG shades

        let mut rgba = vec![0u8; LCD_WIDTH * LCD_HEIGHT * 4];
        for (i, &pix) in self.pixels.iter().enumerate() {
            let shade = PALETTE[pix.min(3) as usize];
            rgba[i * 4] = shade;
            rgba[i * 4 + 1] = shade;
            rgba[i * 4 + 2] = shade;
            rgba[i * 4 + 3] = 0xFF;
        }
        rgba
    }

    /// Encode as a 160×144 RGBA PNG. DMG palette: 0=white, 3=black.
    #[cfg(feature = "png")]
    pub fn to_png(&self) -> Vec<u8> {
        let rgba = self.to_rgba();

        let mut buf = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut buf, LCD_WIDTH as u32, LCD_HEIGHT as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&rgba).unwrap();
        }
        buf
    }
}

/// Flush a scanline buffer into the frame pixel data.
/// Takes the last 160 pixels (extra pixels push earlier ones off the left,
/// matching real LCD hardware behavior).
fn flush_scanline(scanline_buf: &[u8], y: usize, pixels: &mut [u8]) {
    if y >= LCD_HEIGHT || scanline_buf.is_empty() {
        return;
    }
    let start = if scanline_buf.len() > LCD_WIDTH {
        scanline_buf.len() - LCD_WIDTH
    } else {
        0
    };
    let visible = &scanline_buf[start..];
    for (x, &pix) in visible.iter().enumerate() {
        if x < LCD_WIDTH {
            pixels[y * LCD_WIDTH + x] = pix;
        }
    }
}

/// Reconstruct LCD frames from a column store's `pix` field.
///
/// Returns one `Frame` per detected frame boundary. Uses `ly` to track
/// scanlines and resets the x cursor when `ly` changes.
pub fn reconstruct_frames(store: &ColumnStore) -> Vec<Frame> {
    let pix_col = match store.field_col("pix") {
        Some(c) => c,
        None => return Vec::new(),
    };
    let ly_col = store.field_col("ly");

    let boundaries = store.frame_boundaries();
    if boundaries.is_empty() {
        return Vec::new();
    }

    let total = store.entry_count();
    let mut frames = Vec::new();

    for (fi, &boundary_start) in boundaries.iter().enumerate() {
        let start = boundary_start as usize;
        let end = if fi + 1 < boundaries.len() {
            boundaries[fi + 1] as usize
        } else {
            total
        };

        let mut frame = Frame::new(fi, start);
        frame.end_entry = end;

        let mut y: usize = 0;
        let mut prev_ly: Option<u8> = None;
        let mut scanline_buf: Vec<u8> = Vec::with_capacity(LCD_WIDTH + 16);

        for i in start..end {
            let pix_str = store.column(pix_col).get_str(i);
            if pix_str.is_empty() { continue; }

            // Full-frame pixel dump (160*144 = 23040 chars): write directly
            // to the framebuffer, ignoring ly tracking.
            if pix_str.len() == LCD_WIDTH * LCD_HEIGHT {
                for (j, ch) in pix_str.bytes().enumerate() {
                    if ch >= b'0' && ch <= b'3' {
                        frame.pixels[j] = ch - b'0';
                    }
                }
                continue;
            }

            // Get y position from ly
            if let Some(lc) = ly_col {
                let cur_ly = store.column(lc).get_numeric(i) as u8;
                if let Some(pl) = prev_ly {
                    if cur_ly != pl {
                        // Flush previous scanline: take last 160 pixels
                        flush_scanline(&scanline_buf, y, &mut frame.pixels);
                        scanline_buf.clear();
                        if (cur_ly as usize) < LCD_HEIGHT {
                            y = cur_ly as usize;
                        }
                    }
                } else if (cur_ly as usize) < LCD_HEIGHT {
                    y = cur_ly as usize;
                }
                prev_ly = Some(cur_ly);
            }

            // Accumulate pixels for the current scanline
            for ch in pix_str.bytes() {
                if ch >= b'0' && ch <= b'3' {
                    scanline_buf.push(ch - b'0');
                }
            }
        }

        // Flush final scanline
        flush_scanline(&scanline_buf, y, &mut frame.pixels);

        frames.push(frame);
    }

    frames
}
