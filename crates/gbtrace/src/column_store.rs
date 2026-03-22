//! Columnar trace storage.
//!
//! Stores trace data as one typed vector per field, avoiding the
//! per-row `BTreeMap<String, serde_json::Value>` overhead of `TraceEntry`.
//! A 7M-row trace with 14 fields uses ~100MB instead of ~2GB.

use std::collections::HashMap;

use crate::entry::TraceEntry;
use crate::error::{Error, Result};
use crate::header::TraceHeader;
use crate::profile::{field_type, FieldType};
use crate::query::{self, Condition};

// ---------------------------------------------------------------------------
// Column data
// ---------------------------------------------------------------------------

/// A single typed column of trace data.
pub enum ColumnData {
    U64(Vec<u64>),
    U16(Vec<u16>),
    U8(Vec<u8>),
    Bool(Vec<bool>),
}

impl ColumnData {
    fn with_capacity(ft: FieldType, cap: usize) -> Self {
        match ft {
            FieldType::UInt64 => Self::U64(Vec::with_capacity(cap)),
            FieldType::UInt16 => Self::U16(Vec::with_capacity(cap)),
            FieldType::UInt8 => Self::U8(Vec::with_capacity(cap)),
            FieldType::Bool => Self::Bool(Vec::with_capacity(cap)),
        }
    }

    /// Read a value as u64 regardless of stored width.
    pub fn get_numeric(&self, row: usize) -> u64 {
        match self {
            Self::U64(v) => v[row],
            Self::U16(v) => v[row] as u64,
            Self::U8(v) => v[row] as u64,
            Self::Bool(v) => v[row] as u64,
        }
    }

    pub fn get_bool(&self, row: usize) -> bool {
        match self {
            Self::Bool(v) => v[row],
            other => other.get_numeric(row) != 0,
        }
    }

}

// ---------------------------------------------------------------------------
// ColumnStore
// ---------------------------------------------------------------------------

/// Columnar trace storage. One contiguous Vec per field.
pub struct ColumnStore {
    header: TraceHeader,
    columns: Vec<ColumnData>,
    field_index: HashMap<String, usize>,
    len: usize,
}

impl ColumnStore {
    /// Create an empty store with the given header, pre-allocating for `capacity` rows.
    pub fn with_capacity(header: TraceHeader, capacity: usize) -> Self {
        let field_index: HashMap<String, usize> = header
            .fields
            .iter()
            .enumerate()
            .map(|(i, f)| (f.clone(), i))
            .collect();
        let columns = header
            .fields
            .iter()
            .map(|f| ColumnData::with_capacity(field_type(f), capacity))
            .collect();
        Self {
            header,
            columns,
            field_index,
            len: 0,
        }
    }

    /// Create an empty store.
    pub fn new(header: TraceHeader) -> Self {
        Self::with_capacity(header, 0)
    }

    // --- Appending ---

    /// Push a u64 value to a column by index.
    pub fn push_u64(&mut self, col: usize, val: u64) {
        match &mut self.columns[col] {
            ColumnData::U64(v) => v.push(val),
            ColumnData::U16(v) => v.push(val as u16),
            ColumnData::U8(v) => v.push(val as u8),
            ColumnData::Bool(v) => v.push(val != 0),
        }
    }

    /// Push a u16 value to a column by index.
    pub fn push_u16(&mut self, col: usize, val: u16) {
        match &mut self.columns[col] {
            ColumnData::U64(v) => v.push(val as u64),
            ColumnData::U16(v) => v.push(val),
            ColumnData::U8(v) => v.push(val as u8),
            ColumnData::Bool(v) => v.push(val != 0),
        }
    }

    /// Push a u8 value to a column by index.
    pub fn push_u8(&mut self, col: usize, val: u8) {
        match &mut self.columns[col] {
            ColumnData::U64(v) => v.push(val as u64),
            ColumnData::U16(v) => v.push(val as u16),
            ColumnData::U8(v) => v.push(val),
            ColumnData::Bool(v) => v.push(val != 0),
        }
    }

    /// Push a bool value to a column by index.
    pub fn push_bool(&mut self, col: usize, val: bool) {
        match &mut self.columns[col] {
            ColumnData::Bool(v) => v.push(val),
            ColumnData::U8(v) => v.push(val as u8),
            ColumnData::U16(v) => v.push(val as u16),
            ColumnData::U64(v) => v.push(val as u64),
        }
    }

    /// Mark a row as complete. Call after pushing all columns for a row.
    pub fn finish_row(&mut self) {
        self.len += 1;
    }

    // --- Access ---

    pub fn header(&self) -> &TraceHeader {
        &self.header
    }

    pub fn entry_count(&self) -> usize {
        self.len
    }

    /// Get the column index for a field name.
    pub fn field_col(&self, name: &str) -> Option<usize> {
        self.field_index.get(name).copied()
    }

    /// Get a column by index.
    pub fn column(&self, col: usize) -> &ColumnData {
        &self.columns[col]
    }

    /// Get cycle count for a row.
    pub fn cy(&self, row: usize) -> u64 {
        if let Some(&col) = self.field_index.get("cy") {
            self.columns[col].get_numeric(row)
        } else {
            0
        }
    }

    /// Get a numeric value by field name.
    pub fn get_numeric_named(&self, name: &str, row: usize) -> Option<u64> {
        self.field_index
            .get(name)
            .map(|&col| self.columns[col].get_numeric(row))
    }

    /// Get a u8 value by field name.
    pub fn get_u8_named(&self, name: &str, row: usize) -> Option<u8> {
        self.get_numeric_named(name, row).map(|v| v as u8)
    }

    /// Get a u16 value by field name.
    pub fn get_u16_named(&self, name: &str, row: usize) -> Option<u16> {
        self.get_numeric_named(name, row).map(|v| v as u16)
    }

    /// Get a bool value by field name.
    pub fn get_bool_named(&self, name: &str, row: usize) -> Option<bool> {
        self.field_index
            .get(name)
            .map(|&col| self.columns[col].get_bool(row))
    }

    /// Get a zero-allocation view of a row.
    pub fn row(&self, index: usize) -> EntryView<'_> {
        EntryView {
            store: self,
            index,
        }
    }

    // --- Conversion ---

    /// Reconstruct a `TraceEntry` from columnar data. Allocates.
    pub fn to_entry(&self, index: usize) -> TraceEntry {
        let mut entry = TraceEntry::new();
        for (col, name) in self.header.fields.iter().enumerate() {
            match &self.columns[col] {
                ColumnData::U64(v) => {
                    if name == "cy" {
                        entry.set_cy(v[index]);
                    } else {
                        // Shouldn't happen with current fields but handle it
                        entry.set_cy(v[index]);
                    }
                }
                ColumnData::U16(v) => entry.set_u16(name, v[index]),
                ColumnData::U8(v) => entry.set_u8(name, v[index]),
                ColumnData::Bool(v) => entry.set_bool(name, v[index]),
            }
        }
        entry
    }

    /// Build from a slice of TraceEntry (compatibility path).
    pub fn from_entries(header: TraceHeader, entries: &[TraceEntry]) -> Self {
        let mut store = Self::with_capacity(header, entries.len());
        for entry in entries {
            for (col, name) in store.header.fields.clone().iter().enumerate() {
                let val = entry.get(name).and_then(|v| v.as_u64()).unwrap_or(0);
                store.push_u64(col, val);
            }
            store.finish_row();
        }
        store
    }

    // --- Transform ---

    /// Collapse T-cycle entries to instruction boundaries.
    /// Groups consecutive entries with the same PC and keeps the first entry
    /// of each new PC group (matching instruction-level adapter behaviour).
    pub fn collapse_to_instructions(&self) -> Result<Self> {
        let pc_col = self.field_col("pc")
            .ok_or_else(|| Error::Diff("no pc field for collapse".into()))?;
        let count = self.entry_count();
        if count == 0 {
            return Ok(Self::new(self.header.clone()));
        }

        let mut new_store = Self::with_capacity(self.header.clone(), count / 4);
        let mut prev_pc = self.columns[pc_col].get_numeric(0);
        let ncols = self.header.fields.len();

        // Emit first entry
        for col in 0..ncols {
            new_store.push_u64(col, self.columns[col].get_numeric(0));
        }
        new_store.finish_row();

        for i in 1..count {
            let cur_pc = self.columns[pc_col].get_numeric(i);
            if cur_pc != prev_pc {
                for col in 0..ncols {
                    new_store.push_u64(col, self.columns[col].get_numeric(i));
                }
                new_store.finish_row();
            }
            prev_pc = cur_pc;
        }

        Ok(new_store)
    }

    /// Skip entries until the first entry with the given PC value.
    pub fn skip_to_pc(&self, target_pc: u16) -> Result<Self> {
        let pc_col = self.field_col("pc")
            .ok_or_else(|| Error::Diff("no pc field".into()))?;
        let count = self.entry_count();

        let start = (0..count)
            .find(|&i| self.columns[pc_col].get_numeric(i) as u16 == target_pc)
            .ok_or_else(|| Error::Diff(format!("PC 0x{:04x} not found", target_pc)))?;

        let ncols = self.header.fields.len();
        let mut new_store = Self::with_capacity(self.header.clone(), count - start);

        for i in start..count {
            for col in 0..ncols {
                new_store.push_u64(col, self.columns[col].get_numeric(i));
            }
            new_store.finish_row();
        }

        Ok(new_store)
    }

    // --- Query ---

    /// Evaluate a condition against all rows and return matching indices.
    pub fn query(&self, condition_str: &str) -> std::result::Result<Vec<u32>, String> {
        let condition = query::parse_condition(condition_str)?;
        Ok(self.query_condition(&condition))
    }

    /// Evaluate a parsed condition against all rows.
    pub fn query_condition(&self, condition: &Condition) -> Vec<u32> {
        let mut indices = Vec::new();
        for i in 0..self.len {
            if self.eval_condition(condition, i) {
                indices.push(i as u32);
            }
        }
        indices
    }

    fn eval_condition(&self, cond: &Condition, row: usize) -> bool {
        match cond {
            Condition::FieldEquals { field, value } => {
                if let Some(target) = query::parse_number(value) {
                    self.get_numeric_named(field, row) == Some(target)
                } else if let Some(target_bool) = parse_bool_str(value) {
                    self.get_bool_named(field, row) == Some(target_bool)
                } else {
                    false
                }
            }

            Condition::FieldChanges { field } => {
                if row == 0 {
                    return false;
                }
                if let Some(col) = self.field_col(field) {
                    self.columns[col].get_numeric(row) != self.columns[col].get_numeric(row - 1)
                } else {
                    false
                }
            }

            Condition::FieldChangesTo { field, value } => {
                if let Some(target) = query::parse_number(value) {
                    let cur = self.get_numeric_named(field, row);
                    let prev = if row > 0 {
                        self.get_numeric_named(field, row - 1)
                    } else {
                        None
                    };
                    cur == Some(target) && prev != Some(target)
                } else {
                    false
                }
            }

            Condition::FieldChangesFrom { field, value } => {
                if row == 0 {
                    return false;
                }
                if let Some(target) = query::parse_number(value) {
                    let cur = self.get_numeric_named(field, row);
                    let prev = self.get_numeric_named(field, row - 1);
                    prev == Some(target) && cur != Some(target)
                } else {
                    false
                }
            }

            Condition::PpuEntersMode(mode) => {
                if row == 0 {
                    return false;
                }
                let cur = self.get_u8_named("stat", row).map(|s| s & 0x03);
                let prev = self.get_u8_named("stat", row - 1).map(|s| s & 0x03);
                cur == Some(*mode) && prev != Some(*mode)
            }

            Condition::LcdTurnsOn => self.bit_transition("lcdc", 7, false, true, row),
            Condition::LcdTurnsOff => self.bit_transition("lcdc", 7, true, false, row),

            Condition::TimerOverflow => {
                if row == 0 {
                    return false;
                }
                let cur = self.get_u8_named("tima", row);
                let prev = self.get_u8_named("tima", row - 1);
                matches!((cur, prev), (Some(c), Some(p)) if c < p && p > 0x80)
            }

            Condition::InterruptFires(bit) => {
                self.bit_transition("if_", *bit, false, true, row)
            }

            Condition::FlagSet(bit) => {
                self.get_u8_named("f", row).map_or(false, |f| (f >> bit) & 1 == 1)
            }

            Condition::FlagClear(bit) => {
                self.get_u8_named("f", row).map_or(false, |f| (f >> bit) & 1 == 0)
            }

            Condition::FlagBecomesSet(bit) => {
                self.bit_transition("f", *bit, false, true, row)
            }

            Condition::FlagBecomesClear(bit) => {
                self.bit_transition("f", *bit, true, false, row)
            }

            Condition::All(cs) => cs.iter().all(|c| self.eval_condition(c, row)),
            Condition::Any(cs) => cs.iter().any(|c| self.eval_condition(c, row)),
        }
    }

    fn bit_transition(&self, field: &str, bit: u8, from: bool, to: bool, row: usize) -> bool {
        if row == 0 {
            return false;
        }
        let cur = self.get_u8_named(field, row);
        let prev = self.get_u8_named(field, row - 1);
        match (cur, prev) {
            (Some(c), Some(p)) => {
                let cur_bit = (c >> bit) & 1 == 1;
                let prv_bit = (p >> bit) & 1 == 1;
                prv_bit == from && cur_bit == to
            }
            _ => false,
        }
    }
}

fn parse_bool_str(s: &str) -> Option<bool> {
    match s {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// EntryView — zero-allocation row reference
// ---------------------------------------------------------------------------

/// A lightweight, non-allocating view into a single row of a `ColumnStore`.
pub struct EntryView<'a> {
    store: &'a ColumnStore,
    index: usize,
}

impl<'a> EntryView<'a> {
    pub fn cy(&self) -> u64 {
        self.store.cy(self.index)
    }

    pub fn get_numeric(&self, name: &str) -> Option<u64> {
        self.store.get_numeric_named(name, self.index)
    }

    pub fn get_u8(&self, name: &str) -> Option<u8> {
        self.store.get_u8_named(name, self.index)
    }

    pub fn get_u16(&self, name: &str) -> Option<u16> {
        self.store.get_u16_named(name, self.index)
    }

    pub fn get_bool(&self, name: &str) -> Option<bool> {
        self.store.get_bool_named(name, self.index)
    }
}

// ---------------------------------------------------------------------------
// Loading from readers
// ---------------------------------------------------------------------------

#[cfg(feature = "parquet")]
use crate::parquet::ParquetTraceReader;

/// Load a column store from any supported trace file format.
pub fn load_column_store(path: impl AsRef<std::path::Path>) -> Result<ColumnStore> {
    let path = path.as_ref();
    #[cfg(feature = "parquet")]
    if path.extension().is_some_and(|ext| ext == "parquet") {
        return load_from_parquet(path);
    }
    load_from_jsonl(path)
}

#[cfg(feature = "parquet")]
fn load_from_parquet(path: &std::path::Path) -> Result<ColumnStore> {
    use arrow::array::{BooleanArray, UInt16Array, UInt64Array, UInt8Array};

    let reader = ParquetTraceReader::open(path)?;
    let header = reader.header().clone();
    let field_types: Vec<FieldType> = header.fields.iter().map(|n| field_type(n)).collect();

    let mut store = ColumnStore::new(header);

    // Access the underlying batch reader directly by re-opening
    let file = std::fs::File::open(path)?;
    let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(file)?;
    let batch_reader = builder.with_batch_size(65536).build()?;

    for batch_result in batch_reader {
        let batch = batch_result.map_err(Error::Arrow)?;
        let num_rows = batch.num_rows();

        for (col_idx, ft) in field_types.iter().enumerate() {
            let col = batch.column(col_idx);
            match ft {
                FieldType::UInt64 => {
                    let arr = col.as_any().downcast_ref::<UInt64Array>().unwrap();
                    if let ColumnData::U64(v) = &mut store.columns[col_idx] {
                        v.extend_from_slice(arr.values());
                    }
                }
                FieldType::UInt16 => {
                    let arr = col.as_any().downcast_ref::<UInt16Array>().unwrap();
                    if let ColumnData::U16(v) = &mut store.columns[col_idx] {
                        v.extend_from_slice(arr.values());
                    }
                }
                FieldType::UInt8 => {
                    let arr = col.as_any().downcast_ref::<UInt8Array>().unwrap();
                    if let ColumnData::U8(v) = &mut store.columns[col_idx] {
                        v.extend_from_slice(arr.values());
                    }
                }
                FieldType::Bool => {
                    let arr = col.as_any().downcast_ref::<BooleanArray>().unwrap();
                    if let ColumnData::Bool(v) = &mut store.columns[col_idx] {
                        for i in 0..num_rows {
                            v.push(arr.value(i));
                        }
                    }
                }
            }
        }
        store.len += num_rows;
    }

    Ok(store)
}

fn load_from_jsonl(path: &std::path::Path) -> Result<ColumnStore> {
    let reader = crate::reader::TraceReader::open(path)?;
    let header = reader.header().clone();
    let mut store = ColumnStore::new(header);

    for result in reader {
        let entry = result?;
        for (col, name) in store.header.fields.clone().iter().enumerate() {
            if let Some(val) = entry.get(name) {
                if let Some(b) = val.as_bool() {
                    store.push_bool(col, b);
                } else {
                    store.push_u64(col, val.as_u64().unwrap_or(0));
                }
            } else {
                store.push_u64(col, 0);
            }
        }
        store.finish_row();
    }

    Ok(store)
}

/// Load a column store from in-memory bytes (for WASM).
#[cfg(feature = "parquet")]
pub fn load_column_store_from_bytes(data: &[u8]) -> Result<ColumnStore> {
    use arrow::array::{BooleanArray, UInt16Array, UInt64Array, UInt8Array};

    const PARQUET_MAGIC: &[u8] = b"PAR1";

    if data.len() >= 4 && &data[..4] == PARQUET_MAGIC {
        // Parquet path
        let reader = ParquetTraceReader::from_bytes(data.to_vec())?;
        let header = reader.header().clone();
        let field_types: Vec<FieldType> = header.fields.iter().map(|n| field_type(n)).collect();

        let mut store = ColumnStore::new(header);

        let bytes = bytes::Bytes::from(data.to_vec());
        let builder =
            parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(bytes)?;
        let batch_reader = builder.with_batch_size(65536).build()?;

        for batch_result in batch_reader {
            let batch = batch_result.map_err(Error::Arrow)?;
            let num_rows = batch.num_rows();

            for (col_idx, ft) in field_types.iter().enumerate() {
                let col = batch.column(col_idx);
                match ft {
                    FieldType::UInt64 => {
                        let arr = col.as_any().downcast_ref::<UInt64Array>().unwrap();
                        if let ColumnData::U64(v) = &mut store.columns[col_idx] {
                            v.extend_from_slice(arr.values());
                        }
                    }
                    FieldType::UInt16 => {
                        let arr = col.as_any().downcast_ref::<UInt16Array>().unwrap();
                        if let ColumnData::U16(v) = &mut store.columns[col_idx] {
                            v.extend_from_slice(arr.values());
                        }
                    }
                    FieldType::UInt8 => {
                        let arr = col.as_any().downcast_ref::<UInt8Array>().unwrap();
                        if let ColumnData::U8(v) = &mut store.columns[col_idx] {
                            v.extend_from_slice(arr.values());
                        }
                    }
                    FieldType::Bool => {
                        let arr = col.as_any().downcast_ref::<BooleanArray>().unwrap();
                        if let ColumnData::Bool(v) = &mut store.columns[col_idx] {
                            for i in 0..num_rows {
                                v.push(arr.value(i));
                            }
                        }
                    }
                }
            }
            store.len += num_rows;
        }

        return Ok(store);
    }

    // JSONL/gzip path
    use std::io::{BufRead, BufReader, Cursor, Read};
    use flate2::read::GzDecoder;

    let reader_box: Box<dyn Read> = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        Box::new(GzDecoder::new(Cursor::new(data)))
    } else {
        Box::new(Cursor::new(data))
    };

    let mut lines = BufReader::with_capacity(64 * 1024, reader_box);
    let mut header_line = String::new();
    lines.read_line(&mut header_line).map_err(|e| {
        Error::Diff(format!("failed to read header: {e}"))
    })?;

    if header_line.is_empty() {
        return Err(Error::Diff("empty trace file".into()));
    }

    let header: TraceHeader = serde_json::from_str(&header_line)?;
    header.validate()?;

    let mut store = ColumnStore::new(header);
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = lines.read_line(&mut line).map_err(|e| {
            Error::Diff(format!("read error: {e}"))
        })?;
        if bytes_read == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(trimmed)?;
        if let Some(entry) = TraceEntry::from_json_value(&value) {
            for (col, name) in store.header.fields.clone().iter().enumerate() {
                if let Some(val) = entry.get(name) {
                    if let Some(b) = val.as_bool() {
                        store.push_bool(col, b);
                    } else {
                        store.push_u64(col, val.as_u64().unwrap_or(0));
                    }
                } else {
                    store.push_u64(col, 0);
                }
            }
            store.finish_row();
        }
    }

    Ok(store)
}
