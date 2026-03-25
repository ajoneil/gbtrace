//! LCD framebuffer reconstruction from pixel trace data.
//!
//! Reads the `pix` field from trace entries and reconstructs 160×144
//! 2-bit grayscale frames. Each character in the `pix` string is a
//! pixel value ('0'-'3'). Pixels are pushed left-to-right per scanline,
//! with scanline boundaries detected from `ly` changes.

use crate::column_store::TraceStore;
use crate::downsample::DownsampledStore;

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
        // DMG green palette (BGB style): #e0f8d0, #88c070, #346856, #081820
        const PALETTE: [(u8, u8, u8); 4] = [
            (0xe0, 0xf8, 0xd0), // lightest
            (0x88, 0xc0, 0x70), // light
            (0x34, 0x68, 0x56), // dark
            (0x08, 0x18, 0x20), // darkest
        ];

        let mut rgba = vec![0u8; LCD_WIDTH * LCD_HEIGHT * 4];
        for (i, &pix) in self.pixels.iter().enumerate() {
            if pix == 0xFF {
                // Unrendered — transparent
                continue;
            }
            let (r, g, b) = PALETTE[pix.min(3) as usize];
            rgba[i * 4] = r;
            rgba[i * 4 + 1] = g;
            rgba[i * 4 + 2] = b;
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

    let ly_col = store.field_col("ly");
    let use_ly = ly_col.is_some();

    // Track pixel placement: either via ly (scanline) or sequential counting
    let mut pixel_idx: usize = 0;
    let mut scanline_x: usize = 0;
    let mut last_ly: Option<u8> = None;

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

        if use_ly {
            // Use ly to determine the scanline (Y), count X within scanline
            let ly = store.get_numeric(ly_col.unwrap(), i) as u8;
            if last_ly != Some(ly) {
                scanline_x = 0;
                last_ly = Some(ly);
            }
            let y = ly as usize;
            for ch in pix_str.bytes() {
                if ch >= b'0' && ch <= b'3' && y < LCD_HEIGHT && scanline_x < LCD_WIDTH {
                    frame.pixels[y * LCD_WIDTH + scanline_x] = ch - b'0';
                    scanline_x += 1;
                }
            }
        } else {
            // Fallback: sequential pixel counting
            for ch in pix_str.bytes() {
                if ch >= b'0' && ch <= b'3' && pixel_idx < LCD_WIDTH * LCD_HEIGHT {
                    frame.pixels[pixel_idx] = ch - b'0';
                    pixel_idx += 1;
                }
            }
        }
    }

    frame
}

/// Reconstruct a partial frame for a downsampled store.
/// Takes downsampled frame_start and stop_entry, maps them to raw indices,
/// and reconstructs from the inner (full-resolution) store.
pub fn reconstruct_partial_frame_downsampled(
    ds: &DownsampledStore,
    frame_start: usize,
    stop_entry: usize,
) -> Frame {
    let raw_start = ds.original_index(frame_start).unwrap_or(0);
    let raw_stop = if stop_entry < ds.entry_count() {
        // Include all T-cycles up to the next instruction boundary
        ds.original_index(stop_entry + 1).unwrap_or(ds.inner().entry_count())
    } else {
        ds.inner().entry_count()
    };
    reconstruct_partial_frame(ds.inner(), raw_start, raw_stop)
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

    let ly_col = store.field_col("ly");
    let use_ly = ly_col.is_some();
    let mut pixel_idx: usize = 0;
    let mut scanline_x: usize = 0;
    let mut last_ly: Option<u8> = None;

    let end = frame_end.min(store.entry_count());
    for i in frame_start..end {
        let pix_str = store.get_str(pix_col, i);
        if pix_str.is_empty() { continue; }

        // Skip full-frame dumps for position tracking
        if pix_str.len() == LCD_WIDTH * LCD_HEIGHT { continue; }

        let idx = i - frame_start;
        if use_ly {
            let ly = store.get_numeric(ly_col.unwrap(), i) as u8;
            if last_ly != Some(ly) {
                scanline_x = 0;
                last_ly = Some(ly);
            }
            let y = ly as usize;
            for ch in pix_str.bytes() {
                if ch >= b'0' && ch <= b'3' && y < LCD_HEIGHT && scanline_x < LCD_WIDTH {
                    map[idx] = (scanline_x as u16, y as u16);
                    scanline_x += 1;
                }
            }
        } else {
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
    }

    map
}

/// Build a reverse pixel position map: for each LCD position (x, y), returns the
/// global entry index of the trace entry that produced that pixel.
/// Returns a flat array of LCD_WIDTH * LCD_HEIGHT entries, where index = y * LCD_WIDTH + x.
/// Value of u32::MAX means no pixel was produced at that position.
pub fn build_reverse_pixel_map(
    store: &dyn TraceStore,
    frame_start: usize,
    frame_end: usize,
) -> Vec<u32> {
    let mut rmap = vec![u32::MAX; LCD_WIDTH * LCD_HEIGHT];

    let pix_col = match store.field_col("pix") {
        Some(c) => c,
        None => return rmap,
    };

    let ly_col = store.field_col("ly");
    let use_ly = ly_col.is_some();
    let mut pixel_idx: usize = 0;
    let mut scanline_x: usize = 0;
    let mut last_ly: Option<u8> = None;

    let end = frame_end.min(store.entry_count());
    for i in frame_start..end {
        let pix_str = store.get_str(pix_col, i);
        if pix_str.is_empty() { continue; }

        // Skip full-frame dumps
        if pix_str.len() == LCD_WIDTH * LCD_HEIGHT { continue; }

        if use_ly {
            let ly = store.get_numeric(ly_col.unwrap(), i) as u8;
            if last_ly != Some(ly) {
                scanline_x = 0;
                last_ly = Some(ly);
            }
            let y = ly as usize;
            for ch in pix_str.bytes() {
                if ch >= b'0' && ch <= b'3' && y < LCD_HEIGHT && scanline_x < LCD_WIDTH {
                    rmap[y * LCD_WIDTH + scanline_x] = i as u32;
                    scanline_x += 1;
                }
            }
        } else {
            for ch in pix_str.bytes() {
                if ch >= b'0' && ch <= b'3' {
                    let x = pixel_idx % LCD_WIDTH;
                    let y = pixel_idx / LCD_WIDTH;
                    if y < LCD_HEIGHT {
                        rmap[y * LCD_WIDTH + x] = i as u32;
                    }
                    pixel_idx += 1;
                }
            }
        }
    }

    rmap
}

/// Build a reverse pixel map for a downsampled store.
/// Operates on the inner (full-resolution) store but returns downsampled indices.
pub fn build_reverse_pixel_map_downsampled(
    ds: &DownsampledStore,
    frame_start: usize,
    frame_end: usize,
) -> Vec<u32> {
    // Get the raw frame range from the inner store
    let inner = ds.inner();
    let raw_start = ds.original_index(frame_start).unwrap_or(0);
    let raw_end = if frame_end < ds.entry_count() {
        ds.original_index(frame_end).unwrap_or(inner.entry_count())
    } else {
        inner.entry_count()
    };

    // Build the raw reverse map
    let raw_rmap = build_reverse_pixel_map(inner, raw_start, raw_end);

    // Remap raw indices to downsampled indices
    raw_rmap.iter().map(|&raw_idx| {
        if raw_idx == u32::MAX { return u32::MAX; }
        ds.downsampled_index(raw_idx as usize)
            .map(|di| di as u32)
            .unwrap_or(u32::MAX)
    }).collect()
}

/// Build a pixel position map for a downsampled store.
/// Operates on the inner (full-resolution) store but returns a map indexed
/// by downsampled entry offset within the frame.
pub fn build_pixel_position_map_downsampled(
    ds: &DownsampledStore,
    frame_start: usize,
    frame_end: usize,
) -> Vec<(u16, u16)> {
    let raw_start = ds.original_index(frame_start).unwrap_or(0);
    let raw_end = if frame_end < ds.entry_count() {
        ds.original_index(frame_end).unwrap_or(ds.inner().entry_count())
    } else {
        ds.inner().entry_count()
    };

    // Build the raw map (indexed by raw entry offset)
    let raw_map = build_pixel_position_map(ds.inner(), raw_start, raw_end);

    // Remap: for each downsampled entry in [frame_start..frame_end],
    // find the last pixel position from any of its T-cycles
    let ds_count = frame_end.saturating_sub(frame_start);
    let mut result = vec![(0xFFFFu16, 0xFFFFu16); ds_count];

    for (raw_offset, &pos) in raw_map.iter().enumerate() {
        if pos.0 == 0xFFFF { continue; }
        let raw_idx = raw_start + raw_offset;
        if let Some(di) = ds.downsampled_index(raw_idx) {
            if di >= frame_start && di < frame_end {
                result[di - frame_start] = pos;
            }
        }
    }

    result
}
