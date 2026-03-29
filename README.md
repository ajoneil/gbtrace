# gbtrace

**Capture and explore detailed execution traces from Game Boy emulators.**

gbtrace records what happens inside a Game Boy — every instruction, register value, CPU flag, and IO state change — and provides tools to explore, query, and compare that data. Use it to understand how the hardware works, debug emulator behaviour, investigate how games use specific features, or verify accuracy against a gate-level reference.

**[Try the web viewer](https://ajoneil.github.io/gbtrace/)** — browse pre-captured traces from hundreds of test ROMs across multiple emulators, or upload your own.

## Features

- **Detailed execution traces** — capture every register value, CPU flag, and IO state change, per-instruction or per-T-cycle
- **Field value charts** with drag-to-zoom to visualise how registers and IO change over time
- **Side-by-side trace comparison** with per-field and per-flag diff highlighting
- **Pre-captured reference traces** from a gate-level simulator (GateBoy) and multiple emulators across 600+ test ROMs
- **CLI query engine** — search traces by condition (e.g. `pc=0150`, `flag z becomes set`, `a changes`)
- **SM83 disassembly** in the web viewer, inline with register state
- **Open trace format** — any emulator can produce compatible traces

## Trace format

gbtrace uses a compact binary format for efficient storage and querying. There are two ways to produce traces:

**Native format** — use the `gbtrace` Rust library (or its C FFI bindings) to write `.gbtrace` files directly.

**JSONL format** — for quick integration, emit `.gbtrace.jsonl` files (one JSON object per line). Both the CLI and web viewer can work with JSONL files directly, but you can convert them for smaller file sizes and faster loading:

```bash
gbtrace convert trace.gbtrace.jsonl -o trace.gbtrace
```

### JSONL format

The first line is a header describing the trace:

```json
{"_header":true,"format_version":"0.1.0","emulator":"my-emulator","emulator_version":"1.0","rom_sha256":"...","model":"DMG-B","boot_rom":"skip","profile":"gbmicrotest","fields":["pc","sp","a","f","b","c","d","e","h","l","lcdc","stat","ly"],"trigger":"instruction"}
```

Each subsequent line is a trace entry with the fields listed in the header:

```json
{"pc":256,"sp":65534,"a":1,"f":176,"b":0,"c":19,"d":0,"e":216,"h":1,"l":77,"lcdc":145,"stat":128,"ly":153}
```

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

The `trigger` field indicates granularity: `"instruction"` for one entry per instruction, `"mcycle"` for one entry per M-cycle, or `"tcycle"` for one entry per T-cycle. Traces at different granularities can be compared — the viewer automatically downsamples higher-granularity traces to match.

Capture profiles define which fields to record, but you don't need to provide all of them — include whatever level of detail your emulator can supply.

## Web viewer

The [web viewer](https://ajoneil.github.io/gbtrace/) provides:

- **Test ROM browser** — pre-captured traces from hundreds of test ROMs across multiple emulators, with pass/fail indicators
- **Trace viewer** — virtual-scrolling table with inline SM83 disassembly, field value charts, and search/filter
- **Comparison mode** — side-by-side diff with synced scrolling, per-field and per-flag highlighting, and match percentage statistics
- **Drag-to-zoom charts** — visualise any field over the trace timeline, with dual-trace overlay in comparison mode
- **Upload your own traces** — drop a `.gbtrace` or `.gbtrace.jsonl` (or gzipped `.gbtrace.jsonl.gz`) file to view or compare

### Included traces

The pre-captured traces come from several emulators:

- **[GateBoy](https://github.com/aappleby/metroboy)** — a gate-level simulation of the Game Boy CPU, providing the closest reference to actual hardware. Gate propagation delay and analogue effects mean it doesn't perfectly match real hardware behaviour in all cases.
- **[Missingno](https://github.com/ajoneil/missingno)** — the author's emulator, with full support for all gbtrace trace features.
- Traces from several well-regarded community emulators ([SameBoy](https://github.com/LIJI32/SameBoy), [gambatte](https://github.com/pokemon-speedrunning/gambatte-speedrun), [mGBA](https://github.com/mgba-emu/mgba)) are also included, though their traces capture less detail than GateBoy and Missingno.

## CLI

The `gbtrace` tool provides offline trace inspection:

```bash
# Show trace metadata
gbtrace info trace.gbtrace

# Find entries matching a condition
gbtrace query trace.gbtrace -w "pc=0x0150"
gbtrace query trace.gbtrace -w "a changes"

# Compare two traces
gbtrace diff gateboy.gbtrace gambatte.gbtrace --fields pc,a,f

# Convert JSONL to native format
gbtrace convert trace.gbtrace.jsonl -o trace.gbtrace
```

Run `gbtrace --help` for a full list of commands.

## Building

gbtrace is a Rust workspace. The crates are not yet published to crates.io — install from git:

```bash
# CLI
cargo install --git https://github.com/ajoneil/gbtrace --features cli gbtrace

# Local web viewer (requires wasm-pack)
make serve
```
