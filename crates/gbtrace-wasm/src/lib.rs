use std::io::{BufRead, BufReader, Cursor, Read};

use flate2::read::GzDecoder;
use gbtrace::query::{self, ConditionEvaluator};
use gbtrace::{ParquetTraceReader, TraceEntry, TraceHeader};
use wasm_bindgen::prelude::*;

use std::collections::BTreeMap;

/// Parquet magic bytes: "PAR1"
const PARQUET_MAGIC: &[u8] = b"PAR1";

/// Serializable entry type that avoids serde_json::Value going through serde_wasm_bindgen.
/// Numbers become strings, booleans stay booleans, strings stay strings.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum JsField {
    Str(String),
    Num(f64),
    Bool(bool),
}

/// Convert a TraceEntry to a plain map with primitive values.
fn entry_to_map(entry: &TraceEntry) -> BTreeMap<String, JsField> {
    let val = entry.to_json_value();
    let mut out = BTreeMap::new();
    if let serde_json::Value::Object(map) = val {
        for (k, v) in map {
            let field = match v {
                serde_json::Value::String(s) => JsField::Str(s),
                serde_json::Value::Number(n) => JsField::Num(n.as_f64().unwrap_or(0.0)),
                serde_json::Value::Bool(b) => JsField::Bool(b),
                _ => JsField::Str(v.to_string()),
            };
            out.insert(k, field);
        }
    }
    out
}

/// Serialize to JS using plain objects (not Maps) for BTreeMap keys.
fn to_js(value: &impl serde::Serialize) -> Result<JsValue, JsError> {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    Ok(value.serialize(&serializer)?)
}

/// In-memory trace store for the browser.
///
/// Holds the parsed header and all entries. Exposed to JS via wasm-bindgen.
#[wasm_bindgen]
pub struct TraceStore {
    header: TraceHeader,
    entries: Vec<TraceEntry>,
}

#[wasm_bindgen]
impl TraceStore {
    /// Load a trace from raw bytes (detects gzip automatically).
    /// Accepts .gbtrace (JSONL) or .gbtrace.gz (gzipped JSONL).
    #[wasm_bindgen(constructor)]
    pub fn from_bytes(data: &[u8]) -> Result<TraceStore, JsError> {
        let (header, entries) = parse_trace_bytes(data)?;
        Ok(TraceStore { header, entries })
    }

    /// Return the trace header as a JS object.
    pub fn header(&self) -> Result<JsValue, JsError> {
        Ok(to_js(&self.header)?)
    }

    /// Number of entries in the trace.
    #[wasm_bindgen(js_name = entryCount)]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Get the field names from the header.
    #[wasm_bindgen(js_name = fieldNames)]
    pub fn field_names(&self) -> Result<JsValue, JsError> {
        Ok(to_js(&self.header.fields)?)
    }

    /// Get a single entry as a JS object. Returns null if out of range.
    pub fn entry(&self, index: usize) -> Result<JsValue, JsError> {
        match self.entries.get(index) {
            Some(e) => Ok(to_js(&entry_to_map(e))?),
            None => Ok(JsValue::NULL),
        }
    }

    /// Get a range of entries as a JS array. Used for virtual scrolling.
    #[wasm_bindgen(js_name = entriesRange)]
    pub fn entries_range(&self, start: usize, count: usize) -> Result<JsValue, JsError> {
        let end = (start + count).min(self.entries.len());
        let slice: Vec<_> = self.entries[start..end]
            .iter()
            .map(entry_to_map)
            .collect();
        Ok(to_js(&slice)?)
    }

    /// Parse a condition string and find all matching entry indices.
    /// Returns a Uint32Array of indices.
    pub fn query(&self, condition_str: &str) -> Result<js_sys::Uint32Array, JsError> {
        let condition = query::parse_condition(condition_str)
            .map_err(|e| JsError::new(&e))?;

        let mut evaluator = ConditionEvaluator::new(condition);
        let mut indices: Vec<u32> = Vec::new();

        for (i, entry) in self.entries.iter().enumerate() {
            if evaluator.evaluate(entry) {
                indices.push(i as u32);
            }
        }

        let arr = js_sys::Uint32Array::new_with_length(indices.len() as u32);
        arr.copy_from(&indices);
        Ok(arr)
    }

    /// Get the cycle count for a specific entry. Returns 0 if not found.
    #[wasm_bindgen(js_name = entryCycle)]
    pub fn entry_cycle(&self, index: usize) -> f64 {
        self.entries
            .get(index)
            .and_then(|e| e.cy())
            .unwrap_or(0) as f64
    }
}

/// Parse trace bytes (auto-detecting format: Parquet, gzip JSONL, or plain JSONL).
fn parse_trace_bytes(data: &[u8]) -> Result<(TraceHeader, Vec<TraceEntry>), JsError> {
    // Detect Parquet by magic bytes
    if data.len() >= 4 && &data[..4] == PARQUET_MAGIC {
        return parse_parquet_bytes(data);
    }

    // Try gzip first (check magic bytes)
    let reader: Box<dyn Read> = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        Box::new(GzDecoder::new(Cursor::new(data)))
    } else {
        Box::new(Cursor::new(data))
    };

    let mut lines = BufReader::with_capacity(64 * 1024, reader);

    // First line is the header
    let mut header_line = String::new();
    lines.read_line(&mut header_line)
        .map_err(|e| JsError::new(&format!("failed to read header: {e}")))?;

    if header_line.is_empty() {
        return Err(JsError::new("empty trace file"));
    }

    let header: TraceHeader = serde_json::from_str(&header_line)
        .map_err(|e| JsError::new(&format!("invalid header: {e}")))?;

    header.validate()
        .map_err(|e| JsError::new(&format!("header validation: {e}")))?;

    // Read entries
    let mut entries = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = lines.read_line(&mut line)
            .map_err(|e| JsError::new(&format!("read error: {e}")))?;
        if bytes_read == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| JsError::new(&format!("entry parse error at line {}: {e}", entries.len() + 2)))?;
        if let Some(entry) = TraceEntry::from_json_value(&value) {
            entries.push(entry);
        }
    }

    Ok((header, entries))
}

/// Parse Parquet trace from in-memory bytes.
fn parse_parquet_bytes(data: &[u8]) -> Result<(TraceHeader, Vec<TraceEntry>), JsError> {
    let reader = ParquetTraceReader::from_bytes(data.to_vec())
        .map_err(|e| JsError::new(&format!("parquet error: {e}")))?;

    let header = reader.header().clone();
    let mut entries = Vec::new();

    for result in reader {
        let entry = result.map_err(|e| JsError::new(&format!("parquet entry error: {e}")))?;
        entries.push(entry);
    }

    Ok((header, entries))
}
