# morepork-gearcoleco

A morepork adapter embedding [Gearcoleco](https://github.com/drhelius/Gearcoleco)
(Ignacio Sanchez's ColecoVision emulator) as a second, **independent-lineage
trace oracle** for the TI VDP test suite's `.col` builds, alongside MAME's
coleco driver.

## How it works

Gearcoleco's core is a plain C++ library that exposes everything the
morepork `ti_vdp` catalogue names **without patches**: the full Z80
register file (shadow set, `wz`, `i`/`r`, `iff1/2`, `im`, `halted` via
`Processor::GetState()`), the eight VDP write registers, side-effect-free
status (`Video::GetStatusReg()`, not the CPU-visible `GetStatusFlags()`),
the internal `addr`/`latch`/`buffer` machinery, and the beam position
(`GetRenderLine()`/`GetCycleCounter()` → `line`/`dot`).

The adapter drives `Processor::RunInstruction()` directly, mirroring the
body of the core's own `RunToVBlank` tick loop (video/audio/memory ticks
per instruction, audio drained per frame), and logs one entry per
instruction with the RESULT block sampled live — the trace ends on the
entry where RESULT latches `$A5`/`$5A`. The core's internal framebuffer
holds **raw TMS colour indices at native 256×192**, so the `indexed8`
frame snapshot is a straight copy — the only adapter that needs no
palette reverse-mapping at all. ~50ms per ROM.

Verified against Gearcoleco @e8cf314e (pinned in the Makefile): the
suite's `sanity.col` PASSes with a 336-entry trace whose common register
columns diff **100% against the MAME coleco capture** (335 aligned
entries, pc-synced), and the readout frame matches the other adapters'.

The ColecoVision BIOS is required (the core refuses to run without it)
and deliberately not bundled:

```
morepork-gearcoleco -rom test.col -bios colecovision.rom -out trace.morepork
```

Flags: `-rom` `-bios` (both required) `-out` `-spec NTSC` `-frames`
(budget cap) `-frame=false` (skip the frame snapshot).

## Build notes

`make` clones the pinned upstream into `gearcoleco/` (gitignored),
compiles `src/` + `src/audio/` + the vendored miniz, and links
`libmorepork_ffi.a`. The adapter defines `g_mcp_stdio_mode` (a
frontend-owned flag the core's logger expects) since we are the frontend.
