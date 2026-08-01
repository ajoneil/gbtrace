// openMSX's actual rendered RGB per TI VDP colour, measured from raw
// screenshots of uniform backdrop screens on C-BIOS_MSX1_JP (openMSX 21.0,
// SDLGL-PP renderer, default colour settings). openMSX renders its own
// measured TMS9918A palette — not the classic datasheet table — so the
// reverse map calibrates against these values; nearest-neighbour matching
// backs them up in case a different GL stack shades slightly differently.
// Index 0 (transparent) renders as the backdrop and never appears.
pub static OPENMSX_TI_VDP: [[u8; 3]; 16] = [
    [0, 0, 0],       // 0 transparent
    [0, 5, 0],       // 1 black
    [60, 197, 72],   // 2 medium green
    [115, 217, 124], // 3 light green
    [91, 96, 223],   // 4 dark blue
    [130, 130, 238], // 5 light blue
    [189, 105, 80],  // 6 dark red
    [98, 228, 235],  // 7 cyan
    [221, 112, 89],  // 8 medium red
    [254, 146, 124], // 9 light red
    [204, 204, 92],  // 10 dark yellow
    [221, 216, 134], // 11 light yellow
    [55, 175, 64],   // 12 dark green
    [185, 112, 181], // 13 magenta
    [202, 211, 201], // 14 gray
    [249, 255, 248], // 15 white
];

// Canonical TI VDP palette stamped into frame snapshots (matches the mame
// adapter's table): comparisons happen in colour-index space, RGB is
// presentation policy.
pub static TI_VDP: [[u8; 3]; 16] = [
    [0, 0, 0],
    [0, 0, 0],
    [33, 200, 66],
    [94, 220, 120],
    [84, 85, 237],
    [125, 118, 252],
    [212, 82, 77],
    [66, 235, 245],
    [252, 85, 84],
    [255, 121, 120],
    [212, 193, 84],
    [230, 206, 128],
    [33, 176, 59],
    [201, 91, 186],
    [204, 204, 204],
    [255, 255, 255],
];
