use gbtrace::column_store::{ColumnData, ColumnStore, LazyColumnStore};
use gbtrace::disasm;
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
        Ok(TraceStore { store, rom: None })
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

    /// Get the field names from the header.
    #[wasm_bindgen(js_name = fieldNames)]
    pub fn field_names(&self) -> Result<JsValue, JsError> {
        let fields = match &self.store {
            StoreKind::Lazy(s) => &s.header().fields,
            StoreKind::Eager(s) => &s.header().fields,
        };
        Ok(to_js(fields)?)
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
        let indices = match &self.store {
            StoreKind::Lazy(s) => s.query(condition_str).map_err(|e| JsError::new(&e))?,
            StoreKind::Eager(s) => s.query(condition_str).map_err(|e| JsError::new(&e))?,
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

    /// Per-field diff statistics.
    #[wasm_bindgen(js_name = diffStats)]
    pub fn diff_stats(&self, other: &TraceStore) -> Result<JsValue, JsError> {
        let len = self.entry_count().min(other.entry_count());
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
            for row in 0..len {
                if self.get_numeric_named(name, row) != other.get_numeric_named(name, row) {
                    count += 1;
                    any_diff_flags[row] = true;
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
    fn row_to_map(&self, index: usize) -> BTreeMap<String, JsField> {
        let fields = match &self.store {
            StoreKind::Lazy(s) => s.header().fields.clone(),
            StoreKind::Eager(s) => s.header().fields.clone(),
        };
        let mut map = BTreeMap::new();

        for (col_idx, field_name) in fields.iter().enumerate() {
            let val = match &self.store {
                StoreKind::Eager(s) => {
                    let col = s.column(col_idx);
                    match col {
                        ColumnData::U64(_) | ColumnData::U16(_) | ColumnData::U8(_) => {
                            JsField::Num(col.get_numeric(index) as f64)
                        }
                        ColumnData::Bool(_) => JsField::Bool(col.get_bool(index)),
                    }
                }
                StoreKind::Lazy(s) => {
                    match s.column_type(col_idx) {
                        FieldType::Bool => {
                            JsField::Bool(s.get_bool_named(field_name, index).unwrap_or(false))
                        }
                        _ => {
                            JsField::Num(s.get_numeric(col_idx, index) as f64)
                        }
                    }
                }
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

/// Prepare two TraceStores for comparison.
#[wasm_bindgen(js_name = prepareForDiff)]
pub fn prepare_for_diff(a: TraceStore, b: TraceStore) -> Result<js_sys::Array, JsError> {
    let rom_a = a.rom;
    let rom_b = b.rom;

    // Convert to eager stores for diff preparation
    let store_a = match a.store {
        StoreKind::Eager(s) => s,
        StoreKind::Lazy(s) => s.to_eager().map_err(|e| JsError::new(&format!("{e}")))?,
    };
    let store_b = match b.store {
        StoreKind::Eager(s) => s,
        StoreKind::Lazy(s) => s.to_eager().map_err(|e| JsError::new(&format!("{e}")))?,
    };

    let (new_a, new_b) = ColumnStore::prepare_for_diff(store_a, store_b)
        .map_err(|e| JsError::new(&format!("{e}")))?;

    let arr = js_sys::Array::new();
    arr.push(&JsValue::from(TraceStore { store: StoreKind::Eager(new_a), rom: rom_a }));
    arr.push(&JsValue::from(TraceStore { store: StoreKind::Eager(new_b), rom: rom_b }));
    Ok(arr)
}
