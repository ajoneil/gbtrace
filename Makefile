# gbtrace - Build adapters, generate traces, assemble site
#
# Usage:
#   make adapters            - Build all adapter binaries
#   make cli                 - Build gbtrace CLI
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
CLI := $(PROJECT_DIR)/target/release/gbtrace
BUILD_DIR := $(PROJECT_DIR)/build
PAGES_URL ?= https://ajoneil.github.io/gbtrace

# Adapters
ADAPTERS := gambatte sameboy gateboy missingno docboy
ADAPTER_BINS := $(foreach a,$(ADAPTERS),adapters/$(a)/gbtrace-$(a))

# Emulators to run (comma-separated, override with EMUS=gambatte,mgba)
EMUS ?= gambatte,sameboy,gateboy,missingno,docboy

# Trace output dirs
GBMICROTEST_TRACE_DIR := $(BUILD_DIR)/traces/gbmicrotest
BLARGG_TRACE_DIR := $(BUILD_DIR)/traces/blargg
MOONEYE_TRACE_DIR := $(BUILD_DIR)/traces/mooneye
GAMBATTE_TESTS_TRACE_DIR := $(BUILD_DIR)/traces/gambatte-tests
MEALYBUG_TEAROOM_TRACE_DIR := $(BUILD_DIR)/traces/mealybug-tearoom
AGE_TRACE_DIR := $(BUILD_DIR)/traces/age
MOONEYE_WILBERTPOL_TRACE_DIR := $(BUILD_DIR)/traces/mooneye-wilbertpol
SAMESUITE_TRACE_DIR := $(BUILD_DIR)/traces/samesuite
SCRIBBLTESTS_TRACE_DIR := $(BUILD_DIR)/traces/scribbltests
BULLY_TRACE_DIR := $(BUILD_DIR)/traces/bully
LITTLE_THINGS_TRACE_DIR := $(BUILD_DIR)/traces/little-things
MBC3_TESTER_TRACE_DIR := $(BUILD_DIR)/traces/mbc3-tester
STRIKETHROUGH_TRACE_DIR := $(BUILD_DIR)/traces/strikethrough
TURTLE_TESTS_TRACE_DIR := $(BUILD_DIR)/traces/turtle-tests

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
# Uses find to handle arbitrarily nested directories and filenames with spaces.
.PHONY: pix-refs
pix-refs: scripts/png-to-pix.py
	@find test-suites -name '*.png' -print0 | while IFS= read -r -d '' png; do \
		pix="$${png%.png}.pix"; \
		if [ ! -f "$$pix" ] || [ "$$png" -nt "$$pix" ]; then \
			python3 scripts/png-to-pix.py "$$png" "$$pix"; \
		fi; \
	done

DMG_ACID2_REF := test-suites/dmg-acid2/reference.pix

.PHONY: all adapters cli wasm traces traces-gbmicrotest traces-blargg \
        traces-mooneye traces-gambatte-tests traces-mealybug-tearoom traces-dmg-acid2 \
        traces-age traces-mooneye-wilbertpol traces-samesuite traces-scribbltests \
        traces-bully traces-little-things traces-mbc3-tester traces-strikethrough \
        traces-turtle-tests manifests site serve clean

all: site

adapters: $(ADAPTER_BINS)

cli: $(CLI)

traces: traces-gbmicrotest traces-blargg traces-mooneye traces-gambatte-tests traces-mealybug-tearoom traces-dmg-acid2 \
        traces-age traces-mooneye-wilbertpol traces-samesuite traces-scribbltests \
        traces-bully traces-little-things traces-mbc3-tester traces-strikethrough \
        traces-turtle-tests

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

traces-gambatte-tests: $(RULES_MK) pix-refs $(GAMBATTE_TESTS_STAMPS)
	@echo "Generating gambatte-tests manifest..."
	@python3 scripts/manifest.py "$(GAMBATTE_TESTS_TRACE_DIR)" "test-suites/gambatte"
	@echo "=== gambatte-tests complete ==="

traces-mealybug-tearoom: $(RULES_MK) pix-refs $(MEALYBUG_TEAROOM_STAMPS)
	@echo "Generating mealybug-tearoom manifest..."
	@python3 scripts/manifest.py "$(MEALYBUG_TEAROOM_TRACE_DIR)" "test-suites/mealybug-tearoom"
	@echo "=== mealybug-tearoom complete ==="

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

traces-age: $(RULES_MK) pix-refs $(AGE_STAMPS)
	@echo "Generating age manifest..."
	@python3 scripts/manifest.py "$(AGE_TRACE_DIR)" "test-suites/age"
	@echo "=== age complete ==="

traces-mooneye-wilbertpol: $(RULES_MK) $(MOONEYE_WILBERTPOL_STAMPS)
	@echo "Generating mooneye-wilbertpol manifest..."
	@python3 scripts/manifest.py "$(MOONEYE_WILBERTPOL_TRACE_DIR)" "test-suites/mooneye-wilbertpol"
	@echo "=== mooneye-wilbertpol complete ==="

traces-samesuite: $(RULES_MK) $(SAMESUITE_STAMPS)
	@echo "Generating samesuite manifest..."
	@python3 scripts/manifest.py "$(SAMESUITE_TRACE_DIR)" "test-suites/samesuite"
	@echo "=== samesuite complete ==="

traces-scribbltests: $(RULES_MK) pix-refs $(SCRIBBLTESTS_STAMPS)
	@echo "Generating scribbltests manifest..."
	@python3 scripts/manifest.py "$(SCRIBBLTESTS_TRACE_DIR)" "test-suites/scribbltests"
	@echo "=== scribbltests complete ==="

traces-bully: $(RULES_MK) pix-refs $(BULLY_STAMPS)
	@echo "Generating bully manifest..."
	@python3 scripts/manifest.py "$(BULLY_TRACE_DIR)" "test-suites/bully"
	@echo "=== bully complete ==="

traces-little-things: $(RULES_MK) pix-refs $(LITTLE_THINGS_STAMPS)
	@echo "Generating little-things manifest..."
	@python3 scripts/manifest.py "$(LITTLE_THINGS_TRACE_DIR)" "test-suites/little-things"
	@echo "=== little-things complete ==="

traces-mbc3-tester: $(RULES_MK) pix-refs $(MBC3_TESTER_STAMPS)
	@echo "Generating mbc3-tester manifest..."
	@python3 scripts/manifest.py "$(MBC3_TESTER_TRACE_DIR)" "test-suites/mbc3-tester"
	@echo "=== mbc3-tester complete ==="

traces-strikethrough: $(RULES_MK) pix-refs $(STRIKETHROUGH_STAMPS)
	@echo "Generating strikethrough manifest..."
	@python3 scripts/manifest.py "$(STRIKETHROUGH_TRACE_DIR)" "test-suites/strikethrough"
	@echo "=== strikethrough complete ==="

traces-turtle-tests: $(RULES_MK) pix-refs $(TURTLE_TESTS_STAMPS)
	@echo "Generating turtle-tests manifest..."
	@python3 scripts/manifest.py "$(TURTLE_TESTS_TRACE_DIR)" "test-suites/turtle-tests"
	@echo "=== turtle-tests complete ==="

comma := ,

site: wasm traces
	@echo "Assembling site in $(BUILD_DIR)/site..."
	@rm -rf $(BUILD_DIR)/site
	@mkdir -p $(BUILD_DIR)/site/pkg $(BUILD_DIR)/site/tests
	@cp web/index.html $(BUILD_DIR)/site/
	@cp -r web/src $(BUILD_DIR)/site/
	@cp web/pkg/gbtrace_wasm.js web/pkg/gbtrace_wasm_bg.wasm $(BUILD_DIR)/site/pkg/
	@# Copy traces, ROMs, and profiles for each suite
	@for suite_dir in $(BUILD_DIR)/traces/*/; do \
		[ -d "$$suite_dir" ] || continue; \
		suite=$$(basename "$$suite_dir"); \
		cp -r "$$suite_dir" "$(BUILD_DIR)/site/tests/$$suite"; \
		rom_dir="test-suites/$$suite"; \
		if [ "$$suite" = "gambatte-tests" ]; then rom_dir="test-suites/gambatte"; fi; \
		if [ -d "$$rom_dir" ]; then \
			cd "$$rom_dir" && find . -name '*.gb' -exec sh -c \
				'mkdir -p "$(BUILD_DIR)/site/tests/'"$$suite"'/$$(dirname "{}")" && cp "{}" "$(BUILD_DIR)/site/tests/'"$$suite"'/{}"' \; && cd $(PROJECT_DIR); \
			if [ -f "$$rom_dir/profile.toml" ]; then \
				cp "$$rom_dir/profile.toml" "$(BUILD_DIR)/site/tests/$$suite/"; \
			fi; \
		fi; \
	done
	@echo "Site ready: $(BUILD_DIR)/site/"

serve: wasm
	@python3 $(PROJECT_DIR)/scripts/devserver.py $(PROJECT_DIR) $(PAGES_URL)

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

adapters/missingno/gbtrace-missingno:
	@echo "Building missingno adapter..."
	@cd adapters/missingno && cargo build --release && cp target/release/gbtrace-missingno .

adapters/docboy/gbtrace-docboy:
	@echo "Building docboy adapter..."
	@$(MAKE) -C adapters/docboy


FFI_LIB := $(PROJECT_DIR)/target/release/libgbtrace_ffi.a
FFI_HEADER := $(PROJECT_DIR)/crates/gbtrace-ffi/gbtrace.h

$(CLI): $(wildcard crates/gbtrace/src/*.rs crates/gbtrace/src/**/*.rs)
	@echo "Building gbtrace..."
	@cargo build --release --features cli 2>&1 | tail -1

$(FFI_LIB): $(wildcard crates/gbtrace-ffi/src/*.rs crates/gbtrace/src/*.rs)
	@echo "Building gbtrace-ffi..."
	@cargo build --release -p gbtrace-ffi 2>&1 | tail -1

ffi: $(FFI_LIB)

wasm: web/pkg/gbtrace_wasm_bg.wasm

web/pkg/gbtrace_wasm_bg.wasm: $(wildcard crates/gbtrace-wasm/src/*.rs crates/gbtrace/src/*.rs)
	@echo "Building WASM module..."
	@wasm-pack build crates/gbtrace-wasm --target web --out-dir $(PROJECT_DIR)/web/pkg --no-typescript
	@rm -f web/pkg/.gitignore web/pkg/package.json
