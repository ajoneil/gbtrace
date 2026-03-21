use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use gbtrace::{AnyTraceReader, ParquetTraceWriter, TraceEntry, TraceWriter};
use serde_json::Value;

#[derive(Parser)]
#[command(name = "gbtrace-cli", about = "Inspect and compare GB Trace files")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show summary information about a trace file
    Info {
        /// Trace file to inspect
        input: PathBuf,
    },
    /// Convert between trace file formats (JSONL <-> Parquet)
    Convert {
        /// Input file (.gbtrace, .gbtrace.gz, or .parquet)
        input: PathBuf,
        /// Output file (.gbtrace, .gbtrace.gz, or .parquet)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Strip boot ROM entries from a trace, keeping only post-boot data
    StripBoot {
        /// Input trace file
        input: PathBuf,
        /// Output file (default: overwrite input)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Find entries matching a condition (e.g. pc=0x0150, a=0x01)
    Query {
        /// Trace file to search
        input: PathBuf,
        /// Condition as field=value (e.g. pc=0x0150)
        #[arg(long, short)]
        r#where: Vec<String>,
        /// Max results to show (default: 10)
        #[arg(long, default_value_t = 10)]
        max: usize,
        /// Show context entries around each match
        #[arg(long, default_value_t = 0)]
        context: usize,
    },
    /// Trim a trace: keep entries up to or after a condition
    Trim {
        /// Input trace file
        input: PathBuf,
        /// Output file (default: derive from input)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Keep entries up to and including the first match of this condition
        #[arg(long, conflicts_with = "after")]
        until: Option<String>,
        /// Keep entries starting from the first match of this condition
        #[arg(long, conflicts_with = "until")]
        after: Option<String>,
    },
    /// Compare two trace files and report divergences
    Diff {
        /// First trace file (reference)
        trace_a: PathBuf,
        /// Second trace file (to compare)
        trace_b: PathBuf,
        /// Max divergence regions to show (default: 10)
        #[arg(long, default_value_t = 10)]
        max: usize,
        /// Context entries before/after first divergence (default: 2)
        #[arg(long, default_value_t = 2)]
        context: usize,
        /// Only compare these fields (comma-separated, e.g. pc,a,f)
        #[arg(long)]
        fields: Option<String>,
        /// Exclude these fields from comparison (comma-separated, e.g. ime,ly)
        #[arg(long)]
        exclude: Option<String>,
        /// Ignore boot ROM entries (skip to first pc=0x0100)
        #[arg(long)]
        skip_boot: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Info { input } => cmd_info(&input),
        Command::Convert { input, output } => cmd_convert(&input, output),
        Command::StripBoot { input, output } => cmd_strip_boot(&input, output),
        Command::Query { input, r#where: conditions, max, context } => {
            cmd_query(&input, &conditions, max, context)
        }
        Command::Trim { input, output, until, after } => {
            cmd_trim(&input, output, until, after)
        }
        Command::Diff {
            trace_a,
            trace_b,
            max,
            context,
            fields,
            exclude,
            skip_boot,
        } => cmd_diff(&trace_a, &trace_b, max, context, fields, exclude, skip_boot),
    };
    process::exit(code);
}

// ---------------------------------------------------------------------------
// info
// ---------------------------------------------------------------------------

fn cmd_info(path: &PathBuf) -> i32 {
    let reader = match AnyTraceReader::open(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let h = reader.header();
    println!("File:      {}", path.display());
    println!("Emulator:  {}", h.emulator);
    println!("Version:   {}", h.emulator_version);
    println!("Model:     {}", h.model);
    println!("Profile:   {}", h.profile);
    println!("Trigger:   {:?}", h.trigger);
    println!("Boot ROM:  {}", format_boot_rom(&h.boot_rom));
    println!("ROM hash:  {}", h.rom_sha256);
    println!("Fields:    {}", h.fields.join(", "));

    let mut count: u64 = 0;
    let mut cy_min: Option<u64> = None;
    let mut cy_max: Option<u64> = None;

    for result in reader {
        match result {
            Ok(entry) => {
                count += 1;
                if let Some(cy) = entry.cy() {
                    if cy_min.is_none() {
                        cy_min = Some(cy);
                    }
                    cy_max = Some(cy);
                }
            }
            Err(e) => {
                eprintln!("Error reading entry {count}: {e}");
                return 1;
            }
        }
    }

    println!("Entries:   {count}");
    if let (Some(min), Some(max)) = (cy_min, cy_max) {
        println!("Cy range:  {min} .. {max}");
    }

    if let Ok(meta) = std::fs::metadata(path) {
        let size = meta.len();
        println!("File size: {size} bytes ({:.1} MB)", size as f64 / 1024.0 / 1024.0);
    }

    0
}

// ---------------------------------------------------------------------------
// convert
// ---------------------------------------------------------------------------

fn cmd_convert(input: &PathBuf, output: Option<PathBuf>) -> i32 {
    let output = output.unwrap_or_else(|| {
        let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext == "parquet" {
            input.with_extension("gbtrace")
        } else {
            // .gbtrace or .gbtrace.gz -> .parquet
            let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("trace");
            let stem = stem.strip_suffix(".gbtrace").unwrap_or(stem);
            input.with_file_name(format!("{stem}.parquet"))
        }
    });

    let reader = match AnyTraceReader::open(input) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error opening input: {e}");
            return 1;
        }
    };

    let header = reader.header().clone();
    let is_parquet_output = output.extension().is_some_and(|e| e == "parquet");

    if is_parquet_output {
        let mut writer = match ParquetTraceWriter::create(&output, &header) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Error creating output: {e}");
                return 1;
            }
        };

        let mut count: u64 = 0;
        for result in reader {
            match result {
                Ok(entry) => {
                    if let Err(e) = writer.write_entry(&entry) {
                        eprintln!("Error writing entry {count}: {e}");
                        return 1;
                    }
                    count += 1;
                }
                Err(e) => {
                    eprintln!("Error reading entry {count}: {e}");
                    return 1;
                }
            }
        }

        if let Err(e) = writer.finish() {
            eprintln!("Error finalizing: {e}");
            return 1;
        }

        let input_size = std::fs::metadata(input).map(|m| m.len()).unwrap_or(0);
        let output_size = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
        let ratio = if output_size > 0 {
            input_size as f64 / output_size as f64
        } else {
            0.0
        };
        println!("Converted {count} entries to {}", output.display());
        println!("  {input_size} bytes -> {output_size} bytes ({ratio:.1}x compression)");
    } else {
        // Output as JSONL
        let mut writer = match TraceWriter::create(&output, &header) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Error creating output: {e}");
                return 1;
            }
        };

        let mut count: u64 = 0;
        for result in reader {
            match result {
                Ok(entry) => {
                    if let Err(e) = writer.write_entry(&entry) {
                        eprintln!("Error writing entry {count}: {e}");
                        return 1;
                    }
                    count += 1;
                }
                Err(e) => {
                    eprintln!("Error reading entry {count}: {e}");
                    return 1;
                }
            }
        }

        if let Err(e) = writer.finish() {
            eprintln!("Error finalizing: {e}");
            return 1;
        }
        println!("Converted {count} entries to {}", output.display());
    }

    0
}

// ---------------------------------------------------------------------------
// Shared: condition matching
// ---------------------------------------------------------------------------

/// A parsed field=value condition.
struct Condition {
    field: String,
    value: String,
}

fn parse_condition(s: &str) -> Result<Condition, String> {
    let eq = s.find('=').ok_or_else(|| format!("invalid condition '{s}': expected field=value"))?;
    let field = s[..eq].trim().to_string();
    let value = s[eq + 1..].trim().to_string();
    if field.is_empty() || value.is_empty() {
        return Err(format!("invalid condition '{s}': field and value must be non-empty"));
    }
    Ok(Condition { field, value })
}

/// Parse a comma-separated list of conditions: "field1=val1,field2=val2"
fn parse_conditions(s: &str) -> Result<Vec<Condition>, String> {
    s.split(',').map(|part| parse_condition(part.trim())).collect()
}

fn entry_matches(entry: &TraceEntry, conditions: &[Condition]) -> bool {
    conditions.iter().all(|c| {
        entry.get(&c.field).map_or(false, |v| value_to_string(v) == c.value)
    })
}

// ---------------------------------------------------------------------------
// Shared: format-aware writer
// ---------------------------------------------------------------------------

enum AnyWriter {
    Jsonl(TraceWriter),
    Parquet(ParquetTraceWriter),
}

impl AnyWriter {
    fn create(path: &std::path::Path, header: &gbtrace::TraceHeader) -> Result<Self, gbtrace::Error> {
        if path.extension().is_some_and(|e| e == "parquet") {
            Ok(Self::Parquet(ParquetTraceWriter::create(path, header)?))
        } else {
            Ok(Self::Jsonl(TraceWriter::create(path, header)?))
        }
    }

    fn write_entry(&mut self, entry: &TraceEntry) -> Result<(), gbtrace::Error> {
        match self {
            Self::Jsonl(w) => w.write_entry(entry),
            Self::Parquet(w) => w.write_entry(entry),
        }
    }

    fn finish(self) -> Result<(), gbtrace::Error> {
        match self {
            Self::Jsonl(w) => w.finish(),
            Self::Parquet(w) => w.finish(),
        }
    }
}

fn default_output(input: &PathBuf, suffix: &str) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("trace");
    let stem = stem.strip_suffix(".gbtrace").unwrap_or(stem);
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("gbtrace");
    input.with_file_name(format!("{stem}{suffix}.{ext}"))
}

// ---------------------------------------------------------------------------
// strip-boot
// ---------------------------------------------------------------------------

fn cmd_strip_boot(input: &PathBuf, output: Option<PathBuf>) -> i32 {
    let reader = match AnyTraceReader::open(input) {
        Ok(r) => r,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let mut header = reader.header().clone();

    // Update header to reflect stripping
    header.boot_rom = header.boot_rom.to_stripped();

    let output = output.unwrap_or_else(|| default_output(input, "_stripped"));

    let mut writer = match AnyWriter::create(&output, &header) {
        Ok(w) => w,
        Err(e) => { eprintln!("Error creating output: {e}"); return 1; }
    };

    let mut skipping = true;
    let mut skipped: u64 = 0;
    let mut written: u64 = 0;
    let mut cy_base: Option<u64> = None;

    for result in reader {
        let mut entry = match result {
            Ok(e) => e,
            Err(e) => { eprintln!("Error reading: {e}"); return 1; }
        };

        if skipping {
            if let Some(Value::String(pc)) = entry.get("pc") {
                if pc == "0x0100" {
                    skipping = false;
                    cy_base = entry.cy();
                } else {
                    skipped += 1;
                    continue;
                }
            } else {
                skipped += 1;
                continue;
            }
        }

        // Rebase cycle count
        if let (Some(cy), Some(base)) = (entry.cy(), cy_base) {
            entry.set_cy(cy - base);
        }

        if let Err(e) = writer.write_entry(&entry) {
            eprintln!("Error writing: {e}");
            return 1;
        }
        written += 1;
    }

    if let Err(e) = writer.finish() {
        eprintln!("Error finalizing: {e}");
        return 1;
    }

    if skipping {
        eprintln!("WARNING: no entry with pc=0x0100 found, trace may not contain boot data");
    }

    println!("Stripped {skipped} boot entries, wrote {written} entries to {}", output.display());
    println!("  boot_rom: {}", format_boot_rom(&header.boot_rom));

    0
}

// ---------------------------------------------------------------------------
// query
// ---------------------------------------------------------------------------

fn cmd_query(input: &PathBuf, conditions: &[String], max: usize, context: usize) -> i32 {
    if conditions.is_empty() {
        eprintln!("Error: at least one --where condition required");
        return 1;
    }

    let parsed: Vec<Condition> = match conditions.iter().map(|s| parse_condition(s)).collect() {
        Ok(c) => c,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let reader = match AnyTraceReader::open(input) {
        Ok(r) => r,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let fields = reader.header().fields.clone();

    // Ring buffer for context-before entries
    let mut ring: Vec<(u64, TraceEntry)> = Vec::new();
    let mut matches_found: usize = 0;
    let mut entry_idx: u64 = 0;
    // Track how many context-after lines remain to print
    let mut context_after_remaining: usize = 0;
    let mut displayed_matches: usize = 0;

    for result in reader {
        let entry = match result {
            Ok(e) => e,
            Err(e) => { eprintln!("Error reading: {e}"); return 1; }
        };

        let cy = entry.cy().unwrap_or(0);
        let is_match = entry_matches(&entry, &parsed);

        if context > 0 {
            ring.push((entry_idx, entry.clone()));
            if ring.len() > context + 1 {
                ring.remove(0);
            }
        }

        if is_match {
            matches_found += 1;
            if displayed_matches < max {
                if displayed_matches > 0 && context_after_remaining == 0 {
                    println!("  ---");
                }

                // Print context-before from ring (excluding current entry)
                if context > 0 {
                    for (idx, ctx_entry) in &ring {
                        if *idx == entry_idx { continue; } // skip current, print separately
                        let ctx_cy = ctx_entry.cy().unwrap_or(0);
                        print!("  [{idx}] cy={ctx_cy:<10}");
                        print_entry_fields(ctx_entry, &fields);
                        println!();
                    }
                }

                print!("> [{entry_idx}] cy={cy:<10}");
                print_entry_fields(&entry, &fields);
                println!();

                displayed_matches += 1;
                context_after_remaining = context;
            }
        } else if context_after_remaining > 0 {
            print!("  [{entry_idx}] cy={cy:<10}");
            print_entry_fields(&entry, &fields);
            println!();
            context_after_remaining -= 1;
        }

        entry_idx += 1;
    }

    if matches_found == 0 {
        println!("No matches found.");
    } else {
        println!("\n{matches_found} match(es) found.");
        if matches_found > max {
            println!("  (showing first {max}, use --max to see more)");
        }
    }

    0
}

fn print_entry_fields(entry: &TraceEntry, fields: &[String]) {
    for f in fields {
        if f == "cy" { continue; }
        if let Some(v) = entry.get(f) {
            print!(" {f}={}", value_to_string(v));
        }
    }
}

// ---------------------------------------------------------------------------
// trim
// ---------------------------------------------------------------------------

fn cmd_trim(input: &PathBuf, output: Option<PathBuf>, until: Option<String>, after: Option<String>) -> i32 {
    if until.is_none() && after.is_none() {
        eprintln!("Error: one of --until or --after is required");
        return 1;
    }

    let condition_str = until.as_deref().or(after.as_deref()).unwrap();
    let conditions = match parse_conditions(condition_str) {
        Ok(c) => c,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };
    let keep_before = until.is_some();

    let reader = match AnyTraceReader::open(input) {
        Ok(r) => r,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let header = reader.header().clone();
    let suffix = if keep_before { "_trimmed" } else { "_from" };
    let output = output.unwrap_or_else(|| default_output(input, suffix));

    let mut writer = match AnyWriter::create(&output, &header) {
        Ok(w) => w,
        Err(e) => { eprintln!("Error creating output: {e}"); return 1; }
    };

    let mut written: u64 = 0;
    let mut total: u64 = 0;
    let mut found_match = false;

    for result in reader {
        let entry = match result {
            Ok(e) => e,
            Err(e) => { eprintln!("Error reading: {e}"); return 1; }
        };
        total += 1;

        let is_match = !found_match && entry_matches(&entry, &conditions);
        if is_match {
            found_match = true;
            let cy = entry.cy().unwrap_or(0);
            eprintln!("Match at entry {total}, cy={cy}");
        }

        if keep_before {
            // --until: write everything up to and including the first match, then stop
            if found_match && !is_match {
                // Already past the match, just count remaining
                continue;
            }
            if let Err(e) = writer.write_entry(&entry) {
                eprintln!("Error writing: {e}");
                return 1;
            }
            written += 1;
        } else {
            // --after: skip until first match, then write everything from there
            if found_match {
                if let Err(e) = writer.write_entry(&entry) {
                    eprintln!("Error writing: {e}");
                    return 1;
                }
                written += 1;
            }
        }
    }

    if let Err(e) = writer.finish() {
        eprintln!("Error finalizing: {e}");
        return 1;
    }

    if !found_match {
        eprintln!("WARNING: condition never matched, wrote all {total} entries");
    }

    println!("Wrote {written} of {total} entries to {}", output.display());

    0
}

fn format_boot_rom(boot_rom: &gbtrace::BootRom) -> String {
    match boot_rom {
        gbtrace::BootRom::Skip => "skip".to_string(),
        gbtrace::BootRom::Builtin => "builtin".to_string(),
        gbtrace::BootRom::Stripped(orig) => format!("stripped:{orig}"),
        gbtrace::BootRom::Sha256(s) => s.clone(),
    }
}

// ---------------------------------------------------------------------------
// diff
// ---------------------------------------------------------------------------

/// A row from one trace, stored as field name -> string representation.
/// We store the string form for display and comparison (matching the JSONL semantics).
type Row = BTreeMap<String, String>;

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => v.to_string(),
    }
}

fn entry_to_row(entry: &TraceEntry, fields: &[String]) -> Row {
    let mut row = Row::new();
    for f in fields {
        if let Some(v) = entry.get(f) {
            row.insert(f.clone(), value_to_string(v));
        }
    }
    row
}

/// Merged row: values from trace A and trace B at the same cycle count.
struct MergedRow {
    cy: u64,
    vals_a: Row,
    vals_b: Row,
}

impl MergedRow {
    fn is_divergent(&self, compare_fields: &[String]) -> bool {
        for f in compare_fields {
            let a = self.vals_a.get(f);
            let b = self.vals_b.get(f);
            if a != b {
                return true;
            }
        }
        false
    }

    fn divergent_fields(&self, compare_fields: &[String]) -> Vec<String> {
        compare_fields
            .iter()
            .filter(|f| self.vals_a.get(*f) != self.vals_b.get(*f))
            .cloned()
            .collect()
    }
}

fn load_trace(
    path: &PathBuf,
    all_fields: &[String],
    skip_boot: bool,
    name: &str,
) -> Result<(gbtrace::TraceHeader, Vec<(u64, Row)>), String> {
    let reader = AnyTraceReader::open(path).map_err(|e| format!("Error opening {}: {e}", path.display()))?;
    let header = reader.header().clone();

    let mut rows: Vec<(u64, Row)> = Vec::new();
    let mut skipping_boot = skip_boot;

    for result in reader {
        let entry = result.map_err(|e| format!("Error reading {}: {e}", path.display()))?;
        let cy = entry.cy().unwrap_or(0);

        if skipping_boot {
            if let Some(Value::String(pc)) = entry.get("pc") {
                if pc == "0x0100" {
                    skipping_boot = false;
                } else {
                    continue;
                }
            } else {
                continue;
            }
        }

        let row = entry_to_row(&entry, all_fields);
        rows.push((cy, row));
    }

    if skip_boot && !skipping_boot {
        let original_len = rows.len(); // can't know skipped count easily, but we can report
        // We only have post-skip rows. The skip count is reported by the caller.
        let _ = original_len;
    }

    if skip_boot {
        if rows.is_empty() && skipping_boot {
            eprintln!("  WARNING: {name} has no entry with pc=0x0100, cannot skip boot");
        } else {
            // Count comes from the difference with total, but we don't track that.
            // We just note if boot was skipped.
        }
    }

    Ok((header, rows))
}

fn cmd_diff(
    path_a: &PathBuf,
    path_b: &PathBuf,
    max_regions: usize,
    context: usize,
    fields_filter: Option<String>,
    exclude_filter: Option<String>,
    skip_boot: bool,
) -> i32 {
    // Peek headers first to get field lists and names
    let reader_a = match AnyTraceReader::open(path_a) {
        Ok(r) => r,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };
    let reader_b = match AnyTraceReader::open(path_b) {
        Ok(r) => r,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let header_a = reader_a.header().clone();
    let header_b = reader_b.header().clone();
    drop(reader_a);
    drop(reader_b);

    let name_a = &header_a.emulator;
    let name_b = &header_b.emulator;

    println!("Comparing: {name_a} vs {name_b}");

    // Boot ROM info
    let boot_a = format_boot_rom(&header_a.boot_rom);
    let boot_b = format_boot_rom(&header_b.boot_rom);
    if boot_a != boot_b {
        println!("  Boot ROM: {name_a}={boot_a}  {name_b}={boot_b}");
        if skip_boot {
            println!("  Aligning at program start (--skip-boot)");
        } else {
            println!("  HINT: use --skip-boot to ignore boot ROM differences");
        }
    }

    // ROM hash check
    if header_a.rom_sha256 != header_b.rom_sha256 {
        println!("  WARNING: ROM hashes differ!");
        println!("    {name_a}: {}...", &header_a.rom_sha256[..16.min(header_a.rom_sha256.len())]);
        println!("    {name_b}: {}...", &header_b.rom_sha256[..16.min(header_b.rom_sha256.len())]);
    }

    // Determine common fields
    let fields_a: BTreeSet<&str> = header_a.fields.iter().map(|s| s.as_str()).collect();
    let fields_b: BTreeSet<&str> = header_b.fields.iter().map(|s| s.as_str()).collect();

    let mut common_fields: Vec<String> = fields_a
        .intersection(&fields_b)
        .filter(|f| **f != "cy")
        .map(|s| s.to_string())
        .collect();
    common_fields.sort();

    // Apply field filters
    if let Some(ref include) = fields_filter {
        let include: BTreeSet<&str> = include.split(',').collect();
        common_fields.retain(|f| include.contains(f.as_str()));
    }
    if let Some(ref exclude) = exclude_filter {
        let exclude: BTreeSet<&str> = exclude.split(',').collect();
        common_fields.retain(|f| !exclude.contains(f.as_str()));
    }

    if common_fields.is_empty() {
        println!("ERROR: No common fields to compare.");
        return 1;
    }

    let only_a: Vec<&str> = fields_a.difference(&fields_b).copied().collect();
    let only_b: Vec<&str> = fields_b.difference(&fields_a).copied().collect();
    if !only_a.is_empty() {
        println!("  Fields only in {name_a}: {}", only_a.join(", "));
    }
    if !only_b.is_empty() {
        println!("  Fields only in {name_b}: {}", only_b.join(", "));
    }
    println!("  Comparing fields: {}", common_fields.join(", "));
    println!();

    // All fields we need to read (common + cy for alignment)
    let mut all_fields: Vec<String> = vec!["cy".to_string()];
    all_fields.extend(common_fields.iter().cloned());
    // Also grab pc, op, a for context display if available
    for extra in &["pc", "op", "a"] {
        let s = extra.to_string();
        if !all_fields.contains(&s) && (fields_a.contains(extra) || fields_b.contains(extra)) {
            all_fields.push(s);
        }
    }

    // Load traces
    let (_, mut rows_a) = match load_trace(path_a, &all_fields, skip_boot, name_a) {
        Ok(v) => v,
        Err(e) => { eprintln!("{e}"); return 1; }
    };
    let (_, mut rows_b) = match load_trace(path_b, &all_fields, skip_boot, name_b) {
        Ok(v) => v,
        Err(e) => { eprintln!("{e}"); return 1; }
    };

    println!("  Entries:  {} vs {}", rows_a.len(), rows_b.len());

    // Rebase cycle counts if skip_boot and bases differ
    if skip_boot && !rows_a.is_empty() && !rows_b.is_empty() {
        let base_a = rows_a[0].0;
        let base_b = rows_b[0].0;
        if base_a != base_b {
            for row in &mut rows_a {
                row.0 -= base_a;
                row.1.insert("cy".to_string(), (row.0).to_string());
            }
            for row in &mut rows_b {
                row.0 -= base_b;
                row.1.insert("cy".to_string(), (row.0).to_string());
            }
            println!("  Rebased cycle counts: {name_a} -{base_a}, {name_b} -{base_b}");
        }
    }

    // Merge-join by cycle count (both are sorted by cy)
    let mut merged: Vec<MergedRow> = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < rows_a.len() && j < rows_b.len() {
        let cy_a = rows_a[i].0;
        let cy_b = rows_b[j].0;
        match cy_a.cmp(&cy_b) {
            std::cmp::Ordering::Equal => {
                merged.push(MergedRow {
                    cy: cy_a,
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

    if merged.is_empty() {
        println!("ERROR: No matching cycle counts. Traces may not be aligned.");
        if !rows_a.is_empty() && !rows_b.is_empty() {
            println!(
                "  {name_a} cy range: {} .. {}",
                rows_a.first().unwrap().0,
                rows_a.last().unwrap().0
            );
            println!(
                "  {name_b} cy range: {} .. {}",
                rows_b.first().unwrap().0,
                rows_b.last().unwrap().0
            );
        }
        return 1;
    }

    let matched_pct = merged.len() as f64 / rows_a.len().max(rows_b.len()) as f64 * 100.0;
    println!("Aligned {} entries by cycle count ({matched_pct:.1}% overlap)", merged.len());

    // Find divergences per field
    let mut field_divergences: Vec<FieldDivergence> = Vec::new();
    for field in &common_fields {
        let mut count = 0u64;
        let mut first: Option<(u64, String, String)> = None;
        for row in &merged {
            let a = row.vals_a.get(field);
            let b = row.vals_b.get(field);
            if a != b {
                count += 1;
                if first.is_none() {
                    first = Some((
                        row.cy,
                        a.cloned().unwrap_or_default(),
                        b.cloned().unwrap_or_default(),
                    ));
                }
            }
        }
        if count > 0 {
            let (cy, va, vb) = first.unwrap();
            field_divergences.push(FieldDivergence {
                field: field.clone(),
                count,
                first_cy: cy,
                val_a: va,
                val_b: vb,
            });
        }
    }

    if field_divergences.is_empty() {
        println!("\nNo divergences found! Traces match perfectly.");
        return 0;
    }

    field_divergences.sort_by_key(|d| d.first_cy);

    println!("\nFound divergences in {} field(s):\n", field_divergences.len());
    for d in &field_divergences {
        println!(
            "  {:6}  {:>8} differences, first at cy={}: {name_a}={}  {name_b}={}",
            d.field, d.count, d.first_cy, d.val_a, d.val_b
        );
    }

    // Mark divergent rows and collect indices
    let div_indices: Vec<usize> = merged
        .iter()
        .enumerate()
        .filter(|(_, row)| row.is_divergent(&common_fields))
        .map(|(i, _)| i)
        .collect();

    let total_div = div_indices.len();

    // Group consecutive divergences into regions
    let ranges = group_divergence_ranges(&merged, &div_indices);

    println!("\n{total_div} divergent entries in {} region(s):\n", ranges.len());
    for (j, r) in ranges.iter().enumerate().take(max_regions) {
        if r.start_cy == r.end_cy {
            println!("  Region {}: cy={} ({} entry)", j + 1, r.start_cy, r.count);
        } else {
            println!(
                "  Region {}: cy={}..{} ({} entries)",
                j + 1,
                r.start_cy,
                r.end_cy,
                r.count
            );
        }
    }
    if ranges.len() > max_regions {
        println!("  ... and {} more regions", ranges.len() - max_regions);
    }

    // Detailed view of first divergence with context
    let first_div_idx = div_indices[0];
    let first_div_cy = merged[first_div_idx].cy;

    println!("\n{}", "=".repeat(72));
    println!("Detail: first divergence at cy={first_div_cy}");
    println!("{}\n", "=".repeat(72));

    let window_start = first_div_idx.saturating_sub(context);
    let window_end = (first_div_idx + 5 + context).min(merged.len());

    for idx in window_start..window_end {
        let row = &merged[idx];
        let is_div = row.is_divergent(&common_fields);
        let marker = if is_div { ">" } else { " " };
        print_entry_row(row, &common_fields, name_a, name_b, marker);
    }

    let remaining = total_div.saturating_sub(5);
    if remaining > 0 {
        println!("\n... {remaining} more divergent entries");
    }

    1
}

struct FieldDivergence {
    field: String,
    count: u64,
    first_cy: u64,
    val_a: String,
    val_b: String,
}

struct DivRange {
    start_cy: u64,
    end_cy: u64,
    count: usize,
}

fn group_divergence_ranges(merged: &[MergedRow], div_indices: &[usize]) -> Vec<DivRange> {
    if div_indices.is_empty() {
        return vec![];
    }

    let mut ranges: Vec<DivRange> = Vec::new();
    let mut range_start = 0usize; // index into div_indices
    let mut range_end = 0usize;

    for k in 1..div_indices.len() {
        let prev_merged_idx = div_indices[k - 1];
        let cur_merged_idx = div_indices[k];
        if cur_merged_idx - prev_merged_idx <= 2 {
            range_end = k;
        } else {
            // Close current range
            ranges.push(DivRange {
                start_cy: merged[div_indices[range_start]].cy,
                end_cy: merged[div_indices[range_end]].cy,
                count: range_end - range_start + 1,
            });
            range_start = k;
            range_end = k;
        }
    }
    // Close last range
    ranges.push(DivRange {
        start_cy: merged[div_indices[range_start]].cy,
        end_cy: merged[div_indices[range_end]].cy,
        count: range_end - range_start + 1,
    });

    ranges
}

fn print_entry_row(
    row: &MergedRow,
    compare_fields: &[String],
    _name_a: &str,
    _name_b: &str,
    marker: &str,
) {
    let diff_fields = row.divergent_fields(compare_fields);

    if !diff_fields.is_empty() {
        let diff_strs: Vec<String> = diff_fields
            .iter()
            .map(|f| {
                let a = row.vals_a.get(f).map(|s| s.as_str()).unwrap_or("?");
                let b = row.vals_b.get(f).map(|s| s.as_str()).unwrap_or("?");
                format!("{f}: {a} vs {b}")
            })
            .collect();
        println!("{marker} cy={:>10}  {}", row.cy, diff_strs.join(", "));
    } else {
        let pc = row.vals_a.get("pc").map(|s| s.as_str()).unwrap_or("?");
        let op = row.vals_a.get("op").map(|s| s.as_str()).unwrap_or("?");
        let a = row.vals_a.get("a").map(|s| s.as_str()).unwrap_or("?");
        println!("{marker} cy={:>10}  pc={pc} op={op} a={a}  (match)", row.cy);
    }
}
