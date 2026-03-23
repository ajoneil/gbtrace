#!/bin/bash
# Apply gbtrace pixel-tracing patches to gambatte-speedrun.
# Run from the gambatte adapter directory.
set -euo pipefail

GAMBATTE="gambatte-speedrun/gambatte_core"
LIBDIR="$GAMBATTE/libgambatte"
PPU="$LIBDIR/src/video/ppu.cpp"
PPUH="$LIBDIR/src/video/ppu.h"
GBH="$LIBDIR/include/gambatte.h"
GBCPP="$LIBDIR/src/gambatte.cpp"

if grep -q 'GBTRACE_PIXEL_PATCHED' "$PPU" 2>/dev/null; then
    echo "Already patched."
    exit 0
fi

echo "Applying pixel trace patches to gambatte..."

# 1. Create pixelbuf.h
cat > "$LIBDIR/include/pixelbuf.h" << 'EOF'
#ifndef PIXELBUF_H
#define PIXELBUF_H

class PixelBuf {
public:
    static const int CAPACITY = 160 * 144 + 256;
    PixelBuf() : head_(0), tail_(0) {}
    void push(unsigned char shade) {
        buf_[head_ % CAPACITY] = shade;
        ++head_;
    }
    int size() const { return head_ - tail_; }
    int read(char *dst, int n) const {
        int avail = size();
        if (n > avail) n = avail;
        for (int i = 0; i < n; i++)
            dst[i] = '0' + buf_[(tail_ + i) % CAPACITY];
        return n;
    }
    void clear() { tail_ = head_; }
    void reset() { head_ = tail_ = 0; }
private:
    unsigned char buf_[CAPACITY];
    int head_;
    int tail_;
};

#endif
EOF
echo "  Created pixelbuf.h"

# 2. Patch ppu.h — add pixbuf to PPUPriv and accessor to PPU
sed -i '/#include "gbint.h"/a #include "pixelbuf.h"' "$PPUH"
sed -i '/PPUPriv(NextM0Time &nextM0Time.*vram);/a\\tPixelBuf pixbuf;' "$PPUH"
sed -i '/void setFrameBuf/a\\tPixelBuf \& pixelBuf() { return p_.pixbuf; }\n\tPixelBuf const \& pixelBuf() const { return p_.pixbuf; }' "$PPUH"
echo "  Patched ppu.h"

# 3. Patch gambatte.h — add pixelBuf() to public API
sed -i '/#include "gbint.h"/a #include "pixelbuf.h"' "$GBH"
sed -i '/void setTraceCallback/a\\n\tPixelBuf \& pixelBuf();\n\tPixelBuf const \& pixelBuf() const;' "$GBH"
echo "  Patched gambatte.h"

# 4. Patch gambatte.cpp — add pixelBuf() implementation
# Find the closing brace of the namespace and insert before it
if ! grep -q 'GB::pixelBuf' "$GBCPP"; then
    # Append at the very end of the file, after the closing namespace brace
    cat >> "$GBCPP" << 'PIXBUF'

PixelBuf & gambatte::GB::pixelBuf() { return p_->cpu.ppu().pixelBuf(); }
PixelBuf const & gambatte::GB::pixelBuf() const { return p_->cpu.ppu().pixelBuf(); }
PIXBUF
fi
echo "  Patched gambatte.cpp"

# 5. Patch ppu.cpp — instrument every pixel write
# Strategy: use sed to transform each palette write pattern.
# The patterns are:
#   dst[N] = p.bgPalette[EXPR];    -> { unsigned _idx = (EXPR); dst[N] = p.bgPalette[_idx]; p.pixbuf.push(_idx & 3); }
#   dst[N] = bgPalette[EXPR];      -> { unsigned _idx = (EXPR); dst[N] = bgPalette[_idx]; p.pixbuf.push(_idx & 3); }
#   d[N] = spPalette[EXPR];        -> { unsigned _idx = (EXPR); d[N] = spPalette[_idx]; p.pixbuf.push(_idx & 3); }
#   *dst++ = p.bgPalette[0];       -> { *dst++ = p.bgPalette[0]; p.pixbuf.push(0); }

# Mark as patched
sed -i '1 a\// GBTRACE_PIXEL_PATCHED' "$PPU"

# Handle *dst++ = p.bgPalette[0]; (blank bg fills)
sed -i 's|\*dst++ = p\.bgPalette\[0\];|{ *dst++ = p.bgPalette[0]; p.pixbuf.push(0); }|g' "$PPU"

# Handle dst[N] = p.bgPalette[ EXPR ]; (DMG bg tiles — 8 per tile)
# These have the form: dst[0..7] = p.bgPalette[ expr ];
# We need to extract the index expression and push it.
python3 - "$PPU" << 'PYEOF'
import re, sys

path = sys.argv[1]
with open(path) as f:
    lines = f.readlines()

out = []
# Pattern: dst[N] = PALETTE[ EXPR ];
# where PALETTE is p.bgPalette, bgPalette, or spPalette
pat = re.compile(
    r'^(\s*)((?:dst|d)\[\d+\])\s*=\s*((?:p\.)?(?:bg|sp)Palette)\[([^\]]+)\];(.*)$'
)

for line in lines:
    m = pat.match(line)
    if m:
        indent, lhs, pal, idx_expr, rest = m.groups()
        idx_expr = idx_expr.strip()
        # Emit: { unsigned _s = (expr); lhs = pal[_s]; p.pixbuf.push(_s & 3); }
        out.append(f'{indent}{{ unsigned _s = ({idx_expr}); {lhs} = {pal}[_s]; p.pixbuf.push(_s & 3); }}{rest}\n')
    else:
        out.append(line)

with open(path, 'w') as f:
    f.writelines(out)

print(f"  Patched {sum(1 for l in out if 'pixbuf.push' in l)} pixel write sites in ppu.cpp")
PYEOF

# Handle the memcpy of prebuf to dbufline (partial first tile).
# After the memcpy, we need to push those pixels too.
# Line: std::memcpy(dbufline, prebuf + (tile_len - xpos), (newxpos - tile_len) * sizeof *dbufline);
# Add a loop after it to push the copied pixels.
sed -i '/std::memcpy(dbufline, prebuf.*newxpos.*tile_len/a\\t\t\tfor (int _pi = 0; _pi < newxpos - tile_len; _pi++) { unsigned long _px = dbufline[_pi]; int _sh = 0; for (int _j = 0; _j < 4; _j++) { if (p.bgPalette[_j] == _px) { _sh = _j; break; } } p.pixbuf.push(_sh \& 3); }' "$PPU"

echo "  Done patching ppu.cpp"
echo ""
echo "Rebuild with: cd gambatte-speedrun/gambatte_core/libgambatte && scons -j\$(nproc) && cd ../../.. && make"
