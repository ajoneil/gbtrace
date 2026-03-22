#!/bin/bash
set -euo pipefail

# Run gbmicrotest suite across all adapters, producing minimal parquet traces.
# Filenames include _pass or _fail suffix based on test result.

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROM_DIR="$PROJECT_DIR/docs/tests/gbmicrotest"
PROFILE="$ROM_DIR/gbmicrotest.toml"
CLI="$PROJECT_DIR/target/release/gbtrace-cli"

GAMBATTE="$PROJECT_DIR/adapters/gambatte/build/gbtrace-gambatte"
SAMEBOY="$PROJECT_DIR/adapters/sameboy/gbtrace-sameboy"
MGBA="$PROJECT_DIR/adapters/mgba/gbtrace-mgba"
LOGICBOY="$PROJECT_DIR/adapters/logicboy/gbtrace-logicboy"

export LD_LIBRARY_PATH="$PROJECT_DIR/adapters/sameboy/SameBoy/build/lib:${LD_LIBRARY_PATH:-}"

FRAMES=2

pass=0; fail=0; error=0; total=0

run_one() {
    local emu_name="$1" emu_bin="$2" rom="$3" name="$4"
    local raw="/tmp/gbtrace_micro_${name}_${emu_name}.gbtrace"

    # Capture
    if ! "$emu_bin" --rom "$rom" --profile "$PROFILE" --output "$raw" \
        --frames "$FRAMES" \
        --stop-when FF82=01 --stop-when FF82=FF 2>/dev/null; then
        printf "  %-40s %-10s ERROR (capture)\n" "$name" "$emu_name"
        error=$((error + 1))
        return
    fi

    # Strip boot ROM entries if present, then trim to test result
    local stripped="$raw.stripped"
    if "$CLI" strip-boot "$raw" --output "$stripped" 2>/dev/null; then
        : # stripped successfully
    else
        cp "$raw" "$stripped"
    fi

    # Trim to the instruction where test_pass is set (pass=01 or fail=FF).
    local total_entries
    total_entries=$("$CLI" info "$stripped" 2>/dev/null | grep Entries | awk '{print $2}')
    "$CLI" trim "$stripped" --output "$raw.trimmed" --until "test_pass=01" 2>/dev/null
    local trimmed_entries
    trimmed_entries=$("$CLI" info "$raw.trimmed" 2>/dev/null | grep Entries | awk '{print $2}')
    if [ "$trimmed_entries" = "$total_entries" ]; then
        "$CLI" trim "$stripped" --output "$raw.trimmed" --until "test_pass=FF" 2>/dev/null
    fi

    # Determine pass/fail from trimmed trace
    local result_pass result_fail status suffix
    result_pass=$("$CLI" query "$raw.trimmed" -w "test_pass=01" --max 1 2>&1 | grep -cP '^\d+ match' || true)
    result_fail=$("$CLI" query "$raw.trimmed" -w "test_pass=FF" --max 1 2>&1 | grep -cP '^\d+ match' || true)

    if [ "$result_pass" -gt 0 ]; then
        status="PASS"; suffix="_pass"
        pass=$((pass + 1))
    elif [ "$result_fail" -gt 0 ]; then
        status="FAIL"; suffix="_fail"
        fail=$((fail + 1))
    else
        status="????"; suffix="_fail"
        error=$((error + 1))
    fi

    # Convert to parquet with pass/fail suffix
    local out="$ROM_DIR/${name}_${emu_name}${suffix}.gbtrace.parquet"
    "$CLI" convert "$raw.trimmed" --output "$out" 2>/dev/null

    local entries
    entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
    printf "  %-40s %-10s %s  %6s entries\n" "$name" "$emu_name" "$status" "$entries"

    rm -f "$raw" "$raw.trimmed" "$stripped"
    rm -f "${rom%.gb}.sav"
    total=$((total + 1))
}

# Parse args
EMUS="gambatte sameboy mgba logicboy"
FILTER=""
while [ "${1:-}" != "" ]; do
    case "$1" in
        --emu) EMUS="$2"; shift 2 ;;
        --filter) FILTER="$2"; shift 2 ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

for emu_name in $EMUS; do
    case "$emu_name" in
        gambatte)  emu_bin="$GAMBATTE" ;;
        sameboy)   emu_bin="$SAMEBOY" ;;
        mgba)      emu_bin="$MGBA" ;;
        logicboy)  emu_bin="$LOGICBOY" ;;
        *) echo "Unknown emulator: $emu_name"; exit 1 ;;
    esac

    echo "=== $emu_name ==="
    for rom in "$ROM_DIR"/*.gb; do
        name="$(basename "$rom" .gb)"
        if [ -n "$FILTER" ] && [[ "$name" != *"$FILTER"* ]]; then
            continue
        fi
        run_one "$emu_name" "$emu_bin" "$rom" "$name"
    done
    echo
done

echo "=== Summary ==="
echo "  Pass: $pass  Fail: $fail  Error: $error  Total: $total"

# Generate manifest with pass/fail info per emulator
echo "Generating manifest..."
python3 -c "
import json, glob, os, re

rom_dir = '$ROM_DIR'
tests = sorted(set(os.path.splitext(os.path.basename(f))[0] for f in glob.glob(os.path.join(rom_dir, '*.gb'))))
emus = ['logicboy', 'gambatte', 'sameboy', 'mgba']

manifest = []
for test in tests:
    entry = {'name': test, 'rom': test + '.gb', 'emulators': {}}
    for emu in emus:
        for status in ['pass', 'fail']:
            fname = f'{test}_{emu}_{status}.gbtrace.parquet'
            if os.path.exists(os.path.join(rom_dir, fname)):
                entry['emulators'][emu] = status
                break
    manifest.append(entry)

with open(os.path.join(rom_dir, 'manifest.json'), 'w') as f:
    json.dump(manifest, f)
print(f'  {len(manifest)} tests, {sum(1 for t in manifest for e in t[\"emulators\"])} traces')
"
