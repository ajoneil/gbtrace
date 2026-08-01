# morepork-mame

A morepork adapter driving [MAME](https://www.mamedev.org/) as an
**independent-lineage** behavioural oracle. Two systems share the one
mechanism (a `-system` flag selects; the per-system knowledge lives in the
`systems` table in `main.go`):

- **`vcs`** (default) — the `a2600` driver, a third oracle for the VCS test
  suite alongside Stella and Gopher2600. (Those two descend from shared
  TIA-core work; MAME's driver is its own, so it's a genuine third vote —
  not just a third copy.)
- **`sg1000`** — the `sg1000` driver, an oracle for the TI VDP (TMS9918A)
  test suite (`missingno-ti-vdp-tests`). See the SG-1000 section below.

## Approach (differs from the Stella/Gopher2600 adapters)

MAME is far too large to link the way the Stella and Gopher2600 adapters embed
their emulators. But the output must still be a **native `.morepork` file written
through the morepork FFI** — no JSONL. MAME is driven for per-instruction state
via its scripting/debugger, and that state is fed to the FFI to write native.

Two candidate mechanisms (finalise against installed MAME):

1. **Lua ⇄ FFI binding (one-step, preferred).** Build a small Lua C module
   (`morepork_lua.so`) that wraps `libmorepork_ffi.a` (writer_new / set_u8/u16 /
   finish_entry / mark_frame_indexed / close). A MAME `-autoboot_script`
   `require`s it, steps the CPU (`devices[":maincpu"].debug:step()`), reads
   `state[...]` + `spaces["program"]:read_u8()`, and writes native morepork
   directly. Risk: MAME's Lua sandbox may restrict `require` of C modules.

2. **Debugger trace-log → FFI converter (two-step fallback).** A `-debugscript`
   emits a per-instruction text log (`tracelog "%04X %02X ...",pc,a,...`); a
   small C/Go converter (linking the FFI, like the other adapters) parses it and
   writes native morepork. The intermediate is MAME's own trace format, not morepork
   JSONL.

Either way the output is native morepork. Fields match the other adapters:
`pc a x y s p line clock` + the RESULT convention RAM bytes (`$80–$83`).

## How it works (implemented — full-speed)

`morepork-mame` (Go + cgo/FFI) launches `mame a2600 -debug -debugger gdbstub`
headless and drives it over the **GDB remote protocol**, but does **not**
single-step over the wire (that was ~19s/ROM). Instead, after the handshake
(`qSupported` + fetch `target.xml` — MAME's gdbstub only answers `monitor`/`g`
afterwards), it uses the GDB **`monitor`** (`qRcmd`) escape to install MAME's own
debugger commands and then runs the machine at **full emulation speed**:

1. `monitor trace <log>,maincpu,noloop,{tracelog "R%04X %02X %02X %02X %02X %02X\n",pc,a,x,y,sp,p}`
   — a full-speed per-instruction register log written by MAME itself.
2. `monitor wpset 0x80,1,w,{(wpdata==0xa5)||(wpdata==0x5a)}` — a watchpoint on the
   RESULT byte, to stop at the verdict.
3. `c` (continue) — runs full-speed to the verdict (or `-seconds_to_run` cap).
4. `m80,4` at the stop reads the RESULT bytes; `monitor trace off` flushes the log.
5. The `R…` lines are parsed into a native `.morepork` via the FFI; the RESULT
   bytes land on the final (verdict) entry.

**~1s/ROM including MAME launch** (~260ms of actual emulation), vs 19s for the
old per-instruction stepping — ~70× faster on the emulation, and MAME is now a
routine third oracle rather than a blue-moon check.

Two things that were essential to get right (both cost real time — noted here so
they don't have to be rediscovered):
- The stack register symbol in MAME's m6502 debugger expressions is **`sp`**, not
  `s`. One invalid symbol makes the whole `tracelog` action error out and fall
  back to plain disassembly (no `R` lines) — silently.
- The **`noloop`** trace flag is required. By default `trace` collapses loops
  (logs a repeated loop body once), so CLEAN_START's clear-loop would drop
  thousands of instructions. `noloop` logs every instruction.

Fields: `pc a x y s p result code observed expected`. Verified: **100% agreement
with the Stella and Gopher2600 adapters on the instruction stream** for pure-
compute ROMs (t01), synced to the harness anchor, with matching PASS verdicts.

### Notes / limitations
- **No per-instruction RESULT bytes.** A memory read (`b@0x80`) inside the
  `tracelog` format breaks it, so `$80–$83` are captured only once, at the
  verdict (via `m80,4`), and placed on the final entry. MAME's role is
  independent confirmation of the **instruction stream + final verdict**;
  `scripts/compare.sh` excludes `result/code/observed/expected` from the MAME
  per-instruction diff and checks the verdict separately.
- **Timer-readback micro-diffs are genuine findings, not capture bugs.** On the
  timer ROMs a handful of `a`-register values differ from Stella at INTIM/TIMINT
  reads (e.g. mame=b3 vs stella=b5), at different points than Gopher's F1 — an
  independent-lineage timer-edge disagreement worth adjudicating (logged as F4 in
  `receipts/notes/cross-oracle-findings.md`). t01 (no timer) is 100%.
- **Frame snapshots ride a second MAME pass** — gdbstub exposes registers +
  memory only, so the final frame comes from a separate gdbstub-free launch
  whose autoboot Lua dumps the screen's pixels (`captureFrame`), reverse-mapped
  to canonical palette indices. Best-effort: a capture failure only warns.
- **No `line`/`clock`** — the TIA beam isn't exposed over gdbstub/tracelog.
- **Console switches are best-effort** (autoboot Lua sets `:SWB`), so t06 isn't
  dependable on MAME; input tests are rarely in ROM suites, so not chased.
- **`read-tap` was a dead end** — reading `cpu.state[...]` inside a memory-tap
  callback core-dumps MAME; gdbstub is the working path.
- **No cartridge-type forcing (by design).** Unlike the Stella (`-type`) and
  Gopher2600 (`-mapping`) adapters, this adapter has no force flag. MAME's `a2600`
  cartridge slot chooses a mapper for a loose ROM purely via
  `identify_cart_type()` (size whitelist + signature scans in
  `src/devices/bus/vcs/vcs_slot.cpp`); the only way to override it is a **softlist**
  entry (`hash/a2600.xml`, `<feature name="slot">…`), which requires the ROM to be
  a catalogued softlist item, not a loose file. Since the suite feeds loose test
  ROMs, forcing is deliberately not implemented — MAME always autodetects, and
  `scripts/cartcheck.py` ports `identify_cart_type()` to check what it would pick.

## Notes

- MAME's `a2600` cartridge slot autodetects the bankswitch type from the `.bin`.
- TV standard via the `a2600`/`a2600p` machine or a slot option (NTSC vs PAL).
- The morepork system is `vcs`; emit a JSONL header with `"system":"vcs"` and the
  same field set, then diff against Stella/Gopher2600 via `scripts/compare.sh`.

## SG-1000 / TI VDP suite

`-system sg1000` drives MAME's `sg1000` machine through the same
trace-command + RESULT-watchpoint mechanism. Everything below was verified
against MAME 0.288:

- **CPU device tag is `z80`, not `maincpu`** — `trace <log>,maincpu,...`
  fails with "Unable to find device" on this driver.
- **Tracelog symbols**: `pc,sp,a,f,b,c,d,e,h,l,ix,iy` are all valid z80
  debugger symbols (including `f`). Fields written:
  `pc sp a f b c d e h l ix iy result code observed expected`.
- **RESULT convention** (from `missingno-ti-vdp-tests/include/result.inc`):
  the verdict block sits at the base of SG-1000 RAM — `$C000` RESULT
  (`$A5` PASS / `$5A` FAIL), then CODE/OBSERVED/EXPECTED. Same watchpoint
  values as the VCS suite, different address.
- **Frame snapshot**: the Lua second pass dumps MAME's 280×216 screen; the
  TMS9918A active area is the centred 256×192 at offset (12,12)
  (empirically located with a border-probe ROM). Pixels reverse-map to TMS
  colour indices 1-15 — MAME's rendered RGBs match the canonical
  datasheet palette exactly — and the `indexed8` frame stamps that
  16-entry palette with the 8:7 NTSC pixel aspect. Index 0 (transparent)
  renders as backdrop and never appears in a capture.
- **NTSC only**: the `sg1000` machine carries a TMS9918A. TMS9929A (PAL)
  capture would need a different driver (`sc3000` etc.); the suite treats
  PAL behaviour as provisional anyway.

The suite's ColecoVision (`.col`, RESULT at `$7000`) and MSX1 (`.mx1`,
RESULT at `$E000`, VDP ports `$98/$99`) builds map onto future `systems`
table rows — same mechanism, different machine + RESULT address.

Example:

```
morepork-mame -system sg1000 -rom sanity.sg -out sanity_mame.morepork
```
