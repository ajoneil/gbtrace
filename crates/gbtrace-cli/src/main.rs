use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use gbtrace::{AnyTraceReader, Condition, ConditionEvaluator, TraceEntry, TraceWriter};
use gbtrace::header::TraceHeader;
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
        /// Show the last N entries (no --where needed)
        #[arg(long)]
        last: Option<usize>,
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
        /// Trim to the frame whose rendered pixels match this .pix reference file
        #[arg(long, conflicts_with_all = ["until", "after"])]
        reference: Option<PathBuf>,
    },
    /// Show frame boundaries detected from ly scanline counter
    Frames {
        /// Trace file to inspect
        input: PathBuf,
    },
    /// Render LCD frames from pixel trace data to PNG files
    Render {
        /// Trace file with pix field
        input: PathBuf,
        /// Output directory (default: current directory)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Only render specific frame numbers (1-based, comma-separated)
        #[arg(long)]
        frames: Option<String>,
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
        /// Sync both traces at a condition before comparing.
        /// Format: field=value (exact) or field&mask (bitmask non-zero).
        /// Example: --sync "lcdc&0x80" syncs at PPU-on.
        #[arg(long)]
        sync: Option<String>,
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
        Command::Query { input, r#where: conditions, max, context, last } => {
            if let Some(n) = last {
                cmd_query_last(&input, n)
            } else {
                cmd_query(&input, &conditions, max, context)
            }
        }
        Command::Frames { input } => cmd_frames(&input),
        Command::Render { input, output, frames } => cmd_render(&input, output, frames),
        Command::Trim { input, output, until, after, reference } => {
            if reference.is_some() {
                cmd_trim_reference(&input, output, reference.unwrap())
            } else {
                cmd_trim(&input, output, until, after)
            }
        }
        Command::Diff {
            trace_a,
            trace_b,
            max,
            context,
            fields,
            exclude,
            skip_boot,
            sync,
            align,
            summary,
            json,
            classify,
        } => cmd_diff(&trace_a, &trace_b, max, context, fields, exclude, skip_boot, sync.as_deref(), &align, summary, json, classify),
    };
    process::exit(code);
}

// ---------------------------------------------------------------------------
// info
// ---------------------------------------------------------------------------

fn cmd_info(path: &PathBuf) -> i32 {
    let store = match gbtrace::store::open_trace_store(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let h = store.header();
    println!("File:      {}", path.display());
    println!("Emulator:  {}", h.emulator);
    println!("Version:   {}", h.emulator_version);
    println!("Model:     {}", h.model);
    println!("Profile:   {}", h.profile);
    println!("Trigger:   {:?}", h.trigger);
    println!("Boot ROM:  {}", format_boot_rom(&h.boot_rom));
    println!("ROM hash:  {}", h.rom_sha256);
    println!("Fields:    {}", h.fields.join(", "));

    let count = store.entry_count();
    println!("Entries:   {count}");

    let boundaries = store.frame_boundaries();
    if !boundaries.is_empty() {
        println!("Frames:    {}", boundaries.len());
    }

    if let Ok(meta) = std::fs::metadata(path) {
        let size = meta.len();
        println!("File size: {size} bytes ({:.1} MB)", size as f64 / 1024.0 / 1024.0);
    }

    0
}

// ---------------------------------------------------------------------------
// frames
// ---------------------------------------------------------------------------

fn cmd_frames(path: &PathBuf) -> i32 {
    let store = match gbtrace::store::open_trace_store(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let boundaries = store.frame_boundaries();
    if boundaries.is_empty() {
        println!("No frames detected (trace has no ly field)");
        return 0;
    }

    let total = store.entry_count();
    println!("Frames: {}", boundaries.len());
    println!("Entries: {total}");
    println!();

    for (i, &start) in boundaries.iter().enumerate() {
        let start = start as usize;
        let end = if i + 1 < boundaries.len() {
            boundaries[i + 1] as usize
        } else {
            total
        };
        let size = end - start;
        println!("  Frame {:>3}  entries {:>8}..{:<8}  ({} entries)", i + 1, start, end, size);
    }

    0
}

// ---------------------------------------------------------------------------
// render
// ---------------------------------------------------------------------------

fn cmd_render(path: &PathBuf, output_dir: Option<PathBuf>, frame_filter: Option<String>) -> i32 {
    let store = match gbtrace::store::open_trace_store(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let frames = gbtrace::framebuffer::reconstruct_frames(store.as_ref());
    if frames.is_empty() {
        eprintln!("No frames with pixel data found (trace needs a 'pix' field)");
        return 1;
    }

    let out_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));
    if !out_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!("Failed to create output directory: {e}");
            return 1;
        }
    }

    // Parse frame filter
    let selected: Option<Vec<usize>> = frame_filter.map(|s| {
        s.split(',')
            .filter_map(|n| n.trim().parse::<usize>().ok())
            .collect()
    });

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("frame");

    for frame in &frames {
        let frame_num = frame.index + 1; // 1-based for display
        if let Some(ref sel) = selected {
            if !sel.contains(&frame_num) { continue; }
        }

        let png_data = frame.to_png();
        let out_path = out_dir.join(format!("{stem}_frame{frame_num:03}.png"));
        match std::fs::write(&out_path, &png_data) {
            Ok(_) => {
                let pix_count: usize = frame.pixels.iter().filter(|&&p| p > 0).count();
                println!("  Frame {:>3}  {} ({} non-zero pixels)",
                    frame_num, out_path.display(), pix_count);
            }
            Err(e) => {
                eprintln!("  Frame {:>3}  ERROR: {e}", frame_num);
            }
        }
    }

    println!("Rendered {} frame(s)", frames.len());
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
            // Default: produce .gbtrace binary format
            let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("trace");
            let stem = stem.strip_suffix(".gbtrace").unwrap_or(stem);
            input.with_file_name(format!("{stem}.gbtrace"))
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

    // Extract frame boundaries from the source for preservation during convert.
    let frame_boundaries: Vec<u64> = if !is_stdin {
        gbtrace::store::open_trace_store(input)
            .map(|store| store.frame_boundaries().iter().map(|&b| b as u64).collect())
            .unwrap_or_default()
    } else {
        vec![]
    };

    let out_ext = output.extension().and_then(|e| e.to_str()).unwrap_or("");
    let out_path_str = output.to_string_lossy();

    if out_ext == "gbtrace" && !out_path_str.ends_with(".gbtrace.jsonl") {
        // Output as native .gbtrace binary
        convert_to_gbtrace(reader, &output, &header, &frame_boundaries)
    } else {
        // Output as JSONL
        convert_to_jsonl(reader, &output, &header)
    }
}

fn convert_to_gbtrace(
    reader: AnyTraceReader,
    output: &PathBuf,
    header: &TraceHeader,
    frame_boundaries: &[u64],
) -> i32 {
    use gbtrace::format::write::GbtraceWriter;
    use gbtrace::format::FieldGroup;
    use gbtrace::format::read::GbtraceStore;
    use gbtrace::profile::{field_type, field_nullable, FieldType};

    // Derive field groups from the header
    let groups = gbtrace::format::read::derive_groups_pub(&header.fields);

    let mut writer = match GbtraceWriter::create(output, header, &groups) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Error creating output: {e}");
            return 1;
        }
    };

    let mut count: u64 = 0;
    let mut boundary_idx = 0;

    for result in reader {
        match result {
            Ok(entry) => {
                // Mark frame boundaries at the correct positions
                while boundary_idx < frame_boundaries.len()
                    && frame_boundaries[boundary_idx] == count
                {
                    let _ = writer.mark_frame(None);
                    boundary_idx += 1;
                }

                // Set all field values from the entry
                for (col, name) in header.fields.iter().enumerate() {
                    let val = entry.get(name);
                    let ft = field_type(name);
                    let nullable = field_nullable(name);

                    if nullable && val.is_none() {
                        writer.set_null(col);
                        continue;
                    }

                    match ft {
                        FieldType::UInt64 => {
                            writer.set_u64(col, val.and_then(|v| v.as_u64()).unwrap_or(0));
                        }
                        FieldType::UInt16 => {
                            let v = val
                                .and_then(|v| v.as_u64().or_else(|| {
                                    v.as_str().and_then(|s| {
                                        let s = s.strip_prefix("0x").unwrap_or(s);
                                        u64::from_str_radix(s, 16).ok()
                                    })
                                }))
                                .unwrap_or(0) as u16;
                            if nullable && v == 0 { writer.set_null(col); }
                            else { writer.set_u16(col, v); }
                        }
                        FieldType::UInt8 => {
                            let v = val
                                .and_then(|v| v.as_u64().or_else(|| {
                                    v.as_str().and_then(|s| {
                                        let s = s.strip_prefix("0x").unwrap_or(s);
                                        u64::from_str_radix(s, 16).ok()
                                    })
                                }))
                                .unwrap_or(0) as u8;
                            if nullable && v == 0 { writer.set_null(col); }
                            else { writer.set_u8(col, v); }
                        }
                        FieldType::Bool => {
                            writer.set_bool(col, val.and_then(|v| v.as_bool()).unwrap_or(false));
                        }
                        FieldType::Str => {
                            let s = val.and_then(|v| v.as_str()).unwrap_or("");
                            if nullable && s.is_empty() { writer.set_null(col); }
                            else { writer.set_str(col, s); }
                        }
                    }
                }

                if let Err(e) = writer.finish_entry() {
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

    let input_size = std::fs::metadata("").map(|m| m.len()).unwrap_or(0);
    let output_size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    println!("Converted {count} entries to {} ({output_size} bytes)", output.display());
    0
}

fn convert_to_jsonl(
    reader: AnyTraceReader,
    output: &PathBuf,
    header: &TraceHeader,
) -> i32 {
    let mut writer = match TraceWriter::create(output, header) {
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
}

impl AnyWriter {
    fn create(path: &std::path::Path, header: &gbtrace::TraceHeader) -> Result<Self, gbtrace::Error> {
        Ok(Self::Jsonl(TraceWriter::create(path, header)?))
    }

    fn write_entry(&mut self, entry: &TraceEntry) -> Result<(), gbtrace::Error> {
        match self {
            Self::Jsonl(w) => w.write_entry(entry),
        }
    }

    fn finish(self) -> Result<(), gbtrace::Error> {
        match self {
            Self::Jsonl(w) => w.finish(),
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

    let store = match gbtrace::store::open_trace_store(input) {
        Ok(s) => s,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let fields = store.header().fields.clone();

    // Use the store's query_range for the first condition, then filter
    let condition_str = conditions.join(" AND ");
    let matches = match store.query_range(&condition_str, 0, store.entry_count()) {
        Ok(m) => m,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let matches_found = matches.len();
    let displayed_matches = matches_found.min(max);

    for (display_idx, &entry_idx) in matches.iter().enumerate() {
        if display_idx >= max { break; }
        let i = entry_idx as usize;

        if display_idx > 0 { println!("  ---"); }

        // Context before
        if context > 0 {
            let ctx_start = if i >= context { i - context } else { 0 };
            for ci in ctx_start..i {
                print!("  [{ci}]");
                print_store_entry(&*store, ci, &fields);
                println!();
            }
        }

        // The match
        print!("> [{i}]");
        print_store_entry(&*store, i, &fields);
        println!();

        // Context after
        if context > 0 {
            let ctx_end = (i + context + 1).min(store.entry_count());
            for ci in (i + 1)..ctx_end {
                print!("  [{ci}]");
                print_store_entry(&*store, ci, &fields);
                println!();
            }
        }
    }

    // Mimic the old output format
    println!("\n{matches_found} match(es) found.");
    if displayed_matches < matches_found {
        println!("  (showing first {displayed_matches}, use --max to see more)");
    }

    0
}

// Keep old cmd_query code below for reference but it's dead
fn _cmd_query_old(input: &PathBuf, conditions: &[String], max: usize, context: usize) -> i32 {
    let _ = (input, conditions, max, context);
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

fn cmd_query_last(input: &PathBuf, n: usize) -> i32 {
    let store = match gbtrace::store::open_trace_store(input) {
        Ok(s) => s,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let fields = store.header().fields.clone();
    let total = store.entry_count();
    let start = total.saturating_sub(n);

    for i in start..total {
        print!(" ");
        print_store_entry(&*store, i, &fields);
        println!();
    }

    0
}

fn print_store_entry(store: &dyn gbtrace::store::TraceStore, row: usize, fields: &[String]) {
    use gbtrace::profile::{field_type, FieldType};
    for (col, name) in fields.iter().enumerate() {
        if store.is_null(col, row) { continue; }
        let ft = field_type(name);
        match ft {
            FieldType::Bool => {
                let v = store.get_bool(col, row);
                print!(" {name}={v}");
            }
            FieldType::Str => {
                let v = store.get_str(col, row);
                if !v.is_empty() { print!(" {name}={v}"); }
            }
            _ => {
                let v = store.get_numeric(col, row);
                print!(" {name}={v:02x}");
            }
        }
    }
}

fn _cmd_query_last_old(input: &PathBuf, n: usize) -> i32 {
    let reader = match AnyTraceReader::open(input) {
        Ok(r) => r,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let fields = reader.header().fields.clone();

    // Collect entries into a ring buffer of size n
    let mut ring: Vec<TraceEntry> = Vec::with_capacity(n);
    for result in reader {
        let entry = match result {
            Ok(e) => e,
            Err(e) => { eprintln!("Error reading: {e}"); return 1; }
        };
        if ring.len() >= n {
            ring.remove(0);
        }
        ring.push(entry);
    }

    for entry in &ring {
        print!(" ");
        print_entry_fields(entry, &fields);
        println!();
    }

    0
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

fn cmd_trim_reference(input: &PathBuf, output: Option<PathBuf>, reference: PathBuf) -> i32 {
    use gbtrace::framebuffer::{self, LCD_WIDTH, LCD_HEIGHT};

    // Load reference .pix file and convert to pixel values (0-3)
    let ref_str = match std::fs::read_to_string(&reference) {
        Ok(d) => d,
        Err(e) => { eprintln!("Error reading reference: {e}"); return 1; }
    };
    if ref_str.len() != LCD_WIDTH * LCD_HEIGHT {
        eprintln!("Error: reference file should be {} bytes, got {}", LCD_WIDTH * LCD_HEIGHT, ref_str.len());
        return 1;
    }
    let ref_pixels: Vec<u8> = ref_str.bytes().map(|b| b.wrapping_sub(b'0').min(3)).collect();

    // Load store and reconstruct frames — same logic as the viewer uses.
    let store = match gbtrace::store::open_trace_store(input) {
        Ok(s) => s,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };

    let frames = framebuffer::reconstruct_frames(store.as_ref());
    let mut end_entry = None;
    for frame in &frames {
        if frame.pixels[..] == ref_pixels[..] {
            eprintln!("Reference matches frame {} (entries 0..{})", frame.index + 1, frame.end_entry);
            end_entry = Some(frame.end_entry);
            break;
        }
    }

    let total = store.entry_count();
    let cut = match end_entry {
        // Include one extra entry so the frame boundary (ly wrap) is captured,
        // allowing the rendered output to detect the matching frame properly.
        Some(e) => (e + 1).min(total),
        None => {
            eprintln!("WARNING: no frame matches reference, writing all entries");
            total
        }
    };

    // Re-read trace and write entries up to cut point
    let reader = match AnyTraceReader::open(input) {
        Ok(r) => r,
        Err(e) => { eprintln!("Error: {e}"); return 1; }
    };
    let header = reader.header().clone();
    let output = output.unwrap_or_else(|| default_output(input, "_trimmed"));

    let mut writer = match AnyWriter::create(&output, &header) {
        Ok(w) => w,
        Err(e) => { eprintln!("Error creating output: {e}"); return 1; }
    };

    let mut written: u64 = 0;
    for result in reader {
        if written as usize >= cut { break; }
        let entry = match result {
            Ok(e) => e,
            Err(e) => { eprintln!("Error reading: {e}"); return 1; }
        };
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

    println!("Wrote {written} of {total} entries to {}", output.display());
    if end_entry.is_some() { 0 } else { 1 }
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

fn load_store(path: &PathBuf) -> Result<Box<dyn gbtrace::store::TraceStore>, String> {
    gbtrace::store::open_trace_store(path)
        .map_err(|e| format!("Error opening {}: {e}", path.display()))
}

/// Build TraceEntry from a store row (for the legacy diff module).
fn store_entry_at(store: &dyn gbtrace::store::TraceStore, row: usize) -> gbtrace::TraceEntry {
    let fields = &store.header().fields;
    let mut entry = gbtrace::TraceEntry::new();
    for (col, name) in fields.iter().enumerate() {
        if store.is_null(col, row) { continue; }
        let ft = gbtrace::profile::field_type(name);
        match ft {
            gbtrace::FieldType::Bool => entry.set_bool(name, store.get_bool(col, row)),
            gbtrace::FieldType::Str => {
                let s = store.get_str(col, row);
                if !s.is_empty() { entry.set_str(name, &s); }
            }
            gbtrace::FieldType::UInt64 => entry.set_cy(store.get_numeric(col, row)),
            gbtrace::FieldType::UInt16 => entry.set_u16(name, store.get_numeric(col, row) as u16),
            gbtrace::FieldType::UInt8 => entry.set_u8(name, store.get_numeric(col, row) as u8),
        }
    }
    entry
}

fn cmd_diff(
    path_a: &PathBuf,
    trace_b_paths: &[PathBuf],
    max_regions: usize,
    context: usize,
    fields_filter: Option<String>,
    exclude_filter: Option<String>,
    skip_boot: bool,
    sync: Option<&str>,
    align: &str,
    summary: bool,
    json: bool,
    classify: bool,
) -> i32 {
    use gbtrace::diff::{AlignmentStrategy, DiffConfig, TraceDiffer};
    use gbtrace::comparison::TraceComparison;

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

    // Load all stores
    let store_a = match load_store(path_a) {
        Ok(s) => s,
        Err(e) => { eprintln!("{e}"); return 1; }
    };

    let stores_b: Vec<Box<dyn gbtrace::store::TraceStore>> = {
        let mut v = vec![];
        for path in trace_b_paths {
            match load_store(path) {
                Ok(s) => v.push(s),
                Err(e) => { eprintln!("{e}"); return 1; }
            }
        }
        v
    };

    // Use TraceComparison to align stores (handles collapse + sync)
    let sync_mode = if skip_boot { Some("pc") } else { sync };

    // Multi-trace comparison
    if stores_b.len() > 1 {
        let mut all_entries = vec![];

        // Build entries for store A (used as base for all comparisons)
        let comp_first = match TraceComparison::align(&*store_a, &*stores_b[0], sync_mode) {
            Ok(c) => c,
            Err(e) => { eprintln!("Error aligning: {e}"); return 1; }
        };
        let header_a = store_a.header().clone();
        let entries_a: Vec<gbtrace::TraceEntry> = comp_first.map_a.iter()
            .map(|&i| store_entry_at(&*store_a, i))
            .collect();
        all_entries.push((header_a, entries_a));

        for store_b in &stores_b {
            let comp = match TraceComparison::align(&*store_a, &**store_b, sync_mode) {
                Ok(c) => c,
                Err(e) => { eprintln!("Error aligning: {e}"); return 1; }
            };
            let header_b = store_b.header().clone();
            let entries_b: Vec<gbtrace::TraceEntry> = comp.map_b.iter()
                .map(|&i| store_entry_at(&**store_b, i))
                .collect();
            all_entries.push((header_b, entries_b));
        }

        let multi = match differ.compare_multi(all_entries) {
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
    let comp = match TraceComparison::align(&*store_a, &*stores_b[0], sync_mode) {
        Ok(c) => c,
        Err(e) => { eprintln!("Error aligning: {e}"); return 1; }
    };

    let header_a = store_a.header().clone();
    let header_b = stores_b[0].header().clone();
    let entries_a: Vec<gbtrace::TraceEntry> = comp.map_a.iter()
        .map(|&i| store_entry_at(&*store_a, i))
        .collect();
    let entries_b: Vec<gbtrace::TraceEntry> = comp.map_b.iter()
        .map(|&i| store_entry_at(&*stores_b[0], i))
        .collect();

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
