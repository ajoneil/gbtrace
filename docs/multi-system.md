# Going multi-system

morepork was built for one machine and has since grown past it, the same way its
sibling project missingno did — that frontend drives Game Boy, Atari 2600, Master
System, and NES cores through system-agnostic seams (`docs/adding-a-system.md` in
the missingno repo, https://github.com/ajoneil/missingno). This document is the
equivalent map for the trace side: how the format, core library, CLI, FFI, and
web viewer stay system-agnostic, where system knowledge lives (the registry
currently hosts the `dmg`, `cgb`, `nes`, `vcs`, and `sg1000` systems, on the
`sm83`, `6502`, and `z80` ISAs), and what adding a system involves. Trust the
seams named here, but verify signatures against the source before building on
them.

## Two orthogonal axes

The header carries two small strings that, with the existing `pix_format`,
replace the old monolithic `family` tag:

- **`isa`** (`"sm83"`, `"6502"`, `"z80"`) — the instruction-set architecture.
  Names the decoder (disassembly is driven by the shared `missingno_core`
  instruction-set vocabulary, keyed on this id) and the flag vocabulary.
  Systems that share silicon share an ISA: the Game Boy's DMG and CGB are both
  `sm83`; the NES's 2A03 and the VCS's 6507 are both `6502`.
- **`system`** (`"dmg"`, `"cgb"`, `"nes"`, `"vcs"`, `"sg1000"`) — the machine
  identity.
  Selects the default field catalogue, semantic query phrases, diff-alignment
  hints, and viewer panels. Distinct from `model` (the free-form hardware
  revision, `"DMG-B"`/`"CGB-C"`).

Frame reconstruction keys off `pix_format` (`shade2`/`rgb555` → the GB pixel
replay, `indexed8` → the system-agnostic indexed-frame path), not the system.

**DMG↔CGB** are two `system`s on the shared `sm83` ISA + GB render: same
disassembler, flags, phrases, and reconstruction; the CGB adds a `cgb`
subsystem (colour palettes, KEY1 double-speed, VRAM/WRAM banks, HDMA). The
`systems.{dmg,cgb}` manifest dimension and `SYSTEMS=` build sharding already
carry this split end-to-end. **A new machine** (NES, SMS, VCS, …) adds a
`system` (and an `isa` if its CPU is new): a new field catalogue, frame
geometry, and — when the ISA is new — decode table and flag semantics.

## What is generic (do not "fix" these)

The data plane is system-agnostic and must stay that way:

- `entry.rs` — `TraceEntry` is a `BTreeMap<String, serde_json::Value>`; setters
  are name-agnostic.
- `store.rs` / `reader.rs` / `downsample.rs` — the `TraceStore` trait is
  columns-by-name; JSONL reading infers fields.
- `format/` — the container (chunks, Arrow IPC field groups, zstd, footer,
  dictionary encoding) is field-name-driven.
- `comparison.rs` — the diff engine operates on arbitrary columns; system
  specifics enter only through alignment hints.
- `morepork-ffi` — the C writer API is column-index + field-name driven (the
  adapter builds the header JSON itself and pushes typed values by column).
  No register structs, no screen dimensions.
- Web shell — trace-table, trace-diff-table, chart, timeline, query, selector,
  file-loader are column-generic and driven by header metadata.

## The architecture

Two principles, in tension-free layers:

### 1. The format is fully self-describing

Readers need **zero system-specific knowledge** for info/query/diff/table/chart, and
self-description is *required*: the reader rejects a header without field
metadata (there is no catalogue fallback — old traces get a clear
"regenerate" error). The header carries, beyond the ordered `fields` list:

- `system: String` — `"dmg"`, `"cgb"`, `"nes"`, `"vcs"`, … Absent means `"dmg"`.
- `isa: String` — `"sm83"`, `"6502"`. Empty on construction; the writer derives
  it from `system` (so disassembly stays self-describing for unknown systems).
- `field_defs` — ordered typed declarations `{ name, type, subsystem, layer,
  nullable, dictionary }`; the source of truth for resolution.
- `field_groups` — the chunk storage layout actually used for this file (each
  group is one Arrow IPC block).
- `instruction_addr_field` — names the column that means "address of the
  current instruction" (the writer prefers `op_addr`, which is stable across
  an instruction's T-cycles, over `pc`).
- `snapshot_kinds` — tag-indexed kind names. `frame` (tag 0) and `memory`
  (tag 1) are the only kinds anything writes or decodes; the writer stamps
  those two, and a reader resolves any higher tag by name from this list.

`MoreporkWriter::create` enriches the header itself — field defs and the
instruction-address column from the system catalogue, the `frame`/`memory`
snapshot-kind names, and storage groups from the defs when the caller passes
none — so every producer (FFI adapters, missingno, `convert`) writes
self-describing traces without changes on their side.

`pix_format` values: `shade2` (DMG greyscale pix stream), `rgb555` (CGB colour
pix stream), and `indexed8` — the system-agnostic form, one palette index per
pixel, with per-frame dimensions, the frame-end palette, and the display pixel
aspect carried in each `frame` snapshot payload (`snapshot::IndexedFrame`,
mirroring missingno's `IndexedFrame`; VCS frame height is emergent, SMS CRAM
is mutable, so both ride per-frame). GB traces keep their raw frame payloads.

### 2. System knowledge lives in one registry in the core

`crates/morepork/src/system/` — a static registry (like missingno's `FAMILIES`
table): `mod.rs` holds the `Isa` and `System` structs plus the `ISAS`/`SYSTEMS`
registries, and one module per machine — `gb/` (the `dmg` and `cgb` systems,
which share the SM83 disassembler, catalogue base, and rendering), `nes.rs`,
`vcs.rs`, `sg1000.rs`. Chips shared across systems live in the sibling
`crates/morepork/src/hardware/`, mirroring missingno's `crates/hardware/` vs
`crates/systems/` split: `hardware/mos6502.rs` carries the flag vocabulary and
CPU field catalogue shared by the NES's 2A03 and the VCS's 6507 (each system
keeps only its CPU-address-to-ROM-offset mapping), `hardware/z80.rs` and
`hardware/ti_vdp.rs` the same for the SG-1000 line (ColecoVision and MSX
carry the identical Z80 + TMS9918A pair), while single-system silicon
stays with its system (the SM83 in `system/gb`, like missingno's
`systems/gb/src/isa.rs`). An `Isa` carries the flag vocabulary; a
`System` names its `Isa` and provides:

- **Default field catalogue** (`subsystems`) — validates profiles and types
  their fields at write time. The GB catalogue lives in
  `system/gb/catalogue.rs`.
- **Flag vocabulary** (`isa.flags`) — name → (field, bit), carried by the
  system's `Isa` (shared across systems on the same ISA), driving the query
  engine's `flag …` conditions and the viewer's flag rendering (exported
  through wasm `flagDefs()`).
- **Semantic query phrases** (`exact_phrases`, `numbered_phrases`) — named
  conditions (`"lcd on"`, `"ppu enters mode N"`, `"vblank starts"`) that
  desugar to the generic `Condition` variants; `parse_condition` takes the
  system whose vocabulary it parses. The wasm `semanticPhrases()` export
  surfaces these to drive the query builder's one-click chips.
- **Diff alignment hint** (`entry_addrs`) — the address every trace of the
  system reaches at program entry plus the entry's second instruction (GB:
  cartridge entry `0x0100`/`0x0101`); systems without a fixed entry use the
  generic first-common-address alignment.
- **Frame reconstruction** — the GB `pix`/`ly` replay and VRAM/tile logic
  (`system/gb/framebuffer.rs`, `system/gb/vram.rs`) are system capabilities,
  not format features. The generic path is `frame` snapshots. The render gate
  keys on `pix_format` (`shade2`/`rgb555` → this GB replay); promote to a
  function-table hook when a second render model implements reconstruction.
- **Snapshot payloads** — `frame` and `memory` are the only snapshot kinds,
  both system-agnostic (`src/snapshot.rs`: `IndexedFrame`, `MemoryRegion`).
  Console state is no longer re-founded from morepork-side `gb.*` payloads —
  missingno restores from its own `missingno_core` state vocabulary — so the
  registry defines no typed per-system snapshot layouts.

What stays *out* of the registry: everything in the "generic" list. The
registry is consulted only for rendering, semantic query sugar, catalogue
defaults/validation, and diff alignment hints; disassembly is ISA-keyed
through the shared `missingno_core` instruction set (`src/disasm.rs`), not a
per-system entry.

The `profile.rs` free functions (`lookup_field`, `field_type`,
`field_nullable`) consult the GB catalogue only — a write-side convenience
for GB producers (missingno-gb types its emitters through them). Readers use
`TraceHeader::resolve_*`; other systems go through their registry entry.

### Profiles

```toml
[profile]
name = "nes-smoke"
system = "nes"          # absent = "dmg"
trigger = "cycle"

[fields]
cpu = ["pc", "a", "x", "y", "s", "p"]
```

`[fields]` keys are validated against the system catalogue (unknown subsystem
keys are an error), resolved in catalogue order. `[fields.memory]` and
`[fields.extensions]` are system-independent. (A profile's `system` is the
write-side catalogue baseline; the trace header's `system` is set by the
adapter from `--model`, so a shared `dmg` profile can be captured as `cgb` —
the CGB catalogue is a superset, so shared fields still validate.)

## Compatibility constraints

1. **Trace-file backward compatibility is NOT required.** There is no
   external userbase and captured traces are regenerable, so the format may
   evolve freely; prefer deleting legacy fallbacks over freezing them. The
   reader rejects headers without field metadata with a clear "regenerate"
   error. After a format change, regenerate the Spaces corpus (`traces.yml`)
   and any local `build/traces`.
2. **missingno tracks morepork's git HEAD with no pin**
   (`missingno-{gb,gbc,nes,vcs}/Cargo.toml: morepork = { git = ... }`).
   Breaking the Rust API on main breaks missingno's `--features morepork`
   build immediately. Land breaking changes together with the matching
   missingno update, and push morepork first, then missingno immediately
   after. The consumer surface:
   - `morepork::format::write::MoreporkWriter` — `create(path, &header,
     &groups)` (usually `&[]`: the writer groups by the header's field
     defs), `set_u8/u16/bool/str/null(col, v)`, `finish_entry`,
     `mark_frame`, `write_snapshot(tag, &[u8])`, `finish`.
   - `morepork::format::{TAG_FRAME, TAG_MEMORY}` — the only snapshot tags.
   - `morepork::header::{TraceHeader (all fields), HeaderFieldDef,
     ExtensionField, PixFormat}`.
   - `morepork::profile::{FieldType, field_type, field_nullable}`.
   - `morepork::{BootRom, Profile (.trigger/.fields/.extensions/.memory/.name),
     Trigger, Error::Profile}`.
   - `morepork::snapshot::{IndexedFrame, MemoryRegion, build_memory_payload}` —
     the system-agnostic frame/memory payloads (the NES and VCS tracers write
     `IndexedFrame` frames). Console state is restored on the missingno side
     from its own `missingno_core` state vocabulary, not from morepork-side
     `gb.*` snapshot structs (those were removed).
3. **Adapter CLI surface is frozen** (`--rom/--profile/--output/--frames/
   --stop-when/--stop-opcode/--reference/--model`): `gen-rules.py` and the
   trace scripts hard-code it. Additions must not disturb existing
   invocations.

## What each system brings

| | NES | VCS | SMS |
|---|---|---|---|
| CPU state | 6502: `a,x,y,s,p,pc` (+rdy) | same 6502 core (6507) | Z80: full main+shadow set, `ix,iy,sp,pc,wz,i,r,im,iff1/2` |
| Stepping | `step_cycle` / `step_instruction` / `step_frame` | same + own core-side `Debugger` | `Cpu::step` returns T-states |
| Frame | 256×240 fixed, 6-bit colour indices | `Vec<[u8; VISIBLE_CLOCKS]>`, **emergent height**, TIA indices | 256×192, CRAM-indexed + per-frame 32-byte CRAM |
| Disassembler | ✓ shared `hardware/mos6502` + iNES map | ✓ shared core + 6507 cartridge map | ✗ none exists |
| Trace hooks in missingno | ✓ `missingno-nes/src/trace.rs` | ✓ `missingno-vcs/src/trace.rs` | none (its `bus_trace()` is test-only) |

NES went second because it exercises every seam (catalogue, flags, disasm,
indexed frames) with fixed geometry; VCS third as the stress test of the
per-frame-dimensions model (its emergent height is why `IndexedFrame`
carries dimensions per frame). SMS waits for a Z80 disassembler or ships
with hex-dump disassembly.

**SG-1000** (`sg1000`, the first `z80` system) entered ahead of SMS as the
host for TI VDP (TMS9918A) tests: Z80 + `hardware/ti_vdp` + SN76489, fixed
256×192 `indexed8` frames, cartridge at 0x0000 with no BIOS. ColecoVision
and MSX carry the identical Z80 + TMS9918A pair and become thin sibling
entries when tests or oracles target them. missingno has no SG-1000 core
yet, so its first oracles are external adapters; disassembly shares SMS's
blocker (the `hardware/z80` crate's `InstructionSet` impl in missingno).

On the missingno side each family's tracer is a `trace` module in its core
crate behind a `morepork` feature (a `Tracer` with per-field emitters,
`mark_frame` writing self-contained `IndexedFrame` payloads), routed from
the `missingno trace` CLI subcommand by ROM detection — a per-family tracer
there is missingno work, but the family contract in this document is what
it implements.

## Web viewer notes

Field display is metadata-driven: the wasm store exposes `fieldDefs()`,
`flagDefs()`, and `semanticPhrases()`; `web/src/lib/format.js` keeps its GB
tables only as defaults for legacy traces, and the query builder's chips
come from the system vocabulary. Frames render through two paths: the GB
per-entry pix replay (fixed 160×144, partial-frame scrubbing), and indexed
frame snapshots (`hasIndexedFrames()`/`indexedFrame()`), where each payload
carries its own dimensions, palette, and pixel aspect. One deliberate
GB-shaped remainder: the ASM column anchors at the visible `pc` column.
Every surveyed system names its program counter `pc`, while
`instruction_addr_field` is typically the hidden `op_addr` — anchoring
there would remove the column from the default GB view.

GB-specific panels (sprite table, APU, FIFO, VRAM, pixel replay) are gated
on the SM83/gb-render systems (`_isGbLike`, i.e. `isa === 'sm83'`, covering
`dmg` and `cgb`) plus the fields they render; default visible columns come
from the curated GB register set for gb-line traces and from the header's
field defs for any other system. A per-system panel registry keyed on
`header.system` becomes worthwhile when a non-gb system ships panels of
its own.

## Naming

The rename ("emutrace"?) is mechanical but wide: crate names, `morepork.h` /
`morepork_*` C symbols, the `MPRK` magic, binary name, repo name, CI, Pages
URL, Spaces paths, missingno's git dependency URL, and the `.morepork`
extension. Nothing in the architecture depends on it, so: build everything
under the current names and rename in one commit once a name is chosen.
Format note for that day: with back-compat waived, the magic can simply
change with the name; regenerate traces after.

## Order of work

The generalization landed in this order, each step leaving the GB pipeline
green (`cargo test -p morepork`, spot-check `make traces-<suite>`):
self-describing format → system registry (GB moved behind it,
`Indexed8`/`IndexedFrame`) → NES (catalogue, flags, 6502 disassembler,
missingno tracer, viewer) → system-aware web viewer (indexed frames,
semantic phrase chips, panel gating) → VCS (the emergent-height stress
test, on the shared `mos6502` core) → SG-1000 (the `z80` ISA and shared
`ti_vdp` catalogue in `hardware/`). What remains:

1. **Z80 disassembly** — the `z80` ISA and flag vocabulary are registered,
   but decode needs an `InstructionSet` impl in missingno's `hardware/z80`
   crate (its decode table exists; only the display mapping is missing).
   Until it lands, `z80` traces disassemble as hex. Unblocks SMS too.
2. **SMS** — beyond the disassembler, its missingno core has no trace
   hooks yet, and its VDP (11 registers, CRAM, counter ports) gets its
   own `hardware/` module distinct from `ti_vdp`.
3. **Non-GB test suites** — the manifest's `systems.{dmg,cgb}` map and the
   test picker stay GB-only until one exists; they need a system level then
   (`scripts/manifest.py`, `web/src/components/test-picker.js`).
4. **Rename** — blocked on the name decision; deliberately last.
