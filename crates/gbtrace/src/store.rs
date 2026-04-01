//! Trace store trait and loading functions.
//!
//! `TraceStore` is the single interface for reading trace data. The primary
//! implementation is `GbtraceStore` (native .gbtrace format, chunk-based
//! lazy loading). `DownsampledStore` wraps a store for instruction-level
//! views of T-cycle data.

use arrow::array::ArrayRef;
use crate::error::Result;
use crate::header::TraceHeader;

/// A contiguous slice of an Arrow column within a single chunk.
pub struct ColumnSegment {
    pub array: ArrayRef,
    /// Offset within the array where this segment starts.
    pub offset: usize,
    /// Number of rows in this segment.
    pub len: usize,
}

// ---------------------------------------------------------------------------
// Trait — the single interface for reading trace data
// ---------------------------------------------------------------------------

/// Read-only access to trace data.
///
/// Implementations: `GbtraceStore` (native format), `DownsampledStore` (view wrapper).
pub trait TraceStore {
    fn header(&self) -> &TraceHeader;
    fn entry_count(&self) -> usize;
    fn field_col(&self, name: &str) -> Option<usize>;
    fn frame_boundaries(&self) -> Vec<u32>;

    // Column value access by (col_index, row_index)
    fn get_str(&self, col: usize, row: usize) -> String;
    fn get_numeric(&self, col: usize, row: usize) -> u64;
    fn get_bool(&self, col: usize, row: usize) -> bool;
    /// Whether the value at (col, row) is null.
    fn is_null(&self, col: usize, row: usize) -> bool;

    // Convenience accessors by field name (default implementations)
    fn get_numeric_named(&self, name: &str, row: usize) -> Option<u64> {
        self.field_col(name).map(|col| self.get_numeric(col, row))
    }

    fn get_str_named(&self, name: &str, row: usize) -> Option<String> {
        self.field_col(name).map(|col| self.get_str(col, row))
    }

    fn has_field(&self, name: &str) -> bool {
        self.field_col(name).is_some()
    }

    /// Get column segments for a field over a contiguous row range.
    /// Each segment is a slice of an Arrow array within one chunk.
    /// Returns None if bulk access is not supported (e.g. downsampled stores).
    fn get_column_segments(&self, _field: &str, _start: usize, _end: usize) -> Option<Vec<ColumnSegment>> {
        None
    }

    /// Evaluate a condition within a range and return matching global indices.
    fn query_range(&self, condition_str: &str, start: usize, end: usize) -> std::result::Result<Vec<u32>, String> {
        let condition = crate::query::parse_condition(condition_str)?;
        let total = self.entry_count();
        let start = start.min(total);
        let end = end.min(total);
        let mut indices = Vec::new();
        for i in start..end {
            if self.eval_condition_trait(&condition, i) {
                indices.push(i as u32);
            }
        }
        Ok(indices)
    }

    /// Evaluate a parsed condition against a single row.
    /// Default implementation handles stateless conditions via get_numeric/get_str.
    fn eval_condition_trait(&self, cond: &crate::query::Condition, row: usize) -> bool {
        use crate::query::Condition;
        match cond {
            Condition::FieldEquals { field, value } => {
                if let Some(col) = self.field_col(field) {
                    let v = self.get_numeric(col, row);
                    // Parse the condition value: supports 0x prefix (hex),
                    // bare hex, and decimal
                    if let Some(target) = parse_query_value(value) {
                        v == target
                    } else {
                        // Fall back to string comparison for non-numeric fields
                        let s = self.get_str(col, row);
                        s == *value
                    }
                } else {
                    false
                }
            }
            Condition::FieldChanges { field } => {
                if row == 0 { return false; }
                if let Some(col) = self.field_col(field) {
                    self.get_numeric(col, row) != self.get_numeric(col, row - 1)
                } else {
                    false
                }
            }
            Condition::FieldChangesTo { field, value } => {
                if row == 0 { return false; }
                if let Some(col) = self.field_col(field) {
                    let cur = self.get_numeric(col, row);
                    let prev = self.get_numeric(col, row - 1);
                    if cur == prev { return false; }
                    if let Some(target) = parse_query_value(value) {
                        cur == target
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            Condition::FieldChangesFrom { field, value } => {
                if row == 0 { return false; }
                if let Some(col) = self.field_col(field) {
                    let cur = self.get_numeric(col, row);
                    let prev = self.get_numeric(col, row - 1);
                    if cur == prev { return false; }
                    if let Some(target) = parse_query_value(value) {
                        prev == target
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            // Semantic conditions (PPU mode, LCD on/off, etc.) need previous row state
            _ => false,
        }
    }

    /// Downsample a field for chart display. Returns min/max pairs per bucket.
    fn field_summary(
        &self,
        field: &str,
        start: usize,
        end: usize,
        buckets: usize,
    ) -> std::result::Result<Vec<f64>, String> {
        let col_idx = self.field_col(field)
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
}

/// Parse a query value as a number. Supports:
/// - `0x1A` or `0X1A` (hex with prefix)
/// - `0d256` or `0D256` (decimal with prefix)
/// - `1a` (bare hex)
/// - `256` (decimal fallback)
fn parse_query_value(s: &str) -> Option<u64> {
    // Explicit hex prefix
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok();
    }
    // Explicit decimal prefix (RGBDS convention)
    if let Some(dec) = s.strip_prefix("0d").or_else(|| s.strip_prefix("0D")) {
        return dec.parse::<u64>().ok();
    }
    // Try bare hex first (most values in traces are hex)
    if let Ok(v) = u64::from_str_radix(s, 16) {
        return Some(v);
    }
    // Fall back to decimal
    s.parse::<u64>().ok()
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load a trace store from any supported format.
/// Detects format by magic bytes: GBTR (native), or JSONL (converted on load).
pub fn open_trace_store(path: impl AsRef<std::path::Path>) -> Result<Box<dyn TraceStore>> {
    let data = std::fs::read(path.as_ref())?;
    open_trace_store_from_bytes(&data)
}

/// Load from in-memory bytes, detecting format by magic.
pub fn open_trace_store_from_bytes(data: &[u8]) -> Result<Box<dyn TraceStore>> {
    // Native .gbtrace format
    if data.len() >= 4 && &data[..4] == crate::format::MAGIC {
        let store = crate::format::read::GbtraceStore::from_bytes(data)?;
        return Ok(Box::new(store));
    }

    // JSONL — convert to native format on load
    let store = crate::format::convert::jsonl_to_store(data)?;
    Ok(Box::new(store))
}

// Re-export DownsampledStore
pub use crate::downsample::DownsampledStore;
