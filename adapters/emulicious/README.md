# morepork-emulicious

A morepork adapter driving [Emulicious](https://emulicious.net/) as a
**verdict-level** oracle for the TI VDP (TMS9918A) test suite, on both
SG-1000 (`.sg`) and MSX1 (`.mx1`, booted on Emulicious's bundled C-BIOS).
Emulicious's VDP is Calindro's own lineage — independent of MAME's and
openMSX's — so its PASS/FAIL + CODE/OBSERVED/EXPECTED vote is a genuine
third opinion on both machines.

## How it works

Emulicious is launched with its DAP remote-debugging server
(`java -jar Emulicious.jar -remotedebug <port> -turbo`) and driven as a
DAP client: `launch {program, stopOnEntry}` → wait for the entry stop (so
every run starts deterministically from reset) → `continue` → poll the
RESULT byte **while running** with `evaluate` (`@$c000` / `@$e000` — live
evaluation is the one fast surface the server has) → on `$A5`/`$5A`,
`pause` and capture the RESULT block plus the final CPU state and the
VDP internals Emulicious exposes as expression variables (`va` → `addr`,
`scanline` → `line`). The output is a **single-entry trace** carrying the
verdict.

~3s per ROM. A GUI window appears during capture (Java/AWT has no
offscreen mode here); use xvfb-run if that matters.

## Why verdict-level, not an instruction trace

Findings from probing the DAP server (Emulicious 2026-03-27), recorded so
they don't have to be rediscovered:

- **Every DAP request costs ~40ms** (the server synchronizes with the
  emulation/UI per request), so `stepIn` + per-register `evaluate` tops
  out at 2–12 instructions/second. Instruction-stream capture is off the
  table; this is likely the wall the earlier gbtrace-era attempt hit.
- **Address breakpoints are out of reach remotely.** Function breakpoints
  resolve debug symbols only (`Unknown variable encountered: _name`), and
  no symfile format we tried (wla-dx with/without `[labels]`, plain,
  no$gmb-style, `EQU`) resolved for a plain-ROM load. Without an address
  breakpoint there is no way to stop at a cartridge INIT vector, which is
  also why the MSX capture can't scope tracing to the cart body.
- **`configurationDone` is not implemented** — the request never gets a
  response; skip it.
- **`-remotedebug` takes the port as its argument** (`-remotedebug 12345`);
  a following flag would be swallowed as the port.
- **`evaluate` works while running** and returns live values —
  `@$c000 = $A5` — which is what makes the verdict poll possible. The
  printed value is introduced by the *last* `$` (the expression echoes
  its own `$` first).
- `supportsStepBack` is real (rewind) but steps at the same ~40ms/request.

If Emulicious ever grows an in-process scripting or trace-logging surface,
this adapter should be upgraded to a full instruction-stream oracle; the
expression language (see `Expressions.txt` beside the jar) is already rich
enough — registers, `va`, `scanline`, `@` memory reads.

```
morepork-emulicious -rom sanity.sg  -out trace.morepork   # sg1000
morepork-emulicious -rom sanity.mx1 -out trace.morepork   # msx1
```

Flags: `-rom` `-out` `-system sg1000|msx1` (default: from the ROM
extension) `-spec NTSC` `-frames` (verdict deadline) `-jar` (default
`/usr/share/emulicious-bin/Emulicious.jar`).
