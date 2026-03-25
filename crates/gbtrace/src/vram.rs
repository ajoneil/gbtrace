//! VRAM state reconstruction from trace write data.
//!
//! Replays `vram_addr` / `vram_data` fields to reconstruct the 8KB VRAM
//! (0x8000-0x9FFF) at any point in the trace. Caches snapshots at frame
//! boundaries for fast scrubbing.

use crate::column_store::TraceStore;

pub const VRAM_SIZE: usize = 8192;
pub const VRAM_BASE: u16 = 0x8000;

/// Reconstructed VRAM state at a specific trace entry.
#[derive(Clone)]
pub struct VramSnapshot {
    pub data: [u8; VRAM_SIZE],
    /// The trace entry this snapshot is valid at (inclusive).
    pub entry: usize,
}

impl VramSnapshot {
    pub fn new() -> Self {
        Self {
            data: [0; VRAM_SIZE],
            entry: 0,
        }
    }

    /// Read a byte from the VRAM snapshot.
    pub fn read(&self, addr: u16) -> u8 {
        let offset = (addr.wrapping_sub(VRAM_BASE)) as usize;
        if offset < VRAM_SIZE {
            self.data[offset]
        } else {
            0
        }
    }
}

/// Cache of VRAM snapshots at frame boundaries for fast random access.
pub struct VramCache {
    /// Snapshots at frame boundaries + periodic intervals.
    checkpoints: Vec<VramSnapshot>,
    /// Most recently reconstructed snapshot — used as a cursor for
    /// sequential access (scrubbing forward replays from here).
    last_result: Option<VramSnapshot>,
}

impl VramCache {
    /// Interval between periodic checkpoints (in entries).
    /// Smaller = more memory but faster random access.
    const CHECKPOINT_INTERVAL: usize = 4096;

    /// Build a VRAM cache from a trace store, creating checkpoints at each
    /// frame boundary and at regular intervals. Scans the entire trace once.
    pub fn build(store: &dyn TraceStore) -> Option<Self> {
        let addr_col = store.field_col("vram_addr")?;
        let data_col = store.field_col("vram_data")?;

        let boundaries = store.frame_boundaries();
        let total = store.entry_count();

        let mut vram = VramSnapshot::new();
        let mut checkpoints = Vec::new();

        // Checkpoint at entry 0
        checkpoints.push(vram.clone());

        let mut next_boundary_idx = 0;
        let mut last_checkpoint_entry = 0usize;

        for i in 0..total {
            // Checkpoint at frame boundaries
            while next_boundary_idx < boundaries.len()
                && boundaries[next_boundary_idx] as usize == i
            {
                let mut snap = vram.clone();
                snap.entry = i;
                checkpoints.push(snap);
                last_checkpoint_entry = i;
                next_boundary_idx += 1;
            }

            // Periodic checkpoint every CHECKPOINT_INTERVAL entries
            if i > 0 && i - last_checkpoint_entry >= Self::CHECKPOINT_INTERVAL {
                let mut snap = vram.clone();
                snap.entry = i;
                checkpoints.push(snap);
                last_checkpoint_entry = i;
            }

            // Apply write
            let addr = store.get_numeric(addr_col, i) as u16;
            if addr >= VRAM_BASE && addr < VRAM_BASE + VRAM_SIZE as u16 {
                let data = store.get_numeric(data_col, i) as u8;
                vram.data[(addr - VRAM_BASE) as usize] = data;
            }
        }

        // Final snapshot
        vram.entry = total;
        checkpoints.push(vram);

        // Sort by entry for binary search
        checkpoints.sort_by_key(|cp| cp.entry);
        checkpoints.dedup_by_key(|cp| cp.entry);

        Some(Self { checkpoints, last_result: None })
    }

    /// Reconstruct VRAM state at a specific entry index.
    ///
    /// Uses a three-tier strategy for fast access:
    /// 1. If the last result is at this exact entry, return it (free)
    /// 2. If the last result is behind the target, replay forward from it
    /// 3. Otherwise, replay forward from the nearest earlier checkpoint
    ///
    /// This makes sequential scrubbing (the common case) nearly free.
    pub fn at_entry(&mut self, store: &dyn TraceStore, entry: usize) -> Option<VramSnapshot> {
        // Fast path: exact hit
        if let Some(ref last) = self.last_result {
            if last.entry == entry {
                return Some(last.clone());
            }
        }

        let addr_col = store.field_col("vram_addr")?;
        let data_col = store.field_col("vram_data")?;

        // Pick the best starting point: last_result (if ahead of it) or checkpoint
        let mut vram = if let Some(ref last) = self.last_result {
            if last.entry <= entry {
                // Last result is behind target — replay forward from it
                last.clone()
            } else {
                // Going backwards — use checkpoint
                self.nearest_checkpoint(entry)
            }
        } else {
            self.nearest_checkpoint(entry)
        };

        // Replay writes from current position to target
        let start = vram.entry;
        let end = entry.min(store.entry_count());
        for i in start..end {
            let addr = store.get_numeric(addr_col, i) as u16;
            if addr >= VRAM_BASE && addr < VRAM_BASE + VRAM_SIZE as u16 {
                let data = store.get_numeric(data_col, i) as u8;
                vram.data[(addr - VRAM_BASE) as usize] = data;
            }
        }

        vram.entry = entry;
        self.last_result = Some(vram.clone());
        Some(vram)
    }

    fn nearest_checkpoint(&self, entry: usize) -> VramSnapshot {
        let idx = self.checkpoints.partition_point(|cp| cp.entry <= entry);
        let idx = if idx > 0 { idx - 1 } else { 0 };
        self.checkpoints[idx].clone()
    }

    /// Number of checkpoints (roughly = number of frames + 1).
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }
}

// --- Tile rendering helpers ---

/// Tile dimensions
pub const TILE_WIDTH: usize = 8;
pub const TILE_HEIGHT: usize = 8;
pub const TILES_PER_ROW: usize = 16;
pub const TILE_COUNT: usize = 384;

/// Decode a single 8x8 tile from VRAM data.
/// Returns 64 palette indices (0-3), row-major.
pub fn decode_tile(vram: &[u8; VRAM_SIZE], tile_index: usize) -> [u8; 64] {
    let mut pixels = [0u8; 64];
    let base = tile_index * 16; // 16 bytes per tile
    if base + 16 > VRAM_SIZE { return pixels; }

    for row in 0..8 {
        let lo = vram[base + row * 2];
        let hi = vram[base + row * 2 + 1];
        for col in 0..8 {
            let bit = 7 - col;
            let color = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
            pixels[row * 8 + col] = color;
        }
    }
    pixels
}

/// Render all 384 tiles as a 16×24 grid of 8×8 tiles.
/// Returns RGBA data (128×192×4 = 98304 bytes).
pub fn render_tile_sheet(vram: &[u8; VRAM_SIZE], palette: &[(u8, u8, u8); 4]) -> Vec<u8> {
    let width = TILES_PER_ROW * TILE_WIDTH;   // 128
    let height = (TILE_COUNT / TILES_PER_ROW) * TILE_HEIGHT; // 192
    let mut rgba = vec![0u8; width * height * 4];

    for tile_idx in 0..TILE_COUNT {
        let pixels = decode_tile(vram, tile_idx);
        let tile_x = (tile_idx % TILES_PER_ROW) * TILE_WIDTH;
        let tile_y = (tile_idx / TILES_PER_ROW) * TILE_HEIGHT;

        for py in 0..TILE_HEIGHT {
            for px in 0..TILE_WIDTH {
                let color_idx = pixels[py * TILE_WIDTH + px] as usize;
                let (r, g, b) = palette[color_idx.min(3)];
                let x = tile_x + px;
                let y = tile_y + py;
                let off = (y * width + x) * 4;
                rgba[off] = r;
                rgba[off + 1] = g;
                rgba[off + 2] = b;
                rgba[off + 3] = 0xFF;
            }
        }
    }

    rgba
}

/// Render the 32×32 background tilemap as a 256×256 pixel image.
/// `tilemap_base`: 0x1800 for BG map at 0x9800, 0x1C00 for 0x9C00.
/// `tile_data_mode`: if true, tiles are unsigned (0x8000 base),
///                   if false, signed (0x8800/0x9000 base).
pub fn render_tilemap(
    vram: &[u8; VRAM_SIZE],
    tilemap_base: usize,
    signed_addressing: bool,
    palette: &[(u8, u8, u8); 4],
) -> Vec<u8> {
    let width = 256;
    let height = 256;
    let mut rgba = vec![0u8; width * height * 4];

    for map_y in 0..32 {
        for map_x in 0..32 {
            let tile_idx_raw = vram[tilemap_base + map_y * 32 + map_x];
            let tile_idx = if signed_addressing {
                // Signed: 0x80-0xFF map to tiles 0-127 (at 0x8800),
                //         0x00-0x7F map to tiles 128-255 (at 0x9000)
                ((tile_idx_raw as i8 as i16) + 128) as usize + 128
            } else {
                tile_idx_raw as usize
            };

            let pixels = decode_tile(vram, tile_idx);
            let base_x = map_x * 8;
            let base_y = map_y * 8;

            for py in 0..8 {
                for px in 0..8 {
                    let color_idx = pixels[py * 8 + px] as usize;
                    let (r, g, b) = palette[color_idx.min(3)];
                    let x = base_x + px;
                    let y = base_y + py;
                    let off = (y * width + x) * 4;
                    rgba[off] = r;
                    rgba[off + 1] = g;
                    rgba[off + 2] = b;
                    rgba[off + 3] = 0xFF;
                }
            }
        }
    }

    rgba
}
