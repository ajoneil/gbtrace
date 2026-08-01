# morepork-gearsystem

A morepork adapter embedding [Gearsystem](https://github.com/drhelius/Gearsystem)
in SG-1000 mode, a second trace oracle for the TI VDP test suite's `.sg`
builds alongside MAME's sg1000 driver.

**Lineage note:** Gearsystem and Gearcoleco share an author and API
family, so this is a *semi*-independent vote — a distinct VDP
implementation (the SMS VDP's TMS9918 legacy modes vs Gearcoleco's
dedicated TMS core), but related craftsmanship. MAME remains the fully
independent lineage on this machine.

## How it works

Same embedding shape as the gearcoleco adapter: drive
`Processor::RunInstruction()` directly (mirroring `RunToVBlank`'s tick
loop — Gearsystem ticks video+audio only), log every instruction with
the full Z80 file, the eight VDP registers, status
(side-effect-free), the `addr`/`latch`/`buffer` internals, beam
`line`/`dot`, and the RESULT block sampled live. The framebuffer holds
raw TMS colour indices in TMS9918 modes, so the `indexed8` frame is a
straight copy. No BIOS — SG-1000 carts boot at `$0000`. ~50ms per ROM.

Gearsystem's `Video` keeps the TMS-era debug state private, so a small
checked-in patch (`gearsystem-trace-api.patch`) ports Gearcoleco's own
upstream accessors across (`GetStatusReg`, `GetBufferReg`,
`GetAddressReg`, `GetLatch`, `GetRenderLine`, `GetCycleCounter`).

Verified against the pinned upstream (26024c2c): the suite's `sanity.sg`
PASSes with a 326-entry trace at **99.9% agreement with the MAME sg1000
capture** (325 aligned entries, pc-synced). The only diffs are the four
pre-`ld sp` entries: Gearsystem powers the Z80 on with SP=$DFF0 (an SMS
BIOS convention) where MAME resets it to $0000 — a genuine
power-on-state divergence between emulators, worth remembering when a
test reads SP before initializing it.

```
morepork-gearsystem -rom test.sg -out trace.morepork
```

Flags: `-rom` (required) `-out` `-spec NTSC` `-frames` (budget cap)
`-frame=false` (skip the frame snapshot).
