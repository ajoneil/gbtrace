# gbtrace

**Compare Game Boy emulators instruction-by-instruction.**

gbtrace captures execution traces from multiple Game Boy emulators and lets you compare them side-by-side to find exactly where emulation behaviour diverges — down to individual register values, CPU flags, and IO state.

**[Try the web viewer](https://ajoneil.github.io/gbtrace/)** — browse pre-captured traces from 500+ test ROMs across four emulators, or upload your own traces to compare.

## Why?

Building an accurate Game Boy emulator means getting thousands of subtle hardware behaviours right. When your emulator fails a test, you need to know *exactly* which instruction produced the wrong result and *why*. gbtrace gives you that visibility:

- **Side-by-side comparison** of two traces with per-field diff highlighting
- **SM83 disassembly** inline with register state so you can follow program flow
- **Per-flag diff highlighting** — see exactly which CPU flags (Z/N/H/C) differ
- **Field value charts** with drag-to-zoom to spot patterns in divergence
- **Pre-captured reference traces** from a gate-level accurate simulator (GateBoy) to compare against

## Trace format

A `.gbtrace` file is JSONL (one JSON object per line). The first line is a header describing the trace:

```json
{"_header":true,"format_version":"0.1.0","emulator":"my-emulator","emulator_version":"1.0","rom_sha256":"...","model":"DMG-B","boot_rom":"skip","profile":"gbmicrotest","fields":["pc","sp","a","f","b","c","d","e","h","l","lcdc","stat","ly"],"trigger":"instruction"}
```

Each subsequent line is a trace entry with the fields listed in the header:

```json
{"pc":256,"sp":65534,"a":1,"f":176,"b":0,"c":19,"d":0,"e":216,"h":1,"l":77,"lcdc":145,"stat":128,"ly":153}
```

### Adding trace output to your emulator

To produce traces compatible with gbtrace, emit one JSONL line per instruction (or per T-cycle for higher granularity) with:

1. **Header line** — must include `_header`, `format_version`, `emulator`, `rom_sha256`, `model`, `boot_rom`, `fields`, and `trigger`
2. **Entry lines** — one per instruction/T-cycle, containing numeric values for each field

The `fields` array defines what's captured. Common configurations:

**CPU only:**
```json
"fields": ["pc", "sp", "a", "f", "b", "c", "d", "e", "h", "l"]
```

**CPU + PPU + interrupts + timer:**
```json
"fields": ["pc", "sp", "a", "f", "b", "c", "d", "e", "h", "l", "lcdc", "stat", "ly", "lyc", "scy", "scx", "if_", "ie", "ime", "div", "tima", "tma", "tac"]
```

Values should be numeric (not hex strings). 8-bit fields use 0-255, 16-bit fields (pc, sp) use 0-65535, booleans (ime) use `true`/`false`.

The `trigger` field indicates granularity: `"instruction"` for one entry per instruction, `"tcycle"` for one entry per T-cycle. The web viewer can compare traces at different granularities by automatically downsampling T-cycle traces.

### Capture profiles

Profiles are TOML files that define which fields to capture:

```toml
[profile]
name = "my_profile"
trigger = "instruction"

[fields]
cpu = ["pc", "sp", "a", "f", "b", "c", "d", "e", "h", "l"]
ppu = ["lcdc", "stat", "ly"]
interrupt = ["if_", "ie", "ime"]

[fields.memory]
my_addr = "FF80"    # read memory at 0xFF80 each entry
```

The included adapters use these profiles, but you can produce traces however you like as long as the JSONL format matches.

## Web viewer

The [web viewer](https://ajoneil.github.io/gbtrace/) provides:

- **Test ROM browser** — 500+ pre-captured gbmicrotest cycle-accuracy tests and 11 Blargg CPU instruction tests, with pass/fail indicators per emulator
- **Single trace viewer** — virtual-scrolling table with inline disassembly, field value charts, and search/filter
- **Comparison mode** — side-by-side diff table with synced scrolling, per-field and per-flag diff highlighting, match percentage statistics, and a diff lane on the chart
- **Column toggles** — show/hide fields to focus on what matters; hidden fields are excluded from diff statistics
- **Drag-to-zoom charts** — visualise any field's value over the trace timeline, with dual-trace overlay in comparison mode
- **Upload your own traces** — drop a `.gbtrace`, `.gbtrace.gz`, or `.gbtrace.parquet` file to view or compare

## CLI

The `gbtrace-cli` tool provides offline trace inspection:

```bash
# Show trace metadata
gbtrace-cli info trace.gbtrace.parquet

# Find entries matching a condition
gbtrace-cli query trace.parquet -w "pc=0150"
gbtrace-cli query trace.parquet -w "a changes"
gbtrace-cli query trace.parquet -w "flag z becomes set"

# Compare two traces
gbtrace-cli diff gateboy.parquet gambatte.parquet --fields pc,a,f

# Convert between formats
gbtrace-cli convert trace.gbtrace -o trace.gbtrace.parquet

# Trim a trace
gbtrace-cli trim trace.parquet --until "test_pass=01"
```

The diff command automatically handles traces at different granularities (T-cycle vs instruction) and aligns them by PC.

## Included adapters

Four adapters are included for reference. Each links against an emulator as a library and runs ROMs headlessly:

| Adapter | Emulator | Granularity | Accuracy |
|---------|----------|-------------|----------|
| **gateboy** | [GateBoy](https://github.com/aappleby/metroboy) | T-cycle | Gate-level (reference) |
| **gambatte** | [gambatte-speedrun](https://github.com/pokemon-speedrunning/gambatte-speedrun) | Instruction | High |
| **sameboy** | [SameBoy](https://github.com/LIJI32/SameBoy) | Instruction | High |
| **mgba** | [mGBA](https://github.com/mgba-emu/mgba) | Instruction | Good |

You don't need to use these adapters — any emulator that produces the JSONL format above will work with the viewer and CLI.

## Building

```bash
# Rust CLI
cargo build --release

# Adapters (each Makefile handles dependencies)
make -C adapters/gambatte
make -C adapters/gateboy

# WASM for web viewer (requires wasm-pack)
bash build-web.sh

# Run test suites
bash scripts/run-gbmicrotest.sh
bash scripts/run-blargg-tests.sh

# Local dev server (no-cache headers)
bash scripts/serve.sh
```

## License

MIT — see [LICENSE](LICENSE).
