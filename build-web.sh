#!/bin/bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")" && pwd)"

echo "Building gbtrace WASM module..."
wasm-pack build "$PROJECT_ROOT/crates/gbtrace-wasm" \
  --target web \
  --out-dir "$PROJECT_ROOT/docs/pkg" \
  --no-typescript

# Clean wasm-pack noise — only keep the .wasm and .js glue
rm -f "$PROJECT_ROOT/docs/pkg/.gitignore" "$PROJECT_ROOT/docs/pkg/package.json"

echo "Done. WASM output in docs/pkg/"
