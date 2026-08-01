#!/usr/bin/env python3
"""Print the -I/-D/-std compile flags ares' own build used for a given TU.

The ares CMake tree can't be add_subdirectory'd (its dependency machinery
assumes it is the top-level project), so the adapter builds two-phase:
configure/build ares standalone with CMAKE_EXPORT_COMPILE_COMMANDS, then
compile the frontend TU with the same include/define surface as the cores.

  extract-flags.py <build-dir>/compile_commands.json <tu-substring>...
"""
import json
import shlex
import sys

db_path, *substrings = sys.argv[1:]
flags = []
seen = set()
db = json.load(open(db_path))
for substring in substrings:
    for entry in db:
        if substring not in entry["file"]:
            continue
        args = shlex.split(entry.get("command") or " ".join(entry["arguments"]))
        i = 0
        while i < len(args):
            arg = args[i]
            if arg == "-isystem" and i + 1 < len(args):
                pair = f"-isystem {args[i + 1]}"
                if pair not in seen:
                    seen.add(pair)
                    flags.extend([arg, args[i + 1]])
                i += 2
                continue
            if arg.startswith(("-I", "-D", "-std=")) and arg not in seen:
                seen.add(arg)
                flags.append(arg)
            i += 1
        break
print(" ".join(shlex.quote(f) for f in flags))
