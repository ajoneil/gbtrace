use gbtrace::column_store::{ColumnData, ColumnStore, LazyColumnStore};
use gbtrace::disasm;
use gbtrace::framebuffer::{self, Frame};
use gbtrace::profile::FieldType;
use wasm_bindgen::prelude::*;

use std::cell::RefCell;
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

/// Either a lazy (on-demand row group) or eager (fully decoded) store.
enum StoreKind {
    Lazy(LazyColumnStore),
    Eager(ColumnStore),
}

/// In-memory trace store for the browser.
///
/// Parquet files are loaded lazily — only a few row groups (frames) are
/// decoded at a time. JSONL files and post-diff stores are loaded eagerly.
#[wasm_bindgen]
pub struct TraceStore {
    store: StoreKind,
    rom: Option<Vec<u8>>,
    /// Original bytes for re-loading when sync changes.
    original_bytes: Option<Vec<u8>>,
    /// Cached reconstructed frames (lazily populated).
    frames_cache: RefCell<Option<Vec<Frame>>>,
}

#[wasm_bindgen]
impl TraceStore {
    /// Load a trace from raw bytes (detects format automatically).
    #[wasm_bindgen(constructor)]
    pub fn from_bytes(data: &[u8]) -> Result<TraceStore, JsError> {
        const PARQUET_MAGIC: &[u8] = b"PAR1";

        let store = if data.len() >= 4 && &data[..4] == PARQUET_MAGIC {
            StoreKind::Lazy(
                gbtrace::column_store::load_lazy_column_store_from_bytes(data)
                    .map_err(|e| JsError::new(&format!("{e}")))?
            )
        } else {
            StoreKind::Eager(
                gbtrace::column_store::load_column_store_from_bytes(data)
                    .map_err(|e| JsError::new(&format!("{e}")))?
            )
        };
        Ok(TraceStore { store, rom: None, original_bytes: Some(data.to_vec()), frames_cache: RefCell::new(None) })
    }

    /// Return the trace header as a JS object.
    pub fn header(&self) -> Result<JsValue, JsError> {
        let h = match &self.store {
            StoreKind::Lazy(s) => s.header(),
            StoreKind::Eager(s) => s.header(),
        };
        Ok(to_js(h)?)
    }

    /// Number of entries in the trace.
    #[wasm_bindgen(js_name = entryCount)]
    pub fn entry_count(&self) -> usize {
        match &self.store {
            StoreKind::Lazy(s) => s.entry_count(),
            StoreKind::Eager(s) => s.entry_count(),
        }
    }

    /// Get frame boundary entry indices as a Uint32Array.
    ///
    /// Frame boundary entry indices. Uses explicit boundaries from parquet
    /// metadata when available, otherwise falls back to reconstruct_frames.
    #[wasm_bindgen(js_name = frameBoundaries)]
    pub fn frame_boundaries(&self) -> js_sys::Uint32Array {
        // Check for explicit boundaries from the store first
        let explicit = match &self.store {
            StoreKind::Lazy(s) => s.frame_boundaries(),
            StoreKind::Eager(s) => s.frame_boundaries(),
        };
        if !explicit.is_empty() {
            let arr = js_sys::Uint32Array::new_with_length(explicit.len() as u32);
            arr.copy_from(&explicit);
            return arr;
        }

        // Fallback to reconstruct_frames
        self.ensure_frames();
        let cache = self.frames_cache.borrow();
        let boundaries: Vec<u32> = cache.as_ref()
            .map(|frames| frames.iter().map(|f| f.start_entry as u32).collect())
            .unwrap_or_default();
        let arr = js_sys::Uint32Array::new_with_length(boundaries.len() as u32);
        arr.copy_from(&boundaries);
        arr
    }

    /// Get the field names from the header (excludes internal fields like `pix`).
    #[wasm_bindgen(js_name = fieldNames)]
    pub fn field_names(&self) -> Result<JsValue, JsError> {
        let fields = match &self.store {
            StoreKind::Lazy(s) => &s.header().fields,
            StoreKind::Eager(s) => &s.header().fields,
        };
        let filtered: Vec<&String> = fields.iter().filter(|f| f.as_str() != "pix").collect();
        Ok(to_js(&filtered)?)
    }

    /// Whether this trace has pixel data (a `pix` column).
    #[wasm_bindgen(js_name = hasPixels)]
    pub fn has_pixels(&self) -> bool {
        self.has_field("pix")
    }

    /// Whether this is a T-cycle level trace with per-pixel pix data.
    #[wasm_bindgen(js_name = isTcyclePixels)]
    pub fn is_tcycle_pixels(&self) -> bool {
        if !self.has_field("pix") { return false; }
        let header = match &self.store {
            StoreKind::Lazy(s) => s.header(),
            StoreKind::Eager(s) => s.header(),
        };
        header.trigger == gbtrace::header::Trigger::Tcycle
    }

    /// Number of reconstructed pixel frames.
    #[wasm_bindgen(js_name = frameCount)]
    pub fn frame_count(&self) -> usize {
        let store: &dyn gbtrace::column_store::TraceStore = match &self.store {
            StoreKind::Lazy(s) => s,
            StoreKind::Eager(s) => s,
        };
        store.frame_boundaries().len()
    }

    /// Render a complete frame as RGBA pixel data (160×144×4 = 92160 bytes).
    /// The library handles all internal decoding transparently.
    #[wasm_bindgen(js_name = renderFrame)]
    pub fn render_frame(&self, frame_index: usize) -> Result<JsValue, JsError> {
        let (start, end) = match self.frame_entry_range(frame_index) {
            Some(r) => r,
            None => return Ok(JsValue::NULL),
        };
        let store: &dyn gbtrace::column_store::TraceStore = match &self.store {
            StoreKind::Lazy(s) => s,
            StoreKind::Eager(s) => s,
        };
        let frame = framebuffer::reconstruct_partial_frame(store, start, end);
        Ok(js_sys::Uint8ClampedArray::from(&frame.to_rgba()[..]).into())
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
        let store: &dyn gbtrace::column_store::TraceStore = match &self.store {
            StoreKind::Lazy(s) => s,
            StoreKind::Eager(s) => s,
        };
        let frame = framebuffer::reconstruct_partial_frame(store, start, stop_entry);
        Ok(js_sys::Uint8ClampedArray::from(&frame.to_rgba()[..]).into())
    }

    /// Get pixel values for a range of entries as a Uint8Array.
    /// Each byte is 0-3 (pixel shade) or 255 (no pixel at this entry).
    #[wasm_bindgen(js_name = pixRange)]
    pub fn pix_range(&self, start: usize, count: usize) -> Result<JsValue, JsError> {
        if !self.has_field("pix") { return Ok(JsValue::NULL); }
        let mut result = vec![255u8; count];
        let end = (start + count).min(self.entry_count());
        for i in start..end {
            let pix_val = match &self.store {
                StoreKind::Eager(s) => s.get_str_named("pix", i).unwrap_or("").to_string(),
                StoreKind::Lazy(s) => s.get_str_named("pix", i).unwrap_or_default(),
            };
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
        let store: &dyn gbtrace::column_store::TraceStore = match &self.store {
            StoreKind::Lazy(s) => s,
            StoreKind::Eager(s) => s,
        };
        let map = framebuffer::build_pixel_position_map(store, frame_start, frame_end);
        let packed: Vec<u32> = map.iter().map(|&(x, y)| {
            if x == 0xFFFF { 0xFFFFFFFF } else { ((x as u32) << 16) | (y as u32) }
        }).collect();
        let arr = js_sys::Uint32Array::new_with_length(packed.len() as u32);
        arr.copy_from(&packed);
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
        let indices = match &self.store {
            StoreKind::Lazy(s) => s.query_range(condition_str, start, end).map_err(|e| JsError::new(&e))?,
            StoreKind::Eager(s) => s.query_range(condition_str, start, end).map_err(|e| JsError::new(&e))?,
        };
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
        let out = match &self.store {
            StoreKind::Lazy(s) => {
                s.field_summary(field, start, end, buckets)
                    .map_err(|e| JsError::new(&e))?
            }
            StoreKind::Eager(s) => {
                let col_idx = s.field_col(field)
                    .ok_or_else(|| JsError::new(&format!("unknown field: {field}")))?;
                let col = s.column(col_idx);
                let total = s.entry_count();
                let end = end.min(total);
                let start = start.min(end);
                let range = end - start;

                if range == 0 || buckets == 0 {
                    Vec::new()
                } else {
                    let mut out = Vec::with_capacity(buckets * 2);
                    for b in 0..buckets {
                        let b_start = start + (b * range) / buckets;
                        let b_end = start + ((b + 1) * range) / buckets;
                        if b_start >= b_end {
                            let v = if b_start > 0 {
                                col.get_numeric(b_start.min(total - 1)) as f64
                            } else { 0.0 };
                            out.push(v);
                            out.push(v);
                            continue;
                        }
                        let mut min = f64::MAX;
                        let mut max = f64::MIN;
                        for i in b_start..b_end {
                            let v = col.get_numeric(i) as f64;
                            if v < min { min = v; }
                            if v > max { max = v; }
                        }
                        out.push(min);
                        out.push(max);
                    }
                    out
                }
            }
        };

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
            let a = self.get_numeric_named(field, i);
            let b = other.get_numeric_named(field, i);
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

        let fields = match &self.store {
            StoreKind::Lazy(s) => s.header().fields.clone(),
            StoreKind::Eager(s) => s.header().fields.clone(),
        };

        let mut field_counts: Vec<(String, u64)> = Vec::new();
        let mut any_diff_count: usize = 0;
        let mut any_diff_flags = vec![false; len];

        for name in &fields {
            let has_a = self.has_field(name);
            let has_b = other.has_field(name);
            if !has_a || !has_b { continue; }

            let mut count = 0u64;
            for i in 0..len {
                let row = start + i;
                if self.get_numeric_named(name, row) != other.get_numeric_named(name, row) {
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
        let fields = match &self.store {
            StoreKind::Lazy(s) => s.header().fields.clone(),
            StoreKind::Eager(s) => s.header().fields.clone(),
        };

        // Collect field names present in both
        let common_fields: Vec<&str> = fields.iter()
            .filter(|n| self.has_field(n) && other.has_field(n))
            .map(|n| n.as_str())
            .collect();

        let mut indices = Vec::new();
        for row in 0..len {
            for &name in &common_fields {
                if self.get_numeric_named(name, row) != other.get_numeric_named(name, row) {
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
                let pc = match &self.store {
                    StoreKind::Lazy(s) => s.get_u16_named("pc", i).unwrap_or(0),
                    StoreKind::Eager(s) => s.get_u16_named("pc", i).unwrap_or(0),
                };
                disasm::disassemble(rom, pc).0
            })
            .collect();
        Ok(to_js(&mnemonics)?)
    }
}

// Private helpers
impl TraceStore {
    /// Lazily reconstruct frames and cache the result.
    ///
    /// If the current store yields no frames (e.g. after T-cycle collapse
    /// strips pixel data), falls back to reconstructing from the original
    /// bytes so pixel display still works in comparison mode.
    fn ensure_frames(&self) {
        if self.frames_cache.borrow().is_some() {
            return;
        }
        let frames = self.reconstruct_from_store();
        if Self::has_visible_pixels(&frames) {
            *self.frames_cache.borrow_mut() = Some(frames);
            return;
        }
        // Current store has no visible pixel frames (e.g. after T-cycle
        // collapse stripped pix data) — reconstruct from original bytes.
        if let Some(ref bytes) = self.original_bytes {
            if let Ok(store) = gbtrace::column_store::load_column_store_from_bytes(bytes) {
                let frames = framebuffer::reconstruct_frames(&store);
                if !frames.is_empty() {
                    *self.frames_cache.borrow_mut() = Some(frames);
                    return;
                }
            }
        }
        // Fall back to whatever we got (may be empty or blank)
        *self.frames_cache.borrow_mut() = Some(frames);
    }

    /// Check if any frame has non-zero pixel data.
    fn has_visible_pixels(frames: &[Frame]) -> bool {
        frames.iter().any(|f| f.pixels.iter().any(|&p| p != 0))
    }

    /// Get the entry range (start, end) for a frame by index.
    fn frame_entry_range(&self, frame_index: usize) -> Option<(usize, usize)> {
        let boundaries = match &self.store {
            StoreKind::Lazy(s) => s.frame_boundaries(),
            StoreKind::Eager(s) => s.frame_boundaries(),
        };
        if frame_index >= boundaries.len() {
            return None;
        }
        let start = boundaries[frame_index] as usize;
        let end = if frame_index + 1 < boundaries.len() {
            boundaries[frame_index + 1] as usize
        } else {
            match &self.store {
                StoreKind::Lazy(s) => s.entry_count(),
                StoreKind::Eager(s) => s.entry_count(),
            }
        };
        Some((start, end))
    }

    /// Decode a range of entries into an eager column store.
    /// For lazy stores, only decodes the row groups that overlap the range.
    fn decode_range(&self, start: usize, end: usize) -> Result<ColumnStore, JsError> {
        match &self.store {
            StoreKind::Lazy(s) => {
                s.decode_range(start, end)
                    .map_err(|e| JsError::new(&format!("{e}")))
            }
            StoreKind::Eager(s) => {
                // For eager stores, create a sub-store view
                Ok(s.slice(start, end))
            }
        }
    }

    fn get_eager_store(&self) -> Result<ColumnStore, JsError> {
        match &self.store {
            StoreKind::Eager(s) => {
                // Clone is expensive but needed for the borrow checker.
                // For lazy stores we decode from bytes instead.
                // For eager stores used in partial rendering, reconstruct from original_bytes if available.
                if let Some(ref bytes) = self.original_bytes {
                    gbtrace::column_store::load_column_store_from_bytes(bytes)
                        .map_err(|e| JsError::new(&format!("{e}")))
                } else {
                    Err(JsError::new("no original bytes for eager store"))
                }
            }
            StoreKind::Lazy(s) => {
                s.to_eager().map_err(|e| JsError::new(&format!("{e}")))
            }
        }
    }

    fn reconstruct_from_store(&self) -> Vec<Frame> {
        match &self.store {
            StoreKind::Eager(s) => framebuffer::reconstruct_frames(s),
            StoreKind::Lazy(s) => {
                match s.to_eager() {
                    Ok(e) => framebuffer::reconstruct_frames(&e),
                    Err(_) => Vec::new(),
                }
            }
        }
    }

    fn row_to_map(&self, index: usize) -> BTreeMap<String, JsField> {
        let store: &dyn gbtrace::column_store::TraceStore = match &self.store {
            StoreKind::Lazy(s) => s,
            StoreKind::Eager(s) => s,
        };
        let fields = store.header().fields.clone();
        let mut map = BTreeMap::new();

        for (col_idx, field_name) in fields.iter().enumerate() {
            let ft = gbtrace::profile::field_type(field_name);
            let val = match ft {
                FieldType::Bool => JsField::Bool(store.get_bool(col_idx, index)),
                FieldType::Str => {
                    // For pix: expose single-char pixel values as numbers
                    let s = store.get_str(col_idx, index);
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
                _ => JsField::Num(store.get_numeric(col_idx, index) as f64),
            };
            map.insert(field_name.clone(), val);
        }
        map
    }

    fn get_numeric_named(&self, name: &str, row: usize) -> Option<u64> {
        match &self.store {
            StoreKind::Lazy(s) => s.get_numeric_named(name, row),
            StoreKind::Eager(s) => s.get_numeric_named(name, row),
        }
    }

    fn has_field(&self, name: &str) -> bool {
        match &self.store {
            StoreKind::Lazy(s) => s.field_col(name).is_some(),
            StoreKind::Eager(s) => s.field_col(name).is_some(),
        }
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

    // Convert to eager stores for diff preparation
    let store_a = match a.store {
        StoreKind::Eager(s) => s,
        StoreKind::Lazy(s) => s.to_eager().map_err(|e| JsError::new(&format!("{e}")))?,
    };
    let store_b = match b.store {
        StoreKind::Eager(s) => s,
        StoreKind::Lazy(s) => s.to_eager().map_err(|e| JsError::new(&format!("{e}")))?,
    };

    let sync_str = sync.as_deref();
    let (new_a, new_b) = ColumnStore::prepare_for_diff_with_sync(store_a, store_b, sync_str)
        .map_err(|e| JsError::new(&format!("{e}")))?;

    let arr = js_sys::Array::new();
    arr.push(&JsValue::from(TraceStore { store: StoreKind::Eager(new_a), rom: rom_a, original_bytes: bytes_a, frames_cache: RefCell::new(None) }));
    arr.push(&JsValue::from(TraceStore { store: StoreKind::Eager(new_b), rom: rom_b, original_bytes: bytes_b, frames_cache: RefCell::new(None) }));
    Ok(arr)
}
