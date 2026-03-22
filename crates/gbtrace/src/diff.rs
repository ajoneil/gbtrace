use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use crate::entry::TraceEntry;
use crate::error::{Error, Result};
use crate::header::{CycleUnit, TraceHeader};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// How to align entries from two traces for comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AlignmentStrategy {
    /// Merge-join by normalised T-cycle count.
    Cycle,
    /// Match by instruction index (1st vs 1st, 2nd vs 2nd).
    Sequence,
    /// Auto-detect: use Sequence if either trace uses instruction counting,
    /// otherwise Cycle.
    Auto,
}

impl Default for AlignmentStrategy {
    fn default() -> Self {
        Self::Auto
    }
}

/// Configuration for a trace diff.
#[derive(Debug, Clone)]
pub struct DiffConfig {
    /// Only compare these fields (if `Some`). Applied after intersection.
    pub include_fields: Option<Vec<String>>,
    /// Exclude these fields from comparison.
    pub exclude_fields: Option<Vec<String>>,
    /// Alignment strategy.
    pub alignment: AlignmentStrategy,
    /// Skip boot ROM entries (advance to first pc=0x0100).
    pub skip_boot: bool,
    /// Max divergence regions to report.
    pub max_regions: usize,
    /// Context entries around first divergence.
    pub context: usize,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            include_fields: None,
            exclude_fields: None,
            alignment: AlignmentStrategy::Auto,
            skip_boot: false,
            max_regions: 10,
            context: 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Per-field divergence statistics.
#[derive(Debug, Clone, Serialize)]
pub struct FieldDivergence {
    pub field: String,
    pub count: u64,
    pub first_index: u64,
    pub first_val_a: Value,
    pub first_val_b: Value,
}

/// A contiguous range of divergent entries.
#[derive(Debug, Clone, Serialize)]
pub struct DivergenceRegion {
    pub start_index: u64,
    pub end_index: u64,
    pub count: usize,
}

/// High-level classification of a divergence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DivergenceClass {
    /// Traces are identical on all compared fields.
    Identical,
    /// Only the F register (flags) diverges; pc/op match.
    FlagOnly,
    /// The program counter diverges — emulators took different code paths.
    ExecutionPathSplit,
    /// Registers differ but instruction sequence (pc, op) matches.
    RegisterDrift,
    /// Only cycle counts differ (not usually visible since cy is excluded).
    TimingOnly,
    /// A combination of the above.
    Mixed,
}

impl std::fmt::Display for DivergenceClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Identical => write!(f, "identical"),
            Self::FlagOnly => write!(f, "flag-only"),
            Self::ExecutionPathSplit => write!(f, "execution-path-split"),
            Self::RegisterDrift => write!(f, "register-drift"),
            Self::TimingOnly => write!(f, "timing-only"),
            Self::Mixed => write!(f, "mixed"),
        }
    }
}

/// One entry in the context window around the first divergence.
#[derive(Debug, Clone, Serialize)]
pub struct ContextEntry {
    /// Index (instruction index or cycle, depending on alignment).
    pub index: u64,
    pub vals_a: BTreeMap<String, Value>,
    pub vals_b: BTreeMap<String, Value>,
    pub is_divergent: bool,
    pub divergent_fields: Vec<String>,
}

/// Full result of a pairwise trace comparison.
#[derive(Debug, Clone, Serialize)]
pub struct DiffResult {
    pub name_a: String,
    pub name_b: String,
    pub entries_a: usize,
    pub entries_b: usize,
    pub aligned_count: usize,
    pub overlap_pct: f64,
    pub alignment_used: AlignmentStrategy,
    pub common_fields: Vec<String>,
    pub only_in_a: Vec<String>,
    pub only_in_b: Vec<String>,
    pub rom_mismatch: bool,
    pub boot_rom_mismatch: bool,
    pub field_divergences: Vec<FieldDivergence>,
    pub regions: Vec<DivergenceRegion>,
    pub total_divergent: usize,
    pub classification: DivergenceClass,
    pub context_window: Vec<ContextEntry>,
}

impl DiffResult {
    pub fn is_identical(&self) -> bool {
        self.classification == DivergenceClass::Identical
    }
}

/// Result of an N-way comparison (all pairwise results).
#[derive(Debug, Clone, Serialize)]
pub struct MultiDiffResult {
    pub pairwise: Vec<DiffResult>,
}

// ---------------------------------------------------------------------------
// Internal row type
// ---------------------------------------------------------------------------

type Row = BTreeMap<String, Value>;

fn entry_to_row(entry: &TraceEntry, fields: &[String]) -> Row {
    let mut row = Row::new();
    for f in fields {
        if let Some(v) = entry.get(f) {
            row.insert(f.clone(), v.clone());
        }
    }
    row
}

// ---------------------------------------------------------------------------
// TraceDiffer
// ---------------------------------------------------------------------------

/// Compares two (or more) traces.
pub struct TraceDiffer {
    config: DiffConfig,
}

impl TraceDiffer {
    pub fn new(config: DiffConfig) -> Self {
        Self { config }
    }

    /// Compare two traces. Accepts pre-collected entries (since both alignment
    /// strategies need random access).
    pub fn compare(
        &self,
        header_a: &TraceHeader,
        entries_a: Vec<TraceEntry>,
        header_b: &TraceHeader,
        entries_b: Vec<TraceEntry>,
    ) -> Result<DiffResult> {
        let name_a = header_a.emulator.clone();
        let name_b = header_b.emulator.clone();

        // Resolve alignment
        let use_sequence = self.resolve_alignment(header_a, header_b)?;
        let alignment_used = if use_sequence {
            AlignmentStrategy::Sequence
        } else {
            AlignmentStrategy::Cycle
        };

        // Common fields
        let fields_a: BTreeSet<&str> = header_a.fields.iter().map(|s| s.as_str()).collect();
        let fields_b: BTreeSet<&str> = header_b.fields.iter().map(|s| s.as_str()).collect();

        let mut common_fields: Vec<String> = fields_a
            .intersection(&fields_b)
            .map(|s| s.to_string())
            .collect();
        common_fields.sort();

        // Apply include/exclude
        if let Some(ref include) = self.config.include_fields {
            let set: BTreeSet<&str> = include.iter().map(|s| s.as_str()).collect();
            common_fields.retain(|f| set.contains(f.as_str()));
        }
        if let Some(ref exclude) = self.config.exclude_fields {
            let set: BTreeSet<&str> = exclude.iter().map(|s| s.as_str()).collect();
            common_fields.retain(|f| !set.contains(f.as_str()));
        }

        if common_fields.is_empty() {
            return Err(Error::Diff("no common fields to compare".into()));
        }

        let only_in_a: Vec<String> = fields_a
            .difference(&fields_b)
            .map(|s| s.to_string())
            .collect();
        let only_in_b: Vec<String> = fields_b
            .difference(&fields_a)
            .map(|s| s.to_string())
            .collect();

        // All fields we need (common + extras for context)
        let mut all_fields: Vec<String> = common_fields.clone();
        for extra in &["pc", "op", "a"] {
            let s = extra.to_string();
            if !all_fields.contains(&s)
                && (fields_a.contains(extra) || fields_b.contains(extra))
            {
                all_fields.push(s);
            }
        }

        // Load rows with optional boot skip and cycle normalisation
        let normalize = !use_sequence;
        let mut rows_a = self.load_rows(entries_a, &all_fields, &header_a.cy_unit, normalize);
        let mut rows_b = self.load_rows(entries_b, &all_fields, &header_b.cy_unit, normalize);

        let entries_a_count = rows_a.len();
        let entries_b_count = rows_b.len();

        // Rebase cycles if skip_boot and bases differ
        if !use_sequence && self.config.skip_boot && !rows_a.is_empty() && !rows_b.is_empty() {
            let base_a = rows_a[0].0;
            let base_b = rows_b[0].0;
            if base_a != base_b {
                for row in &mut rows_a {
                    row.0 -= base_a;
                }
                for row in &mut rows_b {
                    row.0 -= base_b;
                }
            }
        }

        // Align
        let merged = if use_sequence {
            self.align_sequence(&rows_a, &rows_b)
        } else {
            self.align_cycle(&rows_a, &rows_b)
        };

        if merged.is_empty() {
            return Err(Error::Diff("no aligned entries (traces may not overlap)".into()));
        }

        let aligned_count = merged.len();
        let overlap_pct =
            aligned_count as f64 / entries_a_count.max(entries_b_count) as f64 * 100.0;

        // Per-field divergences
        let field_divergences = self.compute_field_divergences(&merged, &common_fields);

        // Divergent indices
        let div_indices: Vec<usize> = merged
            .iter()
            .enumerate()
            .filter(|(_, m)| is_divergent(&m.vals_a, &m.vals_b, &common_fields))
            .map(|(i, _)| i)
            .collect();

        let total_divergent = div_indices.len();
        let regions = group_divergence_ranges(&merged, &div_indices);

        // Classification
        let classification = classify(&field_divergences, &common_fields);

        // Context window
        let context_window = if !div_indices.is_empty() {
            self.build_context_window(&merged, &common_fields, div_indices[0])
        } else {
            vec![]
        };

        Ok(DiffResult {
            name_a,
            name_b,
            entries_a: entries_a_count,
            entries_b: entries_b_count,
            aligned_count,
            overlap_pct,
            alignment_used,
            common_fields,
            only_in_a,
            only_in_b,
            rom_mismatch: header_a.rom_sha256 != header_b.rom_sha256,
            boot_rom_mismatch: header_a.boot_rom != header_b.boot_rom,
            field_divergences,
            regions,
            total_divergent,
            classification,
            context_window,
        })
    }

    /// Compare N traces pairwise.
    pub fn compare_multi(
        &self,
        traces: Vec<(TraceHeader, Vec<TraceEntry>)>,
    ) -> Result<MultiDiffResult> {
        let mut pairwise = Vec::new();
        for i in 0..traces.len() {
            for j in (i + 1)..traces.len() {
                let result = self.compare(
                    &traces[i].0,
                    traces[i].1.clone(),
                    &traces[j].0,
                    traces[j].1.clone(),
                )?;
                pairwise.push(result);
            }
        }
        Ok(MultiDiffResult { pairwise })
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn resolve_alignment(
        &self,
        header_a: &TraceHeader,
        header_b: &TraceHeader,
    ) -> Result<bool> {
        match &self.config.alignment {
            AlignmentStrategy::Sequence => Ok(true),
            AlignmentStrategy::Cycle => {
                if header_a.cy_unit == CycleUnit::Instruction
                    || header_b.cy_unit == CycleUnit::Instruction
                {
                    return Err(Error::Diff(
                        "cycle alignment requested but a trace uses instruction counting".into(),
                    ));
                }
                Ok(false)
            }
            AlignmentStrategy::Auto => Ok(
                header_a.cy_unit == CycleUnit::Instruction
                    || header_b.cy_unit == CycleUnit::Instruction,
            ),
        }
    }

    fn load_rows(
        &self,
        entries: Vec<TraceEntry>,
        all_fields: &[String],
        cy_unit: &CycleUnit,
        normalize: bool,
    ) -> Vec<(u64, Row)> {
        let mut rows = Vec::new();
        let mut skipping = self.config.skip_boot;

        for entry in entries {
            if skipping {
                if entry.get_u16("pc") == Some(0x0100) {
                    skipping = false;
                } else {
                    continue;
                }
            }

            let raw_cy = entry.cy().unwrap_or(0);
            let cy = if normalize {
                cy_unit.to_tcycles(raw_cy).unwrap_or(raw_cy)
            } else {
                raw_cy
            };

            let row = entry_to_row(&entry, all_fields);
            rows.push((cy, row));
        }
        rows
    }

    fn align_sequence(
        &self,
        rows_a: &[(u64, Row)],
        rows_b: &[(u64, Row)],
    ) -> Vec<MergedRow> {
        let len = rows_a.len().min(rows_b.len());
        (0..len)
            .map(|idx| MergedRow {
                index: idx as u64,
                vals_a: rows_a[idx].1.clone(),
                vals_b: rows_b[idx].1.clone(),
            })
            .collect()
    }

    fn align_cycle(
        &self,
        rows_a: &[(u64, Row)],
        rows_b: &[(u64, Row)],
    ) -> Vec<MergedRow> {
        let mut merged = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < rows_a.len() && j < rows_b.len() {
            let cy_a = rows_a[i].0;
            let cy_b = rows_b[j].0;
            match cy_a.cmp(&cy_b) {
                std::cmp::Ordering::Equal => {
                    merged.push(MergedRow {
                        index: cy_a,
                        vals_a: rows_a[i].1.clone(),
                        vals_b: rows_b[j].1.clone(),
                    });
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
            }
        }
        merged
    }

    fn compute_field_divergences(
        &self,
        merged: &[MergedRow],
        compare_fields: &[String],
    ) -> Vec<FieldDivergence> {
        let mut divs: Vec<FieldDivergence> = Vec::new();
        for field in compare_fields {
            let mut count = 0u64;
            let mut first: Option<(u64, Value, Value)> = None;
            for row in merged {
                let a = row.vals_a.get(field);
                let b = row.vals_b.get(field);
                if a != b {
                    count += 1;
                    if first.is_none() {
                        first = Some((
                            row.index,
                            a.cloned().unwrap_or(Value::Null),
                            b.cloned().unwrap_or(Value::Null),
                        ));
                    }
                }
            }
            if count > 0 {
                let (idx, va, vb) = first.unwrap();
                divs.push(FieldDivergence {
                    field: field.clone(),
                    count,
                    first_index: idx,
                    first_val_a: va,
                    first_val_b: vb,
                });
            }
        }
        divs.sort_by_key(|d| d.first_index);
        divs
    }

    fn build_context_window(
        &self,
        merged: &[MergedRow],
        compare_fields: &[String],
        first_div_idx: usize,
    ) -> Vec<ContextEntry> {
        let start = first_div_idx.saturating_sub(self.config.context);
        let end = (first_div_idx + 5 + self.config.context).min(merged.len());
        (start..end)
            .map(|idx| {
                let row = &merged[idx];
                let divf = divergent_fields(&row.vals_a, &row.vals_b, compare_fields);
                ContextEntry {
                    index: row.index,
                    vals_a: row.vals_a.clone(),
                    vals_b: row.vals_b.clone(),
                    is_divergent: !divf.is_empty(),
                    divergent_fields: divf,
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Internal types and helpers
// ---------------------------------------------------------------------------

struct MergedRow {
    index: u64,
    vals_a: Row,
    vals_b: Row,
}

fn is_divergent(a: &Row, b: &Row, fields: &[String]) -> bool {
    fields.iter().any(|f| a.get(f) != b.get(f))
}

fn divergent_fields(a: &Row, b: &Row, fields: &[String]) -> Vec<String> {
    fields
        .iter()
        .filter(|f| a.get(*f) != b.get(*f))
        .cloned()
        .collect()
}

fn group_divergence_ranges(merged: &[MergedRow], div_indices: &[usize]) -> Vec<DivergenceRegion> {
    if div_indices.is_empty() {
        return vec![];
    }

    let mut ranges = Vec::new();
    let mut range_start = 0usize;
    let mut range_end = 0usize;

    for k in 1..div_indices.len() {
        if div_indices[k] - div_indices[k - 1] <= 2 {
            range_end = k;
        } else {
            ranges.push(DivergenceRegion {
                start_index: merged[div_indices[range_start]].index,
                end_index: merged[div_indices[range_end]].index,
                count: range_end - range_start + 1,
            });
            range_start = k;
            range_end = k;
        }
    }
    ranges.push(DivergenceRegion {
        start_index: merged[div_indices[range_start]].index,
        end_index: merged[div_indices[range_end]].index,
        count: range_end - range_start + 1,
    });

    ranges
}

/// Classify the divergence based on which fields diverge.
pub fn classify(
    field_divergences: &[FieldDivergence],
    _common_fields: &[String],
) -> DivergenceClass {
    if field_divergences.is_empty() {
        return DivergenceClass::Identical;
    }

    let div_names: BTreeSet<&str> = field_divergences.iter().map(|d| d.field.as_str()).collect();

    // TimingOnly no longer applies since cy is removed

    let pc_diverges = div_names.contains("pc");
    let op_diverges = div_names.contains("op");

    // Only flags differ, execution path is the same
    let flag_fields: BTreeSet<&str> = ["f", "fz", "fn", "fh", "fc"].iter().copied().collect();
    let non_flag: BTreeSet<&str> = div_names.difference(&flag_fields).copied().collect();
    if non_flag.is_empty() {
        return DivergenceClass::FlagOnly;
    }

    // PC diverges → different code paths
    if pc_diverges {
        return DivergenceClass::ExecutionPathSplit;
    }

    // Same instruction sequence but register values differ
    if !op_diverges {
        // All diverging fields are registers/memory, not control flow
        let control_fields: BTreeSet<&str> = ["pc", "op"].iter().copied().collect();
        let non_control: BTreeSet<&str> = div_names.difference(&control_fields).copied().collect();
        if !non_control.is_empty() {
            return DivergenceClass::RegisterDrift;
        }
    }

    DivergenceClass::Mixed
}
