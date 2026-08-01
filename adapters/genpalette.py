#!/usr/bin/env python3
"""Generate the canonical VCS NTSC/PAL/SECAM palettes for every morepork VCS adapter.

A GOLD (frame) test's verdict is a captured frame compared against a golden
reference. The pixels a VCS adapter stores are TIA colour codes (the COLUxx
byte, 0..254 even) — emulator-independent. The palette (colour code -> RGB) is
NOT: each emulator ships slightly different RGB. So the SUITE declares ONE
canonical palette per TV standard and every adapter embeds it, so `morepork
render` produces the identical golden PNG no matter which oracle captured the
trace.

Canonical NTSC/PAL palettes = Stella's `ourNTSCPalette` / `ourPALPalette` (the
de-facto community standard). 128 hue/luma colours indexed by (colour code >> 1).
We emit a 256-entry RGB table indexed by the raw colour byte: even index = the
colour, odd index = black (TIA ignores bit 0, and 0xFF is our blank/vblank
sentinel, so it must render black too).

SECAM is fundamentally different: the console has no hue on SECAM. The luminance
nibble alone (bits 3..1 of the colour byte, i.e. `(code >> 1) & 7`) selects one
of only 8 fixed, fully-saturated colours; the hue nibble is ignored entirely. So
the SECAM 128-entry table is the 8 colours tiled 16 times — two codes that
differ only in hue render identically. Canonical SECAM = Stella's `ourSECAMPalette`.

Run from adapters/:  python3 genpalette.py
"""
import os

# 128 canonical NTSC colours (Stella ourNTSCPalette, even entries). Provenance
# is recorded in the suite notes; change here + regenerate to re-declare it.
NTSC_128 = [
    0x000000,0x4a4a4a,0x6f6f6f,0x8e8e8e,0xaaaaaa,0xc0c0c0,0xd6d6d6,0xececec,
    0x484800,0x69690f,0x86861d,0xa2a22a,0xbbbb35,0xd2d240,0xe8e84a,0xfcfc54,
    0x7c2c00,0x904811,0xa26221,0xb47a30,0xc3903d,0xd2a44a,0xdfb755,0xecc860,
    0x901c00,0xa33915,0xb55328,0xc66c3a,0xd5824a,0xe39759,0xf0aa67,0xfcbc74,
    0x940000,0xa71a1a,0xb83232,0xc84848,0xd65c5c,0xe46f6f,0xf08080,0xfc9090,
    0x840064,0x97197a,0xa8308f,0xb846a2,0xc659b3,0xd46cc3,0xe07cd2,0xec8ce0,
    0x500084,0x68199a,0x7d30ad,0x9246c0,0xa459d0,0xb56ce0,0xc57cee,0xd48cfc,
    0x140090,0x331aa3,0x4e32b5,0x6848c6,0x7f5cd5,0x956fe3,0xa980f0,0xbc90fc,
    0x000094,0x181aa7,0x2d32b8,0x4248c8,0x545cd6,0x656fe4,0x7580f0,0x8490fc,
    0x001c88,0x183b9d,0x2d57b0,0x4272c2,0x548ad2,0x65a0e1,0x75b5ef,0x84c8fc,
    0x003064,0x185080,0x2d6d98,0x4288b0,0x54a0c5,0x65b7d9,0x75cceb,0x84e0fc,
    0x004030,0x18624e,0x2d8169,0x429e82,0x54b899,0x65d1ae,0x75e7c2,0x84fcd4,
    0x004400,0x1a661a,0x328432,0x48a048,0x5cba5c,0x6fd26f,0x80e880,0x90fc90,
    0x143c00,0x355f18,0x527e2d,0x6e9c42,0x87b754,0x9ed065,0xb4e775,0xc8fc84,
    0x303800,0x505916,0x6d762b,0x88923e,0xa0ab4f,0xb7c25f,0xccd86e,0xe0ec7c,
    0x482c00,0x694d14,0x866a26,0xa28638,0xbb9f47,0xd2b656,0xe8cc63,0xfce070,
]
assert len(NTSC_128) == 128

# 128 canonical PAL colours (Stella ourPALPalette, active #else branch).
PAL_128 = [
    0x0b0b0b,0x333333,0x595959,0x7b7b7b,0x999999,0xb6b6b6,0xcfcfcf,0xe6e6e6,
    0x0b0b0b,0x333333,0x595959,0x7b7b7b,0x999999,0xb6b6b6,0xcfcfcf,0xe6e6e6,
    0x3b2400,0x664700,0x8b7000,0xac9200,0xc5ae36,0xdec85e,0xf7e27f,0xfff19e,
    0x004500,0x006f00,0x3b9200,0x65b009,0x85ca3d,0xa3e364,0xbffc84,0xd5ffa5,
    0x590000,0x802700,0xa15700,0xbc7937,0xd6985f,0xeeb381,0xffce9e,0xffdcbd,
    0x004900,0x007200,0x169216,0x45af45,0x6bc96b,0x8be38b,0xa9fba9,0xc5ffc5,
    0x640012,0x890821,0xa73d4d,0xc26472,0xdc8491,0xf4a3ae,0xffbeca,0xffdae0,
    0x003d29,0x006a48,0x048e63,0x3caa84,0x62c5a2,0x83dfbe,0xa1f8d9,0xbeffe9,
    0x550046,0x88006e,0xa5318d,0xc159aa,0xda7cc5,0xf39adf,0xffb9f3,0xffd4f6,
    0x003651,0x005a7d,0x117e9c,0x429cb8,0x68b7d2,0x88d2eb,0xa6ebff,0xc3ffff,
    0x4c007c,0x75009d,0x932eb8,0xaf57d2,0xca7aeb,0xe499ff,0xecb7ff,0xf3d4ff,
    0x002d83,0x003ea4,0x2d65bf,0x5685da,0x79a2f2,0x99bfff,0xb7dbff,0xd3f5ff,
    0x220096,0x5200b6,0x7538cf,0x945fe8,0xb181ff,0xc5a0ff,0xd6bdff,0xe8daff,
    0x00009a,0x241db6,0x504ad0,0x746fe9,0x928eff,0xb1adff,0xcecaff,0xe9e5ff,
    0x0b0b0b,0x333333,0x595959,0x7b7b7b,0x999999,0xb6b6b6,0xcfcfcf,0xe6e6e6,
    0x0b0b0b,0x333333,0x595959,0x7b7b7b,0x999999,0xb6b6b6,0xcfcfcf,0xe6e6e6,
]
assert len(PAL_128) == 128

# 8 canonical SECAM colours (Stella ourSECAMPalette), indexed by the luminance
# nibble alone: 0 black, 1 blue, 2 red, 3 magenta, 4 green, 5 cyan, 6 yellow,
# 7 white. SECAM ignores hue, so the 128-entry table is these 8 tiled 16 times
# (SECAM_128[code>>1] = SECAM_8[luma], luma = (code>>1) & 7).
SECAM_8 = [0x000000,0x2121ff,0xf03c79,0xff50ff,0x7fff00,0x7fffff,0xffff3f,0xffffff]
assert len(SECAM_8) == 8
SECAM_128 = [SECAM_8[i & 7] for i in range(128)]
assert len(SECAM_128) == 128


def rgb256(colors):
    """256-entry RGB byte table: even index = colour, odd index = black.

    Index 0 (TIA code $00) is forced to pure black so it matches the blanked-
    pixel representation across adapters. Gopher marks blanked (VBLANK) pixels
    with a black sentinel while Stella leaves them as code $00; on NTSC both are
    0x000000 anyway, but on PAL code $00 is Stella's near-black 0x0b0b0b, which
    made the blanking region diverge. A $00 background and the blanking level are
    both black, so this is both consistent and correct."""
    out = []
    for i in range(256):
        c = colors[i >> 1] if (i & 1) == 0 else 0x000000
        if i == 0:
            c = 0x000000
        out += [(c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF]
    return out


PALETTES = {"canonicalNTSCPalette":  rgb256(NTSC_128),
            "canonicalPALPalette":   rgb256(PAL_128),
            "canonicalSECAMPalette": rgb256(SECAM_128)}

HERE = os.path.dirname(os.path.abspath(__file__))
BANNER = "// Generated by adapters/genpalette.py — do not edit. Canonical VCS\n" \
         "// NTSC/PAL/SECAM palettes (Stella ourNTSC/ourPAL/ourSECAMPalette). Each\n" \
         "// 256*3 bytes, index = TIA colour byte; even = colour, odd = black (bit 0\n" \
         "// ignored). SECAM ignores hue: 8 luma colours tiled across the table.\n"


def emit_go(path):
    with open(path, "w") as f:
        f.write(BANNER + "\npackage main\n")
        for name, rgb in PALETTES.items():
            f.write(f"\nvar {name} = [768]byte{{" + ",".join(str(b) for b in rgb) + "}\n")


def emit_c(path):
    with open(path, "w") as f:
        f.write(BANNER + "\n#pragma once\n#include <cstdint>\n")
        for name, rgb in PALETTES.items():
            rows = ["  " + ",".join(str(b) for b in rgb[r:r + 24]) + "," for r in range(0, len(rgb), 24)]
            f.write(f"\nstatic const uint8_t {name}[768] = {{\n" + "\n".join(rows) + "\n};\n")


def emit_rust(path):
    names = {"canonicalNTSCPalette": "CANONICAL_NTSC",
             "canonicalPALPalette": "CANONICAL_PAL",
             "canonicalSECAMPalette": "CANONICAL_SECAM"}
    with open(path, "w") as f:
        f.write(BANNER)
        for name, rgb in PALETTES.items():
            trips = [f"[{rgb[i]},{rgb[i+1]},{rgb[i+2]}]" for i in range(0, len(rgb), 3)]
            rows = ["    " + ",".join(trips[r:r + 8]) + "," for r in range(0, 256, 8)]
            f.write(f"\npub static {names[name]}: [[u8; 3]; 256] = [\n" + "\n".join(rows) + "\n];\n")


emit_go(os.path.join(HERE, "gopher2600", "ntsc_palette.go"))
emit_c(os.path.join(HERE, "stella", "ntsc_palette.h"))
emit_rust(os.path.join(HERE, "missingno", "src", "vcs_palette.rs"))
emit_rust(os.path.join(HERE, "mame", "src", "vcs_palette.rs"))
print("wrote NTSC+PAL+SECAM palettes to gopher2600/ (.go), stella/ (.h), missingno/src and mame/src (.rs)")
