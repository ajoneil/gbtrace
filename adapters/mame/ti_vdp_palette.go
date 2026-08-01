package main

// Canonical TMS9918A palette, RGB triples for colours 0-15 (the classic
// datasheet-derived table every emulator ships). TI defines the palette as
// analog levels only, so RGB is presentation policy — comparisons happen in
// colour-index space and this table is only the frame snapshot's stamped
// presentation. Index 0 (transparent) renders as the backdrop and never
// appears in a captured frame. MAME 0.288's sg1000 output matches these
// values exactly (verified: dark blue, dark green, white).
var tiVdpPaletteRGB = [48]byte{
	0, 0, 0, // 0 transparent
	0, 0, 0, // 1 black
	33, 200, 66, // 2 medium green
	94, 220, 120, // 3 light green
	84, 85, 237, // 4 dark blue
	125, 118, 252, // 5 light blue
	212, 82, 77, // 6 dark red
	66, 235, 245, // 7 cyan
	252, 85, 84, // 8 medium red
	255, 121, 120, // 9 light red
	212, 193, 84, // 10 dark yellow
	230, 206, 128, // 11 light yellow
	33, 176, 59, // 12 dark green
	201, 91, 186, // 13 magenta
	204, 204, 204, // 14 gray
	255, 255, 255, // 15 white
}
