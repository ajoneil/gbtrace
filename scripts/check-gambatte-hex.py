#!/usr/bin/env python3
"""Check if a rendered frame matches a Gambatte hex output value.

Reads a 160x144 .pix file (or generates one from a .gbtrace via CLI)
and checks if the top-left corner shows the expected hex digits.

Usage: check-gambatte-hex.py <expected_hex> <pix_file>
Returns exit code 0 if match, 1 if mismatch.
"""
import sys

# Gambatte hex digit patterns (8x8 each, 0=white/lightest, 3=black/darkest)
# From testrunner.cpp: _ = 0xF8F8F8 (shade 0), O = 0x000000 (shade 3)
HEX_TILES = {
    '0': [
        "00000000",
        "03333333",
        "03000003",
        "03000003",
        "03000003",
        "03000003",
        "03000003",
        "03333333",
    ],
    '1': [
        "00000000",
        "00003000",
        "00003000",
        "00003000",
        "00003000",
        "00003000",
        "00003000",
        "00003000",
    ],
    '2': [
        "00000000",
        "03333333",
        "00000003",
        "00000003",
        "03333333",
        "03000000",
        "03000000",
        "03333333",
    ],
    '3': [
        "00000000",
        "03333333",
        "00000003",
        "00000003",
        "00333333",
        "00000003",
        "00000003",
        "03333333",
    ],
    '4': [
        "00000000",
        "03000003",
        "03000003",
        "03000003",
        "03333333",
        "00000003",
        "00000003",
        "00000003",
    ],
    '5': [
        "00000000",
        "03333333",
        "03000000",
        "03000000",
        "03333330",
        "00000003",
        "00000003",
        "03333330",
    ],
    '6': [
        "00000000",
        "03333333",
        "03000000",
        "03000000",
        "03333333",
        "03000003",
        "03000003",
        "03333333",
    ],
    '7': [
        "00000000",
        "03333333",
        "00000003",
        "00000030",
        "00000300",
        "00003000",
        "00030000",
        "00030000",
    ],
    '8': [
        "00000000",
        "00333330",
        "03000003",
        "03000003",
        "00333330",
        "03000003",
        "03000003",
        "00333330",
    ],
    '9': [
        "00000000",
        "00333330",
        "03000003",
        "03000003",
        "00333333",
        "00000003",
        "00000003",
        "00333330",
    ],
    'A': [
        "00000000",
        "00333330",
        "03000003",
        "03000003",
        "03333333",
        "03000003",
        "03000003",
        "03000003",
    ],
    'B': [
        "00000000",
        "03333330",
        "03000003",
        "03000003",
        "03333330",
        "03000003",
        "03000003",
        "03333330",
    ],
    'C': [
        "00000000",
        "00333333",
        "03000000",
        "03000000",
        "03000000",
        "03000000",
        "03000000",
        "00333333",
    ],
    'D': [
        "00000000",
        "03333330",
        "03000003",
        "03000003",
        "03000003",
        "03000003",
        "03000003",
        "03333330",
    ],
    'E': [
        "00000000",
        "03333333",
        "03000000",
        "03000000",
        "03333333",
        "03000000",
        "03000000",
        "03333333",
    ],
    'F': [
        "00000000",
        "03333333",
        "03000000",
        "03000000",
        "03333333",
        "03000000",
        "03000000",
        "03000000",
    ],
}

def check_hex(expected_hex, pix_data):
    """Check if the screen shows the expected hex value at (0,0)."""
    width = 160
    expected_hex = expected_hex.upper()

    for i, ch in enumerate(expected_hex):
        if ch not in HEX_TILES:
            return False
        tile = HEX_TILES[ch]
        for y in range(8):
            for x in range(8):
                px = i * 8 + x
                py = y
                idx = py * width + px
                if idx >= len(pix_data):
                    return False
                expected_shade = tile[y][x]
                actual_shade = pix_data[idx]
                if expected_shade != actual_shade:
                    return False
    return True

def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <expected_hex> <pix_file>", file=sys.stderr)
        sys.exit(2)

    expected = sys.argv[1]
    pix_path = sys.argv[2]

    with open(pix_path) as f:
        pix_data = f.read()

    if len(pix_data) < 160 * 144:
        print(f"Error: pix file too small ({len(pix_data)} chars)", file=sys.stderr)
        sys.exit(2)

    if check_hex(expected, pix_data):
        sys.exit(0)
    else:
        sys.exit(1)

if __name__ == '__main__':
    main()
