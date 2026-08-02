# morepork - Build the CLI, FFI library, and adapters
#
# Usage:
#   make cli       - Build target/release/morepork
#   make ffi       - Build target/release/libmorepork_ffi.a
#   make adapters  - Build every adapter binary in adapters/<emu>/
#   make clean     - Remove build artifacts

SHELL := /bin/bash
.SHELLFLAGS := -euo pipefail -c

PROJECT_DIR := $(shell pwd)
CLI := $(PROJECT_DIR)/target/release/morepork
BUILD_DIR := $(PROJECT_DIR)/build

# Each adapter builds standalone via its own Makefile (nested cmake/scons
# builds against vendored emulator sources, or plain cargo). The bgb
# (Wine-based) and gateboy adapters are built manually from their dirs.
ADAPTERS := stella gopher2600 mame openmsx ares gearcoleco gearsystem \
            missingno gambatte sameboy mgba docboy

FFI_LIB := $(PROJECT_DIR)/target/release/libmorepork_ffi.a
FFI_HEADER := $(PROJECT_DIR)/crates/morepork-ffi/morepork.h

.PHONY: all cli ffi adapters $(ADAPTERS) clean

all: cli

cli: $(CLI)

$(CLI): $(wildcard crates/morepork/src/*.rs crates/morepork/src/**/*.rs)
	@echo "Building morepork..."
	@cargo build --release --features cli 2>&1 | tail -1

$(FFI_LIB): $(wildcard crates/morepork-ffi/src/*.rs crates/morepork/src/*.rs crates/morepork/src/**/*.rs)
	@echo "Building morepork-ffi..."
	@cargo build --release -p morepork-ffi 2>&1 | tail -1

ffi: $(FFI_LIB)

adapters: $(ADAPTERS)

# C/C++ adapters statically link the FFI, so it is built first and they
# must relink when it changes — otherwise they silently ship a stale
# trace writer. (The newer adapter Makefiles also rebuild it themselves
# via cargo, which is a no-op when it is already current.)
$(ADAPTERS): $(FFI_LIB)
	@echo "Building $@ adapter..."
	@$(MAKE) -C adapters/$@ -j$$(nproc)

clean:
	rm -rf $(BUILD_DIR)
