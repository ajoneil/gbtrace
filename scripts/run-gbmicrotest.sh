#!/bin/bash
set -euo pipefail

# Run gbmicrotest suite across all adapters, producing minimal parquet traces.
# Each test finishes within the first frame; we capture 2 frames then trim
# to the exact instruction where the test writes its pass/fail result.

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROM_DIR="$PROJECT_DIR/docs/tests/gbmicrotest"
PROFILE="$ROM_DIR/gbmicrotest.toml"
CLI="$PROJECT_DIR/target/release/gbtrace-cli"

GAMBATTE="$PROJECT_DIR/adapters/gambatte/build/gbtrace-gambatte"
SAMEBOY="$PROJECT_DIR/adapters/sameboy/gbtrace-sameboy"
MGBA="$PROJECT_DIR/adapters/mgba/gbtrace-mgba"

export LD_LIBRARY_PATH="$PROJECT_DIR/adapters/sameboy/SameBoy/build/lib:${LD_LIBRARY_PATH:-}"

FRAMES=2

pass=0; fail=0; error=0; total=0

run_one() {
    local emu_name="$1" emu_bin="$2" rom="$3" name="$4"
    local raw="/tmp/gbtrace_micro_${name}_${emu_name}.gbtrace"
    local out="$ROM_DIR/${name}_${emu_name}.gbtrace.parquet"

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
    # Try pass first; if it wrote all entries (no match), try fail.
    local total_entries
    total_entries=$("$CLI" info "$stripped" 2>/dev/null | grep Entries | awk '{print $2}')
    "$CLI" trim "$stripped" --output "$raw.trimmed" --until "test_pass=01" 2>/dev/null
    local trimmed_entries
    trimmed_entries=$("$CLI" info "$raw.trimmed" 2>/dev/null | grep Entries | awk '{print $2}')
    if [ "$trimmed_entries" = "$total_entries" ]; then
        # No pass found, try fail
        "$CLI" trim "$stripped" --output "$raw.trimmed" --until "test_pass=FF" 2>/dev/null
    fi

    # Convert to parquet
    "$CLI" convert "$raw.trimmed" --output "$out" 2>/dev/null

    # Check result
    local result
    result=$("$CLI" query "$out" -w "test_pass=01" --max 1 2>&1 | grep -c "match" || true)
    local entries
    entries=$("$CLI" info "$out" 2>&1 | grep Entries | awk '{print $2}')

    if [ "$result" -gt 0 ]; then
        printf "  %-40s %-10s PASS  %6s entries\n" "$name" "$emu_name" "$entries"
        pass=$((pass + 1))
    else
        printf "  %-40s %-10s FAIL  %6s entries\n" "$name" "$emu_name" "$entries"
        fail=$((fail + 1))
    fi

    rm -f "$raw" "$raw.trimmed" "$stripped"
    # Clean up .sav files that emulators create next to the ROM
    rm -f "${rom%.gb}.sav"
    total=$((total + 1))
}

# Parse args
EMUS="gambatte sameboy mgba"
FILTER=""
if [ "${1:-}" = "--emu" ] && [ -n "${2:-}" ]; then
    EMUS="$2"; shift 2
fi
if [ "${1:-}" = "--filter" ] && [ -n "${2:-}" ]; then
    FILTER="$2"; shift 2
fi

for emu_name in $EMUS; do
    case "$emu_name" in
        gambatte) emu_bin="$GAMBATTE" ;;
        sameboy)  emu_bin="$SAMEBOY" ;;
        mgba)     emu_bin="$MGBA" ;;
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
