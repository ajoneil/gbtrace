//! DiffStore: lightweight comparison view over two TraceStores.
//!
//! Instead of copying/transforming traces for comparison, DiffStore holds
//! references to two original stores and maintains index mappings for
//! alignment (sync) and downsampling (tcycle → instruction collapse).
//!
//! No data is copied. All reads go through the index maps to the originals.

use crate::column_store::TraceStore;
use crate::error::{Error, Result};
use crate::profile::{field_type, FieldType};

/// A comparison view over two trace stores.
pub struct DiffStore<'a> {
    pub store_a: &'a dyn TraceStore,
    pub store_b: &'a dyn TraceStore,
    /// Maps aligned index → original entry index in store A.
    pub map_a: Vec<usize>,
    /// Maps aligned index → original entry index in store B.
    pub map_b: Vec<usize>,
    /// Cached per-field diff stats.
    field_stats: Option<Vec<FieldDiffStats>>,
}

/// Per-field diff statistics.
#[derive(Debug, Clone)]
pub struct FieldDiffStats {
    pub name: String,
    pub match_count: usize,
    pub diff_count: usize,
}

impl FieldDiffStats {
    pub fn match_pct(&self) -> f64 {
        let total = self.match_count + self.diff_count;
        if total == 0 { return 100.0; }
        self.match_count as f64 / total as f64 * 100.0
    }
}

impl<'a> DiffStore<'a> {
    /// Create a DiffStore by aligning two traces.
    ///
    /// Sync modes:
    /// - `None` or `Some("pc")`: align by first common PC value
    /// - `Some("none")`: no alignment, compare from entry 0
    /// - `Some("field=value")`: align both to first match of condition
    pub fn align(
        store_a: &'a dyn TraceStore,
        store_b: &'a dyn TraceStore,
        sync: Option<&str>,
    ) -> Result<Self> {
        let a_tcycle = matches!(store_a.header().trigger, crate::header::Trigger::Tcycle);
        let b_tcycle = matches!(store_b.header().trigger, crate::header::Trigger::Tcycle);

        // Build base index maps (all entries, or collapsed to instructions)
        let mut map_a = if a_tcycle && !b_tcycle {
            collapse_indices(store_a)?
        } else {
            (0..store_a.entry_count()).collect()
        };

        let mut map_b = if b_tcycle && !a_tcycle {
            collapse_indices(store_b)?
        } else {
            (0..store_b.entry_count()).collect()
        };

        // Apply sync alignment
        let sync_mode = sync.unwrap_or("pc");
        match sync_mode {
            "none" => {}
            "pc" => {
                align_by_pc(store_a, store_b, &mut map_a, &mut map_b);
            }
            condition => {
                align_by_condition(store_a, store_b, &mut map_a, &mut map_b, condition)?;
            }
        }

        // Truncate to the shorter of the two
        let len = map_a.len().min(map_b.len());
        map_a.truncate(len);
        map_b.truncate(len);

        Ok(Self {
            store_a,
            store_b,
            map_a,
            map_b,
            field_stats: None,
        })
    }

    /// Number of aligned entry pairs.
    pub fn len(&self) -> usize {
        self.map_a.len()
    }

    /// Get the original entry index in store A for an aligned index.
    pub fn original_a(&self, aligned_idx: usize) -> usize {
        self.map_a[aligned_idx]
    }

    /// Get the original entry index in store B for an aligned index.
    pub fn original_b(&self, aligned_idx: usize) -> usize {
        self.map_b[aligned_idx]
    }

    /// Check if a specific field differs at an aligned index.
    pub fn field_differs(&self, field: &str, aligned_idx: usize) -> bool {
        let col_a = self.store_a.field_col(field);
        let col_b = self.store_b.field_col(field);
        match (col_a, col_b) {
            (Some(ca), Some(cb)) => {
                let row_a = self.map_a[aligned_idx];
                let row_b = self.map_b[aligned_idx];
                let ft = field_type(field);
                match ft {
                    FieldType::Bool => {
                        self.store_a.get_bool(ca, row_a) != self.store_b.get_bool(cb, row_b)
                    }
                    FieldType::Str => {
                        self.store_a.get_str(ca, row_a) != self.store_b.get_str(cb, row_b)
                    }
                    _ => {
                        self.store_a.get_numeric(ca, row_a) != self.store_b.get_numeric(cb, row_b)
                    }
                }
            }
            _ => false, // field not in both stores
        }
    }

    /// Compute per-field diff statistics (cached after first call).
    pub fn compute_stats(&mut self) -> &[FieldDiffStats] {
        if self.field_stats.is_some() {
            return self.field_stats.as_ref().unwrap();
        }

        let fields_a = &self.store_a.header().fields;
        let fields_b = &self.store_b.header().fields;

        // Find common fields
        let mut stats = Vec::new();
        for field in fields_a {
            if fields_b.contains(field) {
                let col_a = self.store_a.field_col(field).unwrap();
                let col_b = self.store_b.field_col(field).unwrap();
                let ft = field_type(field);

                let mut match_count = 0usize;
                let mut diff_count = 0usize;

                for i in 0..self.len() {
                    let row_a = self.map_a[i];
                    let row_b = self.map_b[i];

                    let same = match ft {
                        FieldType::Bool => {
                            self.store_a.get_bool(col_a, row_a) == self.store_b.get_bool(col_b, row_b)
                        }
                        FieldType::Str => {
                            self.store_a.get_str(col_a, row_a) == self.store_b.get_str(col_b, row_b)
                        }
                        _ => {
                            self.store_a.get_numeric(col_a, row_a) == self.store_b.get_numeric(col_b, row_b)
                        }
                    };

                    if same { match_count += 1; } else { diff_count += 1; }
                }

                stats.push(FieldDiffStats {
                    name: field.clone(),
                    match_count,
                    diff_count,
                });
            }
        }

        self.field_stats = Some(stats);
        self.field_stats.as_ref().unwrap()
    }

    /// Overall match percentage across all common fields.
    pub fn overall_match_pct(&mut self) -> f64 {
        let stats = self.compute_stats();
        let total_matches: usize = stats.iter().map(|s| s.match_count).sum();
        let total_diffs: usize = stats.iter().map(|s| s.diff_count).sum();
        let total = total_matches + total_diffs;
        if total == 0 { return 100.0; }
        total_matches as f64 / total as f64 * 100.0
    }
}

// --- Alignment helpers ---

/// Build an index map that collapses T-cycle entries to instruction boundaries.
/// Picks one entry per PC change.
fn collapse_indices(store: &dyn TraceStore) -> Result<Vec<usize>> {
    let pc_col = store.field_col("pc")
        .ok_or_else(|| Error::Diff("no pc field for collapse".into()))?;
    let count = store.entry_count();
    if count == 0 { return Ok(vec![]); }

    let mut indices = vec![0]; // always include first entry
    let mut prev_pc = store.get_numeric(pc_col, 0);

    for i in 1..count {
        let cur_pc = store.get_numeric(pc_col, i);
        if cur_pc != prev_pc {
            indices.push(i);
        }
        prev_pc = cur_pc;
    }

    Ok(indices)
}

/// Align index maps by first common PC value.
fn align_by_pc(
    store_a: &dyn TraceStore,
    store_b: &dyn TraceStore,
    map_a: &mut Vec<usize>,
    map_b: &mut Vec<usize>,
) {
    let pc_col_a = store_a.field_col("pc");
    let pc_col_b = store_b.field_col("pc");

    if let (Some(ca), Some(cb)) = (pc_col_a, pc_col_b) {
        if map_a.is_empty() || map_b.is_empty() { return; }

        let pc_a = store_a.get_numeric(ca, map_a[0]) as u16;
        let pc_b = store_b.get_numeric(cb, map_b[0]) as u16;

        if pc_a == pc_b { return; } // already aligned

        // Find a common PC in the first 100 entries of each
        let target = (0..map_a.len().min(100))
            .find(|&i| store_a.get_numeric(ca, map_a[i]) as u16 == pc_b)
            .map(|_| pc_b)
            .or_else(|| {
                (0..map_b.len().min(100))
                    .find(|&i| store_b.get_numeric(cb, map_b[i]) as u16 == pc_a)
                    .map(|_| pc_a)
            });

        if let Some(target_pc) = target {
            if pc_a != target_pc {
                if let Some(pos) = map_a.iter().position(|&idx| {
                    store_a.get_numeric(ca, idx) as u16 == target_pc
                }) {
                    *map_a = map_a[pos..].to_vec();
                }
            }
            if pc_b != target_pc {
                if let Some(pos) = map_b.iter().position(|&idx| {
                    store_b.get_numeric(cb, idx) as u16 == target_pc
                }) {
                    *map_b = map_b[pos..].to_vec();
                }
            }
        }
    }
}

/// Align index maps by first match of a condition string.
fn align_by_condition(
    store_a: &dyn TraceStore,
    store_b: &dyn TraceStore,
    map_a: &mut Vec<usize>,
    map_b: &mut Vec<usize>,
    condition: &str,
) -> Result<()> {
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

    let matches_condition = |store: &dyn TraceStore, row: usize| -> bool {
        if let Some(col) = store.field_col(field) {
            let v = store.get_numeric(col, row);
            match op {
                '&' => (v & val) != 0,
                '=' => v == val,
                _ => false,
            }
        } else {
            false
        }
    };

    if let Some(pos) = map_a.iter().position(|&idx| matches_condition(store_a, idx)) {
        *map_a = map_a[pos..].to_vec();
    }
    if let Some(pos) = map_b.iter().position(|&idx| matches_condition(store_b, idx)) {
        *map_b = map_b[pos..].to_vec();
    }

    Ok(())
}
