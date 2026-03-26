use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use gbtrace::JsonlReader;
use gbtrace::header::TraceHeader;
use gbtrace::query::Condition;

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
    /// Convert JSONL trace to native .gbtrace format
    Convert {
        /// Input JSONL file (.gbtrace.jsonl or -)
        input: PathBuf,
        /// Output .gbtrace file
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
    /// Compare two trace files and report divergences
    Diff {
        /// First trace file (reference)
        trace_a: PathBuf,
        /// Trace file to compare against the reference
        trace_b: PathBuf,
        /// Only compare these fields (comma-separated, e.g. pc,a,f)
        #[arg(long)]
        fields: Option<String>,
        /// Exclude these fields from comparison (comma-separated)
        #[arg(long)]
        exclude: Option<String>,
        /// Sync condition before comparing (default: pc).
        /// "none" for no sync, or field=value / field&mask.
        #[arg(long)]
        sync: Option<String>,
        /// One-line-per-field summary output
        #[arg(long)]
        summary: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Info { input } => cmd_info(&input),
        Command::Convert { input, output } => cmd_convert(&input, output),
        Command::Query { input, r#where: conditions, max, context, last } => {
            if let Some(n) = last {
                cmd_query_last(&input, n)
            } else {
                cmd_query(&input, &conditions, max, context)
            }
        }
        Command::Frames { input } => cmd_frames(&input),
        Command::Render { input, output, frames } => cmd_render(&input, output, frames),
        Command::Diff {
            trace_a,
            trace_b,
            fields,
            exclude,
            sync,
            summary,
        } => cmd_diff(&trace_a, &trace_b, fields, exclude, sync.as_deref(), summary),
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
            let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("trace");
            let stem = stem.strip_suffix(".gbtrace").unwrap_or(stem);
            input.with_file_name(format!("{stem}.gbtrace"))
        }
    };

    let reader = if is_stdin {
        use std::io::BufReader;
        match gbtrace::JsonlReader::from_reader(BufReader::new(std::io::stdin())) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error reading stdin: {e}");
                return 1;
            }
        }
    } else {
        match JsonlReader::open(input) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error opening input: {e}");
                return 1;
            }
        }
    };

    let header = reader.header().clone();
    convert_to_gbtrace(reader, &output, &header)
}

fn convert_to_gbtrace(
    reader: JsonlReader,
    output: &PathBuf,
    header: &TraceHeader,
) -> i32 {
    use gbtrace::format::write::GbtraceWriter;
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

    for result in reader {
        match result {
            Ok(entry) => {
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

fn cmd_diff(
    path_a: &PathBuf,
    path_b: &PathBuf,
    fields_filter: Option<String>,
    exclude_filter: Option<String>,
    sync: Option<&str>,
    summary: bool,
) -> i32 {
    use gbtrace::comparison::TraceComparison;

    let store_a = match gbtrace::store::open_trace_store(path_a) {
        Ok(s) => s,
        Err(e) => { eprintln!("Error opening {}: {e}", path_a.display()); return 1; }
    };
    let store_b = match gbtrace::store::open_trace_store(path_b) {
        Ok(s) => s,
        Err(e) => { eprintln!("Error opening {}: {e}", path_b.display()); return 1; }
    };

    let mut comp = match TraceComparison::align(&*store_a, &*store_b, sync) {
        Ok(c) => c,
        Err(e) => { eprintln!("Error aligning traces: {e}"); return 1; }
    };

    let name_a = &store_a.header().emulator;
    let name_b = &store_b.header().emulator;

    // Determine which fields to compare
    let fields_a = &store_a.header().fields;
    let fields_b = &store_b.header().fields;
    let common_fields: Vec<String> = fields_a.iter()
        .filter(|f| fields_b.contains(f))
        .cloned()
        .collect();

    let include: Option<Vec<String>> = fields_filter
        .map(|s| s.split(',').map(String::from).collect());
    let exclude: Option<Vec<String>> = exclude_filter
        .map(|s| s.split(',').map(String::from).collect());

    let fields_to_compare: Vec<&str> = common_fields.iter()
        .filter(|f| {
            if let Some(ref inc) = include {
                if !inc.iter().any(|i| i == *f) { return false; }
            }
            if let Some(ref exc) = exclude {
                if exc.iter().any(|e| e == *f) { return false; }
            }
            true
        })
        .map(|s| s.as_str())
        .collect();

    // Compute stats via TraceComparison (clone to release borrow)
    let stats = comp.compute_stats().to_vec();
    let aligned_count = comp.len();

    // Filter stats to fields we're comparing
    let relevant_stats: Vec<_> = stats.iter()
        .filter(|s| fields_to_compare.contains(&s.name.as_str()))
        .collect();

    let total_diffs: usize = relevant_stats.iter().map(|s| s.diff_count).sum();
    let is_identical = total_diffs == 0;

    if summary {
        let total_m: usize = relevant_stats.iter().map(|s| s.match_count).sum();
        let total_d: usize = relevant_stats.iter().map(|s| s.diff_count).sum();
        let pct = if total_m + total_d > 0 { total_m as f64 / (total_m + total_d) as f64 * 100.0 } else { 100.0 };
        println!("{name_a} vs {name_b}: {} aligned entries, {:.1}% match",
            aligned_count, pct);
        for s in &relevant_stats {
            if s.diff_count > 0 {
                // Find first diff for this field
                let first_idx = (0..comp.len())
                    .find(|&i| comp.field_differs(&s.name, i))
                    .unwrap_or(0);
                let row_a = comp.original_a(first_idx);
                let row_b = comp.original_b(first_idx);
                let col_a = store_a.field_col(&s.name).unwrap();
                let col_b = store_b.field_col(&s.name).unwrap();
                let val_a = store_a.get_numeric(col_a, row_a);
                let val_b = store_b.get_numeric(col_b, row_b);
                println!("  {:<16} {:>8} diffs, first at idx={}: {}={:02x}  {}={:02x}",
                    s.name, s.diff_count, first_idx,
                    name_a, val_a, name_b, val_b);
            }
        }
    } else {
        println!("Comparing: {} vs {}", name_a, name_b);
        println!("  Aligned entries: {}", aligned_count);
        println!("  Store A entries: {}", store_a.entry_count());
        println!("  Store B entries: {}", store_b.entry_count());

        // Fields only in one trace
        let only_a: Vec<&str> = fields_a.iter()
            .filter(|f| !fields_b.contains(f))
            .map(|s| s.as_str())
            .collect();
        let only_b: Vec<&str> = fields_b.iter()
            .filter(|f| !fields_a.contains(f))
            .map(|s| s.as_str())
            .collect();
        if !only_a.is_empty() {
            println!("  Fields only in {}: {}", name_a, only_a.join(", "));
        }
        if !only_b.is_empty() {
            println!("  Fields only in {}: {}", name_b, only_b.join(", "));
        }

        println!();

        if is_identical {
            println!("  IDENTICAL ({} common fields, {} entries)", fields_to_compare.len(), aligned_count);
        } else {
            println!("  Divergences:");
            for s in &relevant_stats {
                let pct = s.match_pct();
                if s.diff_count > 0 {
                    println!("    {:<16} {:>8} diffs ({:.1}% match)", s.name, s.diff_count, pct);
                }
            }

            // Show first few divergent entries
            println!();
            println!("  First divergences:");
            let mut shown = 0;
            for i in 0..comp.len() {
                let any_diff = fields_to_compare.iter().any(|f| comp.field_differs(f, i));
                if any_diff {
                    let row_a = comp.original_a(i);
                    let row_b = comp.original_b(i);
                    print!("    [{i}] ");
                    for f in &fields_to_compare {
                        if comp.field_differs(f, i) {
                            let col_a = store_a.field_col(f).unwrap();
                            let col_b = store_b.field_col(f).unwrap();
                            let va = store_a.get_numeric(col_a, row_a);
                            let vb = store_b.get_numeric(col_b, row_b);
                            print!("{f}:{:02x}|{:02x} ", va, vb);
                        }
                    }
                    println!();
                    shown += 1;
                    if shown >= 10 { break; }
                }
            }
        }
    }

    if is_identical { 0 } else { 1 }
}
