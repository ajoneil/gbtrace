# gbtrace

Instruction-level and T-cycle trace capture and comparison for Game Boy emulators.

Capture execution traces from multiple emulators, compare them side-by-side, and pinpoint exactly where emulation behaviour diverges — down to individual register values and CPU flags.

## Web viewer

The web-based trace viewer lets you browse pre-captured traces from the included test suites, upload your own, and compare any two traces with:

- Side-by-side diff table with synced scrolling and per-field diff highlighting
- SM83 disassembly inline with trace data
- Field value charts with drag-to-zoom and diff lane
- Column toggles to focus on specific registers/IO
- Per-flag (Z/N/H/C) diff highlighting in the F register
- Auto-alignment and T-cycle-to-instruction downsampling for cross-adapter comparison

## Adapters

Four emulator adapters capture traces in the `.gbtrace` format:

| Adapter | Emulator | Granularity | Notes |
|---------|----------|-------------|-------|
| **gateboy** | [GateBoy/LogicBoy](https://github.com/aappleby/metroboy) | T-cycle | Gate-level accurate, highest accuracy |
| **gambatte** | [gambatte-speedrun](https://github.com/pokemon-speedrunning/gambatte-speedrun) | Instruction | Fast, good accuracy |
| **sameboy** | [SameBoy](https://github.com/LIJI32/SameBoy) | Instruction | High accuracy, runs boot ROM |
| **mgba** | [mGBA](https://github.com/mgba-emu/mgba) | Instruction | GBA/GB emulator |

Each adapter links against its emulator as a library and runs ROMs headlessly, producing `.gbtrace` (JSONL) or `.gbtrace.parquet` files.

## Test suites

### gbmicrotest (509 tests)

Minimal cycle-accuracy tests from [aappleby/gbmicrotest](https://github.com/aappleby/gbmicrotest). Each test checks a single register or memory value at a specific cycle, producing traces of 10-200 entries. Captured at T-cycle granularity with gateboy, instruction-level with others.

### Blargg CPU (11 tests)

Individual CPU instruction tests from the Blargg test ROM suite. Longer traces (up to 7M entries) testing all CPU instructions.

## Building

### Rust CLI and library

```bash
cargo build --release
```

This builds `gbtrace-cli` for trace inspection, conversion, diffing, and querying.

### Adapters

Each adapter has a Makefile that handles dependencies:

```bash
make -C adapters/gambatte   # clones gambatte-speedrun, builds via scons
make -C adapters/sameboy    # requires pre-built SameBoy
make -C adapters/mgba       # requires pre-built mGBA
make -C adapters/gateboy    # clones metroboy/metrolib
```

### WASM (web viewer)

```bash
bash build-web.sh           # requires wasm-pack
```

### Running tests

```bash
bash scripts/run-gbmicrotest.sh           # all adapters, all microtests
bash scripts/run-blargg-tests.sh          # all adapters, blargg CPU tests
bash scripts/run-gbmicrotest.sh --emu gateboy --filter poweron  # specific
```

### Local dev server

```bash
bash scripts/serve.sh       # serves on http://localhost:3080 with no-cache
```

## Trace format

Traces are JSONL files (one JSON object per line). The first line is a header:

```json
{"_header":true,"format_version":"0.1.0","emulator":"gambatte-speedrun","rom_sha256":"...","model":"DMG-B","boot_rom":"skip","profile":"gbmicrotest","fields":["pc","sp","a","f",...],"trigger":"instruction"}
```

Subsequent lines are entries with the fields listed in the header:

```json
{"pc":256,"sp":65534,"a":1,"f":176,"b":0,"c":19,...}
```

Traces can be converted to Parquet for compact storage via `gbtrace-cli convert`.

## CLI

```
gbtrace-cli info    <trace>              # show trace metadata
gbtrace-cli query   <trace> -w "pc=150"  # find matching entries
gbtrace-cli diff    <a> <b> [<c>...]     # compare traces
gbtrace-cli convert <trace> -o out.parquet
gbtrace-cli trim    <trace> --until "test_pass=01"
gbtrace-cli strip-boot <trace>
```

## Profiles

Capture profiles (TOML) define which fields to record:

```toml
[profile]
name = "gbmicrotest"
trigger = "tcycle"

[fields]
cpu = ["pc", "sp", "a", "f", "b", "c", "d", "e", "h", "l"]
ppu = ["lcdc", "stat", "ly", "lyc", "scy", "scx"]
interrupt = ["if_", "ie", "ime"]
timer = ["div", "tima", "tma", "tac"]

[fields.memory]
test_pass = "FF82"
```
