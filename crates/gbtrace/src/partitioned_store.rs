//! Partitioned (lazy) column store backed by parquet row groups.
//!
//! `PartitionedStore` wraps a compressed parquet file and decodes row groups
//! on demand with an LRU cache. This keeps memory proportional to a few
//! decoded row groups rather than the entire trace.

use std::cell::RefCell;
use std::collections::HashMap;

use arrow::array::{BooleanArray, StringArray, UInt16Array, UInt64Array, UInt8Array};

use crate::column_store::{ColumnData, ColumnStore, TraceStore};
use crate::entry::TraceEntry;
use crate::error::{Error, Result};
use crate::header::TraceHeader;
use crate::profile::{field_type, FieldType};
use crate::query::Condition;

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
pub struct PartitionedStore {
    header: TraceHeader,
    field_types: Vec<FieldType>,
    field_index: HashMap<String, usize>,
    index: RowGroupIndex,
    data: bytes::Bytes,
    cache: RefCell<LruCache>,
    /// Explicit frame boundaries from parquet metadata.
    explicit_boundaries: Option<Vec<u32>>,
}

impl PartitionedStore {
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

        // Extract explicit frame boundaries from parquet file metadata
        let explicit_boundaries = metadata.file_metadata().key_value_metadata()
            .and_then(|kvs| kvs.iter().find(|kv| kv.key == "gbtrace_frame_boundaries"))
            .and_then(|kv| kv.value.as_ref())
            .map(|s| s.split(',').filter_map(|v| v.parse::<u32>().ok()).collect::<Vec<_>>());

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
            explicit_boundaries,
        })
    }

    pub fn header(&self) -> &TraceHeader { &self.header }

    pub fn entry_count(&self) -> usize { self.index.total_rows() }

    pub fn num_row_groups(&self) -> usize { self.index.num_row_groups() }

    pub fn row_group_start(&self, rg: usize) -> usize { self.index.row_group_start(rg) }

    pub fn row_group_len(&self, rg: usize) -> usize { self.index.row_group_len(rg) }

    /// Decode a single row group into an eager ColumnStore.
    pub fn row_group_store(&self, rg: usize) -> Result<ColumnStore> {
        self.decode_row_group(rg)
    }

    pub fn field_col(&self, name: &str) -> Option<usize> {
        self.field_index.get(name).copied()
    }

    /// Decode only the row groups overlapping [start..end) into an eager store.
    /// The returned store has entries re-indexed from 0.
    pub fn decode_range(&self, start: usize, end: usize) -> Result<ColumnStore> {
        let end = end.min(self.entry_count());
        let mut store = ColumnStore::with_capacity(self.header.clone(), end - start);
        let ncols = self.header.fields.len();

        for rg in 0..self.index.num_row_groups() {
            let rg_start = self.index.row_group_start(rg);
            let rg_end = rg_start + self.index.row_group_len(rg);

            // Skip row groups outside the range
            if rg_end <= start || rg_start >= end { continue; }

            self.ensure_loaded(rg);
            let cache = self.cache.borrow();
            let rg_store = &cache.entries.iter().find(|(k, _)| *k == rg).unwrap().1;

            let local_start = if start > rg_start { start - rg_start } else { 0 };
            let local_end = if end < rg_end { end - rg_start } else { rg_store.entry_count() };

            for row in local_start..local_end {
                for col in 0..ncols {
                    let src = rg_store.column(col);
                    match src {
                        ColumnData::Str(_) => store.push_str(col, src.get_str(row)),
                        ColumnData::Bool(_) => store.push_bool(col, src.get_bool(row)),
                        _ => store.push_u64(col, src.get_numeric(row)),
                    }
                }
                store.finish_row();
            }
        }

        // The sub-store represents a single frame range — mark entry 0
        // as the boundary so reconstruct_frames treats it as one frame.
        store.explicit_boundaries = Some(vec![0]);

        Ok(store)
    }

    /// Detect frame boundaries from row group starts.
    ///
    /// Row groups are aligned with frames by the parquet writer (flushed
    /// at ly wraps and full-frame pix dumps).
    pub fn frame_boundaries(&self) -> Vec<u32> {
        // Use explicit boundaries from parquet metadata if available
        // Use explicit boundaries from parquet metadata if available
        if let Some(ref boundaries) = self.explicit_boundaries {
            if !boundaries.is_empty() {
                return boundaries.clone();
            }
        }
        // Fallback to row group starts (cheap, no decoding)
        (0..self.index.num_row_groups())
            .map(|rg| self.index.row_group_start(rg) as u32)
            .collect()
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
                        if let ColumnData::U64(v) = store.column_mut(col_idx) {
                            v.extend_from_slice(arr.values());
                        }
                    }
                    FieldType::UInt16 => {
                        let arr = col.as_any().downcast_ref::<UInt16Array>().unwrap();
                        if let ColumnData::U16(v) = store.column_mut(col_idx) {
                            v.extend_from_slice(arr.values());
                        }
                    }
                    FieldType::UInt8 => {
                        let arr = col.as_any().downcast_ref::<UInt8Array>().unwrap();
                        if let ColumnData::U8(v) = store.column_mut(col_idx) {
                            v.extend_from_slice(arr.values());
                        }
                    }
                    FieldType::Bool => {
                        let arr = col.as_any().downcast_ref::<BooleanArray>().unwrap();
                        if let ColumnData::Bool(v) = store.column_mut(col_idx) {
                            for i in 0..num_rows {
                                v.push(arr.value(i));
                            }
                        }
                    }
                    FieldType::Str => {
                        let arr = col.as_any().downcast_ref::<StringArray>().unwrap();
                        if let ColumnData::Str(v) = store.column_mut(col_idx) {
                            for i in 0..num_rows {
                                v.push(arr.value(i).to_string());
                            }
                        }
                    }
                }
            }
            store.add_rows(num_rows);
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

    pub fn get_str_named(&self, name: &str, row: usize) -> Option<String> {
        let col_idx = *self.field_index.get(name)?;
        let (rg, local) = self.index.locate(row);
        self.ensure_loaded(rg);
        let cache = self.cache.borrow();
        Some(cache.entries.iter().find(|(k, _)| *k == rg)?.1.column(col_idx).get_str(local).to_string())
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
                    let src = rg_store.column(col);
                    match src {
                        ColumnData::Str(_) => store.push_str(col, src.get_str(row)),
                        ColumnData::Bool(_) => store.push_bool(col, src.get_bool(row)),
                        _ => store.push_u64(col, src.get_numeric(row)),
                    }
                }
                store.finish_row();
            }
        }

        // Propagate explicit frame boundaries
        store.explicit_boundaries = self.explicit_boundaries.clone();

        Ok(store)
    }
}

impl TraceStore for PartitionedStore {
    fn header(&self) -> &TraceHeader { &self.header }
    fn entry_count(&self) -> usize { self.index.total_rows() }
    fn field_col(&self, name: &str) -> Option<usize> { self.field_index.get(name).copied() }
    fn frame_boundaries(&self) -> Vec<u32> { self.frame_boundaries() }

    fn get_str(&self, col: usize, row: usize) -> String {
        self.get_str_named(&self.header.fields[col], row).unwrap_or_default()
    }
    fn get_numeric(&self, col: usize, row: usize) -> u64 {
        self.get_numeric(col, row)
    }
    fn get_bool(&self, col: usize, row: usize) -> bool {
        self.get_bool_named(&self.header.fields[col], row).unwrap_or(false)
    }

    fn query_range(&self, condition_str: &str, start: usize, end: usize) -> std::result::Result<Vec<u32>, String> {
        PartitionedStore::query_range(self, condition_str, start, end)
    }

    fn field_summary(
        &self,
        field: &str,
        start: usize,
        end: usize,
        buckets: usize,
    ) -> std::result::Result<Vec<f64>, String> {
        PartitionedStore::field_summary(self, field, start, end, buckets)
    }
}

/// Load a partitioned store from in-memory parquet bytes (for WASM).
pub fn load_partitioned_store_from_bytes(data: &[u8]) -> Result<PartitionedStore> {
    PartitionedStore::from_bytes(data)
}
