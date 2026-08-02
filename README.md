# morepork

**Capture and compare detailed execution traces from emulators.**

morepork records what happens inside an emulated system — every instruction, register value, CPU flag, and IO/video state change — and provides tools to explore, query, and compare that data. Use it to understand how the hardware works, debug emulator behaviour, investigate how software uses specific features, or verify accuracy against independent reference implementations.

The core is one binary trace format and one toolchain shared across systems, plus per-emulator **adapters** that drive each emulator and emit traces. Running the same test ROM through adapters for emulators with independent lineages turns disagreement between traces into a precise, entry-level diff.

## Supported systems

Each trace is tagged with a **`system`** (which machine) and an **`isa`** (which CPU). The `isa` selects the disassembler and flag vocabulary; the `system` selects the field catalogue and query phrases. Everything else is self-described by the trace header, so the format and CLI stay system-agnostic.

| System | CPU | Captured state |
| --- | --- | --- |
| **Game Boy** (`dmg`) | Sharp SM83 (`sm83`) | CPU registers & flags, PPU (LCDC/STAT/LY…), timer, interrupts, memory watches |
| **Game Boy Color** (`cgb`) | Sharp SM83 (`sm83`) | as Game Boy, plus colour PPU state and double-speed timing |
| **Atari VCS / 2600** (`vcs`) | MOS 6507 (`6502`) | 6507 registers & flags, TIA beam position (line/clock), RIOT timer and ports |
| **Sega SG-1000 / SC-3000** (`sg1000`) | Zilog Z80 (`z80`) | full Z80 register file incl. shadow set, TMS9918A VDP registers/status/beam |
| **ColecoVision** (`coleco`) | Zilog Z80 (`z80`) | same Z80 + TMS9918A catalogue |
| **MSX1** (`msx1`) | Zilog Z80 (`z80`) | same Z80 + TMS9918A catalogue |
| **NES** (`nes`) | Ricoh 2A03 (`6502`) | 6502 registers & flags, PPU control/mask/beam |

Systems that share silicon share an ISA: the Game Boy's DMG and CGB are both `sm83`; the NES's 2A03 and the VCS's 6507 are both `6502`; the SG-1000 line shares the `z80` ISA and the TMS9918A ("TI VDP") catalogue.

## Adapters

Each adapter is a stand-alone CLI at `adapters/<emu>/morepork-<emu>` driving an emulator of independent lineage — see the `adapters/` directory for the current set and each adapter's README for its systems and build details. C/C++/Go adapters link against the C FFI (`crates/morepork-ffi`); Rust adapters use the core crate directly.

## Trace format

morepork uses a compact binary format (`.morepork`) for efficient storage and querying. There are two ways to produce traces:

**Native format** — use the `morepork` Rust library (or its C FFI bindings) to write `.morepork` files directly.

**JSONL format** — for quick integration, emit `.morepork.jsonl` files (one JSON object per line) and convert them:

```bash
morepork convert trace.morepork.jsonl -o trace.morepork
```

The first JSONL line is a header declaring the `system`, the fields captured, and the trigger granularity; every subsequent line is one trace entry:

```json
{"_header":true,"format_version":"0.1.0","system":"dmg","isa":"sm83","emulator":"my-emulator","emulator_version":"1.0","rom_sha256":"...","model":"DMG-B","boot_rom":"skip","profile":"smoke","fields":["pc","sp","a","f","b","c","d","e","h","l","lcdc","stat","ly"],"trigger":"instruction"}
{"pc":256,"sp":65534,"a":1,"f":176,"b":0,"c":19,"d":0,"e":216,"h":1,"l":77,"lcdc":145,"stat":128,"ly":153}
```

Values are numeric (not hex strings). The valid field names depend on the `system`; include whatever level of detail your emulator can supply.

Capture **profiles** (TOML) declare the target `system`, the trigger granularity (`instruction` / `cycle` / `mcycle` / `tcycle`), and which subsystem-layer fields to capture; the profile is validated against the system's field catalogue. Traces at different granularities can still be compared — higher-granularity traces are downsampled to match.

## CLI

The `morepork` tool provides offline trace inspection:

```bash
# Show trace metadata
morepork info trace.morepork

# Find entries matching a condition
morepork query trace.morepork -w "pc=0x0150"
morepork query trace.morepork -w "a changes"
morepork query trace.morepork -w "flag c becomes set"

# Compare two traces (e.g. two emulators of the same system)
morepork diff missingno.morepork gambatte.morepork --fields pc,a,f

# Render frames to PNGs
morepork render trace.morepork -o frames/

# Convert JSONL to native format
morepork convert trace.morepork.jsonl -o trace.morepork
```

Run `morepork --help` for a full list of commands.

## Building

```bash
make cli        # build target/release/morepork
make ffi        # build target/release/libmorepork_ffi.a + header
make adapters   # build the adapters (vendored emulator sources are fetched/cloned per adapter)
```

morepork is a Rust workspace; `cargo build --release --features cli` is equivalent to `make cli`, and `cargo test -p morepork` runs the library tests. See `docs/multi-system.md` for the architecture and what adding a system involves.

## Origins

morepork began life as **gbtrace**, a Game Boy trace tool, and grew into the multi-system tool it is now. Two large pieces from that era were retired once they stopped being maintained and are **available in git history**: the WASM-powered **web viewer** (`web/` + `crates/morepork-wasm`, hosted on GitHub Pages), and the **pre-captured trace library** — the in-repo test-ROM suites (~600 ROMs across 17 suites in `test-suites/`), the `scripts/` trace-generation pipeline, and the CI that built and hosted the trace corpus. The Game Boy *systems and adapters* remain fully supported. morepork no longer runs full-suite trace generation itself; how outside projects drive the adapters is up to them.
