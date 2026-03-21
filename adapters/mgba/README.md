# mGBA Adapter

Produces `.gbtrace` files using [mGBA](https://github.com/mgba-emu/mgba) as a library, with **zero source modifications** to mGBA.

## How it works

Uses mGBA's `mDebuggerModule` callback API, which fires before each CPU instruction. The adapter:

1. Creates a GB core via `mCoreFind` and loads the ROM
2. Attaches a custom debugger module with a per-instruction callback
3. Reads CPU registers directly from the `SM83Core` struct and IO state via `rawRead8`
4. Runs the emulator for N frames via `mDebuggerRunFrame`
5. Produces a `.gbtrace` file matching the spec

## Prerequisites

Build mGBA as a static library with debugger support:

```bash
git clone https://github.com/mgba-emu/mgba.git
cd mgba && mkdir build && cd build
cmake .. -DLIBMGBA_ONLY=ON -DBUILD_STATIC=ON -DCMAKE_BUILD_TYPE=Release -DENABLE_DEBUGGERS=ON
make
```

## Build

```bash
make
```

**Note**: The Makefile extracts compile defines from mGBA's build to ensure struct layout compatibility. The mGBA build directory must be present at `mgba/build/`.

## Usage

```bash
./gbtrace-mgba --rom cpu_instrs.gb --profile ../../profiles/cpu_basic.toml --output trace.gbtrace --frames 3000
```

Options:
- `--rom <path>` — ROM file (required)
- `--profile <path>` — Capture profile TOML file (required)
- `--output <path>` — output file (default: stdout)
- `--frames <n>` — stop after N frames (default: 3000)
- `--model dmg|cgb` — hardware model (default: dmg)
- `--boot-rom <path>` — boot ROM file (default: skip boot)

## Cycle counting

mGBA's timing system uses 8MHz ticks (via `globalCycles + cpu->cycles`). The adapter converts to T-cycles by dividing by 2, matching the SameBoy adapter's convention.
