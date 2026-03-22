use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use gbtrace::{AnyTraceReader, Condition, ConditionEvaluator, ParquetTraceWriter, TraceEntry, TraceWriter};
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
        /// Input file (.gbtrace, .gbtrace.gz, or .gbtrace.parquet)
        input: PathBuf,
        /// Output file (.gbtrace, .gbtrace.gz, or .gbtrace.parquet)
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
    /// Compare two or more trace files and report divergences
    Diff {
        /// First trace file (reference)
        trace_a: PathBuf,
        /// Trace file(s) to compare against the reference
        trace_b: Vec<PathBuf>,
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
        /// Alignment strategy: auto (default), cycle, or sequence
        #[arg(long, default_value = "auto")]
        align: String,
        /// One-line-per-field summary output (good for scripting)
        #[arg(long)]
        summary: bool,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
        /// Show divergence classification
        #[arg(long)]
        classify: bool,
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
            align,
            summary,
            json,
            classify,
        } => cmd_diff(&trace_a, &trace_b, max, context, fields, exclude, skip_boot, &align, summary, json, classify),
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
    for result in reader {
        match result {
            Ok(_) => count += 1,
            Err(e) => {
                eprintln!("Error reading entry {count}: {e}");
                return 1;
            }
        }
    }

    println!("Entries:   {count}");

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
    let is_stdin = input.as_os_str() == "-";

    let output = match output {
        Some(o) => o,
        None if is_stdin => {
            eprintln!("Error: --output required when reading from stdin");
            return 1;
        }
        None => {
            let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "parquet" {
                input.with_extension("gbtrace")
            } else {
                let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("trace");
                let stem = stem.strip_suffix(".gbtrace").unwrap_or(stem);
                input.with_file_name(format!("{stem}.gbtrace.parquet"))
            }
        }
    };

    let reader = if is_stdin {
        use std::io::BufReader;
        match gbtrace::TraceReader::from_reader(BufReader::new(std::io::stdin())) {
            Ok(r) => AnyTraceReader::Jsonl(r),
            Err(e) => {
                eprintln!("Error reading stdin: {e}");
                return 1;
            }
        }
    } else {
        match AnyTraceReader::open(input) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error opening input: {e}");
                return 1;
            }
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
// Shared: condition parsing (delegates to gbtrace::query)
// ---------------------------------------------------------------------------

fn parse_cli_condition(s: &str) -> Result<Condition, String> {
    gbtrace::query::parse_condition(s)
}

fn parse_cli_conditions(parts: &[String]) -> Result<Condition, String> {
    let conditions: Vec<Condition> = parts
        .iter()
        .map(|s| gbtrace::query::parse_condition(s))
        .collect::<Result<Vec<_>, _>>()?;
    if conditions.len() == 1 {
        Ok(conditions.into_iter().next().unwrap())
    } else {
        Ok(Condition::All(conditions))
    }
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
            if entry.get_u16("pc") == Some(0x0100) {
                skipping = false;
                cy_base = entry.cy();
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

    let condition = match parse_cli_conditions(conditions) {
        Ok(c) => c,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let reader = match AnyTraceReader::open(input) {
        Ok(r) => r,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let fields = reader.header().fields.clone();
    let mut evaluator = ConditionEvaluator::new(condition);

    // Ring buffer for context-before entries
    let mut ring: Vec<(u64, TraceEntry)> = Vec::new();
    let mut matches_found: usize = 0;
    let mut entry_idx: u64 = 0;
    let mut context_after_remaining: usize = 0;
    let mut displayed_matches: usize = 0;

    for result in reader {
        let entry = match result {
            Ok(e) => e,
            Err(e) => { eprintln!("Error reading: {e}"); return 1; }
        };

        let cy = entry.cy().unwrap_or(0);
        let is_match = evaluator.evaluate(&entry);

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

                if context > 0 {
                    for (idx, ctx_entry) in &ring {
                        if *idx == entry_idx { continue; }
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
        // all fields displayed
        if let Some(v) = entry.get(f) {
            print!(" {f}={}", display_val(v));
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
    let condition = match parse_cli_condition(condition_str) {
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

    let mut evaluator = ConditionEvaluator::new(condition);
    let mut written: u64 = 0;
    let mut total: u64 = 0;
    let mut found_match = false;

    for result in reader {
        let entry = match result {
            Ok(e) => e,
            Err(e) => { eprintln!("Error reading: {e}"); return 1; }
        };
        total += 1;

        let is_match = !found_match && evaluator.evaluate(&entry);
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

/// Format a JSON value for display: numbers as zero-padded lowercase hex, strings as-is.
const FIELDS_16BIT: &[&str] = &["pc", "sp"];

fn display_val_field(v: &Value, field: &str) -> String {
    if FIELDS_16BIT.contains(&field) {
        if let Some(n) = v.as_u64() {
            return format!("{n:04x}");
        }
    }
    display_val(v)
}

fn display_val(v: &Value) -> String {
    match v {
        Value::Number(n) => {
            if let Some(n) = n.as_u64() {
                if n <= 0xFF { return format!("{n:02x}"); }
                if n <= 0xFFFF { return format!("{n:04x}"); }
                return format!("{n:x}");
            }
            n.to_string()
        }
        Value::String(s) => {
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                return hex.to_lowercase();
            }
            s.clone()
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => v.to_string(),
    }
}

fn load_trace_entries(path: &PathBuf) -> Result<(gbtrace::TraceHeader, Vec<gbtrace::TraceEntry>), String> {
    let reader = AnyTraceReader::open(path)
        .map_err(|e| format!("Error opening {}: {e}", path.display()))?;
    let header = reader.header().clone();
    let entries: Vec<gbtrace::TraceEntry> = reader
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| format!("Error reading {}: {e}", path.display()))?;
    Ok((header, entries))
}

/// Load trace, collapsing T-cycle traces to instruction level if needed.
/// Also aligns to target_pc if provided.
fn load_trace_for_diff(
    path: &PathBuf,
    collapse_tcycle: bool,
    align_pc: Option<u16>,
) -> Result<(gbtrace::TraceHeader, Vec<gbtrace::TraceEntry>), String> {
    use gbtrace::column_store::load_column_store;

    let mut store = load_column_store(path)
        .map_err(|e| format!("Error opening {}: {e}", path.display()))?;

    if collapse_tcycle && store.header().trigger == gbtrace::header::Trigger::Custom {
        // trigger "tcycle" is stored as Custom in the enum — check the raw string
    }

    // Check trigger from header
    let is_tcycle = matches!(store.header().trigger, gbtrace::header::Trigger::Tcycle);
    if collapse_tcycle && is_tcycle {
        store = store.collapse_to_instructions()
            .map_err(|e| format!("Collapse error: {e}"))?;
    }

    if let Some(pc) = align_pc {
        if let Ok(aligned) = store.skip_to_pc(pc) {
            store = aligned;
        }
    }

    let header = store.header().clone();
    let entries: Vec<gbtrace::TraceEntry> = (0..store.entry_count())
        .map(|i| store.to_entry(i))
        .collect();
    Ok((header, entries))
}

fn cmd_diff(
    path_a: &PathBuf,
    trace_b_paths: &[PathBuf],
    max_regions: usize,
    context: usize,
    fields_filter: Option<String>,
    exclude_filter: Option<String>,
    skip_boot: bool,
    align: &str,
    summary: bool,
    json: bool,
    classify: bool,
) -> i32 {
    use gbtrace::diff::{AlignmentStrategy, DiffConfig, TraceDiffer};

    let alignment = match align {
        "sequence" => AlignmentStrategy::Sequence,
        "cycle" => AlignmentStrategy::Cycle,
        _ => AlignmentStrategy::Auto,
    };

    let config = DiffConfig {
        include_fields: fields_filter.as_ref().map(|s| s.split(',').map(String::from).collect()),
        exclude_fields: exclude_filter.as_ref().map(|s| s.split(',').map(String::from).collect()),
        alignment,
        skip_boot,
        max_regions,
        context,
    };

    let differ = TraceDiffer::new(config);

    // Peek at all headers to detect trigger mismatches and find common start PC
    let headers: Vec<_> = {
        let mut h = vec![];
        for path in std::iter::once(path_a).chain(trace_b_paths.iter()) {
            match AnyTraceReader::open(path) {
                Ok(r) => h.push(r.header().clone()),
                Err(e) => { eprintln!("Error: {e}"); return 1; }
            }
        }
        h
    };

    let any_tcycle = headers.iter().any(|h| matches!(h.trigger, gbtrace::header::Trigger::Tcycle));
    let any_instruction = headers.iter().any(|h| !matches!(h.trigger, gbtrace::header::Trigger::Tcycle));
    let needs_collapse = any_tcycle && any_instruction;

    // Find common start PC by peeking first entries
    let align_pc = if needs_collapse || skip_boot {
        // Find the max starting PC across all traces (after potential collapse)
        let mut start_pcs = vec![];
        for path in std::iter::once(path_a).chain(trace_b_paths.iter()) {
            if let Ok(r) = AnyTraceReader::open(path) {
                if let Some(Ok(entry)) = r.into_iter().next() {
                    if let Some(pc) = entry.get_u16("pc") {
                        start_pcs.push(pc);
                    }
                }
            }
        }
        if start_pcs.len() > 1 && start_pcs.iter().any(|&p| p != start_pcs[0]) {
            Some(*start_pcs.iter().max().unwrap())
        } else {
            None
        }
    } else {
        None
    };

    // Load traces with auto-collapse and alignment
    let (header_a, entries_a) = match load_trace_for_diff(path_a, needs_collapse, align_pc) {
        Ok(v) => v,
        Err(e) => { eprintln!("{e}"); return 1; }
    };

    // Multi-trace comparison
    if trace_b_paths.len() > 1 {
        let mut traces = vec![(header_a, entries_a)];
        for path in trace_b_paths {
            match load_trace_for_diff(path, needs_collapse, align_pc) {
                Ok((h, e)) => traces.push((h, e)),
                Err(e) => { eprintln!("{e}"); return 1; }
            }
        }
        let multi = match differ.compare_multi(traces) {
            Ok(r) => r,
            Err(e) => { eprintln!("Error: {e}"); return 1; }
        };
        if json {
            println!("{}", serde_json::to_string_pretty(&multi).unwrap());
            return if multi.pairwise.iter().all(|r| r.is_identical()) { 0 } else { 1 };
        }
        let mut any_divergent = false;
        for result in &multi.pairwise {
            print_diff_result(result, max_regions, summary, classify);
            if !result.is_identical() { any_divergent = true; }
            println!();
        }
        return if any_divergent { 1 } else { 0 };
    }

    // Single pair comparison
    let (header_b, entries_b) = match load_trace_for_diff(&trace_b_paths[0], needs_collapse, align_pc) {
        Ok(v) => v,
        Err(e) => { eprintln!("{e}"); return 1; }
    };

    let result = match differ.compare(&header_a, entries_a, &header_b, entries_b) {
        Ok(r) => r,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
        return if result.is_identical() { 0 } else { 1 };
    }

    print_diff_result(&result, max_regions, summary, classify);
    if result.is_identical() { 0 } else { 1 }
}

fn print_diff_result(
    result: &gbtrace::DiffResult,
    max_regions: usize,
    summary: bool,
    classify: bool,
) {
    let name_a = &result.name_a;
    let name_b = &result.name_b;

    if summary {
        // Compact one-line-per-field output
        println!("{name_a} vs {name_b}: {} ({} entries, {:.1}% overlap)",
            result.classification, result.aligned_count, result.overlap_pct);
        for d in &result.field_divergences {
            println!("  {:<8} {:>8} diffs, first at idx={}: {}={}  {}={}",
                d.field, d.count, d.first_index,
                name_a, display_val(&d.first_val_a),
                name_b, display_val(&d.first_val_b));
        }
        return;
    }

    println!("Comparing: {name_a} vs {name_b}");

    // Boot ROM info
    if result.boot_rom_mismatch {
        println!("  Boot ROM mismatch");
    }
    if result.rom_mismatch {
        println!("  WARNING: ROM hashes differ!");
    }
    if !result.only_in_a.is_empty() {
        println!("  Fields only in {name_a}: {}", result.only_in_a.join(", "));
    }
    if !result.only_in_b.is_empty() {
        println!("  Fields only in {name_b}: {}", result.only_in_b.join(", "));
    }
    println!("  Comparing fields: {}", result.common_fields.join(", "));
    println!();
    println!("  Entries:  {} vs {}", result.entries_a, result.entries_b);
    println!("Aligned {} entries ({:.1}% overlap)", result.aligned_count, result.overlap_pct);

    if classify || !result.is_identical() {
        println!("  Classification: {}", result.classification);
    }

    if result.is_identical() {
        println!("\nNo divergences found! Traces match perfectly.");
        return;
    }

    println!("\nFound divergences in {} field(s):\n", result.field_divergences.len());
    for d in &result.field_divergences {
        println!(
            "  {:6}  {:>8} differences, first at idx={}: {name_a}={}  {name_b}={}",
            d.field, d.count, d.first_index,
            display_val(&d.first_val_a), display_val(&d.first_val_b)
        );
    }

    println!("\n{} divergent entries in {} region(s):\n",
        result.total_divergent, result.regions.len());
    for (j, r) in result.regions.iter().enumerate().take(max_regions) {
        if r.start_index == r.end_index {
            println!("  Region {}: idx={} ({} entry)", j + 1, r.start_index, r.count);
        } else {
            println!("  Region {}: idx={}..{} ({} entries)",
                j + 1, r.start_index, r.end_index, r.count);
        }
    }
    if result.regions.len() > max_regions {
        println!("  ... and {} more regions", result.regions.len() - max_regions);
    }

    // Context window
    if !result.context_window.is_empty() {
        let first_div = result.context_window.iter().find(|c| c.is_divergent);
        if let Some(first) = first_div {
            println!("\n{}", "=".repeat(72));
            println!("Detail: first divergence at idx={}", first.index);
            println!("{}\n", "=".repeat(72));
        }

        for entry in &result.context_window {
            let marker = if entry.is_divergent { ">" } else { " " };
            if !entry.divergent_fields.is_empty() {
                let diff_strs: Vec<String> = entry.divergent_fields
                    .iter()
                    .map(|f| {
                        let a = entry.vals_a.get(f).map(|v| display_val(v)).unwrap_or_else(|| "?".into());
                        let b = entry.vals_b.get(f).map(|v| display_val(v)).unwrap_or_else(|| "?".into());
                        format!("{f}: {a} vs {b}")
                    })
                    .collect();
                println!("{marker} idx={:>10}  {}", entry.index, diff_strs.join(", "));
            } else {
                let pc = entry.vals_a.get("pc").map(|v| display_val(v)).unwrap_or_else(|| "?".into());
                let op = entry.vals_a.get("op").map(|v| display_val(v)).unwrap_or_else(|| "?".into());
                let a = entry.vals_a.get("a").map(|v| display_val(v)).unwrap_or_else(|| "?".into());
                println!("{marker} idx={:>10}  pc={pc} op={op} a={a}  (match)", entry.index);
            }
        }

        let remaining = result.total_divergent.saturating_sub(5);
        if remaining > 0 {
            println!("\n... {remaining} more divergent entries");
        }
    }
}
