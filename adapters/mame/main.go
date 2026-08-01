// morepork-mame: a morepork adapter driving MAME as an independent-lineage
// behavioural oracle — the a2600 driver for the VCS suite, the sg1000 driver
// for the TI VDP (TMS9918A) suite.
//
// MAME is not linkable like the Stella/Gopher2600 adapters, so this drives it
// headlessly via its gdbstub debugger. For speed it does NOT single-step over
// the wire (that was ~19s/ROM); instead it uses the GDB remote `monitor`
// command (qRcmd) to install MAME's own debugger `trace` command (which logs
// every instruction at full emulation speed) plus a watchpoint on the RESULT
// byte, then `continue`s to the verdict (~250ms/ROM). The trace log is parsed
// into a native .morepork via the FFI (no JSONL).
//
//	morepork-mame -rom test.bin -out trace.morepork -spec NTSC -frames 30
//	morepork-mame -system sg1000 -rom sanity.sg -out trace.morepork
package main

/*
#include <stdlib.h>
#include "morepork.h"
*/
import "C"

import (
	"bufio"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"net"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"syscall"
	"time"
	"unsafe"
)

// --- GDB remote client ---
type gdb struct {
	conn net.Conn
	r    *bufio.Reader
}

func (g *gdb) send(body string) {
	sum := 0
	for i := 0; i < len(body); i++ {
		sum += int(body[i])
	}
	fmt.Fprintf(g.conn, "$%s#%02x", body, sum&0xff)
}

func (g *gdb) recv() string {
	for {
		b, err := g.r.ReadByte()
		if err != nil {
			return ""
		}
		if b == '$' {
			break
		}
	}
	body := make([]byte, 0, 64)
	for {
		b, err := g.r.ReadByte()
		if err != nil {
			return ""
		}
		if b == '#' {
			break
		}
		body = append(body, b)
	}
	g.r.ReadByte()
	g.r.ReadByte()
	g.conn.Write([]byte("+"))
	return string(body)
}

func (g *gdb) cmd(body string) string { g.send(body); return g.recv() }

// mon runs a MAME debugger console command via the GDB `monitor` (qRcmd) escape.
func (g *gdb) mon(command string) string {
	resp := g.cmd("qRcmd," + hex.EncodeToString([]byte(command)))
	dec, _ := hex.DecodeString(resp)
	return string(dec)
}

// --- system definitions ---
//
// Everything system-shaped in one table: the MAME machine, the CPU device
// tag the debugger's `trace` wants, the tracelog register format, and the
// RESULT-block address of the suite's verdict convention. Both suites share
// the verdict values ($A5 PASS / $5A FAIL, then CODE/OBSERVED/EXPECTED).

type fieldSpec struct {
	name string
	u16  bool
}

type sysDef struct {
	id         string // morepork header `system`
	cpuTag     string // MAME device tag for `trace` (a2600: maincpu, sg1000: z80)
	resultAddr string // RESULT block base, hex without 0x
	traceFmt   string // tracelog format string ("R" + one column per field)
	traceSyms  string // tracelog symbols, validated against installed MAME
	cpuFields  []fieldSpec
	machine    func(spec string) (string, error)
}

var systems = map[string]*sysDef{
	"vcs": {
		id: "vcs", cpuTag: "maincpu", resultAddr: "80",
		traceFmt:  "R%04X %02X %02X %02X %02X %02X",
		traceSyms: "pc,a,x,y,sp,p", // register symbol is `sp`, not `s`
		cpuFields: []fieldSpec{
			{"pc", true}, {"a", false}, {"x", false},
			{"y", false}, {"s", false}, {"p", false},
		},
		machine: func(spec string) (string, error) {
			// MAME has no SECAM Atari 2600 machine (only a2600 / a2600p), so
			// it cannot capture a real SECAM field. Reject rather than
			// silently emit an a2600 (NTSC-geometry) frame tagged SECAM.
			if strings.EqualFold(spec, "SECAM") {
				return "", fmt.Errorf("MAME has no SECAM 2600 driver; capture -spec SECAM with the stella or gopher2600 adapter")
			}
			if spec == "PAL" {
				return "a2600p", nil
			}
			return "a2600", nil
		},
	},
	"sg1000": {
		id: "sg1000", cpuTag: "z80", resultAddr: "c000",
		traceFmt:  "R%04X %04X %02X %02X %02X %02X %02X %02X %02X %02X %04X %04X",
		traceSyms: "pc,sp,a,f,b,c,d,e,h,l,ix,iy",
		cpuFields: []fieldSpec{
			{"pc", true}, {"sp", true}, {"a", false}, {"f", false},
			{"b", false}, {"c", false}, {"d", false}, {"e", false},
			{"h", false}, {"l", false}, {"ix", true}, {"iy", true},
		},
		machine: func(spec string) (string, error) {
			// The sg1000 driver carries the NTSC TMS9918A. TMS9929A (PAL)
			// capture would need a different machine; the suite treats PAL
			// as provisional anyway.
			if !strings.EqualFold(spec, "NTSC") {
				return "", fmt.Errorf("MAME sg1000 capture is NTSC-only (TMS9918A); -spec %s is not supported", spec)
			}
			return "sg1000", nil
		},
	},
}

// switchLuaTemplate sets the a2600 console switches (best-effort; see README).
const switchLuaTemplate = `
local v = %d
local function apply()
  local swb = manager.machine.ioport.ports[":SWB"]
  if not swb then return end
  local function set(name, bit)
    local f = swb.fields[name]
    if f then f:set_value(((v >> bit) & 1) ~= 0 and f.mask or 0) end
  end
  set("TV Type", 3)
  set("Left Diff. Switch", 6)
  set("Right Diff. Switch", 7)
end
apply()
emu.register_prestart(apply)
emu.register_frame_done(apply)
`

func main() {
	system := flag.String("system", "vcs", "target system: vcs (a2600) or sg1000")
	rom := flag.String("rom", "", "path to the ROM (.bin/.a26 for vcs, .sg for sg1000)")
	out := flag.String("out", "trace.morepork", "output .morepork path")
	spec := flag.String("spec", "NTSC", "TV spec: NTSC or PAL (vcs: a2600 vs a2600p; sg1000: NTSC only)")
	maxFrames := flag.Int("frames", 30, "cap: seconds_to_run = max(2, frames/60)")
	port := flag.Int("port", 0, "gdbstub port (0 = auto-pick a free ephemeral port)")
	swchb := flag.Int("swchb", 0x48, "vcs console switches: bit3=colour, bit6=P0 diff-A, bit7=P1 diff-A")
	frame := flag.Bool("frame", true, "capture a final frame snapshot (a second headless MAME pass)")
	flag.Parse()
	if *rom == "" {
		fmt.Fprintln(os.Stderr, "error: -rom is required")
		os.Exit(2)
	}
	sys, ok := systems[*system]
	if !ok {
		fmt.Fprintf(os.Stderr, "error: unknown -system %q (vcs, sg1000)\n", *system)
		os.Exit(2)
	}
	if err := run(sys, *rom, *out, *spec, *maxFrames, *port, *swchb, *frame); err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}
}

func run(sys *sysDef, romPath, outPath, spec string, maxFrames, port, swchb int, wantFrame bool) error {
	machine, err := sys.machine(spec)
	if err != nil {
		return err
	}
	romBytes, err := os.ReadFile(romPath)
	if err != nil {
		return err
	}
	romSha := hex.EncodeToString(func() []byte { s := sha256.Sum256(romBytes); return s[:] }())

	// Pick a free ephemeral gdbstub port so rapid successive runs don't collide
	// on a fixed port left in TIME_WAIT (the source of transient "gdbstub never
	// listened" failures).
	if port == 0 {
		l, lerr := net.Listen("tcp", "localhost:0")
		if lerr != nil {
			return fmt.Errorf("pick free port: %w", lerr)
		}
		port = l.Addr().(*net.TCPAddr).Port
		l.Close()
	}

	luaFile, err := os.CreateTemp("", "morepork-mame-*.lua")
	if err != nil {
		return err
	}
	defer os.Remove(luaFile.Name())
	if sys.id == "vcs" { // console switches exist only on the 2600
		luaFile.WriteString(fmt.Sprintf(switchLuaTemplate, swchb))
	}
	luaFile.Close()
	cfgDir, err := os.MkdirTemp("", "morepork-mame-cfg-*")
	if err != nil {
		return err
	}
	defer os.RemoveAll(cfgDir)
	traceLog, err := os.CreateTemp("", "morepork-mame-trace-*.log")
	if err != nil {
		return err
	}
	traceLog.Close()
	defer os.Remove(traceLog.Name())

	seconds := maxFrames / 60
	if seconds < 2 {
		seconds = 2
	}
	mame := exec.Command("mame", machine, "-cart", romPath,
		"-video", "none", "-sound", "none", "-nothrottle",
		"-autoboot_script", luaFile.Name(), "-autoboot_delay", "0",
		"-cfg_directory", cfgDir, "-snapshot_directory", cfgDir,
		"-nvram_directory", cfgDir,
		"-debug", "-debugger", "gdbstub", "-debugger_port", strconv.Itoa(port),
		"-seconds_to_run", strconv.Itoa(seconds))
	mame.Stdout, mame.Stderr = nil, nil
	// Own process group so we can reap MAME *and* any child it forks. Killing
	// only the launcher's PID left the real emulator process lingering.
	mame.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	if err := mame.Start(); err != nil {
		return fmt.Errorf("launch mame: %w", err)
	}
	defer func() {
		// SIGKILL the whole group (negative PID), then Wait to reap the zombie.
		// When MAME is paused at the watchpoint it never reaches seconds_to_run,
		// so it will not exit on its own — this is the only thing that stops it.
		if mame.Process != nil {
			_ = syscall.Kill(-mame.Process.Pid, syscall.SIGKILL)
			_ = mame.Process.Kill()
			_, _ = mame.Process.Wait()
		}
	}()

	var conn net.Conn
	for i := 0; i < 100; i++ {
		conn, err = net.Dial("tcp", fmt.Sprintf("localhost:%d", port))
		if err == nil {
			break
		}
		time.Sleep(50 * time.Millisecond)
	}
	if conn == nil {
		return fmt.Errorf("gdbstub never listened on %d", port)
	}
	defer conn.Close()
	g := &gdb{conn: conn, r: bufio.NewReader(conn)}
	conn.Write([]byte("+"))
	// handshake (MAME's gdbstub answers `g`/monitor only after these)
	g.cmd("qSupported")
	g.cmd("qXfer:features:read:target.xml:0,3fc")

	// install a full-speed per-instruction trace (noloop = don't collapse
	// loops; one invalid register symbol makes the whole tracelog fall back
	// to plain disassembly silently) and a watchpoint on the RESULT verdict.
	g.mon(fmt.Sprintf(`trace %s,%s,noloop,{tracelog "%s\n",%s}`,
		traceLog.Name(), sys.cpuTag, sys.traceFmt, sys.traceSyms))
	g.mon(fmt.Sprintf("wpset 0x%s,1,w,{(wpdata==0xa5)||(wpdata==0x5a)}", sys.resultAddr))
	conn.SetReadDeadline(time.Now().Add(30 * time.Second))
	g.cmd("c") // run full-speed to the verdict (or seconds_to_run)
	// read the RESULT bytes at the stop (per-instruction memory in the trace
	// format breaks tracelog, so we grab the final verdict here).
	res, code, obs, exp := parseMem(g.cmd("m" + sys.resultAddr + ",4"))
	g.mon("trace off") // flush the trace file
	_ = syscall.Kill(-mame.Process.Pid, syscall.SIGKILL) // free the port before pass 2

	// A second, gdbstub-free headless pass captures the final frame via Lua
	// (gdbstub exposes no pixels). Best-effort: a frame is nice-to-have.
	var fr *frameData
	if wantFrame {
		if f, ferr := captureFrame(sys, machine, romPath, spec, maxFrames); ferr == nil {
			fr = f
		} else {
			fmt.Fprintf(os.Stderr, "warning: frame capture failed: %v\n", ferr)
		}
	}

	return writeTrace(outPath, sys, spec, romSha, traceLog.Name(), res, code, obs, exp, fr)
}

type frameData struct {
	width, height int
	pixels        []byte  // canonical-palette indices (TIA codes / TMS colours)
	palette       []byte  // RGB triples stamped into the frame snapshot
	aspect        float32 // display pixel aspect at the system's dot clock
}

// frameLuaTemplate runs the ROM for a few frames, then dumps the screen's
// pixels (ARGB32) to a file and exits. %q = dump path, %d = frame to capture on.
const frameLuaTemplate = `
local target = %d
local n = 0
emu.register_frame_done(function()
  n = n + 1
  if n < target then return end
  local s = manager.machine.screens:at(1)
  local ok, px = pcall(function() return s:pixels() end)
  if ok then
    local f = io.open(%q, "wb"); f:write(px); f:close()
    io.stderr:write(string.format("GBFRAME %%d %%d\n", s.width, s.height))
  end
  manager.machine:exit()
end)
`

// captureFrame launches a second headless MAME, dumps the last frame's pixels,
// and reverse-maps each RGB to a canonical palette index (TIA colour code /
// TMS9918 colour) so the frame is oracle-independent like the other adapters'.
func captureFrame(sys *sysDef, machine, romPath, spec string, maxFrames int) (*frameData, error) {
	dump, err := os.CreateTemp("", "morepork-mame-px-*.bin")
	if err != nil {
		return nil, err
	}
	dump.Close()
	defer os.Remove(dump.Name())
	luaFile, err := os.CreateTemp("", "morepork-mame-frame-*.lua")
	if err != nil {
		return nil, err
	}
	defer os.Remove(luaFile.Name())
	// Redirect MAME's cfg/snapshot/nvram output to a temp dir so it never
	// litters the working directory (cfg/, snap/, nvram/).
	tmpHome, err := os.MkdirTemp("", "morepork-mame-home-*")
	if err != nil {
		return nil, err
	}
	defer os.RemoveAll(tmpHome)
	target := maxFrames
	if target < 8 {
		target = 8 // let a static image settle
	}
	luaFile.WriteString(fmt.Sprintf(frameLuaTemplate, target, dump.Name()))
	luaFile.Close()

	seconds := target/60 + 2
	cmd := exec.Command("mame", machine, "-cart", romPath,
		"-video", "none", "-sound", "none", "-nothrottle",
		"-autoboot_script", luaFile.Name(), "-autoboot_delay", "0",
		"-cfg_directory", tmpHome, "-snapshot_directory", tmpHome,
		"-nvram_directory", tmpHome,
		"-seconds_to_run", strconv.Itoa(seconds))
	var stderr strings.Builder
	cmd.Stderr = &stderr
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	select {
	case <-done:
	case <-time.After(30 * time.Second):
	}
	if cmd.Process != nil {
		_ = syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
		_, _ = cmd.Process.Wait()
	}

	// parse "GBFRAME <w> <h>" from stderr
	var w, h int
	for _, ln := range strings.Split(stderr.String(), "\n") {
		if strings.HasPrefix(ln, "GBFRAME ") {
			fmt.Sscanf(ln, "GBFRAME %d %d", &w, &h)
		}
	}
	if w == 0 || h == 0 {
		return nil, fmt.Errorf("no frame dumped (%s)", firstLine(stderr.String()))
	}
	argb, err := os.ReadFile(dump.Name())
	if err != nil {
		return nil, err
	}
	if len(argb) < w*h*4 {
		return nil, fmt.Errorf("short pixel dump: %d < %d", len(argb), w*h*4)
	}
	if sys.id == "sg1000" {
		return mapTiVdpFrame(w, h, argb)
	}
	// MAME's a2600 screen is 176 wide = the 160 visible pixels + an 8px border
	// each side. Centre-crop to the canonical 160-wide visible so the frame
	// matches the other adapters. (Vertical alignment vs the full-field golden
	// is done in the GOLD compare step — MAME exposes no VSYNC position.)
	const vis = 160
	x0 := (w - vis) / 2
	if x0 < 0 {
		x0 = 0
	}
	cw := vis
	if cw > w {
		cw = w
	}
	// Reverse-map against MAME's calibrated palette FOR THIS REGION (a2600p's
	// PAL palette differs from a2600's NTSC one). Exact match; nearestMame is a
	// safety net. MAME has no anti-aliasing, so its pixels hit the table exactly.
	mamePal := &mamePaletteRGB
	if strings.HasPrefix(strings.ToUpper(spec), "PAL") {
		mamePal = &mamePalPaletteRGB
	}
	exact := map[uint32]uint8{}
	for code := 0; code < 256; code++ {
		k := uint32(mamePal[code*3])<<16 | uint32(mamePal[code*3+1])<<8 | uint32(mamePal[code*3+2])
		if _, seen := exact[k]; !seen { // prefer the lowest code (code 0 for black)
			exact[k] = uint8(code)
		}
	}
	pixels := make([]byte, cw*h)
	cache := map[uint32]uint8{}
	for y := 0; y < h; y++ {
		for cx := 0; cx < cw; cx++ {
			i := y*w + (x0 + cx)
			// MAME pixels() is ARGB32 little-endian: bytes b,g,r,a.
			b, gg, r := argb[i*4], argb[i*4+1], argb[i*4+2]
			key := uint32(r)<<16 | uint32(gg)<<8 | uint32(b)
			idx, ok := cache[key]
			if !ok {
				if e, hit := exact[key]; hit {
					idx = e
				} else {
					idx = nearestMame(mamePal, r, gg, b)
				}
				cache[key] = idx
			}
			pixels[y*cw+cx] = idx
		}
	}
	pal := canonicalNTSCPalette
	if strings.HasPrefix(strings.ToUpper(spec), "PAL") {
		pal = canonicalPALPalette
	}
	return &frameData{width: cw, height: h, pixels: pixels, palette: pal[:], aspect: 12.0 / 7.0}, nil
}

// mapTiVdpFrame crops MAME's sg1000 screen to the TMS9918A active area and
// reverse-maps each RGB to a TMS colour index. Verified against MAME 0.288:
// the screen is 280×216 with the 256×192 active area centred at (12,12), and
// its rendered RGBs match the canonical palette exactly (nearestTiVdp is a
// safety net). Index 0 (transparent) renders as the backdrop, so the map
// covers colours 1-15 only and the frame never contains a 0.
func mapTiVdpFrame(w, h int, argb []byte) (*frameData, error) {
	const visW, visH = 256, 192
	x0, y0 := (w-visW)/2, (h-visH)/2
	if x0 < 0 || y0 < 0 {
		return nil, fmt.Errorf("screen %dx%d smaller than the %dx%d active area", w, h, visW, visH)
	}
	exact := map[uint32]uint8{}
	for c := 1; c < 16; c++ {
		k := uint32(tiVdpPaletteRGB[c*3])<<16 | uint32(tiVdpPaletteRGB[c*3+1])<<8 | uint32(tiVdpPaletteRGB[c*3+2])
		if _, seen := exact[k]; !seen { // colour 1 (black) wins the 0/1 duplicate
			exact[k] = uint8(c)
		}
	}
	pixels := make([]byte, visW*visH)
	cache := map[uint32]uint8{}
	for y := 0; y < visH; y++ {
		for x := 0; x < visW; x++ {
			i := (y0+y)*w + (x0 + x)
			// MAME pixels() is ARGB32 little-endian: bytes b,g,r,a.
			b, g, r := argb[i*4], argb[i*4+1], argb[i*4+2]
			key := uint32(r)<<16 | uint32(g)<<8 | uint32(b)
			idx, ok := cache[key]
			if !ok {
				if e, hit := exact[key]; hit {
					idx = e
				} else {
					idx = nearestTiVdp(r, g, b)
				}
				cache[key] = idx
			}
			pixels[y*visW+x] = idx
		}
	}
	return &frameData{width: visW, height: visH, pixels: pixels, palette: tiVdpPaletteRGB[:], aspect: 8.0 / 7.0}, nil
}

// nearestTiVdp maps an RGB triple to the closest TMS9918 colour (1-15).
func nearestTiVdp(r, g, b uint8) uint8 {
	best := uint8(1)
	bestD := int32(1<<31 - 1)
	for c := 1; c < 16; c++ {
		dr := int32(r) - int32(tiVdpPaletteRGB[c*3])
		dg := int32(g) - int32(tiVdpPaletteRGB[c*3+1])
		db := int32(b) - int32(tiVdpPaletteRGB[c*3+2])
		d := dr*dr + dg*dg + db*db
		if d < bestD {
			bestD = d
			best = uint8(c)
			if d == 0 {
				break
			}
		}
	}
	return best
}

func firstLine(s string) string {
	if i := strings.IndexByte(s, '\n'); i >= 0 {
		return s[:i]
	}
	return s
}

// nearestMame maps an RGB triple to the TIA colour code whose MAME palette
// entry is closest (squared distance) — a safety net for the exact map. Only
// even indices are real colours (odd = black).
func nearestMame(pal *[768]byte, r, g, b uint8) uint8 {
	best := uint8(0)
	bestD := int32(1<<31 - 1)
	for i := 0; i < 256; i += 2 {
		dr := int32(r) - int32(pal[i*3])
		dg := int32(g) - int32(pal[i*3+1])
		db := int32(b) - int32(pal[i*3+2])
		d := dr*dr + dg*dg + db*db
		if d < bestD {
			bestD = d
			best = uint8(i)
			if d == 0 {
				break
			}
		}
	}
	return best
}

func parseMem(h string) (a, b, c, d uint8) {
	bb := func(i int) uint8 {
		if len(h) < i+2 {
			return 0
		}
		v, _ := strconv.ParseUint(h[i:i+2], 16, 8)
		return uint8(v)
	}
	return bb(0), bb(2), bb(4), bb(6)
}

// writeTrace parses the MAME trace log ("R" + one register column per
// cpuField) into a native .morepork. The RESULT bytes are placed on the
// final (verdict) entry.
func writeTrace(outPath string, sys *sysDef, spec, romSha, logPath string, res, code, obs, exp uint8, fr *frameData) error {
	lf, err := os.Open(logPath)
	if err != nil {
		return err
	}
	defer lf.Close()

	ncols := len(sys.cpuFields)
	var entries [][]uint64
	sc := bufio.NewScanner(lf)
	sc.Buffer(make([]byte, 1<<20), 1<<20)
	for sc.Scan() {
		line := sc.Text()
		if len(line) == 0 || line[0] != 'R' {
			continue
		}
		f := strings.Fields(line)
		if len(f) != ncols {
			continue
		}
		hx := func(s string) uint64 { v, _ := strconv.ParseUint(s, 16, 32); return v }
		vals := make([]uint64, ncols)
		vals[0] = hx(f[0][1:])
		for i := 1; i < ncols; i++ {
			vals[i] = hx(f[i])
		}
		entries = append(entries, vals)
	}
	if len(entries) == 0 {
		return fmt.Errorf("no trace entries (empty MAME trace log)")
	}

	fields := make([]string, 0, ncols+4)
	for _, fs := range sys.cpuFields {
		fields = append(fields, fs.name)
	}
	fields = append(fields, "result", "code", "observed", "expected")
	header := map[string]any{
		"_header": true, "format_version": "0.1.0",
		"emulator": "mame", "emulator_version": "adapter", "rom_sha256": romSha,
		"system": sys.id, "model": spec, "profile": "tier1",
		"fields": fields, "trigger": "instruction",
	}
	if fr != nil {
		header["pix_format"] = "indexed8"
	}
	hj, _ := json.Marshal(header)
	cPath := C.CString(outPath)
	defer C.free(unsafe.Pointer(cPath))
	cHdr := C.CString(string(hj))
	defer C.free(unsafe.Pointer(cHdr))
	w := C.morepork_writer_new(cPath, cHdr, C.size_t(len(hj)))
	if w == nil {
		return fmt.Errorf("morepork_writer_new returned NULL")
	}
	col := map[string]C.int{}
	for _, n := range fields {
		cn := C.CString(n)
		col[n] = C.morepork_writer_find_field(w, cn)
		C.free(unsafe.Pointer(cn))
	}
	setU8 := func(n string, v uint8) { C.morepork_writer_set_u8(w, C.size_t(col[n]), C.uint8_t(v)) }
	setU16 := func(n string, v uint16) { C.morepork_writer_set_u16(w, C.size_t(col[n]), C.uint16_t(v)) }

	for i, e := range entries {
		for j, fs := range sys.cpuFields {
			if fs.u16 {
				setU16(fs.name, uint16(e[j]))
			} else {
				setU8(fs.name, uint8(e[j]))
			}
		}
		if i == len(entries)-1 { // RESULT verdict lands on the last entry
			setU8("result", res)
			setU8("code", code)
			setU8("observed", obs)
			setU8("expected", exp)
		} else {
			setU8("result", 0)
			setU8("code", 0)
			setU8("observed", 0)
			setU8("expected", 0)
		}
		C.morepork_writer_finish_entry(w)
	}
	if fr != nil && fr.width > 0 && fr.height > 0 && len(fr.pixels) > 0 && len(fr.palette) > 0 {
		C.morepork_writer_mark_frame_indexed(w,
			C.uint16_t(fr.width), C.uint16_t(fr.height), C.float(fr.aspect),
			(*C.uint8_t)(unsafe.Pointer(&fr.palette[0])), C.size_t(len(fr.palette)/3),
			(*C.uint8_t)(unsafe.Pointer(&fr.pixels[0])), C.size_t(len(fr.pixels)))
	}
	if C.morepork_writer_close(w) != 0 {
		return fmt.Errorf("writer close failed")
	}
	return nil
}
