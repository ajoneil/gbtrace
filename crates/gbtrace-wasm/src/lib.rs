use gbtrace::column_store::ColumnStore;
use gbtrace::disasm;
use gbtrace::framebuffer;
use gbtrace::profile::FieldType;
use wasm_bindgen::prelude::*;

use std::collections::BTreeMap;

/// Serializable entry type for JS interop.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum JsField {
    Num(f64),
    Bool(bool),
}

/// Serialize to JS using plain objects (not Maps) for BTreeMap keys.
fn to_js(value: &impl serde::Serialize) -> Result<JsValue, JsError> {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    Ok(value.serialize(&serializer)?)
}

/// In-memory trace store for the browser.
///
/// Parquet files are loaded lazily — only a few row groups (frames) are
/// decoded at a time. JSONL files and post-diff stores are loaded eagerly.
#[wasm_bindgen]
pub struct TraceStore {
    store: Box<dyn gbtrace::column_store::TraceStore>,
    rom: Option<Vec<u8>>,
    /// Original bytes for re-loading when sync changes.
    original_bytes: Option<Vec<u8>>,
    /// Optional downsampling index: maps downsampled row → original row.
    /// When set, all accessors use this mapping transparently.
    downsample_map: Option<Vec<usize>>,
    /// Cached VRAM reconstruction state (built lazily on first access).
    vram_cache: Option<gbtrace::vram::VramCache>,
}

#[wasm_bindgen]
impl TraceStore {
    /// Load a trace from raw bytes (detects format automatically).
    /// Supports native .gbtrace, legacy parquet, and JSONL.
    #[wasm_bindgen(constructor)]
    pub fn from_bytes(data: &[u8]) -> Result<TraceStore, JsError> {
        let store = gbtrace::column_store::open_trace_store_from_bytes(data)
            .map_err(|e| JsError::new(&format!("{e}")))?;
        Ok(TraceStore { store, rom: None, original_bytes: Some(data.to_vec()), downsample_map: None, vram_cache: None })
    }

    /// Enable instruction-level downsampling. Picks one entry per PC change.
    /// Call this to compare a T-cycle trace against an instruction-level one.
    /// The downsampled view is transparent to all other methods.
    #[wasm_bindgen(js_name = enableDownsampling)]
    pub fn enable_downsampling(&mut self) {
        let store = &*self.store;
        let pc_col = store.field_col("pc");
        let mut map = Vec::new();
        if let Some(pc) = pc_col {
            let count = store.entry_count();
            if count > 0 {
                map.push(0);
                let mut prev_pc = store.get_numeric(pc, 0);
                for i in 1..count {
                    let cur_pc = store.get_numeric(pc, i);
                    if cur_pc != prev_pc {
                        map.push(i);
                        prev_pc = cur_pc;
                    }
                }
            }
        }
        self.downsample_map = if map.is_empty() { None } else { Some(map) };
    }

    /// Disable downsampling, restoring the full-resolution view.
    #[wasm_bindgen(js_name = disableDownsampling)]
    pub fn disable_downsampling(&mut self) {
        self.downsample_map = None;
    }

    /// Whether this store is currently downsampled.
    #[wasm_bindgen(js_name = isDownsampled)]
    pub fn is_downsampled(&self) -> bool {
        self.downsample_map.is_some()
    }

    /// Return the trace header as a JS object.
    pub fn header(&self) -> Result<JsValue, JsError> {
        Ok(to_js(self.store.header())?)
    }

    /// Number of entries in the trace.
    #[wasm_bindgen(js_name = entryCount)]
    pub fn entry_count(&self) -> usize {
        self.effective_entry_count()
    }

    /// Get frame boundary entry indices as a Uint32Array.
    ///
    /// Frame boundary entry indices. Uses explicit boundaries from parquet
    /// metadata when available, otherwise falls back to reconstruct_frames.
    #[wasm_bindgen(js_name = frameBoundaries)]
    pub fn frame_boundaries(&self) -> js_sys::Uint32Array {
        let orig_boundaries = self.store.frame_boundaries();

        let boundaries = if let Some(ref map) = self.downsample_map {
            // Map original boundaries to downsampled indices
            orig_boundaries.iter().filter_map(|&orig_entry| {
                match map.binary_search(&(orig_entry as usize)) {
                    Ok(i) => Some(i as u32),
                    Err(i) if i < map.len() => Some(i as u32),
                    _ => None,
                }
            }).collect()
        } else {
            orig_boundaries
        };

        let arr = js_sys::Uint32Array::new_with_length(boundaries.len() as u32);
        arr.copy_from(&boundaries);
        arr
    }

    /// Get the field names from the header (excludes internal fields like `pix`).
    #[wasm_bindgen(js_name = fieldNames)]
    pub fn field_names(&self) -> Result<JsValue, JsError> {
        let fields = &self.store.header().fields;
        let filtered: Vec<&String> = fields.iter().filter(|f| f.as_str() != "pix").collect();
        Ok(to_js(&filtered)?)
    }

    /// Whether this trace has pixel data (a `pix` column).
    #[wasm_bindgen(js_name = hasPixels)]
    pub fn has_pixels(&self) -> bool {
        self.store.has_field("pix")
    }

    /// Whether this trace has per-entry pixel data (not full-frame dumps).
    /// Returns true even when downsampled, since the underlying data has pixels.
    #[wasm_bindgen(js_name = hasPerEntryPixels)]
    pub fn has_per_entry_pixels(&self) -> bool {
        if !self.store.has_field("pix") { return false; }
        self.store.header().trigger == gbtrace::header::Trigger::Tcycle
    }

    /// Number of reconstructed pixel frames.
    #[wasm_bindgen(js_name = frameCount)]
    pub fn frame_count(&self) -> usize {
        self.store.frame_boundaries().len()
    }

    /// Render a complete frame as RGBA pixel data (160×144×4 = 92160 bytes).
    /// The library handles all internal decoding transparently.
    #[wasm_bindgen(js_name = renderFrame)]
    pub fn render_frame(&self, frame_index: usize) -> Result<JsValue, JsError> {
        let (start, end) = match self.frame_entry_range(frame_index) {
            Some(r) => r,
            None => return Ok(JsValue::NULL),
        };
        let frame = if let Some(ref ds_map) = self.downsample_map {
            let ds = gbtrace::downsample::DownsampledStore::from_map(&*self.store, ds_map.clone());
            framebuffer::reconstruct_partial_frame_downsampled(&ds, start, end)
        } else {
            framebuffer::reconstruct_partial_frame(&*self.store, start, end)
        };
        Ok(js_sys::Uint8ClampedArray::from(&frame.to_rgba()[..]).into())
    }

    /// Render a complete frame as raw palette indices (160×144 = 23040 bytes).
    /// Values 0-3 are palette indices, 0xFF is unrendered.
    /// Used for pixel-level diffing without palette conversion.
    #[wasm_bindgen(js_name = renderFrameRaw)]
    pub fn render_frame_raw(&self, frame_index: usize) -> Result<JsValue, JsError> {
        let (start, end) = match self.frame_entry_range(frame_index) {
            Some(r) => r,
            None => return Ok(JsValue::NULL),
        };
        let frame = if let Some(ref ds_map) = self.downsample_map {
            let ds = gbtrace::downsample::DownsampledStore::from_map(&*self.store, ds_map.clone());
            framebuffer::reconstruct_partial_frame_downsampled(&ds, start, end)
        } else {
            framebuffer::reconstruct_partial_frame(&*self.store, start, end)
        };
        Ok(js_sys::Uint8Array::from(&frame.pixels[..]).into())
    }

    /// Render a partial frame up to `stop_entry` as RGBA pixel data.
    /// Used for the progressive scrubber in T-cycle traces.
    /// The library handles all internal decoding transparently.
    #[wasm_bindgen(js_name = renderPartialFrame)]
    pub fn render_partial_frame(&self, frame_index: usize, stop_entry: usize) -> Result<JsValue, JsError> {
        let (start, _end) = match self.frame_entry_range(frame_index) {
            Some(r) => r,
            None => return Ok(JsValue::NULL),
        };
        let frame = if let Some(ref ds_map) = self.downsample_map {
            let ds = gbtrace::downsample::DownsampledStore::from_map(&*self.store, ds_map.clone());
            framebuffer::reconstruct_partial_frame_downsampled(&ds, start, stop_entry)
        } else {
            framebuffer::reconstruct_partial_frame(&*self.store, start, stop_entry)
        };
        Ok(js_sys::Uint8ClampedArray::from(&frame.to_rgba()[..]).into())
    }

    /// Get pixel values for a range of entries as a Uint8Array.
    /// Each byte is 0-3 (pixel shade) or 255 (no pixel at this entry).
    #[wasm_bindgen(js_name = pixRange)]
    pub fn pix_range(&self, start: usize, count: usize) -> Result<JsValue, JsError> {
        if !self.store.has_field("pix") { return Ok(JsValue::NULL); }
        let mut result = vec![255u8; count];
        let end = (start + count).min(self.entry_count());
        for i in start..end {
            let pix_val = self.store.get_str_named("pix", i).unwrap_or_default();
            if pix_val.len() == 1 {
                let ch = pix_val.as_bytes()[0];
                if ch >= b'0' && ch <= b'3' {
                    result[i - start] = ch - b'0';
                }
            }
        }
        Ok(js_sys::Uint8Array::from(&result[..]).into())
    }

    /// Build a pixel position map for a frame. Returns a Uint32Array
    /// where each element is `(x << 16) | y`, or 0xFFFFFFFF for no pixel.
    #[wasm_bindgen(js_name = buildPixelPositionMap)]
    pub fn build_pixel_position_map(&self, frame_index: usize) -> Result<JsValue, JsError> {
        let (frame_start, frame_end) = match self.frame_entry_range(frame_index) {
            Some(r) => r,
            None => return Ok(JsValue::NULL),
        };
        let map = if let Some(ref ds_map) = self.downsample_map {
            let ds = gbtrace::downsample::DownsampledStore::from_map(&*self.store, ds_map.clone());
            framebuffer::build_pixel_position_map_downsampled(&ds, frame_start, frame_end)
        } else {
            framebuffer::build_pixel_position_map(&*self.store, frame_start, frame_end)
        };
        let packed: Vec<u32> = map.iter().map(|&(x, y)| {
            if x == 0xFFFF { 0xFFFFFFFF } else { ((x as u32) << 16) | (y as u32) }
        }).collect();
        let arr = js_sys::Uint32Array::new_with_length(packed.len() as u32);
        arr.copy_from(&packed);
        Ok(arr.into())
    }

    /// Build a reverse pixel map for a frame. Returns a Uint32Array of
    /// LCD_WIDTH * LCD_HEIGHT entries, where index = y * 160 + x and the value
    /// is the global entry index that produced that pixel (or 0xFFFFFFFF for none).
    #[wasm_bindgen(js_name = buildReversePixelMap)]
    pub fn build_reverse_pixel_map(&self, frame_index: usize) -> Result<JsValue, JsError> {
        let (frame_start, frame_end) = match self.frame_entry_range(frame_index) {
            Some(r) => r,
            None => return Ok(JsValue::NULL),
        };
        let rmap = if let Some(ref ds_map) = self.downsample_map {
            let ds = gbtrace::downsample::DownsampledStore::from_map(&*self.store, ds_map.clone());
            framebuffer::build_reverse_pixel_map_downsampled(&ds, frame_start, frame_end)
        } else {
            framebuffer::build_reverse_pixel_map(&*self.store, frame_start, frame_end)
        };

        let arr = js_sys::Uint32Array::new_with_length(rmap.len() as u32);
        arr.copy_from(&rmap);
        Ok(arr.into())
    }

    /// Get a single entry as a JS object. Returns null if out of range.
    pub fn entry(&self, index: usize) -> Result<JsValue, JsError> {
        if index >= self.entry_count() {
            return Ok(JsValue::NULL);
        }
        Ok(to_js(&self.row_to_map(index))?)
    }

    /// Get a range of entries as a JS array. Used for virtual scrolling.
    #[wasm_bindgen(js_name = entriesRange)]
    pub fn entries_range(&self, start: usize, count: usize) -> Result<JsValue, JsError> {
        let end = (start + count).min(self.entry_count());
        let slice: Vec<_> = (start..end).map(|i| self.row_to_map(i)).collect();
        Ok(to_js(&slice)?)
    }

    /// Parse a condition string and find all matching entry indices.
    pub fn query(&self, condition_str: &str) -> Result<js_sys::Uint32Array, JsError> {
        self.query_range(condition_str, 0, self.entry_count())
    }

    /// Find matching entry indices within a range.
    #[wasm_bindgen(js_name = queryRange)]
    pub fn query_range(&self, condition_str: &str, start: usize, end: usize) -> Result<js_sys::Uint32Array, JsError> {
        let indices = self.store.query_range(condition_str, start, end).map_err(|e| JsError::new(&e))?;
        let arr = js_sys::Uint32Array::new_with_length(indices.len() as u32);
        arr.copy_from(&indices);
        Ok(arr)
    }

    /// Downsample a field for chart display.
    #[wasm_bindgen(js_name = fieldSummary)]
    pub fn field_summary(
        &self,
        field: &str,
        start: usize,
        end: usize,
        buckets: usize,
    ) -> Result<js_sys::Float64Array, JsError> {
        let out = self.store.field_summary(field, start, end, buckets)
            .map_err(|e| JsError::new(&e))?;

        let arr = js_sys::Float64Array::new_with_length(out.len() as u32);
        arr.copy_from(&out);
        Ok(arr)
    }

    /// Compare a field between this store and another.
    #[wasm_bindgen(js_name = diffField)]
    pub fn diff_field(
        &self,
        other: &TraceStore,
        field: &str,
    ) -> Result<js_sys::Uint32Array, JsError> {
        // For diffs, ensure both stores are eager
        let len = self.entry_count().min(other.entry_count());
        let mut indices = Vec::new();

        for i in 0..len {
            let a = self.store.get_numeric_named(field, i);
            let b = other.store.get_numeric_named(field, i);
            if a != b {
                indices.push(i as u32);
            }
        }

        let arr = js_sys::Uint32Array::new_with_length(indices.len() as u32);
        arr.copy_from(&indices);
        Ok(arr)
    }

    /// Per-field diff statistics, optionally scoped to a range.
    #[wasm_bindgen(js_name = diffStatsRange)]
    pub fn diff_stats_range(&self, other: &TraceStore, start: usize, end: usize) -> Result<JsValue, JsError> {
        let max_len = self.entry_count().min(other.entry_count());
        let start = start.min(max_len);
        let end = end.min(max_len);
        let len = if end > start { end - start } else { 0 };

        let fields = self.store.header().fields.clone();

        let mut field_counts: Vec<(String, u64)> = Vec::new();
        let mut any_diff_count: usize = 0;
        let mut any_diff_flags = vec![false; len];

        for name in &fields {
            let has_a = self.store.has_field(name);
            let has_b = other.store.has_field(name);
            if !has_a || !has_b { continue; }

            let mut count = 0u64;
            for i in 0..len {
                let row = start + i;
                if self.store.get_numeric_named(name, row) != other.store.get_numeric_named(name, row) {
                    count += 1;
                    any_diff_flags[i] = true;
                }
            }
            if count > 0 {
                field_counts.push((name.clone(), count));
            }
        }

        for flag in &any_diff_flags {
            if *flag { any_diff_count += 1; }
        }

        let matching = len - any_diff_count;
        let pct = if len > 0 { (matching as f64 / len as f64) * 100.0 } else { 100.0 };

        #[derive(serde::Serialize)]
        struct Stats {
            total: usize,
            matching: usize,
            differing: usize,
            match_pct: f64,
            fields: Vec<(String, u64)>,
        }

        let stats = Stats {
            total: len,
            matching,
            differing: any_diff_count,
            match_pct: (pct * 10.0).round() / 10.0,
            fields: field_counts,
        };

        Ok(to_js(&stats)?)
    }

    /// Compare ALL fields between this store and another.
    #[wasm_bindgen(js_name = diffAll)]
    pub fn diff_all(&self, other: &TraceStore) -> Result<js_sys::Uint32Array, JsError> {
        let len = self.entry_count().min(other.entry_count());
        let fields = self.store.header().fields.clone();

        // Collect field names present in both
        let common_fields: Vec<&str> = fields.iter()
            .filter(|n| self.store.has_field(n) && other.store.has_field(n))
            .map(|n| n.as_str())
            .collect();

        let mut indices = Vec::new();
        for row in 0..len {
            for &name in &common_fields {
                if self.store.get_numeric_named(name, row) != other.store.get_numeric_named(name, row) {
                    indices.push(row as u32);
                    break;
                }
            }
        }

        let arr = js_sys::Uint32Array::new_with_length(indices.len() as u32);
        arr.copy_from(&indices);
        Ok(arr)
    }

    /// Get the original trace bytes for re-loading (e.g. when changing sync mode).
    #[wasm_bindgen(js_name = originalBytes)]
    pub fn original_bytes(&self) -> Option<js_sys::Uint8Array> {
        self.original_bytes.as_ref().map(|b| {
            let arr = js_sys::Uint8Array::new_with_length(b.len() as u32);
            arr.copy_from(b);
            arr
        })
    }

    /// Load ROM bytes for disassembly.
    #[wasm_bindgen(js_name = loadRom)]
    pub fn load_rom(&mut self, data: &[u8]) {
        self.rom = Some(data.to_vec());
    }

    /// Check if ROM is loaded.
    #[wasm_bindgen(js_name = hasRom)]
    pub fn has_rom(&self) -> bool {
        self.rom.is_some()
    }

    /// Disassemble the instruction at the given PC.
    pub fn disassemble(&self, pc: u16) -> String {
        match &self.rom {
            Some(rom) => disasm::disassemble(rom, pc).0,
            None => String::new(),
        }
    }

    /// Disassemble instructions for a range of trace entries.
    #[wasm_bindgen(js_name = disassembleRange)]
    pub fn disassemble_range(&self, start: usize, count: usize) -> Result<JsValue, JsError> {
        let rom = match &self.rom {
            Some(r) => r,
            None => return Ok(to_js(&Vec::<String>::new())?),
        };
        let end = (start + count).min(self.entry_count());
        let mnemonics: Vec<String> = (start..end)
            .map(|i| {
                let pc = self.store.get_numeric_named("pc", i).unwrap_or(0) as u16;
                disasm::disassemble(rom, pc).0
            })
            .collect();
        Ok(to_js(&mnemonics)?)
    }
    // --- VRAM reconstruction ---

    /// Whether this trace has VRAM write tracking data.
    #[wasm_bindgen(js_name = hasVramData)]
    pub fn has_vram_data(&self) -> bool {
        self.store.has_field("vram_addr") && self.store.has_field("vram_data")
    }

    /// Build the VRAM cache (scans the entire trace once).
    /// Call this once after loading; subsequent vram methods use the cache.
    #[wasm_bindgen(js_name = buildVramCache)]
    pub fn build_vram_cache(&mut self) {
        if self.vram_cache.is_none() {
            self.vram_cache = gbtrace::vram::VramCache::build(&*self.store);
        }
    }

    /// Render the 384-tile sheet at a specific entry as RGBA (128×192×4 bytes).
    #[wasm_bindgen(js_name = renderTileSheet)]
    pub fn render_tile_sheet(&mut self, entry: usize) -> Result<JsValue, JsError> {
        let snap = match self.vram_at(entry) {
            Some(s) => s,
            None => return Ok(JsValue::NULL),
        };
        const PALETTE: [(u8, u8, u8); 4] = [
            (0xe0, 0xf8, 0xd0), (0x88, 0xc0, 0x70),
            (0x34, 0x68, 0x56), (0x08, 0x18, 0x20),
        ];
        let rgba = gbtrace::vram::render_tile_sheet(&snap.data, &PALETTE);
        Ok(js_sys::Uint8ClampedArray::from(&rgba[..]).into())
    }

    /// Render a 32×32 tilemap at a specific entry as RGBA (256×256×4 bytes).
    /// `map_select`: 0 for BG map (0x9800), 1 for window map (0x9C00).
    #[wasm_bindgen(js_name = renderTilemap)]
    pub fn render_tilemap(&mut self, entry: usize, map_select: u8) -> Result<JsValue, JsError> {
        // Read LCDC before the mutable borrow
        let mapped = self.map_row(entry);
        let lcdc = self.store.get_numeric(
            self.store.field_col("lcdc").unwrap_or(0), mapped
        ) as u8;

        let snap = match self.vram_at(entry) {
            Some(s) => s,
            None => return Ok(JsValue::NULL),
        };

        let signed_addressing = (lcdc & 0x10) == 0;
        let tilemap_base = if map_select == 0 {
            if (lcdc & 0x08) != 0 { 0x1C00 } else { 0x1800 }
        } else {
            if (lcdc & 0x40) != 0 { 0x1C00 } else { 0x1800 }
        };

        const PALETTE: [(u8, u8, u8); 4] = [
            (0xe0, 0xf8, 0xd0), (0x88, 0xc0, 0x70),
            (0x34, 0x68, 0x56), (0x08, 0x18, 0x20),
        ];
        let rgba = gbtrace::vram::render_tilemap(&snap.data, tilemap_base, signed_addressing, &PALETTE);
        Ok(js_sys::Uint8ClampedArray::from(&rgba[..]).into())
    }

    /// Get raw VRAM bytes at a specific entry (8192 bytes).
    #[wasm_bindgen(js_name = getVramAt)]
    pub fn get_vram_at(&mut self, entry: usize) -> Result<JsValue, JsError> {
        let snap = match self.vram_at(entry) {
            Some(s) => s,
            None => return Ok(JsValue::NULL),
        };
        Ok(js_sys::Uint8Array::from(&snap.data[..]).into())
    }
}

// Private helpers
impl TraceStore {
    /// Map a row index through the downsample map (if any).
    fn map_row(&self, row: usize) -> usize {
        match &self.downsample_map {
            Some(map) => map.get(row).copied().unwrap_or(row),
            None => row,
        }
    }

    /// Entry count respecting downsampling.
    /// Reconstruct VRAM at an entry, handling borrow splitting between
    /// the mutable cache and immutable store.
    fn vram_at(&mut self, entry: usize) -> Option<gbtrace::vram::VramSnapshot> {
        self.build_vram_cache();
        let entry = self.map_row(entry);
        // Split borrows: take cache out, use store, put cache back
        let mut cache = self.vram_cache.take()?;
        let result = cache.at_entry(&*self.store, entry);
        self.vram_cache = Some(cache);
        result
    }

    fn effective_entry_count(&self) -> usize {
        match &self.downsample_map {
            Some(map) => map.len(),
            None => self.store.entry_count(),
        }
    }

    /// Get the entry range (start, end) for a frame by index.
    fn frame_entry_range(&self, frame_index: usize) -> Option<(usize, usize)> {
        let boundaries = self.store.frame_boundaries();
        if frame_index >= boundaries.len() {
            return None;
        }
        let start = boundaries[frame_index] as usize;
        let end = if frame_index + 1 < boundaries.len() {
            boundaries[frame_index + 1] as usize
        } else {
            self.store.entry_count()
        };
        Some((start, end))
    }

    fn row_to_map(&self, index: usize) -> BTreeMap<String, JsField> {
        let store = &*self.store;
        let orig_row = self.map_row(index);
        let fields = store.header().fields.clone();
        let mut map = BTreeMap::new();

        for (col_idx, field_name) in fields.iter().enumerate() {
            // Skip null values — absent keys in JS mean "no data"
            if store.is_null(col_idx, orig_row) {
                continue;
            }

            let ft = gbtrace::profile::field_type(field_name);
            let val = match ft {
                FieldType::Bool => JsField::Bool(store.get_bool(col_idx, orig_row)),
                FieldType::Str => {
                    let s = store.get_str(col_idx, orig_row);
                    if s.len() == 1 {
                        let ch = s.as_bytes()[0];
                        if ch >= b'0' && ch <= b'3' {
                            JsField::Num((ch - b'0') as f64)
                        } else {
                            continue;
                        }
                    } else {
                        continue; // skip multi-char strings (full-frame dumps)
                    }
                }
                _ => JsField::Num(store.get_numeric(col_idx, orig_row) as f64),
            };
            map.insert(field_name.clone(), val);
        }
        map
    }
}

/// Helper: load bytes as an eager ColumnStore (for diff preparation).
/// TODO: refactor diff to work with GbtraceStore directly.
fn load_eager_from_bytes(data: &[u8]) -> Result<ColumnStore, JsError> {
    const PARQUET_MAGIC: &[u8] = b"PAR1";
    if data.len() >= 4 && &data[..4] == PARQUET_MAGIC {
        let ps = gbtrace::column_store::load_partitioned_store_from_bytes(data)
            .map_err(|e| JsError::new(&format!("{e}")))?;
        ps.to_eager().map_err(|e| JsError::new(&format!("{e}")))
    } else {
        gbtrace::column_store::load_column_store_from_bytes(data)
            .map_err(|e| JsError::new(&format!("{e}")))
    }
}

/// Prepare two TraceStores for comparison with a sync condition.
///
/// Sync modes: "pc" (default), "none", or any condition string like "ly=0", "lcdc&80".
#[wasm_bindgen(js_name = prepareForDiff)]
pub fn prepare_for_diff(a: TraceStore, b: TraceStore, sync: Option<String>) -> Result<js_sys::Array, JsError> {
    let rom_a = a.rom;
    let rom_b = b.rom;
    let bytes_a = a.original_bytes;
    let bytes_b = b.original_bytes;

    // Reload as eager stores from original bytes for diff preparation
    let store_a = match &bytes_a {
        Some(data) => load_eager_from_bytes(data)?,
        None => return Err(JsError::new("store A has no original bytes for diff reload")),
    };
    let store_b = match &bytes_b {
        Some(data) => load_eager_from_bytes(data)?,
        None => return Err(JsError::new("store B has no original bytes for diff reload")),
    };

    let sync_str = sync.as_deref();
    let (new_a, new_b) = ColumnStore::prepare_for_diff_with_sync(store_a, store_b, sync_str)
        .map_err(|e| JsError::new(&format!("{e}")))?;

    let arr = js_sys::Array::new();
    arr.push(&JsValue::from(TraceStore { store: Box::new(new_a), rom: rom_a, original_bytes: bytes_a, downsample_map: None, vram_cache: None }));
    arr.push(&JsValue::from(TraceStore { store: Box::new(new_b), rom: rom_b, original_bytes: bytes_b, downsample_map: None, vram_cache: None }));
    Ok(arr)
}
