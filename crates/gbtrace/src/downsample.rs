//! Instruction-level downsampling of trace stores.
//!
//! `DownsampledStore` presents an instruction-level view of an underlying
//! T-cycle store by picking one entry per PC change.

use crate::column_store::TraceStore;
use crate::header::TraceHeader;

/// A decorator that presents an instruction-level view of an underlying
/// store. Picks one entry per instruction boundary (where PC changes),
/// mapping downsampled indices back to the original store transparently.
pub struct DownsampledStore<'a> {
    inner: &'a dyn TraceStore,
    /// Maps downsampled row index → original row index
    index_map: Vec<usize>,
}

impl<'a> DownsampledStore<'a> {
    /// Create a downsampled view from a pre-built index map.
    pub fn from_map(inner: &'a dyn TraceStore, index_map: Vec<usize>) -> Self {
        Self { inner, index_map }
    }

    /// Create a downsampled view by picking entries where PC changes.
    pub fn new(inner: &'a dyn TraceStore) -> Self {
        let pc_col = inner.field_col("pc");
        let mut index_map = Vec::new();

        if let Some(pc) = pc_col {
            let count = inner.entry_count();
            if count > 0 {
                index_map.push(0); // always include first entry
                let mut prev_pc = inner.get_numeric(pc, 0);
                for i in 1..count {
                    let cur_pc = inner.get_numeric(pc, i);
                    if cur_pc != prev_pc {
                        index_map.push(i);
                        prev_pc = cur_pc;
                    }
                }
            }
        } else {
            // No PC column — pass through all entries
            index_map = (0..inner.entry_count()).collect();
        }

        Self { inner, index_map }
    }

    /// Map a downsampled index back to the original store index.
    pub fn original_index(&self, downsampled: usize) -> Option<usize> {
        self.index_map.get(downsampled).copied()
    }

    /// Map an original store index to the nearest downsampled index.
    /// Returns the instruction that contains this T-cycle.
    pub fn downsampled_index(&self, original: usize) -> Option<usize> {
        match self.index_map.binary_search(&original) {
            Ok(i) => Some(i),
            Err(i) => if i > 0 { Some(i - 1) } else { Some(0) },
        }
    }

    /// Access the inner (full-resolution) store.
    pub fn inner(&self) -> &dyn TraceStore {
        self.inner
    }

    /// Access the index map.
    pub fn index_map(&self) -> &[usize] {
        &self.index_map
    }
}

impl<'a> TraceStore for DownsampledStore<'a> {
    fn header(&self) -> &TraceHeader {
        self.inner.header()
    }

    fn entry_count(&self) -> usize {
        self.index_map.len()
    }

    fn field_col(&self, name: &str) -> Option<usize> {
        self.inner.field_col(name)
    }

    fn frame_boundaries(&self) -> Vec<u32> {
        // Map original boundaries to downsampled indices
        let orig_boundaries = self.inner.frame_boundaries();
        let mut mapped = Vec::new();
        for &orig_entry in &orig_boundaries {
            // Find the first downsampled index >= this boundary
            match self.index_map.binary_search(&(orig_entry as usize)) {
                Ok(i) => mapped.push(i as u32),
                Err(i) => {
                    if i < self.index_map.len() {
                        mapped.push(i as u32);
                    }
                }
            }
        }
        mapped
    }

    fn get_str(&self, col: usize, row: usize) -> String {
        if let Some(&orig) = self.index_map.get(row) {
            self.inner.get_str(col, orig)
        } else {
            String::new()
        }
    }

    fn get_numeric(&self, col: usize, row: usize) -> u64 {
        if let Some(&orig) = self.index_map.get(row) {
            self.inner.get_numeric(col, orig)
        } else {
            0
        }
    }

    fn get_bool(&self, col: usize, row: usize) -> bool {
        if let Some(&orig) = self.index_map.get(row) {
            self.inner.get_bool(col, orig)
        } else {
            false
        }
    }

    fn is_null(&self, col: usize, row: usize) -> bool {
        if let Some(&orig) = self.index_map.get(row) {
            self.inner.is_null(col, orig)
        } else {
            true
        }
    }
}
