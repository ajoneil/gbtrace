//! Columnar trace storage.
//!
//! Stores trace data as one typed vector per field, avoiding the
//! per-row `BTreeMap<String, serde_json::Value>` overhead of `TraceEntry`.
//! A 7M-row trace with 14 fields uses ~100MB instead of ~2GB.
//!
//! `LazyColumnStore` (parquet feature) wraps a compressed parquet file and
//! decodes row groups on demand with an LRU cache. This keeps memory
//! proportional to a few decoded row groups rather than the entire trace.

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
    Str(Vec<String>),
}

impl ColumnData {
    fn with_capacity(ft: FieldType, cap: usize) -> Self {
        match ft {
            FieldType::UInt64 => Self::U64(Vec::with_capacity(cap)),
            FieldType::UInt16 => Self::U16(Vec::with_capacity(cap)),
            FieldType::UInt8 => Self::U8(Vec::with_capacity(cap)),
            FieldType::Bool => Self::Bool(Vec::with_capacity(cap)),
            FieldType::Str => Self::Str(Vec::with_capacity(cap)),
        }
    }

    /// Read a value as u64 regardless of stored width.
    pub fn get_numeric(&self, row: usize) -> u64 {
        match self {
            Self::U64(v) => v[row],
            Self::U16(v) => v[row] as u64,
            Self::U8(v) => v[row] as u64,
            Self::Bool(v) => v[row] as u64,
            Self::Str(_) => 0,
        }
    }

    pub fn get_bool(&self, row: usize) -> bool {
        match self {
            Self::Bool(v) => v[row],
            Self::Str(_) => false,
            other => other.get_numeric(row) != 0,
        }
    }

    pub fn get_str(&self, row: usize) -> &str {
        match self {
            Self::Str(v) => &v[row],
            _ => "",
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
            ColumnData::Str(v) => v.push(String::new()),
        }
    }

    /// Push a u16 value to a column by index.
    pub fn push_u16(&mut self, col: usize, val: u16) {
        match &mut self.columns[col] {
            ColumnData::U64(v) => v.push(val as u64),
            ColumnData::U16(v) => v.push(val),
            ColumnData::U8(v) => v.push(val as u8),
            ColumnData::Bool(v) => v.push(val != 0),
            ColumnData::Str(v) => v.push(String::new()),
        }
    }

    /// Push a u8 value to a column by index.
    pub fn push_u8(&mut self, col: usize, val: u8) {
        match &mut self.columns[col] {
            ColumnData::U64(v) => v.push(val as u64),
            ColumnData::U16(v) => v.push(val as u16),
            ColumnData::U8(v) => v.push(val),
            ColumnData::Bool(v) => v.push(val != 0),
            ColumnData::Str(v) => v.push(String::new()),
        }
    }

    /// Push a bool value to a column by index.
    pub fn push_bool(&mut self, col: usize, val: bool) {
        match &mut self.columns[col] {
            ColumnData::Bool(v) => v.push(val),
            ColumnData::U8(v) => v.push(val as u8),
            ColumnData::U16(v) => v.push(val as u16),
            ColumnData::U64(v) => v.push(val as u64),
            ColumnData::Str(v) => v.push(String::new()),
        }
    }

    /// Push a string value to a column by index.
    pub fn push_str(&mut self, col: usize, val: &str) {
        if let ColumnData::Str(v) = &mut self.columns[col] {
            v.push(val.to_string());
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
                ColumnData::Str(v) => entry.set_str(name, &v[index]),
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
    /// Groups consecutive entries with the same PC and keeps the FIRST
    /// T-cycle of each group (the state when PC equals that address).
    pub fn collapse_to_instructions(&self) -> Result<Self> {
        let pc_col = self.field_col("pc")
            .ok_or_else(|| Error::Diff("no pc field for collapse".into()))?;
        let count = self.entry_count();
        if count == 0 {
            return Ok(Self::new(self.header.clone()));
        }

        let mut new_store = Self::with_capacity(self.header.clone(), count / 4);
        let ncols = self.header.fields.len();
        let mut prev_pc = self.columns[pc_col].get_numeric(0);

        // Emit first entry
        for col in 0..ncols {
            new_store.push_u64(col, self.columns[col].get_numeric(0));
        }
        new_store.finish_row();

        for i in 1..count {
            let cur_pc = self.columns[pc_col].get_numeric(i);
            if cur_pc != prev_pc {
                // PC changed — emit first T-cycle of new instruction
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

    /// Skip entries until a field matches a condition.
    ///
    /// Condition format: `field=value` (exact match) or `field&mask` (bitmask test).
    /// For bitmask: skips until `(field_value & mask) != 0`.
    /// Values can be decimal or hex with `0x` prefix.
    pub fn skip_until(&self, condition: &str) -> Result<Self> {
        let (field, op, value) = if let Some(pos) = condition.find('&') {
            (&condition[..pos], '&', &condition[pos + 1..])
        } else if let Some(pos) = condition.find('=') {
            (&condition[..pos], '=', &condition[pos + 1..])
        } else {
            return Err(Error::Diff(format!("invalid sync condition: {condition}")));
        };

        let val = if let Some(hex) = value.strip_prefix("0x") {
            u64::from_str_radix(hex, 16)
                .map_err(|_| Error::Diff(format!("invalid value: {value}")))?
        } else {
            value.parse::<u64>()
                .map_err(|_| Error::Diff(format!("invalid value: {value}")))?
        };

        let col_idx = self.field_col(field)
            .ok_or_else(|| Error::Diff(format!("field '{field}' not found")))?;
        let count = self.entry_count();

        let start = (0..count)
            .find(|&i| {
                let v = self.columns[col_idx].get_numeric(i);
                match op {
                    '&' => (v & val) != 0,
                    '=' => v == val,
                    _ => false,
                }
            })
            .ok_or_else(|| Error::Diff(format!("sync condition '{condition}' never matched")))?;

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

    /// Prepare two stores for comparison with default PC alignment.
    pub fn prepare_for_diff(a: Self, b: Self) -> Result<(Self, Self)> {
        Self::prepare_for_diff_with_sync(a, b, None)
    }

    /// Prepare two stores for comparison: collapse T-cycle traces to
    /// instruction level if triggers differ, then align by sync condition.
    ///
    /// Sync modes:
    /// - `None` or `Some("pc")`: align by first common PC (default)
    /// - `Some("ly=0")`, `Some("lcdc&80")`, etc.: align by first match of condition
    /// - `Some("none")`: no alignment, compare from start
    pub fn prepare_for_diff_with_sync(a: Self, b: Self, sync: Option<&str>) -> Result<(Self, Self)> {
        let trig_a = &a.header.trigger;
        let trig_b = &b.header.trigger;
        let a_tcycle = matches!(trig_a, crate::header::Trigger::Tcycle);
        let b_tcycle = matches!(trig_b, crate::header::Trigger::Tcycle);

        // Collapse whichever trace is T-cycle if the other isn't
        let mut a = if a_tcycle && !b_tcycle {
            a.collapse_to_instructions()?
        } else { a };
        let mut b = if b_tcycle && !a_tcycle {
            b.collapse_to_instructions()?
        } else { b };

        let sync_mode = sync.unwrap_or("pc");

        match sync_mode {
            "none" => {
                // No alignment
            }
            "pc" => {
                // Align by first common PC (original behavior)
                let pc_col_a = a.field_col("pc");
                let pc_col_b = b.field_col("pc");
                if let (Some(ca), Some(cb)) = (pc_col_a, pc_col_b) {
                    if a.entry_count() > 0 && b.entry_count() > 0 {
                        let pc_a = a.columns[ca].get_numeric(0) as u16;
                        let pc_b = b.columns[cb].get_numeric(0) as u16;
                        if pc_a != pc_b {
                            let target = (0..a.entry_count().min(100))
                                .find(|&i| a.columns[ca].get_numeric(i) as u16 == pc_b)
                                .map(|_| pc_b)
                                .or_else(|| {
                                    (0..b.entry_count().min(100))
                                        .find(|&i| b.columns[cb].get_numeric(i) as u16 == pc_a)
                                        .map(|_| pc_a)
                                });
                            if let Some(target_pc) = target {
                                if pc_a != target_pc { a = a.skip_to_pc(target_pc)?; }
                                if pc_b != target_pc { b = b.skip_to_pc(target_pc)?; }
                            }
                        }
                    }
                }
            }
            condition => {
                // Align both traces to first match of the given condition
                a = a.skip_until(condition)?;
                b = b.skip_until(condition)?;
            }
        }

        Ok((a, b))
    }

    // --- Frame boundaries ---

    /// Detect frame boundaries by scanning `ly` for vblank→active transitions.
    /// Returns entry indices where each frame starts. Handles instruction-level
    /// traces where the exact 153→0 may not be sampled.
    pub fn frame_boundaries(&self) -> Vec<u32> {
        let ly_col = match self.field_col("ly") {
            Some(c) => c,
            None => return Vec::new(),
        };
        let mut boundaries = vec![0u32];
        for i in 1..self.len {
            let prev = self.columns[ly_col].get_numeric(i - 1) as u8;
            let cur = self.columns[ly_col].get_numeric(i) as u8;
            if cur < prev && prev >= 144 {
                boundaries.push(i as u32);
            }
        }
        boundaries
    }

    // --- Query ---

    /// Evaluate a condition against all rows and return matching indices.
    pub fn query(&self, condition_str: &str) -> std::result::Result<Vec<u32>, String> {
        self.query_range(condition_str, 0, self.len)
    }

    /// Evaluate a condition within a range and return matching global indices.
    pub fn query_range(&self, condition_str: &str, start: usize, end: usize) -> std::result::Result<Vec<u32>, String> {
        let condition = query::parse_condition(condition_str)?;
        let start = start.min(self.len);
        let end = end.min(self.len);
        let mut indices = Vec::new();
        for i in start..end {
            if self.eval_condition(&condition, i) {
                indices.push(i as u32);
            }
        }
        Ok(indices)
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

    pub fn eval_condition(&self, cond: &Condition, row: usize) -> bool {
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
    use arrow::array::{BooleanArray, StringArray, UInt16Array, UInt64Array, UInt8Array};

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
                FieldType::Str => {
                    let arr = col.as_any().downcast_ref::<StringArray>().unwrap();
                    if let ColumnData::Str(v) = &mut store.columns[col_idx] {
                        for i in 0..num_rows {
                            v.push(arr.value(i).to_string());
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
                } else if let Some(s) = val.as_str() {
                    store.push_str(col, s);
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
    use arrow::array::{BooleanArray, StringArray, UInt16Array, UInt64Array, UInt8Array};

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
                    FieldType::Str => {
                        let arr = col.as_any().downcast_ref::<StringArray>().unwrap();
                        if let ColumnData::Str(v) = &mut store.columns[col_idx] {
                            for i in 0..num_rows {
                                v.push(arr.value(i).to_string());
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

// ---------------------------------------------------------------------------
// LazyColumnStore — on-demand row group decoding
// ---------------------------------------------------------------------------

#[cfg(feature = "parquet")]
mod lazy {
    use super::*;
    use std::cell::RefCell;

    use arrow::array::{BooleanArray, StringArray, UInt16Array, UInt64Array, UInt8Array};

    const LRU_CAPACITY: usize = 3;

    /// Index mapping global row indices to row groups.
    struct RowGroupIndex {
        /// Cumulative row count at the END of each row group.
        cumulative: Vec<usize>,
    }

    impl RowGroupIndex {
        fn total_rows(&self) -> usize {
            self.cumulative.last().copied().unwrap_or(0)
        }

        fn num_row_groups(&self) -> usize {
            self.cumulative.len()
        }

        /// Map a global row index to (row_group_index, local_offset).
        fn locate(&self, global: usize) -> (usize, usize) {
            let rg = self.cumulative.partition_point(|&end| end <= global);
            let start = if rg == 0 { 0 } else { self.cumulative[rg - 1] };
            (rg, global - start)
        }

        /// Global start index of a row group.
        fn row_group_start(&self, rg: usize) -> usize {
            if rg == 0 { 0 } else { self.cumulative[rg - 1] }
        }

        /// Number of rows in a row group.
        fn row_group_len(&self, rg: usize) -> usize {
            let start = self.row_group_start(rg);
            self.cumulative[rg] - start
        }
    }

    /// Simple LRU cache for decoded row groups.
    struct LruCache {
        entries: Vec<(usize, ColumnStore)>,
    }

    impl LruCache {
        fn new() -> Self {
            Self { entries: Vec::with_capacity(LRU_CAPACITY) }
        }

        fn get(&mut self, rg: usize) -> Option<&ColumnStore> {
            if let Some(pos) = self.entries.iter().position(|(k, _)| *k == rg) {
                if pos > 0 {
                    let entry = self.entries.remove(pos);
                    self.entries.insert(0, entry);
                }
                Some(&self.entries[0].1)
            } else {
                None
            }
        }

        fn insert(&mut self, rg: usize, store: ColumnStore) {
            self.entries.retain(|(k, _)| *k != rg);
            if self.entries.len() >= LRU_CAPACITY {
                self.entries.pop();
            }
            self.entries.insert(0, (rg, store));
        }
    }

    /// A column store that decodes parquet row groups on demand.
    ///
    /// Holds the full compressed file in memory but only decodes a few row
    /// groups at a time via an LRU cache. Working memory is proportional to
    /// `LRU_CAPACITY × rows_per_row_group` rather than the total entry count.
    pub struct LazyColumnStore {
        header: TraceHeader,
        field_types: Vec<FieldType>,
        field_index: HashMap<String, usize>,
        index: RowGroupIndex,
        data: bytes::Bytes,
        cache: RefCell<LruCache>,
    }

    impl LazyColumnStore {
        /// Create from in-memory parquet bytes.
        pub fn from_bytes(data: &[u8]) -> Result<Self> {
            let data = bytes::Bytes::from(data.to_vec());

            let builder =
                parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(data.clone())
                    .map_err(|e| Error::Arrow(e.into()))?;

            let schema = builder.schema();
            let kv = schema
                .metadata()
                .get("gbtrace_header")
                .ok_or_else(|| Error::MissingHeader)?;
            let header: TraceHeader = serde_json::from_str(kv)?;
            header.validate()?;

            let metadata = builder.metadata();
            let mut cumulative = Vec::with_capacity(metadata.num_row_groups());
            let mut total = 0usize;
            for i in 0..metadata.num_row_groups() {
                total += metadata.row_group(i).num_rows() as usize;
                cumulative.push(total);
            }

            let field_types: Vec<FieldType> = header.fields.iter().map(|n| field_type(n)).collect();
            let field_index: HashMap<String, usize> = header
                .fields
                .iter()
                .enumerate()
                .map(|(i, f)| (f.clone(), i))
                .collect();

            Ok(Self {
                header,
                field_types,
                field_index,
                index: RowGroupIndex { cumulative },
                data,
                cache: RefCell::new(LruCache::new()),
            })
        }

        pub fn header(&self) -> &TraceHeader { &self.header }

        pub fn entry_count(&self) -> usize { self.index.total_rows() }

        pub fn num_row_groups(&self) -> usize { self.index.num_row_groups() }

        pub fn row_group_start(&self, rg: usize) -> usize { self.index.row_group_start(rg) }

        pub fn row_group_len(&self, rg: usize) -> usize { self.index.row_group_len(rg) }

        pub fn field_col(&self, name: &str) -> Option<usize> {
            self.field_index.get(name).copied()
        }

        /// Detect frame boundaries by scanning `ly` for vblank wraps.
        /// For multi-row-group files with frame-aligned groups, uses metadata.
        /// Otherwise scans ly values.
        pub fn frame_boundaries(&self) -> Vec<u32> {
            if self.index.num_row_groups() > 1 {
                (0..self.index.num_row_groups())
                    .map(|rg| self.index.row_group_start(rg) as u32)
                    .collect()
            } else {
                if self.field_col("ly").is_none() { return Vec::new(); }
                let mut boundaries = vec![0u32];
                let count = self.entry_count();
                for i in 1..count {
                    let prev = self.get_u8_named("ly", i - 1).unwrap_or(0);
                    let cur = self.get_u8_named("ly", i).unwrap_or(0);
                    if cur < prev && prev >= 144 {
                        boundaries.push(i as u32);
                    }
                }
                boundaries
            }
        }

        /// Ensure a row group is in the cache, decoding if necessary.
        fn ensure_loaded(&self, rg: usize) {
            let mut cache = self.cache.borrow_mut();
            if cache.get(rg).is_some() {
                return;
            }
            let store = self.decode_row_group(rg).expect("failed to decode row group");
            cache.insert(rg, store);
        }

        fn decode_row_group(&self, rg: usize) -> Result<ColumnStore> {
            let builder =
                parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(
                    self.data.clone(),
                ).map_err(|e| Error::Arrow(e.into()))?;

            let batch_reader = builder
                .with_row_groups(vec![rg])
                .with_batch_size(self.index.row_group_len(rg))
                .build()
                .map_err(|e| Error::Arrow(e.into()))?;

            let mut store = ColumnStore::new(self.header.clone());

            for batch_result in batch_reader {
                let batch = batch_result.map_err(Error::Arrow)?;
                let num_rows = batch.num_rows();

                for (col_idx, ft) in self.field_types.iter().enumerate() {
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
                        FieldType::Str => {
                            let arr = col.as_any().downcast_ref::<StringArray>().unwrap();
                            if let ColumnData::Str(v) = &mut store.columns[col_idx] {
                                for i in 0..num_rows {
                                    v.push(arr.value(i).to_string());
                                }
                            }
                        }
                    }
                }
                store.len += num_rows;
            }

            Ok(store)
        }

        // --- Access (triggers lazy loading) ---

        pub fn get_numeric_named(&self, name: &str, row: usize) -> Option<u64> {
            let col_idx = *self.field_index.get(name)?;
            Some(self.get_numeric(col_idx, row))
        }

        pub fn get_u8_named(&self, name: &str, row: usize) -> Option<u8> {
            self.get_numeric_named(name, row).map(|v| v as u8)
        }

        pub fn get_u16_named(&self, name: &str, row: usize) -> Option<u16> {
            self.get_numeric_named(name, row).map(|v| v as u16)
        }

        pub fn get_bool_named(&self, name: &str, row: usize) -> Option<bool> {
            let col_idx = *self.field_index.get(name)?;
            let (rg, local) = self.index.locate(row);
            self.ensure_loaded(rg);
            let cache = self.cache.borrow();
            Some(cache.entries.iter().find(|(k, _)| *k == rg)?.1.column(col_idx).get_bool(local))
        }

        /// Get a column value as numeric by column index and global row.
        pub fn get_numeric(&self, col_idx: usize, row: usize) -> u64 {
            let (rg, local) = self.index.locate(row);
            self.ensure_loaded(rg);
            let cache = self.cache.borrow();
            cache.entries.iter()
                .find(|(k, _)| *k == rg)
                .map(|(_, s)| s.column(col_idx).get_numeric(local))
                .unwrap_or(0)
        }

        /// Get the ColumnData type for a column (for serialization dispatch).
        pub fn column_type(&self, col_idx: usize) -> &FieldType {
            &self.field_types[col_idx]
        }

        /// Reconstruct a `TraceEntry` from a global row index.
        pub fn to_entry(&self, index: usize) -> TraceEntry {
            let (rg, local) = self.index.locate(index);
            self.ensure_loaded(rg);
            let cache = self.cache.borrow();
            cache.entries.iter().find(|(k, _)| *k == rg).unwrap().1.to_entry(local)
        }

        // --- Query (one row group at a time) ---

        pub fn query(&self, condition_str: &str) -> std::result::Result<Vec<u32>, String> {
            self.query_range(condition_str, 0, self.entry_count())
        }

        pub fn query_range(&self, condition_str: &str, start: usize, end: usize) -> std::result::Result<Vec<u32>, String> {
            let condition = crate::query::parse_condition(condition_str)?;
            let stateful = Self::is_stateful(&condition);
            let total = self.entry_count();
            let start = start.min(total);
            let end = end.min(total);
            let mut results = Vec::new();

            for rg in 0..self.index.num_row_groups() {
                let rg_start = self.index.row_group_start(rg);
                let rg_len = self.index.row_group_len(rg);
                let rg_end = rg_start + rg_len;

                // Skip row groups entirely outside the range
                if rg_end <= start || rg_start >= end { continue; }

                self.ensure_loaded(rg);
                let global_start = rg_start as u32;

                // Determine local bounds within this row group
                let local_start = if rg_start < start { start - rg_start } else { 0 };
                let local_end = if rg_end > end { end - rg_start } else { rg_len };

                // Handle first row of the local range
                if local_start < local_end {
                    let first = local_start;
                    if first == 0 && stateful && rg > 0 {
                        if self.eval_cross_boundary(&condition, rg) {
                            results.push(global_start);
                        }
                    } else {
                        let cache = self.cache.borrow();
                        let store = &cache.entries.iter().find(|(k, _)| *k == rg).unwrap().1;
                        if store.eval_condition(&condition, first) {
                            results.push(global_start + first as u32);
                        }
                    }

                    // Remaining rows
                    if local_start + 1 < local_end {
                        let cache = self.cache.borrow();
                        let store = &cache.entries.iter().find(|(k, _)| *k == rg).unwrap().1;
                        for local in (local_start + 1)..local_end {
                            if store.eval_condition(&condition, local) {
                                results.push(global_start + local as u32);
                            }
                        }
                    }
                }
            }

            Ok(results)
        }

        fn is_stateful(cond: &Condition) -> bool {
            match cond {
                Condition::FieldChanges { .. }
                | Condition::FieldChangesTo { .. }
                | Condition::FieldChangesFrom { .. }
                | Condition::PpuEntersMode(_)
                | Condition::LcdTurnsOn
                | Condition::LcdTurnsOff
                | Condition::TimerOverflow
                | Condition::InterruptFires(_)
                | Condition::FlagBecomesSet(_)
                | Condition::FlagBecomesClear(_) => true,
                Condition::FieldEquals { .. }
                | Condition::FlagSet(_)
                | Condition::FlagClear(_) => false,
                Condition::All(cs) | Condition::Any(cs) => cs.iter().any(Self::is_stateful),
            }
        }

        fn eval_cross_boundary(&self, cond: &Condition, rg: usize) -> bool {
            if rg == 0 { return false; }

            let prev_rg = rg - 1;
            self.ensure_loaded(prev_rg);
            self.ensure_loaded(rg);
            let cache = self.cache.borrow();
            let prev_store = &cache.entries.iter().find(|(k, _)| *k == prev_rg).unwrap().1;
            let cur_store = &cache.entries.iter().find(|(k, _)| *k == rg).unwrap().1;

            if prev_store.entry_count() == 0 || cur_store.entry_count() == 0 {
                return false;
            }

            // Build a 2-row temp store for boundary evaluation
            let prev_last = prev_store.entry_count() - 1;
            let mut boundary = ColumnStore::with_capacity(self.header.clone(), 2);
            let ncols = self.header.fields.len();
            for col in 0..ncols {
                boundary.push_u64(col, prev_store.column(col).get_numeric(prev_last));
            }
            boundary.finish_row();
            for col in 0..ncols {
                boundary.push_u64(col, cur_store.column(col).get_numeric(0));
            }
            boundary.finish_row();

            boundary.eval_condition(cond, 1)
        }

        /// Downsample a field for chart display.
        pub fn field_summary(
            &self,
            field: &str,
            start: usize,
            end: usize,
            buckets: usize,
        ) -> std::result::Result<Vec<f64>, String> {
            let col_idx = *self.field_index.get(field)
                .ok_or_else(|| format!("unknown field: {field}"))?;
            let total = self.entry_count();
            let end = end.min(total);
            let start = start.min(end);
            let range = end - start;

            if range == 0 || buckets == 0 {
                return Ok(Vec::new());
            }

            let mut out = Vec::with_capacity(buckets * 2);
            for b in 0..buckets {
                let b_start = start + (b * range) / buckets;
                let b_end = start + ((b + 1) * range) / buckets;
                if b_start >= b_end {
                    let v = if b_start > 0 {
                        self.get_numeric(col_idx, b_start.min(total - 1)) as f64
                    } else { 0.0 };
                    out.push(v);
                    out.push(v);
                    continue;
                }
                let mut min = f64::MAX;
                let mut max = f64::MIN;
                for i in b_start..b_end {
                    let v = self.get_numeric(col_idx, i) as f64;
                    if v < min { min = v; }
                    if v > max { max = v; }
                }
                out.push(min);
                out.push(max);
            }

            Ok(out)
        }

        /// Eagerly decode all row groups into a single ColumnStore.
        /// Used for operations that need the full data (e.g. prepare_for_diff).
        pub fn to_eager(&self) -> Result<ColumnStore> {
            let mut store = ColumnStore::with_capacity(self.header.clone(), self.entry_count());
            let ncols = self.header.fields.len();

            for rg in 0..self.index.num_row_groups() {
                self.ensure_loaded(rg);
                let cache = self.cache.borrow();
                let rg_store = &cache.entries.iter().find(|(k, _)| *k == rg).unwrap().1;

                for row in 0..rg_store.entry_count() {
                    for col in 0..ncols {
                        store.push_u64(col, rg_store.column(col).get_numeric(row));
                    }
                    store.finish_row();
                }
            }

            Ok(store)
        }
    }
}

#[cfg(feature = "parquet")]
pub use lazy::LazyColumnStore;

/// Load a lazy column store from in-memory parquet bytes (for WASM).
#[cfg(feature = "parquet")]
pub fn load_lazy_column_store_from_bytes(data: &[u8]) -> Result<LazyColumnStore> {
    LazyColumnStore::from_bytes(data)
}
