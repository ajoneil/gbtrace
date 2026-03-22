use gbtrace::column_store::{ColumnData, ColumnStore};
use gbtrace::disasm;
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
/// Wraps the library's columnar `ColumnStore` directly — no intermediate
/// `Vec<TraceEntry>` conversion. Memory usage is roughly 8 bytes × fields × entries
/// for numeric fields and 1 byte per bool field.
#[wasm_bindgen]
pub struct TraceStore {
    store: ColumnStore,
    rom: Option<Vec<u8>>,
}

#[wasm_bindgen]
impl TraceStore {
    /// Load a trace from raw bytes (detects format automatically).
    #[wasm_bindgen(constructor)]
    pub fn from_bytes(data: &[u8]) -> Result<TraceStore, JsError> {
        let store = gbtrace::column_store::load_column_store_from_bytes(data)
            .map_err(|e| JsError::new(&format!("{e}")))?;
        Ok(TraceStore { store, rom: None })
    }

    /// Return the trace header as a JS object.
    pub fn header(&self) -> Result<JsValue, JsError> {
        Ok(to_js(self.store.header())?)
    }

    /// Number of entries in the trace.
    #[wasm_bindgen(js_name = entryCount)]
    pub fn entry_count(&self) -> usize {
        self.store.entry_count()
    }

    /// Get the field names from the header.
    #[wasm_bindgen(js_name = fieldNames)]
    pub fn field_names(&self) -> Result<JsValue, JsError> {
        Ok(to_js(&self.store.header().fields)?)
    }

    /// Get a single entry as a JS object. Returns null if out of range.
    pub fn entry(&self, index: usize) -> Result<JsValue, JsError> {
        if index >= self.store.entry_count() {
            return Ok(JsValue::NULL);
        }
        Ok(to_js(&self.row_to_map(index))?)
    }

    /// Get a range of entries as a JS array. Used for virtual scrolling.
    #[wasm_bindgen(js_name = entriesRange)]
    pub fn entries_range(&self, start: usize, count: usize) -> Result<JsValue, JsError> {
        let end = (start + count).min(self.store.entry_count());
        let slice: Vec<_> = (start..end)
            .map(|i| self.row_to_map(i))
            .collect();
        Ok(to_js(&slice)?)
    }

    /// Parse a condition string and find all matching entry indices.
    /// Returns a Uint32Array of indices.
    pub fn query(&self, condition_str: &str) -> Result<js_sys::Uint32Array, JsError> {
        let indices = self.store.query(condition_str)
            .map_err(|e| JsError::new(&e))?;
        let arr = js_sys::Uint32Array::new_with_length(indices.len() as u32);
        arr.copy_from(&indices);
        Ok(arr)
    }

    /// Downsample a field for chart display.
    /// Returns a Float64Array of [min0, max0, min1, max1, ...] for `buckets` buckets
    /// covering entries from `start` to `end`.
    #[wasm_bindgen(js_name = fieldSummary)]
    pub fn field_summary(
        &self,
        field: &str,
        start: usize,
        end: usize,
        buckets: usize,
    ) -> Result<js_sys::Float64Array, JsError> {
        let col_idx = self.store.field_col(field)
            .ok_or_else(|| JsError::new(&format!("unknown field: {field}")))?;
        let col = self.store.column(col_idx);
        let total = self.store.entry_count();
        let end = end.min(total);
        let start = start.min(end);
        let range = end - start;

        if range == 0 || buckets == 0 {
            return Ok(js_sys::Float64Array::new_with_length(0));
        }

        let mut out = Vec::with_capacity(buckets * 2);
        for b in 0..buckets {
            let b_start = start + (b * range) / buckets;
            let b_end = start + ((b + 1) * range) / buckets;
            if b_start >= b_end {
                // Empty bucket — repeat last value or 0
                let v = if b_start > 0 {
                    col.get_numeric(b_start.min(total - 1)) as f64
                } else {
                    0.0
                };
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

        let arr = js_sys::Float64Array::new_with_length(out.len() as u32);
        arr.copy_from(&out);
        Ok(arr)
    }

    /// Compare a field between this store and another, returning indices where values differ.
    /// Returns a Uint32Array of differing entry indices.
    #[wasm_bindgen(js_name = diffField)]
    pub fn diff_field(
        &self,
        other: &TraceStore,
        field: &str,
    ) -> Result<js_sys::Uint32Array, JsError> {
        let col_a = self.store.field_col(field)
            .ok_or_else(|| JsError::new(&format!("unknown field in A: {field}")))?;
        let col_b = other.store.field_col(field)
            .ok_or_else(|| JsError::new(&format!("unknown field in B: {field}")))?;

        let len = self.store.entry_count().min(other.store.entry_count());
        let ca = self.store.column(col_a);
        let cb = other.store.column(col_b);

        let mut indices = Vec::new();
        for i in 0..len {
            if ca.get_numeric(i) != cb.get_numeric(i) {
                indices.push(i as u32);
            }
        }

        let arr = js_sys::Uint32Array::new_with_length(indices.len() as u32);
        arr.copy_from(&indices);
        Ok(arr)
    }

    /// Per-field diff statistics: returns a JS object { total, matching, fields: { fieldName: diffCount, ... } }
    #[wasm_bindgen(js_name = diffStats)]
    pub fn diff_stats(&self, other: &TraceStore) -> Result<JsValue, JsError> {
        let len = self.store.entry_count().min(other.store.entry_count());
        let header = self.store.header();

        let mut field_counts: Vec<(&str, u64)> = Vec::new();
        let mut any_diff_count: usize = 0;
        let mut any_diff_flags = vec![false; len];

        for (i, name) in header.fields.iter().enumerate() {
            // all fields compared
            if let Some(j) = other.store.field_col(name) {
                let ca = self.store.column(i);
                let cb = other.store.column(j);
                let mut count = 0u64;
                for row in 0..len {
                    if ca.get_numeric(row) != cb.get_numeric(row) {
                        count += 1;
                        any_diff_flags[row] = true;
                    }
                }
                if count > 0 {
                    field_counts.push((name, count));
                }
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
            fields: field_counts.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        };

        Ok(to_js(&stats)?)
    }

    /// Compare ALL fields between this store and another, returning indices where any field differs.
    #[wasm_bindgen(js_name = diffAll)]
    pub fn diff_all(
        &self,
        other: &TraceStore,
    ) -> Result<js_sys::Uint32Array, JsError> {
        let len = self.store.entry_count().min(other.store.entry_count());
        let header = self.store.header();

        // Collect column pairs for fields present in both stores
        let mut col_pairs: Vec<(usize, usize)> = Vec::new();
        for (i, name) in header.fields.iter().enumerate() {
            // all fields compared
            if let Some(j) = other.store.field_col(name) {
                col_pairs.push((i, j));
            }
        }

        let mut indices = Vec::new();
        for row in 0..len {
            for &(ca, cb) in &col_pairs {
                if self.store.column(ca).get_numeric(row)
                    != other.store.column(cb).get_numeric(row)
                {
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
    /// Returns the mnemonic string, or empty string if no ROM loaded.
    pub fn disassemble(&self, pc: u16) -> String {
        match &self.rom {
            Some(rom) => disasm::disassemble(rom, pc).0,
            None => String::new(),
        }
    }

    /// Disassemble instructions for a range of trace entries.
    /// Returns an array of mnemonic strings. Much faster than calling
    /// disassemble() per entry from JS.
    #[wasm_bindgen(js_name = disassembleRange)]
    pub fn disassemble_range(&self, start: usize, count: usize) -> Result<JsValue, JsError> {
        let rom = match &self.rom {
            Some(r) => r,
            None => return Ok(to_js(&Vec::<String>::new())?),
        };
        let pc_col = self.store.field_col("pc");
        if pc_col.is_none() {
            return Ok(to_js(&Vec::<String>::new())?);
        }
        let pc_col = pc_col.unwrap();
        let end = (start + count).min(self.store.entry_count());
        let mnemonics: Vec<String> = (start..end)
            .map(|i| {
                let pc = self.store.column(pc_col).get_numeric(i) as u16;
                disasm::disassemble(rom, pc).0
            })
            .collect();
        Ok(to_js(&mnemonics)?)
    }
}

impl TraceStore {
    fn row_to_map(&self, index: usize) -> BTreeMap<String, JsField> {
        let header = self.store.header();
        let mut map = BTreeMap::new();
        for (col_idx, field_name) in header.fields.iter().enumerate() {
            let col = self.store.column(col_idx);
            let val = match col {
                ColumnData::U64(_) | ColumnData::U16(_) | ColumnData::U8(_) => {
                    JsField::Num(col.get_numeric(index) as f64)
                }
                ColumnData::Bool(_) => JsField::Bool(col.get_bool(index)),
            };
            map.insert(field_name.clone(), val);
        }
        map
    }
}

/// Prepare two TraceStores for comparison: auto-collapse T-cycle traces
/// to instruction level and align by first common PC value.
/// Returns a JS array [storeA, storeB] with the prepared stores.
#[wasm_bindgen(js_name = prepareForDiff)]
pub fn prepare_for_diff(a: TraceStore, b: TraceStore) -> Result<js_sys::Array, JsError> {
    let rom_a = a.rom;
    let rom_b = b.rom;
    let (new_a, new_b) = gbtrace::column_store::ColumnStore::prepare_for_diff(a.store, b.store)
        .map_err(|e| JsError::new(&format!("{e}")))?;
    let arr = js_sys::Array::new();
    arr.push(&JsValue::from(TraceStore { store: new_a, rom: rom_a }));
    arr.push(&JsValue::from(TraceStore { store: new_b, rom: rom_b }));
    Ok(arr)
}
