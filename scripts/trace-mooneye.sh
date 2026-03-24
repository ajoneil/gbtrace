#!/usr/bin/env bash
# Generate a single mooneye trace: adapter + ROM → parquet
#
# Pass/fail detection:
#   Mooneye tests execute LD B, B (opcode 0x40) when complete.
#   Pass: B=3 C=5 D=8 E=13 H=21 L=34 (Fibonacci sequence)
#   Fail: all registers = 0x42
#
# Usage: trace-mooneye.sh <adapter-binary> <rom> <profile> <output-dir> <rom-dir>
set -euo pipefail

BIN="$1"
ROM="$2"
PROFILE="$3"
OUT_DIR="$4"
ROM_DIR="${5:-$(dirname "$ROM")}"
CLI="${CLI:-target/release/gbtrace-cli}"

NAME="$(basename "$ROM" .gb)"
ADAPTER="$(basename "$BIN" | sed 's/gbtrace-//')"

# Compute relative subdir from ROM_DIR to ROM
ROM_REL="$(realpath --relative-to="$ROM_DIR" "$(dirname "$ROM")")"
if [ "$ROM_REL" = "." ]; then
    TRACE_SUBDIR="$OUT_DIR"
else
    TRACE_SUBDIR="$OUT_DIR/$ROM_REL"
fi

# Check for .pix reference next to the ROM
PIX_REF="$(dirname "$ROM")/${NAME}.pix"

MAX_FRAMES=200
TMP="/tmp/gbtrace_mooneye_${NAME}_${ADAPTER}_$$"
stderr_file="${TMP}.stderr"
tmp_parquet="${TMP}.parquet"
tmp_pipe="${TMP}.pipe"

cleanup() { rm -f "$tmp_pipe" "$stderr_file" "${TMP}_trimmed.parquet" "${ROM%.gb}.sav"; }
trap cleanup EXIT

# --- Capture ---
EXTRA_ARGS=()
if [[ -f "$PIX_REF" ]]; then
    EXTRA_ARGS+=(--reference "$PIX_REF")
fi

ADAPTER_ARGS=(
    --rom "$ROM"
    --profile "$PROFILE"
    --stop-opcode 40
    --extra-frames 2
    --frames "$MAX_FRAMES"
    "${EXTRA_ARGS[@]}"
)

# Adapters with FFI support write parquet directly (detected by name).
# Others use a named pipe to stream JSONL to the converter.
if [[ "$ADAPTER" == "gateboy" ]]; then
    (
        set +eo pipefail
        "$BIN" "${ADAPTER_ARGS[@]}" --output "$tmp_parquet" 2>"$stderr_file" </dev/null
    ) || true
else
    mkfifo "$tmp_pipe"
    (
        set +eo pipefail
        "$BIN" "${ADAPTER_ARGS[@]}" --output "$tmp_pipe" 2>"$stderr_file" </dev/null
    ) &
    adapter_pid=$!
    "$CLI" convert "$tmp_pipe" -o "$tmp_parquet" >/dev/null 2>&1 || true
    wait "$adapter_pid" || true
fi

if [[ ! -s "$tmp_parquet" ]]; then
    err_msg=$(head -1 "$stderr_file" 2>/dev/null || echo "unknown")
    printf "%-40s %-10s ERROR (%s)\n" "$NAME" "$ADAPTER" "$err_msg"
    exit 1
fi

# --- Determine pass/fail ---
# Check registers from the last entry: Fibonacci sequence = pass
# b=03 c=05 d=08 e=0d h=15 l=22
status=$("$CLI" query "$tmp_parquet" --last 1 2>&1 | \
    grep -qP 'b=03\b.*c=05\b.*d=08\b.*e=0d\b.*h=15\b.*l=22\b' \
    && echo "pass" || echo "fail")

# --- Trim to reference if available ---
if [[ "$status" == "pass" ]] && [[ -f "$PIX_REF" ]]; then
    if "$CLI" trim "$tmp_parquet" --reference "$PIX_REF" \
        --output "${TMP}_trimmed.parquet" >/dev/null 2>&1; then
        mv "${TMP}_trimmed.parquet" "$tmp_parquet"
    fi
fi

# --- Output ---
mkdir -p "$TRACE_SUBDIR"
out="${TRACE_SUBDIR}/${NAME}_${ADAPTER}_${status}.gbtrace.parquet"
mv "$tmp_parquet" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-40s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"
