//! LCD framebuffer reconstruction from pixel trace data.
//!
//! Reads the `pix` field from trace entries and reconstructs 160×144
//! 2-bit grayscale frames. Each character in the `pix` string is a
//! pixel value ('0'-'3'). Pixels are pushed left-to-right per scanline,
//! with scanline boundaries detected from `ly` changes.

use crate::column_store::TraceStore;

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
    /// Pixels with value 0xFF (unrendered sentinel) get alpha=0.
    pub fn to_rgba(&self) -> Vec<u8> {
        const PALETTE: [u8; 4] = [0xFF, 0xAA, 0x55, 0x00]; // DMG shades

        let mut rgba = vec![0u8; LCD_WIDTH * LCD_HEIGHT * 4];
        for (i, &pix) in self.pixels.iter().enumerate() {
            if pix == 0xFF {
                // Unrendered — transparent
                continue;
            }
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
/// Reconstruct LCD frames from a column store's `pix` field.
///
/// Returns one `Frame` per detected frame boundary. Uses `ly` to track
/// scanlines and resets the x cursor when `ly` changes.
pub fn reconstruct_frames(store: &dyn TraceStore) -> Vec<Frame> {
    if store.field_col("pix").is_none() {
        return Vec::new();
    }

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

        let mut frame = reconstruct_partial_frame(store, start, end);
        frame.index = fi;
        frames.push(frame);
    }

    frames
}

/// Reconstruct a partial frame up to (but not including) `stop_entry`.
///
/// Processes entries from `frame_start` to `stop_entry`, building the
/// LCD image progressively. Unrendered pixels are set to 0xFF (sentinel)
/// so `to_rgba()` outputs them as transparent.
pub fn reconstruct_partial_frame(
    store: &dyn TraceStore,
    frame_start: usize,
    stop_entry: usize,
) -> Frame {
    let mut frame = Frame::new(0, frame_start);
    frame.pixels.fill(0xFF); // mark all as unrendered
    frame.end_entry = stop_entry;

    let pix_col = match store.field_col("pix") {
        Some(c) => c,
        None => return frame,
    };

    // Count non-empty pix outputs sequentially: each one is the next
    // pixel pushed to the LCD, left-to-right, top-to-bottom.
    let mut pixel_idx: usize = 0;

    let end = stop_entry.min(store.entry_count());
    for i in frame_start..end {
        let pix_str = store.get_str(pix_col, i);
        if pix_str.is_empty() { continue; }

        // Full-frame dump: write all pixels at once
        if pix_str.len() == LCD_WIDTH * LCD_HEIGHT {
            for (j, ch) in pix_str.bytes().enumerate() {
                if ch >= b'0' && ch <= b'3' {
                    frame.pixels[j] = ch - b'0';
                }
            }
            pixel_idx = LCD_WIDTH * LCD_HEIGHT;
            continue;
        }

        // Per-pixel output: each char is one pixel in LCD order
        for ch in pix_str.bytes() {
            if ch >= b'0' && ch <= b'3' && pixel_idx < LCD_WIDTH * LCD_HEIGHT {
                frame.pixels[pixel_idx] = ch - b'0';
                pixel_idx += 1;
            }
        }
    }

    frame
}

/// Build a map of pixel (x, y) positions for each entry in a frame.
///
/// Returns a Vec of `(x, y)` pairs indexed by `entry - frame_start`.
/// Entries with no pixel data get `(0xFFFF, 0xFFFF)`.
/// Position is derived from sequential pixel count (LCD order).
pub fn build_pixel_position_map(
    store: &dyn TraceStore,
    frame_start: usize,
    frame_end: usize,
) -> Vec<(u16, u16)> {
    let count = frame_end.saturating_sub(frame_start);
    let mut map = vec![(0xFFFFu16, 0xFFFFu16); count];

    let pix_col = match store.field_col("pix") {
        Some(c) => c,
        None => return map,
    };

    let mut pixel_idx: usize = 0;

    let end = frame_end.min(store.entry_count());
    for i in frame_start..end {
        let pix_str = store.get_str(pix_col, i);
        if pix_str.is_empty() { continue; }

        // Skip full-frame dumps for position tracking
        if pix_str.len() == LCD_WIDTH * LCD_HEIGHT { continue; }

        let idx = i - frame_start;
        for ch in pix_str.bytes() {
            if ch >= b'0' && ch <= b'3' {
                let x = pixel_idx % LCD_WIDTH;
                let y = pixel_idx / LCD_WIDTH;
                if y < LCD_HEIGHT {
                    map[idx] = (x as u16, y as u16);
                }
                pixel_idx += 1;
            }
        }
    }

    map
}
