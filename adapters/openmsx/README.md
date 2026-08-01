# morepork-openmsx

A morepork adapter driving [openMSX](https://openmsx.org/) as an **MSX1
oracle** for the TI VDP (TMS9918A) test suite (`missingno-ti-vdp-tests`,
`.mx1` builds). openMSX's VDP implementation is its own lineage —
independent of MAME's `tms9928a` — which is the point of having it: two
oracles that disagree flag a question worth answering before the suite
grows on top of it.

## How it works

openMSX runs headlessly (`SDL_VIDEODRIVER=offscreen`) and is driven over
its **stdio control channel** (`openmsx -control stdio`, an XML-framed Tcl
console). The C-BIOS machines make this ROM-free: the default
`C-BIOS_MSX1_JP` boots cartridges on an open-source BIOS with an NTSC
TMS9918A. All hooks are installed before `set power on`, so captures are
deterministic:

1. A **breakpoint at the cartridge's INIT vector** (read from the `.mx1`
   "AB" header) installs a per-instruction `debug set_condition` hook.
   Tracing starts at the cart's own code — the BIOS boot is machine noise,
   not test body, and C-BIOS's boot wouldn't compare against a real BIOS
   anyway.
2. The hook logs one line per instruction, **entirely in-process** (Tcl
   inside openMSX, no round-trips): the Z80 register file *plus the VDP
   state that gdbstub oracles cannot see* — the eight write registers, the
   status register, and the internal machinery the morepork `ti_vdp`
   catalogue names: `addr` (`VRAM pointer`), `latch` (`VDP register latch
   status`), `buffer` (`VDP data latch value`). The suite's sanity ROM
   traces INIT→verdict (~320 instructions) in about a millisecond.
3. A **watchpoint on the RESULT byte** (`$E000`, `$A5` PASS / `$5A` FAIL)
   tears the trace down at the verdict; the RESULT block lands on the
   final entry, like the other adapters.
4. `screenshot -raw` captures the rendered frame after the readout
   settles. The raw frame is 320×240 (sometimes 2×-scaled to 640×480 —
   normalized) with the 256×192 active area at (32,24), located
   empirically with an all-white VRAM fill driven from Tcl. Pixels
   reverse-map to TI VDP colour indices against **openMSX's own rendered
   palette** (measured per backdrop colour — openMSX does not render the
   classic datasheet RGB values), with nearest-neighbour as the safety net
   for GL stacks that shade slightly differently. The `indexed8` snapshot
   stamps the canonical TI VDP palette, same as the mame adapter.

Verified against openMSX 21.0: the suite's `sanity.mx1` PASSes with a
324-entry trace and a readout frame identical to the MAME sg1000/sc3000/
coleco captures. ~1.2s per ROM including launch.

```
morepork-openmsx -rom sanity.mx1 -out trace.morepork
```

Flags (mame-adapter conventions): `-rom` `-out` `-machine` (default
`C-BIOS_MSX1_JP`) `-spec NTSC` `-frames` (emulated-seconds cap)
`-frame[=bool]`.

## Notes

- **Debuggable reads are side-effect-free** (openMSX peeks) — logging the
  status register per instruction does not clear F/5S/C.
- **Real-BIOS runs**: openMSX can also boot machine configs using real
  MSX1 BIOS dumps (e.g. via a custom machine XML pointing at local system
  ROMs) for comparing C-BIOS-hosted behaviour against a production BIOS;
  `-machine` selects. The VDP subjects shouldn't care, which is itself
  worth confirming once.
- **PAL/TMS9929A**: `C-BIOS_MSX1_EU` carries the PAL VDP; blocked on the
  suite treating PAL as provisional, not on this adapter.
