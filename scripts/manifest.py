#!/usr/bin/env python3
"""Generate manifest.json for a test suite directory.

Usage: manifest.py <trace-dir> <rom-dir>

Scans trace-dir for .gbtrace.parquet files and rom-dir for .gb files,
then writes manifest.json to trace-dir.
"""
import json
import os
import sys

EMULATORS = ['gateboy', 'missingno', 'gambatte', 'sameboy', 'mgba']

def generate_manifest(trace_dir, rom_dir):
    # Find all ROMs
    roms = {}
    for dirpath, _, filenames in sorted(os.walk(rom_dir)):
        for fname in sorted(filenames):
            if fname.endswith('.gb'):
                name = fname[:-3]
                rel = os.path.relpath(os.path.join(dirpath, fname), rom_dir)
                roms[name] = rel

    # Find all traces
    traces = {}
    for dirpath, _, filenames in sorted(os.walk(trace_dir)):
        for fname in sorted(filenames):
            if not fname.endswith('.gbtrace.parquet'):
                continue
            base = fname.replace('.gbtrace.parquet', '')
            for emu in EMULATORS:
                for status in ['pass', 'fail']:
                    suffix = f'_{emu}_{status}'
                    if base.endswith(suffix):
                        test_name = base[:-len(suffix)]
                        traces.setdefault(test_name, {})[emu] = status
                        break

    # Build manifest
    manifest = []
    for name, rom_rel in sorted(roms.items()):
        entry = {
            'name': name,
            'rom': rom_rel,
            'emulators': traces.get(name, {}),
        }
        manifest.append(entry)

    out_path = os.path.join(trace_dir, 'manifest.json')
    with open(out_path, 'w') as f:
        json.dump(manifest, f)

    total_traces = sum(len(e['emulators']) for e in manifest)
    print(f'  {len(manifest)} tests, {total_traces} traces -> {out_path}')

if __name__ == '__main__':
    if len(sys.argv) != 3:
        print(f'Usage: {sys.argv[0]} <trace-dir> <rom-dir>', file=sys.stderr)
        sys.exit(1)
    generate_manifest(sys.argv[1], sys.argv[2])
