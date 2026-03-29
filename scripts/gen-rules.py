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


def gen_suite(suite_name, rom_dir, profile, trace_dir, emus, script, exclude_dirs=None, name_base=None):
    exclude_dirs = set(exclude_dirs or [])
    # name_base: directory used for relative path naming (defaults to rom_dir)
    name_base = name_base or rom_dir
    roms = []
    for dirpath, dirnames, filenames in os.walk(rom_dir):
        # Prune excluded subdirectories
        dirnames[:] = [d for d in dirnames if os.path.join(dirpath, d) not in exclude_dirs]
        for f in sorted(filenames):
            if f.endswith('.gb'):
                roms.append(os.path.join(dirpath, f))
    roms.sort()

    stamps = []
    for emu in emus:
        for rom in roms:
            # Include relative path in stamp name to avoid collisions
            rel = os.path.relpath(rom, name_base)
            name = os.path.splitext(rel)[0]
            safe = sanitize(name)
            stamp = f'{trace_dir}/.stamp_{safe}_{emu}'
            stamps.append(stamp)

            # Use single quotes around the ROM path to handle spaces
            print(f"{stamp}: adapters/{emu}/gbtrace-{emu} {profile} | $(CLI)")
            print(f"\t@mkdir -p {trace_dir}")
            print(f"\t@bash {script} adapters/{emu}/gbtrace-{emu} '{rom}' {profile} {trace_dir} {name_base} || true")
            print(f"\t@touch $@")
            print()

    return stamps


def main():
    emus = sys.argv[1].split(',') if len(sys.argv) > 1 else ['gambatte', 'sameboy', 'mgba', 'gateboy', 'missingno']
    blargg_emus = emus

    micro_stamps = gen_suite(
        'gbmicrotest',
        'test-suites/gbmicrotest',
        'test-suites/gbmicrotest/profile.toml',
        '$(GBMICROTEST_TRACE_DIR)',
        emus,
        'scripts/trace-gbmicrotest.sh',
    )

    blargg_stamps = gen_suite(
        'blargg',
        'test-suites/blargg',
        'test-suites/blargg/profile.toml',
        '$(BLARGG_TRACE_DIR)',
        blargg_emus,
        'scripts/trace-blargg.sh',
        exclude_dirs={'test-suites/blargg/dmg_sound'},
    )

    mooneye_stamps = gen_suite(
        'mooneye',
        'test-suites/mooneye',
        'test-suites/mooneye/profile.toml',
        '$(MOONEYE_TRACE_DIR)',
        emus,
        'scripts/trace-mooneye.sh',
    )

    gambatte_stamps = gen_suite(
        'gambatte-tests',
        'test-suites/gambatte',
        'test-suites/gambatte/profile.toml',
        '$(GAMBATTE_TESTS_TRACE_DIR)',
        emus,
        'scripts/trace-gambatte-tests.sh',
    )

    mealybug_stamps = gen_suite(
        'mealybug-tearoom',
        'test-suites/mealybug-tearoom',
        'test-suites/mealybug-tearoom/profile.toml',
        '$(MEALYBUG_TEAROOM_TRACE_DIR)',
        emus,
        'scripts/trace-mealybug-tearoom.sh',
    )

    dmg_sound_stamps = gen_suite(
        'dmg-sound',
        'test-suites/blargg/dmg_sound',
        'test-suites/blargg/dmg_sound/profile.toml',
        '$(BLARGG_TRACE_DIR)',
        emus,
        'scripts/trace-dmg-sound.sh',
        name_base='test-suites/blargg',
    )

    # Output stamp lists as Make variables
    print(f"GBMICROTEST_STAMPS := {' '.join(micro_stamps)}")
    print()
    print(f"BLARGG_STAMPS := {' '.join(blargg_stamps + dmg_sound_stamps)}")
    print()
    print(f"MOONEYE_STAMPS := {' '.join(mooneye_stamps)}")
    print()
    print(f"GAMBATTE_TESTS_STAMPS := {' '.join(gambatte_stamps)}")
    print()
    print(f"MEALYBUG_TEAROOM_STAMPS := {' '.join(mealybug_stamps)}")


if __name__ == '__main__':
    main()
