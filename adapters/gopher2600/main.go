// morepork-gopher2600: a morepork adapter for the Gopher2600 emulator (VCS family).
//
// Drives Gopher2600 headlessly one CPU instruction at a time and writes a
// native .morepork file (via the morepork FFI): per-instruction 6507 registers,
// TIA beam position, RIOT timer/ports, a set of memory addresses (the
// test-suite RESULT convention bytes + collisions, etc), and a final frame
// pixel snapshot (embedded in the trace like the Game Boy framebuffer).
//
//	morepork-gopher2600 -rom test.bin -out trace.morepork -spec NTSC -frames 30
package main

/*
// Include path and the libmorepork_ffi.a link are supplied by the Makefile via
// CGO_CFLAGS / CGO_LDFLAGS, so this file stays location-independent.
#include <stdlib.h>
#include "morepork.h"
*/
import "C"

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"strings"
	"unsafe"

	"github.com/jetsetilly/gopher2600/cartridgeloader"
	"github.com/jetsetilly/gopher2600/environment"
	"github.com/jetsetilly/gopher2600/hardware"
	"github.com/jetsetilly/gopher2600/hardware/preferences"
	"github.com/jetsetilly/gopher2600/hardware/riot/ports"
	"github.com/jetsetilly/gopher2600/hardware/television"
	"github.com/jetsetilly/gopher2600/hardware/television/frameinfo"
	"github.com/jetsetilly/gopher2600/hardware/television/signal"
	"github.com/jetsetilly/gopher2600/hardware/television/specification"
)

// memoryField is a named trace field sourced from a peeked memory address.
type memoryField struct {
	name string
	addr uint16
}

// The Tier 1 capture profile (hardcoded for the MVP).
var memoryFields = []memoryField{
	{"timer", 0x0284},  // INTIM
	{"port_a", 0x0280}, // SWCHA
	{"port_b", 0x0282}, // SWCHB
	{"result", 0x0080},
	{"code", 0x0081},
	{"observed", 0x0082},
	{"expected", 0x0083},
}

func fieldOrder() []string {
	fields := []string{"pc", "a", "x", "y", "s", "p", "line", "clock"}
	for _, mf := range memoryFields {
		fields = append(fields, mf.name)
	}
	return fields
}

// --- frame capture (implements television.PixelRenderer) ---
// SetPixels hands over the whole frame at once as a 228-wide signal bitmap;
// we crop to the 160-wide visible area and keep the latest frame.
type frameCapture struct {
	spec          specification.Spec
	haveSpec      bool
	width, height int
	pixels        []uint8 // palette indices (0xFF = black / vblank / unwritten)
}

const (
	clksScanline = 228
	clksHBlank   = 68
	clksVisible  = 160
)

func (f *frameCapture) NewFrame(fi frameinfo.Current) error {
	f.spec = fi.Spec
	f.haveSpec = true
	return nil
}
func (f *frameCapture) NewScanline(int) error { return nil }
func (f *frameCapture) SetPixels(sig []signal.SignalAttributes, last int) error {
	if last < 0 {
		return nil
	}
	rows := last/clksScanline + 1
	px := make([]uint8, clksVisible*rows)
	for i := range px {
		px[i] = 0xFF
	}
	vsync := make([]bool, rows)
	for i := 0; i <= last && i < len(sig); i++ {
		row := i / clksScanline
		if sig[i].VSync {
			vsync[row] = true
		}
		col := i % clksScanline
		if col < clksHBlank {
			continue
		}
		vc := col - clksHBlank
		if sig[i].VBlank || sig[i].VSync {
			px[row*clksVisible+vc] = 0xFF // blanked -> canonical black
		} else {
			px[row*clksVisible+vc] = uint8(sig[i].Color)
		}
	}
	// Anchor row 0 at the VSYNC-deassert edge (first non-VSYNC row following a
	// VSYNC row) and roll, so the full field is comparable across oracles
	// regardless of where each starts its frame buffer. (Stella anchors the same
	// point via its YStart offset.)
	anchor := 0
	for r := 0; r < rows; r++ {
		prev := (r - 1 + rows) % rows
		if vsync[prev] && !vsync[r] {
			anchor = r
			break
		}
	}
	if anchor != 0 {
		rolled := make([]uint8, len(px))
		for r := 0; r < rows; r++ {
			src := (anchor + r) % rows
			copy(rolled[r*clksVisible:(r+1)*clksVisible], px[src*clksVisible:(src+1)*clksVisible])
		}
		px = rolled
	}
	f.width = clksVisible
	f.height = rows
	f.pixels = px
	return nil
}
func (f *frameCapture) Reset()               {}
func (f *frameCapture) EndRendering() error  { return nil }

func (f *frameCapture) emit(w *C.MoreporkWriter, pal *[768]byte) {
	if !f.haveSpec || f.width == 0 || f.height == 0 || len(f.pixels) == 0 {
		return
	}
	// Embed the SUITE's canonical palette for the region (not Gopher's own), so a
	// golden PNG rendered from this trace is identical to one rendered from any
	// other oracle's trace — the pixels are emulator-independent TIA colour codes
	// and so is the colour table. See adapters/genpalette.py.
	C.morepork_writer_mark_frame_indexed(w,
		C.uint16_t(f.width), C.uint16_t(f.height), C.float(12.0/7.0),
		(*C.uint8_t)(unsafe.Pointer(&pal[0])), C.size_t(256),
		(*C.uint8_t)(unsafe.Pointer(&f.pixels[0])), C.size_t(len(f.pixels)))
}

func main() {
	rom := flag.String("rom", "", "path to the .bin/.a26 ROM")
	out := flag.String("out", "trace.morepork", "output .morepork path")
	spec := flag.String("spec", "NTSC", "TV spec: NTSC, PAL, PAL60, SECAM, AUTO")
	maxFrames := flag.Int("frames", 30, "cap: stop after this many frames")
	frame := flag.Bool("frame", true, "embed a final frame pixel snapshot")
	swchb := flag.Int("swchb", 0x48, "console switches: bit3=colour, bit6=P0 diff-A, bit7=P1 diff-A")
	mapping := flag.String("mapping", "AUTO", "force cartridge mapping (AUTO, F8, F6, F4, 2K, 4K, CV, FA, FE, E0, E7, 3F, 3E, AR, DPC, SB, EF, WD, UA...)")
	flag.Parse()

	if *rom == "" {
		fmt.Fprintln(os.Stderr, "error: -rom is required")
		os.Exit(2)
	}
	if err := run(*rom, *out, *spec, *maxFrames, *frame, uint8(*swchb), *mapping); err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}
}

func run(romPath, outPath, spec string, maxFrames int, captureFrame bool, swchb uint8, mapping string) error {
	romBytes, err := os.ReadFile(romPath)
	if err != nil {
		return err
	}
	sum := sha256.Sum256(romBytes)
	romSha := hex.EncodeToString(sum[:])

	prefs, err := preferences.NewPreferences()
	if err != nil {
		return fmt.Errorf("preferences: %w", err)
	}
	tv, err := television.NewTelevision(spec)
	if err != nil {
		return fmt.Errorf("television: %w", err)
	}
	tv.SetFPSLimit(false)

	fc := &frameCapture{}
	if captureFrame {
		tv.AddPixelRenderer(fc)
	}

	vcs, err := hardware.NewVCS(environment.Label("morepork"), tv, nil, prefs)
	if err != nil {
		return fmt.Errorf("vcs: %w", err)
	}
	loader, err := cartridgeloader.NewLoaderFromData(romPath, romBytes, mapping, "", nil, nil)
	if err != nil {
		return fmt.Errorf("loader: %w", err)
	}
	if err := vcs.AttachCartridge(loader, nil); err != nil {
		return fmt.Errorf("attach: %w", err)
	}
	// Report the resolved cartridge mapping (the detection linter consumes this).
	fmt.Fprintf(os.Stderr, "cartridge-type: %s\n", vcs.Mem.Cart.ID())

	// Set the console panel switches to a known state (the latching colour and
	// difficulty switches) so SWCHB reads are deterministic. bit3=colour,
	// bit6=P0 difficulty A, bit7=P1 difficulty A.
	panel := vcs.RIOT.Ports.Panel
	panel.HandleEvent(ports.PanelSetColor, swchb&0x08 != 0)
	panel.HandleEvent(ports.PanelSetPlayer0Pro, swchb&0x40 != 0)
	panel.HandleEvent(ports.PanelSetPlayer1Pro, swchb&0x80 != 0)

	header := map[string]any{
		"_header":          true,
		"format_version":   "0.1.0",
		"emulator":         "gopher2600",
		"emulator_version": "adapter-mvp",
		"rom_sha256":       romSha,
		"system":           "vcs",
		"model":            spec,
		"profile":          "tier1",
		"fields":           fieldOrder(),
		"trigger":          "instruction",
		"pix_format":       "indexed8",
	}
	headerJSON, err := json.Marshal(header)
	if err != nil {
		return err
	}

	cPath := C.CString(outPath)
	defer C.free(unsafe.Pointer(cPath))
	cHeader := C.CString(string(headerJSON))
	defer C.free(unsafe.Pointer(cHeader))

	w := C.morepork_writer_new(cPath, cHeader, C.size_t(len(headerJSON)))
	if w == nil {
		return fmt.Errorf("morepork_writer_new returned NULL (bad header/profile?)")
	}

	cols := map[string]C.int{}
	for _, name := range fieldOrder() {
		cn := C.CString(name)
		col := C.morepork_writer_find_field(w, cn)
		C.free(unsafe.Pointer(cn))
		if col < 0 {
			C.morepork_writer_close(w)
			return fmt.Errorf("field %q not in trace", name)
		}
		cols[name] = col
	}
	setU8 := func(name string, v uint8) {
		C.morepork_writer_set_u8(w, C.size_t(cols[name]), C.uint8_t(v))
	}
	setU16 := func(name string, v uint16) {
		C.morepork_writer_set_u16(w, C.size_t(cols[name]), C.uint16_t(v))
	}

	noop := func(bool) error { return nil }
	verdict := false
	for {
		if err := vcs.Step(noop); err != nil {
			C.morepork_writer_close(w)
			return fmt.Errorf("step: %w", err)
		}
		// Emit one entry per *retired* instruction. When the CPU is halted
		// (RDY low after a WSYNC — including WSYNC strobed via stack writes to
		// TIA mirrors in CLEAN_START), Step spins without retiring an
		// instruction; skipping those keeps the trace one-entry-per-instruction
		// and aligned with adapters that step whole instructions (e.g. Stella).
		if !vcs.CPU.RdyFlg {
			c := vcs.TV.GetCoords()
			if c.Frame >= maxFrames {
				break
			}
			continue
		}
		setU16("pc", vcs.CPU.PC.Address())
		setU8("a", vcs.CPU.A.Value())
		setU8("x", vcs.CPU.X.Value())
		setU8("y", vcs.CPU.Y.Value())
		setU8("s", uint8(vcs.CPU.SP.Address()))
		setU8("p", vcs.CPU.Status.Value())
		c := vcs.TV.GetCoords()
		setU16("line", uint16(c.Scanline))
		// Canonical VCS clock convention: 0..227 with 0 = start of HBLANK.
		// Gopher's coord origin is visible-start (HBLANK is -68..-1), so shift
		// by the HBLANK width to match (Stella's clocksThisLine is already
		// HBLANK-origin). Keeps the `clock` field comparable across adapters.
		setU8("clock", uint8(c.Clock+68))
		for _, mf := range memoryFields {
			v, _ := vcs.Mem.Peek(mf.addr)
			setU8(mf.name, v)
		}
		if C.morepork_writer_finish_entry(w) != 0 {
			C.morepork_writer_close(w)
			return fmt.Errorf("finish_entry failed")
		}
		if c.Frame >= maxFrames {
			break
		}
		// stop tracing once the RESULT byte holds a terminal verdict
		// ($A5 PASS / $5A FAIL). Non-convention ROMs never hit these.
		if v, _ := vcs.Mem.Peek(0x0080); v == 0xA5 || v == 0x5A {
			verdict = true
			break
		}
	}

	// Capture the result screen: a SELF test publishes its verdict *before*
	// the pass/fail screen renders, so step a couple more frames (no trace
	// entries) to let it draw, then embed the frame snapshot.
	if captureFrame {
		if verdict {
			target := vcs.TV.GetCoords().Frame + 2
			for vcs.TV.GetCoords().Frame < target {
				if err := vcs.Step(noop); err != nil {
					break
				}
			}
		}
		pal := &canonicalNTSCPalette
		switch {
		case strings.EqualFold(spec, "SECAM"):
			pal = &canonicalSECAMPalette
		case strings.HasPrefix(strings.ToUpper(spec), "PAL"):
			pal = &canonicalPALPalette
		}
		fc.emit(w, pal)
	}

	if C.morepork_writer_close(w) != 0 {
		return fmt.Errorf("writer close failed")
	}
	return nil
}
