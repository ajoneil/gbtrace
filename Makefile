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

# Adapters
ADAPTERS := gambatte sameboy mgba gateboy
ADAPTER_BINS := $(foreach a,$(ADAPTERS),adapters/$(a)/gbtrace-$(a))

# Emulators to run (comma-separated, override with EMUS=gambatte,mgba)
EMUS ?= gambatte,sameboy,mgba,gateboy

# Trace output dirs
GBMICROTEST_TRACE_DIR := $(BUILD_DIR)/traces/gbmicrotest
BLARGG_TRACE_DIR := $(BUILD_DIR)/traces/blargg

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

.PHONY: all adapters cli wasm traces traces-gbmicrotest traces-blargg \
        manifests site serve clean

all: site

adapters: $(ADAPTER_BINS)

cli: $(CLI)

traces: traces-gbmicrotest traces-blargg

traces-gbmicrotest: $(RULES_MK) $(GBMICROTEST_STAMPS)
	@echo "Generating gbmicrotest manifest..."
	@python3 scripts/manifest.py "$(GBMICROTEST_TRACE_DIR)" "test-suites/gbmicrotest"
	@echo "=== gbmicrotest complete ==="

traces-blargg: $(RULES_MK) $(BLARGG_STAMPS)
	@echo "Generating blargg manifest..."
	@python3 scripts/manifest.py "$(BLARGG_TRACE_DIR)" "test-suites/blargg"
	@echo "=== blargg complete ==="

site: wasm traces
	@echo "Assembling site in $(BUILD_DIR)/site..."
	@rm -rf $(BUILD_DIR)/site
	@mkdir -p $(BUILD_DIR)/site/pkg $(BUILD_DIR)/site/tests
	@cp web/index.html $(BUILD_DIR)/site/
	@cp -r web/src $(BUILD_DIR)/site/
	@cp web/pkg/gbtrace_wasm.js web/pkg/gbtrace_wasm_bg.wasm $(BUILD_DIR)/site/pkg/
	@cp -r $(GBMICROTEST_TRACE_DIR) $(BUILD_DIR)/site/tests/gbmicrotest
	@cp -r $(BLARGG_TRACE_DIR) $(BUILD_DIR)/site/tests/blargg
	@# Copy ROMs so the viewer can load them for disassembly
	@find test-suites/gbmicrotest -name '*.gb' -exec cp {} $(BUILD_DIR)/site/tests/gbmicrotest/ \;
	@cd test-suites/blargg && find . -name '*.gb' -exec sh -c 'mkdir -p "$(BUILD_DIR)/site/tests/blargg/$$(dirname "{}")" && cp "{}" "$(BUILD_DIR)/site/tests/blargg/{}"' \;
	@echo "Site ready: $(BUILD_DIR)/site/"

serve: wasm
	@echo "Assembling dev site..."
	@mkdir -p $(BUILD_DIR)/site/pkg $(BUILD_DIR)/site/tests
	@cp web/index.html $(BUILD_DIR)/site/
	@cp -r web/src $(BUILD_DIR)/site/
	@cp web/pkg/gbtrace_wasm.js web/pkg/gbtrace_wasm_bg.wasm $(BUILD_DIR)/site/pkg/
	@if [ -d "$(GBMICROTEST_TRACE_DIR)" ]; then cp -r $(GBMICROTEST_TRACE_DIR) $(BUILD_DIR)/site/tests/gbmicrotest; fi
	@if [ -d "$(BLARGG_TRACE_DIR)" ]; then cp -r $(BLARGG_TRACE_DIR) $(BUILD_DIR)/site/tests/blargg; fi
	@echo "Serving on http://localhost:3080"
	@cd $(BUILD_DIR)/site && python3 -c "\
	from http.server import HTTPServer, SimpleHTTPRequestHandler; \
	class H(SimpleHTTPRequestHandler): \
	    def end_headers(self): \
	        self.send_header('Cache-Control', 'no-cache'); \
	        super().end_headers() \
	; HTTPServer(('', 3080), H).serve_forever()"

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

$(CLI): $(wildcard crates/gbtrace-cli/src/*.rs crates/gbtrace/src/*.rs)
	@echo "Building gbtrace-cli..."
	@cargo build --release -p gbtrace-cli 2>&1 | tail -1

wasm: web/pkg/gbtrace_wasm_bg.wasm

web/pkg/gbtrace_wasm_bg.wasm: $(wildcard crates/gbtrace-wasm/src/*.rs crates/gbtrace/src/*.rs)
	@echo "Building WASM module..."
	@wasm-pack build crates/gbtrace-wasm --target web --out-dir $(PROJECT_DIR)/web/pkg --no-typescript
	@rm -f web/pkg/.gitignore web/pkg/package.json
