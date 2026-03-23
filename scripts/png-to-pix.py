#!/usr/bin/env python3
"""Convert a 160x144 DMG grayscale PNG to a .pix reference file.

Each pixel is mapped to a shade character '0'-'3' based on brightness.
Output is a flat 23040-character file (160*144).

Usage: png-to-pix.py <input.png> <output.pix>
"""
import sys
from PIL import Image

def rgb_to_shade(r):
    if r >= 192: return '0'
    if r >= 112: return '1'
    if r >= 48:  return '2'
    return '3'

def main():
    if len(sys.argv) != 3:
        print(f'Usage: {sys.argv[0]} <input.png> <output.pix>', file=sys.stderr)
        sys.exit(1)

    img = Image.open(sys.argv[1]).convert('RGB')
    if img.size != (160, 144):
        print(f'Error: expected 160x144, got {img.size[0]}x{img.size[1]}', file=sys.stderr)
        sys.exit(1)

    pix = ''.join(rgb_to_shade(r) for r, g, b in img.getdata())
    with open(sys.argv[2], 'w') as f:
        f.write(pix)
    print(f'  {sys.argv[1]} -> {sys.argv[2]} ({len(pix)} pixels)')

if __name__ == '__main__':
    main()
