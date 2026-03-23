#!/usr/bin/env python3
"""Generate Makefile rules for trace targets.

Outputs Make rules to stdout, one per ROM × emulator combination.
Handles filenames with spaces/special characters by sanitizing stamp names.
"""
import hashlib
import os
import sys


def sanitize(name):
    """Replace non-alphanumeric chars with underscores for Make target names."""
    return ''.join(c if c.isalnum() or c in '-_' else '_' for c in name)


def gen_suite(suite_name, rom_dir, profile, trace_dir, emus, script):
    roms = []
    for dirpath, _, filenames in os.walk(rom_dir):
        for f in sorted(filenames):
            if f.endswith('.gb'):
                roms.append(os.path.join(dirpath, f))
    roms.sort()

    stamps = []
    for emu in emus:
        for rom in roms:
            name = os.path.splitext(os.path.basename(rom))[0]
            safe = sanitize(name)
            stamp = f'{trace_dir}/.stamp_{safe}_{emu}'
            stamps.append(stamp)

            # Use single quotes around the ROM path to handle spaces
            print(f"{stamp}: adapters/{emu}/gbtrace-{emu} {profile} | $(CLI)")
            print(f"\t@mkdir -p {trace_dir}")
            print(f"\t@bash {script} adapters/{emu}/gbtrace-{emu} '{rom}' {profile} {trace_dir} {rom_dir} || true")
            print(f"\t@touch $@")
            print()

    return stamps


def main():
    emus = sys.argv[1].split(',') if len(sys.argv) > 1 else ['gambatte', 'sameboy', 'mgba', 'gateboy']
    blargg_emus = [e for e in emus if e != 'gateboy']

    micro_stamps = gen_suite(
        'gbmicrotest',
        'test-suites/gbmicrotest',
        'test-suites/gbmicrotest/gbmicrotest.toml',
        '$(GBMICROTEST_TRACE_DIR)',
        emus,
        'scripts/trace-gbmicrotest.sh',
    )

    blargg_stamps = gen_suite(
        'blargg',
        'test-suites/blargg',
        'test-suites/blargg/blargg.toml',
        '$(BLARGG_TRACE_DIR)',
        blargg_emus,
        'scripts/trace-blargg.sh',
    )

    # Output stamp lists as Make variables
    print(f"GBMICROTEST_STAMPS := {' '.join(micro_stamps)}")
    print()
    print(f"BLARGG_STAMPS := {' '.join(blargg_stamps)}")


if __name__ == '__main__':
    main()
