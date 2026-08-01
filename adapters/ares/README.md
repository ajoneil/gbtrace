# morepork-ares

A morepork adapter embedding [ares](https://ares-emu.net/) as an
**independent-lineage trace oracle** for the TI VDP test suite — the
ColecoVision core today; the SG-1000 and MSX cores build into the same
binary and are staged to be wired next (`-system` reports them as such).

## How it works

ares embeds as static libraries (cmake targets `ares` + `mia`, cores
limited to cv/sg/msx) behind a thin `ares::Platform` frontend. **No ares
source patches**: the CPU's per-instruction debugger hook routes tracer
notifications to `Platform::log`, so enabling the instruction tracer
(`setTerminal(true)`) yields a synchronous callback at every instruction
boundary on the CPU's own cothread. The callback ignores the disassembly
text and reads core state directly — `ares::ColecoVision::cpu` is a Z80
with public registers; the TMS9918's decomposed io state is read through
a TU-local access-specifier override (layout-neutral; keeps the embed
patch-free). The eight write registers and status byte are reconstructed
as the exact inverse of `TMS9918::register` / the status read.

mia builds the system/cartridge paks (BIOS via `-bios`, never bundled),
`Platform::pak` serves them, and `root->run()` drives frames until the
RESULT watch (sampled live per instruction, like the other TI VDP
adapters) sees the verdict. The frame comes from `Platform::video`,
reverse-mapped against a palette built at runtime by calling the core's
own `vdp.color()` — a calibration table that cannot drift. ~75ms per ROM.

## Build notes

ares' CMake tree assumes it is the top-level project (its dependency
machinery breaks under `add_subdirectory`), so the Makefile builds
two-phase: configure/build the pinned clone standalone, then compile the
frontend with the same flags the cores used (`extract-flags.py` reads the
compile database) and link `libares.a`, `libmia.a`, libco, the nall
objects (minus nall's `main` trampoline), and the thirdparty archives.

## Verified

Against the pinned upstream (b80f67d3): the suite's `sanity.col` PASSes
with a 336-entry trace and a readout frame identical to the other
adapters'. Cross-oracle register diffs surface genuine **power-on-state
divergences** rather than capture noise — three emulators, three
conventions:

| | ares | Gearcoleco | MAME |
|---|---|---|---|
| SP at reset | `$FFFF` | `$DFF0` | `$0000` |
| AF at reset | `$FFFF` | `$0040` | `$0040` |
| IX/IY at reset | `$0000` | `$FFFF` | `$FFFF` |

The suite's harness initializes everything it reads, so verdicts agree;
a future test probing uninitialized state has three votes to compare
(and hardware endorsement to arbitrate).

```
morepork-ares -system coleco -rom test.col -bios colecovision.rom -out trace.morepork
```

Flags: `-system coleco` `-rom` `-bios` (required) `-out` `-spec NTSC`
`-frames` (budget cap) `-frame=false`.
