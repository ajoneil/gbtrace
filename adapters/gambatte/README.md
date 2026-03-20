# Gambatte-Speedrun Adapter

Produces `.gbtrace` files using [gambatte-speedrun](https://github.com/pokemon-speedrunning/gambatte-speedrun) as a library, with **zero source modifications** to gambatte.

## How it works

Uses gambatte's public `setTraceCallback` API, which fires before each CPU instruction with full register state. The adapter:

1. Loads a ROM via `libgambatte`
2. Registers a trace callback that writes JSONL to the output
3. Runs the emulator for N frames
4. Produces a `.gbtrace` file matching the spec

## Prerequisites

Build gambatte-speedrun's core library:

```bash
git clone --recursive https://github.com/pokemon-speedrunning/gambatte-speedrun.git
cd gambatte-speedrun/gambatte_core/libgambatte
scons
```

## Build

```bash
make GAMBATTE_DIR=/path/to/gambatte-speedrun/gambatte_core
```

## Usage

```bash
./gbtrace-gambatte --rom cpu_instrs.gb --output trace.gbtrace --frames 3000
```

Options:
- `--rom <path>` — ROM file (required)
- `--output <path>` — output file (default: stdout)
- `--frames <n>` — stop after N frames (default: 3000)
- `--model dmg|cgb` — hardware model (default: dmg)

## Cycle counting

Gambatte internally counts audio samples rather than T-cycles. The adapter converts using:
- Normal speed: 1 sample = 4 T-cycles
- CGB double speed: needs verification (not yet supported)
