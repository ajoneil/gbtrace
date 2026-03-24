# gbtrace - Build adapters, generate traces, assemble site
#
# Usage:
#   make adapters            - Build all adapter binaries
#   make cli                 - Build gbtrace-cli
#   make wasm                - Build WASM module
#   make traces              - Generate all traces (use -jN for parallelism)
#   make traces-gbmicrotest  - Generate gbmicrotest traces only
#   make traces-blargg       - Generate blargg traces only
#   make site                - Assemble deployable site in build/
#   make serve               - Serve locally for development
#   make clean               - Remove build artifacts
#
# Parallel trace generation:
#   make traces -j$(nproc)
#
# Override emulators:
#   make traces EMUS=gambatte,mgba

SHELL := /bin/bash
.SHELLFLAGS := -euo pipefail -c

PROJECT_DIR := $(shell pwd)
CLI := $(PROJECT_DIR)/target/release/gbtrace-cli
BUILD_DIR := $(PROJECT_DIR)/build
PAGES_URL ?= https://ajoneil.github.io/gbtrace

# Adapters
ADAPTERS := gambatte sameboy mgba gateboy
ADAPTER_BINS := $(foreach a,$(ADAPTERS),adapters/$(a)/gbtrace-$(a))

# Emulators to run (comma-separated, override with EMUS=gambatte,mgba)
EMUS ?= gambatte,sameboy,mgba,gateboy

# Trace output dirs
GBMICROTEST_TRACE_DIR := $(BUILD_DIR)/traces/gbmicrotest
BLARGG_TRACE_DIR := $(BUILD_DIR)/traces/blargg
MOONEYE_TRACE_DIR := $(BUILD_DIR)/traces/mooneye

export LD_LIBRARY_PATH := $(PROJECT_DIR)/adapters/sameboy/SameBoy/build/lib:$(LD_LIBRARY_PATH)
export CLI

# ── Generated rules ──────────────────────────────────────────────────
# gen-rules.py produces per-ROM×emulator stamp targets and the
# GBMICROTEST_STAMPS / BLARGG_STAMPS variable definitions.

RULES_MK := $(BUILD_DIR)/rules.mk

$(RULES_MK): scripts/gen-rules.py
	@mkdir -p $(BUILD_DIR)
	@python3 scripts/gen-rules.py $(EMUS) > $@

-include $(RULES_MK)

# ── Top-level targets ────────────────────────────────────────────────

# Screenshot test reference files: .png (checked in) → .pix (generated)
# Uses a shell loop to handle filenames with spaces.
.PHONY: pix-refs
pix-refs: scripts/png-to-pix.py
	@for png in test-suites/blargg/*.png test-suites/blargg/**/*.png test-suites/dmg-acid2/*.png; do \
		[ -f "$$png" ] || continue; \
		pix="$${png%.png}.pix"; \
		if [ ! -f "$$pix" ] || [ "$$png" -nt "$$pix" ]; then \
			python3 scripts/png-to-pix.py "$$png" "$$pix"; \
		fi; \
	done

DMG_ACID2_REF := test-suites/dmg-acid2/reference.pix

.PHONY: all adapters cli wasm traces traces-gbmicrotest traces-blargg \
        traces-mooneye traces-dmg-acid2 manifests site serve clean

all: site

adapters: $(ADAPTER_BINS)

cli: $(CLI)

traces: traces-gbmicrotest traces-blargg traces-mooneye traces-dmg-acid2

traces-gbmicrotest: $(RULES_MK) $(GBMICROTEST_STAMPS)
	@echo "Generating gbmicrotest manifest..."
	@python3 scripts/manifest.py "$(GBMICROTEST_TRACE_DIR)" "test-suites/gbmicrotest"
	@echo "=== gbmicrotest complete ==="

traces-blargg: $(RULES_MK) pix-refs $(BLARGG_STAMPS)
	@echo "Generating blargg manifest..."
	@python3 scripts/manifest.py "$(BLARGG_TRACE_DIR)" "test-suites/blargg"
	@echo "=== blargg complete ==="

traces-mooneye: $(RULES_MK) $(MOONEYE_STAMPS)
	@echo "Generating mooneye manifest..."
	@python3 scripts/manifest.py "$(MOONEYE_TRACE_DIR)" "test-suites/mooneye"
	@echo "=== mooneye complete ==="

DMG_ACID2_TRACE_DIR := $(BUILD_DIR)/traces/dmg-acid2
DMG_ACID2_ROM := test-suites/dmg-acid2/dmg-acid2.gb
DMG_ACID2_PROFILE := test-suites/dmg-acid2/profile.toml

traces-dmg-acid2: pix-refs | $(CLI)
	@echo "=== dmg-acid2 ==="
	@mkdir -p $(DMG_ACID2_TRACE_DIR)
	@for emu in $(subst $(comma), ,$(EMUS)); do \
		if [ -x "adapters/$$emu/gbtrace-$$emu" ]; then \
			bash scripts/trace-screenshot.sh \
				"adapters/$$emu/gbtrace-$$emu" \
				"$(DMG_ACID2_ROM)" \
				"$(DMG_ACID2_PROFILE)" \
				"$(DMG_ACID2_REF)" \
				"$(DMG_ACID2_TRACE_DIR)" \
				30 || true; \
		fi; \
	done
	@python3 scripts/manifest.py "$(DMG_ACID2_TRACE_DIR)" "test-suites/dmg-acid2"
	@echo "=== dmg-acid2 complete ==="

comma := ,

site: wasm traces
	@echo "Assembling site in $(BUILD_DIR)/site..."
	@rm -rf $(BUILD_DIR)/site
	@mkdir -p $(BUILD_DIR)/site/pkg $(BUILD_DIR)/site/tests
	@cp web/index.html $(BUILD_DIR)/site/
	@cp -r web/src $(BUILD_DIR)/site/
	@cp web/pkg/gbtrace_wasm.js web/pkg/gbtrace_wasm_bg.wasm $(BUILD_DIR)/site/pkg/
	@cp -r $(GBMICROTEST_TRACE_DIR) $(BUILD_DIR)/site/tests/gbmicrotest
	@cp -r $(BLARGG_TRACE_DIR) $(BUILD_DIR)/site/tests/blargg
	@if [ -d "$(MOONEYE_TRACE_DIR)" ]; then cp -r $(MOONEYE_TRACE_DIR) $(BUILD_DIR)/site/tests/mooneye; fi
	@if [ -d "$(DMG_ACID2_TRACE_DIR)" ]; then cp -r $(DMG_ACID2_TRACE_DIR) $(BUILD_DIR)/site/tests/dmg-acid2; fi
	@# Copy ROMs so the viewer can load them for disassembly
	@find test-suites/gbmicrotest -name '*.gb' -exec cp {} $(BUILD_DIR)/site/tests/gbmicrotest/ \;
	@cd test-suites/blargg && find . -name '*.gb' -exec sh -c 'mkdir -p "$(BUILD_DIR)/site/tests/blargg/$$(dirname "{}")" && cp "{}" "$(BUILD_DIR)/site/tests/blargg/{}"' \;
	@if [ -d "$(BUILD_DIR)/site/tests/dmg-acid2" ]; then cp test-suites/dmg-acid2/dmg-acid2.gb $(BUILD_DIR)/site/tests/dmg-acid2/; fi
	@# Copy profile TOMLs so the viewer can offer them for download
	@cp test-suites/gbmicrotest/profile.toml $(BUILD_DIR)/site/tests/gbmicrotest/
	@cp test-suites/blargg/profile.toml $(BUILD_DIR)/site/tests/blargg/
	@if [ -d "$(BUILD_DIR)/site/tests/dmg-acid2" ]; then cp test-suites/dmg-acid2/profile.toml $(BUILD_DIR)/site/tests/dmg-acid2/; fi
	@echo "Site ready: $(BUILD_DIR)/site/"

serve: wasm
	@echo "Assembling dev site..."
	@mkdir -p $(BUILD_DIR)/site/pkg $(BUILD_DIR)/site/tests
	@cp web/index.html $(BUILD_DIR)/site/
	@cp -r web/src $(BUILD_DIR)/site/
	@cp web/pkg/gbtrace_wasm.js web/pkg/gbtrace_wasm_bg.wasm $(BUILD_DIR)/site/pkg/
	@if [ -d "$(GBMICROTEST_TRACE_DIR)" ]; then cp -r $(GBMICROTEST_TRACE_DIR) $(BUILD_DIR)/site/tests/gbmicrotest; fi
	@if [ -d "$(BLARGG_TRACE_DIR)" ]; then cp -r $(BLARGG_TRACE_DIR) $(BUILD_DIR)/site/tests/blargg; fi
	@if [ -d "$(MOONEYE_TRACE_DIR)" ]; then cp -r $(MOONEYE_TRACE_DIR) $(BUILD_DIR)/site/tests/mooneye; fi
	@if [ -d "$(DMG_ACID2_TRACE_DIR)" ]; then cp -r $(DMG_ACID2_TRACE_DIR) $(BUILD_DIR)/site/tests/dmg-acid2; fi
	@if [ -d "$(BUILD_DIR)/site/tests/gbmicrotest" ]; then cp test-suites/gbmicrotest/profile.toml $(BUILD_DIR)/site/tests/gbmicrotest/; fi
	@if [ -d "$(BUILD_DIR)/site/tests/blargg" ]; then cp test-suites/blargg/profile.toml $(BUILD_DIR)/site/tests/blargg/; fi
	@if [ -d "$(BUILD_DIR)/site/tests/dmg-acid2" ]; then cp test-suites/dmg-acid2/profile.toml $(BUILD_DIR)/site/tests/dmg-acid2/; fi
	@echo "Serving on http://localhost:3080"
	@echo "  Local files from web/, traces from local build or $(PAGES_URL)"
	@cd $(BUILD_DIR)/site && python3 $(PROJECT_DIR)/scripts/devserver.py $(PAGES_URL)

clean:
	rm -rf $(BUILD_DIR)

# ── Adapter builds ───────────────────────────────────────────────────

adapters/gambatte/gbtrace-gambatte:
	@echo "Building gambatte adapter..."
	@$(MAKE) -C adapters/gambatte -j$$(nproc)

adapters/sameboy/gbtrace-sameboy:
	@echo "Building sameboy adapter..."
	@$(MAKE) -C adapters/sameboy -j$$(nproc)

adapters/mgba/gbtrace-mgba:
	@echo "Building mgba adapter..."
	@$(MAKE) -C adapters/mgba -j$$(nproc)

adapters/gateboy/gbtrace-gateboy:
	@echo "Building gateboy adapter..."
	@$(MAKE) -C adapters/gateboy -j$$(nproc)

FFI_LIB := $(PROJECT_DIR)/target/release/libgbtrace_ffi.a
FFI_HEADER := $(PROJECT_DIR)/crates/gbtrace-ffi/gbtrace.h

$(CLI): $(wildcard crates/gbtrace-cli/src/*.rs crates/gbtrace/src/*.rs)
	@echo "Building gbtrace-cli..."
	@cargo build --release -p gbtrace-cli 2>&1 | tail -1

$(FFI_LIB): $(wildcard crates/gbtrace-ffi/src/*.rs crates/gbtrace/src/*.rs)
	@echo "Building gbtrace-ffi..."
	@cargo build --release -p gbtrace-ffi 2>&1 | tail -1

ffi: $(FFI_LIB)

wasm: web/pkg/gbtrace_wasm_bg.wasm

web/pkg/gbtrace_wasm_bg.wasm: $(wildcard crates/gbtrace-wasm/src/*.rs crates/gbtrace/src/*.rs)
	@echo "Building WASM module..."
	@wasm-pack build crates/gbtrace-wasm --target web --out-dir $(PROJECT_DIR)/web/pkg --no-typescript
	@rm -f web/pkg/.gitignore web/pkg/package.json
