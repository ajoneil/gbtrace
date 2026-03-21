#!/usr/bin/env python3
"""gbtrace-cli: Convert and compare GB Trace files.

Commands:
  convert   Convert .gbtrace JSONL to .parquet (or vice versa)
  diff      Compare two trace files and report divergences
  info      Show summary information about a trace file

Examples:
  gbtrace-cli convert trace.gbtrace -o trace.parquet
  gbtrace-cli diff reference.parquet my_emu.gbtrace
  gbtrace-cli diff reference.parquet my_emu.gbtrace --context 3 --max 20
  gbtrace-cli info trace.gbtrace
"""

import argparse
import json
import sys
from pathlib import Path

import polars as pl


# ---------------------------------------------------------------------------
# I/O helpers
# ---------------------------------------------------------------------------

def read_trace(path: str) -> tuple[dict, pl.DataFrame]:
    """Read a trace file (JSONL or Parquet) and return (header, dataframe)."""
    p = Path(path)

    if p.suffix == ".parquet":
        df = pl.read_parquet(path)
        # Header is stored as Parquet metadata
        metadata = df.collect_schema()  # not what we want
        # We store the header as JSON in parquet file metadata
        pf = pl.read_parquet_schema(path)
        # Try reading metadata from the parquet file directly
        import pyarrow.parquet as pq
        pf = pq.read_metadata(path)
        header_json = pf.metadata.get(b"gbtrace_header", None)
        if header_json:
            header = json.loads(header_json)
        else:
            header = {"emulator": "unknown", "fields": df.columns}
        return header, df

    # JSONL (.gbtrace or .gbtrace.gz)
    with _open_trace(p) as f:
        header_line = f.readline()
        header = json.loads(header_line)

        # Read remaining lines as JSONL
        entries = []
        for line in f:
            line = line.strip()
            if line:
                entries.append(json.loads(line))

    if not entries:
        return header, pl.DataFrame()

    df = pl.DataFrame(entries)
    # Ensure cy is integer
    if "cy" in df.columns:
        df = df.with_columns(pl.col("cy").cast(pl.UInt64))

    return header, df


def _open_trace(p: Path):
    """Open a trace file, handling gzip transparently."""
    if p.suffix == ".gz" or p.suffixes[-2:] == [".gbtrace", ".gz"]:
        import gzip
        return gzip.open(p, "rt", encoding="utf-8")
    return open(p, "r", encoding="utf-8")


def write_parquet(header: dict, df: pl.DataFrame, output: str):
    """Write a trace as Parquet with header in file metadata."""
    # Convert through pyarrow to attach metadata
    table = df.to_arrow()
    import pyarrow.parquet as pq

    existing_meta = table.schema.metadata or {}
    existing_meta[b"gbtrace_header"] = json.dumps(header).encode("utf-8")
    table = table.replace_schema_metadata(existing_meta)
    pq.write_table(table, output, compression="zstd")


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

def cmd_convert(args):
    """Convert between JSONL and Parquet."""
    header, df = read_trace(args.input)

    output = args.output
    if not output:
        p = Path(args.input)
        if p.suffix == ".parquet":
            output = str(p.with_suffix(".gbtrace"))
        else:
            # .gbtrace or .gbtrace.gz -> .parquet
            stem = p.stem
            if stem.endswith(".gbtrace"):
                stem = stem[: -len(".gbtrace")]
            output = str(p.with_name(stem + ".parquet"))

    p_out = Path(output)
    if p_out.suffix == ".parquet":
        write_parquet(header, df, output)
        input_size = Path(args.input).stat().st_size
        output_size = p_out.stat().st_size
        ratio = input_size / output_size if output_size > 0 else 0
        print(f"Converted {len(df):,} entries to {output}")
        print(f"  {input_size:,} bytes -> {output_size:,} bytes ({ratio:.1f}x compression)")
    else:
        # Parquet -> JSONL
        with open(output, "w") as f:
            f.write(json.dumps(header) + "\n")
            for row in df.iter_rows(named=True):
                f.write(json.dumps(row) + "\n")
        print(f"Converted {len(df):,} entries to {output}")


def _skip_boot_entries(df: pl.DataFrame, name: str) -> pl.DataFrame:
    """Remove entries before PC first reaches 0x0100 (game entry point after boot).

    This filters out boot ROM execution so traces with different boot ROM
    configurations can still be compared from the point the game starts.
    """
    if "pc" not in df.columns or len(df) == 0:
        return df

    # Find the first row where pc == "0x0100"
    mask = df["pc"] == "0x0100"
    if not mask.any():
        print(f"  WARNING: {name} has no entry with pc=0x0100, cannot skip boot")
        return df

    first_idx = mask.arg_true()[0]
    skipped = first_idx
    df = df.slice(first_idx)

    if skipped > 0:
        print(f"  Skipped {skipped:,} boot entries from {name}")

    return df


def cmd_diff(args):
    """Compare two trace files and report divergences."""
    header_a, df_a = read_trace(args.trace_a)
    header_b, df_b = read_trace(args.trace_b)

    name_a = header_a.get("emulator", Path(args.trace_a).stem)
    name_b = header_b.get("emulator", Path(args.trace_b).stem)

    # Header comparison
    print(f"Comparing: {name_a} vs {name_b}")
    print(f"  Entries:  {len(df_a):,} vs {len(df_b):,}")

    boot_a = header_a.get("boot_rom", "?")
    boot_b = header_b.get("boot_rom", "?")
    if boot_a != boot_b:
        print(f"  Boot ROM: {name_a}={boot_a}  {name_b}={boot_b}")
        if args.skip_boot:
            print(f"  Aligning at program start (--skip-boot)")
        else:
            print(f"  HINT: use --skip-boot to ignore boot ROM differences")

    rom_a = header_a.get("rom_sha256", "")
    rom_b = header_b.get("rom_sha256", "")
    if rom_a != rom_b:
        print(f"  WARNING: ROM hashes differ!")
        print(f"    {name_a}: {rom_a[:16]}...")
        print(f"    {name_b}: {rom_b[:16]}...")

    # Skip boot entries if requested
    if args.skip_boot:
        df_a = _skip_boot_entries(df_a, name_a)
        df_b = _skip_boot_entries(df_b, name_b)

        # Rebase cycle counts so both traces start from cy=0 at program entry.
        # Without this, different boot ROM durations would prevent cycle alignment.
        if len(df_a) > 0 and len(df_b) > 0 and "cy" in df_a.columns:
            base_a = df_a["cy"][0]
            base_b = df_b["cy"][0]
            if base_a != base_b:
                df_a = df_a.with_columns((pl.col("cy") - base_a).alias("cy"))
                df_b = df_b.with_columns((pl.col("cy") - base_b).alias("cy"))
                print(f"  Rebased cycle counts: {name_a} -{base_a}, {name_b} -{base_b}")

    # Determine common fields (excluding cy which is used for alignment)
    fields_a = set(df_a.columns) - {"cy"}
    fields_b = set(df_b.columns) - {"cy"}
    common_fields = sorted(fields_a & fields_b)

    # Apply field filters
    if args.fields:
        include = set(args.fields.split(","))
        common_fields = [f for f in common_fields if f in include]
    if args.exclude:
        exclude = set(args.exclude.split(","))
        common_fields = [f for f in common_fields if f not in exclude]

    if not common_fields:
        print("ERROR: No common fields to compare.")
        return 1

    only_a = fields_a - fields_b
    only_b = fields_b - fields_a
    if only_a:
        print(f"  Fields only in {name_a}: {', '.join(sorted(only_a))}")
    if only_b:
        print(f"  Fields only in {name_b}: {', '.join(sorted(only_b))}")
    print(f"  Comparing fields: {', '.join(common_fields)}")
    print()

    # Align by cycle count
    merged = df_a.join(df_b, on="cy", how="inner", suffix="_b")

    if len(merged) == 0:
        print("ERROR: No matching cycle counts. Traces may not be aligned.")
        print(f"  {name_a} cy range: {df_a['cy'].min()} .. {df_a['cy'].max()}")
        print(f"  {name_b} cy range: {df_b['cy'].min()} .. {df_b['cy'].max()}")
        return 1

    matched_pct = len(merged) / max(len(df_a), len(df_b)) * 100
    print(f"Aligned {len(merged):,} entries by cycle count ({matched_pct:.1f}% overlap)")

    # Find divergences per field
    divergences = []
    for field in common_fields:
        col_a = field
        col_b = field + "_b"
        if col_b not in merged.columns:
            continue

        mask = merged[col_a] != merged[col_b]
        n_diff = mask.sum()
        if n_diff > 0:
            # Get first divergence
            first_idx = mask.arg_true()[0]
            first_row = merged.row(first_idx, named=True)
            divergences.append({
                "field": field,
                "count": n_diff,
                "first_cy": first_row["cy"],
                "val_a": first_row[col_a],
                "val_b": first_row[col_b],
            })

    if not divergences:
        print("\nNo divergences found! Traces match perfectly.")
        return 0

    # Sort by first divergence cycle
    divergences.sort(key=lambda d: d["first_cy"])

    print(f"\nFound divergences in {len(divergences)} field(s):\n")
    for d in divergences:
        print(f"  {d['field']:6s}  {d['count']:>8,} differences, "
              f"first at cy={d['first_cy']}: "
              f"{name_a}={d['val_a']}  {name_b}={d['val_b']}")

    # Build mask of all divergent rows
    diff_exprs = []
    for field in common_fields:
        col_b = field + "_b"
        if col_b in merged.columns:
            diff_exprs.append(pl.col(field) != pl.col(col_b))

    if diff_exprs:
        combined_mask = diff_exprs[0]
        for expr in diff_exprs[1:]:
            combined_mask = combined_mask | expr
        # Add a boolean column marking divergent rows
        merged = merged.with_columns(combined_mask.alias("_div"))
    else:
        return 0

    div_rows = merged.filter(pl.col("_div"))
    total_div = len(div_rows)

    # Group consecutive divergences into ranges for compact display
    div_cycles = div_rows["cy"].to_list()
    all_cycles = merged["cy"].to_list()

    # Find contiguous runs of divergence
    ranges = []
    i = 0
    while i < len(div_cycles):
        start_cy = div_cycles[i]
        end_cy = start_cy
        while i + 1 < len(div_cycles):
            # Check if next divergence is within a few entries (consecutive)
            cur_idx = all_cycles.index(div_cycles[i])
            next_idx = all_cycles.index(div_cycles[i + 1])
            if next_idx - cur_idx <= 2:  # allow 1 matching entry gap
                i += 1
                end_cy = div_cycles[i]
            else:
                break
        count = sum(1 for c in div_cycles if start_cy <= c <= end_cy)
        ranges.append((start_cy, end_cy, count))
        i += 1

    print(f"\n{total_div:,} divergent entries in {len(ranges)} region(s):\n")
    for j, (start, end, count) in enumerate(ranges[:args.max]):
        if start == end:
            print(f"  Region {j+1}: cy={start} ({count} entry)")
        else:
            print(f"  Region {j+1}: cy={start}..{end} ({count} entries)")
    if len(ranges) > args.max:
        print(f"  ... and {len(ranges) - args.max} more regions")

    # Detailed view of first divergence with context
    first_div_cy = divergences[0]["first_cy"]
    context = args.context

    print(f"\n{'='*72}")
    print(f"Detail: first divergence at cy={first_div_cy}")
    print(f"{'='*72}\n")

    # Get a window around the first divergence
    first_div_idx = all_cycles.index(first_div_cy)
    window_start = max(0, first_div_idx - context)
    # Show context + up to 5 divergent entries + context after
    window_end = min(len(merged), first_div_idx + 5 + context)
    window = merged.slice(window_start, window_end - window_start)

    for i in range(len(window)):
        row = window.row(i, named=True)
        is_div = row["_div"]
        marker = ">" if is_div else " "
        _print_entry_row(row, common_fields, name_a, name_b, marker=marker)

    remaining_after = total_div - min(5, total_div)
    if remaining_after > 0:
        print(f"\n... {remaining_after:,} more divergent entries")

    return 1


def _print_entry_row(row, common_fields, name_a, name_b, marker=" "):
    """Print a single comparison row."""
    cy = row["cy"]
    diffs = []
    for field in common_fields:
        col_b = field + "_b"
        if col_b in row and row[field] != row[col_b]:
            diffs.append(field)

    if diffs:
        diff_strs = [f"{f}: {row[f]} vs {row[f + '_b']}" for f in diffs]
        print(f"{marker} cy={cy:>10}  {', '.join(diff_strs)}")
    else:
        # For context rows, show key fields compactly
        pc = row.get("pc", "?")
        op = row.get("op", "?")
        a = row.get("a", "?")
        print(f"{marker} cy={cy:>10}  pc={pc} op={op} a={a}  (match)")


def cmd_info(args):
    """Show summary info about a trace file."""
    header, df = read_trace(args.input)

    print(f"File:      {args.input}")
    print(f"Emulator:  {header.get('emulator', '?')}")
    print(f"Version:   {header.get('emulator_version', '?')}")
    print(f"Model:     {header.get('model', '?')}")
    print(f"Profile:   {header.get('profile', '?')}")
    print(f"Trigger:   {header.get('trigger', '?')}")
    print(f"ROM hash:  {header.get('rom_sha256', '?')}")
    print(f"Fields:    {', '.join(header.get('fields', []))}")
    print(f"Entries:   {len(df):,}")

    if len(df) > 0 and "cy" in df.columns:
        print(f"Cy range:  {df['cy'].min()} .. {df['cy'].max()}")

    file_size = Path(args.input).stat().st_size
    print(f"File size: {file_size:,} bytes ({file_size / 1024 / 1024:.1f} MB)")


# ---------------------------------------------------------------------------
# CLI entry point
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="GB Trace CLI — convert and compare emulator traces",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    sub = parser.add_subparsers(dest="command", required=True)

    # convert
    p_conv = sub.add_parser("convert", help="Convert between JSONL and Parquet")
    p_conv.add_argument("input", help="Input trace file (.gbtrace or .parquet)")
    p_conv.add_argument("-o", "--output", help="Output file path (auto-detected if omitted)")

    # diff
    p_diff = sub.add_parser("diff", help="Compare two trace files")
    p_diff.add_argument("trace_a", help="First trace file (reference)")
    p_diff.add_argument("trace_b", help="Second trace file (to compare)")
    p_diff.add_argument("--max", type=int, default=10,
                        help="Max divergence points to show in detail (default: 10)")
    p_diff.add_argument("--context", type=int, default=2,
                        help="Context entries before/after each divergence (default: 2)")
    p_diff.add_argument("--fields",
                        help="Only compare these fields (comma-separated, e.g. pc,a,f)")
    p_diff.add_argument("--exclude",
                        help="Exclude these fields from comparison (comma-separated, e.g. ime,ly)")
    p_diff.add_argument("--skip-boot", action="store_true",
                        help="Ignore boot ROM entries (skip to first pc=0x0100)")

    # info
    p_info = sub.add_parser("info", help="Show trace file summary")
    p_info.add_argument("input", help="Trace file to inspect")

    args = parser.parse_args()

    if args.command == "convert":
        cmd_convert(args)
    elif args.command == "diff":
        sys.exit(cmd_diff(args))
    elif args.command == "info":
        cmd_info(args)


if __name__ == "__main__":
    main()
