# SameBoy Adapter

Produces `.gbtrace` files using [SameBoy](https://github.com/LIJI32/SameBoy) as a library, with **zero source modifications** to SameBoy.

## How it works

Uses SameBoy's public `GB_set_execution_callback` API, which fires before each CPU instruction with the PC and opcode. The adapter calls `GB_get_registers` and `GB_safe_read_memory` to capture the full CPU/IO state. The adapter:

1. Loads a ROM via `libsameboy`
2. Registers an execution callback that writes JSONL to the output
3. Runs the emulator for N frames (with rendering disabled for speed)
4. Produces a `.gbtrace` file matching the spec

## Prerequisites

Build SameBoy as a static library:

```bash
git clone https://github.com/LIJI32/SameBoy.git
cd SameBoy
make lib CONF=release
```

## Build

```bash
make
```

## Usage

```bash
./gbtrace-sameboy --rom cpu_instrs.gb --profile ../../profiles/cpu_basic.toml --output trace.gbtrace --frames 3000
```

Options:
- `--rom <path>` — ROM file (required)
- `--profile <path>` — Capture profile TOML file (required)
- `--output <path>` — output file (default: stdout)
- `--frames <n>` — stop after N frames (default: 3000)
- `--model dmg|cgb` — hardware model (default: dmg)

## Cycle counting

SameBoy internally counts in 8MHz ticks. The adapter converts using:
- Normal speed: 8MHz ticks / 2 = T-cycles (4.194MHz)
- CGB double speed: needs verification (not yet supported)

## Differences from gambatte adapter

- **IME field**: SameBoy exposes the IME register, so the `ime` field works correctly (gambatte currently hardcodes `false`)
- **Rendering**: Uses `GB_set_rendering_disabled` and `GB_set_turbo_mode` for faster trace generation
