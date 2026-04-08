# BGB Adapter (Experimental)

BGB is a closed-source, Windows-only Game Boy emulator with high accuracy.
This adapter runs it under Wine, using BGB's per-instruction debug-message
breakpoint feature to capture CPU and IO register state.

**Status: Not integrated into CI or the web UI.** The adapter builds and
works locally for individual tests, but is not reliable enough for
automated trace generation at scale.

## How it works

- BGB is downloaded automatically on first `make` (not redistributable).
- Runs under `xvfb-run wine` in headless mode.
- A named pipe (FIFO) at `debugmsg.txt` streams BGB's debug message
  output directly into the FFI trace writer -- no intermediate files.
- Multiple comma-separated `any` breakpoints with letter prefixes
  (A, B, C...) work around BGB's 127-char debug message limit,
  giving ~30 fields per entry.
- Stop conditions (`--stop-when`, `--stop-opcode`) are implemented as
  boolean expression tokens in the debug message, checked by the
  adapter's parser.
- Screenshot matching (`--reference`) uses a separate headless BGB run
  with `-screenonexit` and a `TOTALCLKS` breakpoint for frame limiting.

## Known issues

- **Slow throughput**: ~8.8K instructions/sec through the debug message
  pipe. A single frame (~17K instructions) takes ~2s. Tests requiring
  many frames (blargg at 1200 frames, samesuite at 7200) would take
  minutes to hours.
- **Screenshot tests require two passes**: One fast pass for the
  screenshot comparison, then a slow pass for the per-instruction trace.
  This doubles the already-slow runtime for screenshot-based suites.
- **No pixel data in traces**: BGB's debug message interface cannot
  access the rendered framebuffer, so the `pix` field is never populated.
- **TOTALCLKS-based frame limiting is approximate**: The starting clock
  count varies slightly, so frame counts are not exact.
- **Wine/Xvfb dependency**: Requires `wine` and `xvfb-run` at runtime,
  adding CI complexity.

## Building

```
make          # downloads BGB and compiles the adapter
```

## Usage

```
./gbtrace-bgb --rom test.gb --profile profile.toml --output trace.gbtrace \
    [--stop-when FF82=01] [--stop-opcode 40] [--extra-frames 2] \
    [--reference ref.pix] [--frames 30]
```

## Re-enabling on CI

To add BGB back to CI, add `bgb` to:
- `.github/workflows/build.yml` adapter matrix
- `.github/workflows/traces.yml` EMUS list and dropdown
- `.github/workflows/deploy.yml` EMUS array
- `Makefile` ADAPTERS and EMUS variables
- `scripts/gen-rules.py` default emus list
- `scripts/manifest.py` EMULATORS list
- `web/src/components/test-picker.js` EMULATORS and EMU_SHORT
