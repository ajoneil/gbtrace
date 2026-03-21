use gbtrace::column_store::{ColumnData, ColumnStore};
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
}

#[wasm_bindgen]
impl TraceStore {
    /// Load a trace from raw bytes (detects format automatically).
    #[wasm_bindgen(constructor)]
    pub fn from_bytes(data: &[u8]) -> Result<TraceStore, JsError> {
        let store = gbtrace::column_store::load_column_store_from_bytes(data)
            .map_err(|e| JsError::new(&format!("{e}")))?;
        Ok(TraceStore { store })
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

    /// Get the cycle count for a specific entry. Returns 0 if not found.
    #[wasm_bindgen(js_name = entryCycle)]
    pub fn entry_cycle(&self, index: usize) -> f64 {
        if index >= self.store.entry_count() {
            return 0.0;
        }
        self.store.cy(index) as f64
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
