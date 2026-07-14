// morepork-stella: a morepork adapter for the Stella emulator (VCS family).
//
// Drives Stella's emucore headlessly one CPU instruction at a time (via the
// libretro OSystem for setup, but the emucore directly for stepping) and writes
// a native .morepork: per-instruction 6507 registers, TIA beam position, and the
// test-suite RESULT convention RAM bytes. An independent per-instruction oracle
// alongside the Gopher2600 adapter.
//
//   morepork-stella -rom test.bin -out trace.morepork -spec NTSC -frames 30

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>
#include <sys/stat.h>
#include <unistd.h>

#include "StellaLIBRETRO.hxx"
#include "SettingsLIBRETRO.hxx"
#include "Console.hxx"
#include "System.hxx"
#include "M6502.hxx"
#include "TIA.hxx"
#include "M6532.hxx"
#include "Switches.hxx"
#include "libretro.h"

#include "morepork.h"
#include "ntsc_palette.h"   // canonical VCS NTSC palette (see genpalette.py)

// --- minimal libretro glue referenced by the Stella core ---
// We drive the emucore directly (no libretro frontend). FSNodeLIBRETRO needs a
// VFS with a working stat() to recognise the ROM as a file; we provide a real
// filesystem stat, and the base FSNode then reads the .bin from disk normally.
static int vfs_stat(const char* path, int32_t* size) {
  struct stat st;
  if (::stat(path, &st) != 0) return 0;
  if (size) *size = (int32_t)st.st_size;
  int flags = RETRO_VFS_STAT_IS_VALID;
  if (S_ISDIR(st.st_mode)) flags |= RETRO_VFS_STAT_IS_DIRECTORY;
  return flags;
}
static retro_vfs_interface make_vfs() {
  retro_vfs_interface v{};
  v.stat = vfs_stat;
  return v;
}
static retro_vfs_interface g_vfs = make_vfs();
retro_vfs_interface* libretro_vfs = &g_vfs;  // NOLINT
string libretro_rom_path;                     // NOLINT
string libretro_save_dir;                     // NOLINT
static const uint8_t* g_rom = nullptr;
static uInt32 g_romSize = 0;
uInt32 libretro_get_rom_size() { return g_romSize; }
uInt32 libretro_read_rom(void* data) {
  if (!g_rom) return 0;
  std::memcpy(data, g_rom, g_romSize);
  return g_romSize;
}
void libretro_logger(int, const char*) {}
void post_message(const char*, retro_log_level, unsigned) {}
void libretro_show_message(const char*) {}
void update_input() {}

static std::string jsonHeader(const std::string& spec, const std::string& romSha,
                              bool withFrame) {
  // fields: pc a x y s p line clock result code observed expected
  std::string h = "{";
  h += "\"_header\":true,";
  h += "\"format_version\":\"0.1.0\",";
  h += "\"emulator\":\"stella\",";
  h += "\"emulator_version\":\"adapter-mvp\",";
  h += "\"rom_sha256\":\"" + romSha + "\",";
  h += "\"system\":\"vcs\",";
  h += "\"model\":\"" + spec + "\",";
  h += "\"profile\":\"tier1\",";
  h += "\"fields\":[\"pc\",\"a\",\"x\",\"y\",\"s\",\"p\",\"line\",\"clock\","
       "\"result\",\"code\",\"observed\",\"expected\"],";
  if (withFrame) h += "\"pix_format\":\"indexed8\",";
  h += "\"trigger\":\"instruction\"";
  h += "}";
  return h;
}

// tiny hex sha256-less id: we don't have a crypto lib linked, so hash the ROM
// with a simple FNV-1a and hex-encode. (Adapters only need a stable id here.)
static std::string romId(const std::vector<uint8_t>& rom) {
  uint64_t h = 1469598103934665603ULL;
  for (uint8_t b : rom) { h ^= b; h *= 1099511628211ULL; }
  char buf[17];
  std::snprintf(buf, sizeof(buf), "%016llx", (unsigned long long)h);
  return std::string(buf);
}

int main(int argc, char** argv) {
  const char* rom = nullptr;
  const char* out = "trace.morepork";
  std::string spec = "NTSC";
  int maxFrames = 30;
  int swchb = 0x48;   // bit3=colour, bit6=P0 diff-A, bit7=P1 diff-A
  bool wantFrame = true;   // embed a final frame snapshot (GOLD); -frame=false to skip
  int holdReset = 0;  // hold SWCHB reset (bit0) low for the first N frames
  int stopFrame = 0;  // stop after N completed frames (0 = use verdict/budget)
  int watchPc = -1;   // report first frame the CPU PC hits this address (game mode)
  std::string cartType;  // force cartridge bankswitch type (empty = autodetect)
  for (int i = 1; i < argc; i++) {
    std::string a = argv[i];
    auto next = [&]() { return (i + 1 < argc) ? argv[++i] : ""; };
    if (a == "-rom") rom = next();
    else if (a == "-out") out = next();
    else if (a == "-spec") spec = next();
    else if (a == "-type") cartType = next();
    else if (a == "-frames") maxFrames = std::atoi(next());
    else if (a == "-swchb") swchb = (int)std::strtol(next(), nullptr, 0);
    else if (a == "-holdreset") holdReset = std::atoi(next());
    else if (a == "-stopframe") stopFrame = std::atoi(next());
    else if (a == "-watchpc") watchPc = (int)std::strtol(next(), nullptr, 0);
    else if (a == "-frame") wantFrame = true;
    else if (a == "-frame=false" || a == "-frame=0") wantFrame = false;
    else if (a == "-frame=true" || a == "-frame=1") wantFrame = true;
  }
  // A game (stopframe/holdreset) has no RESULT verdict; don't stop on RAM $80.
  const bool gameMode = (stopFrame > 0) || (holdReset > 0);
  if (!rom) { std::fprintf(stderr, "error: -rom is required\n"); return 2; }

  FILE* f = std::fopen(rom, "rb");
  if (!f) { std::fprintf(stderr, "error: cannot open %s\n", rom); return 1; }
  std::fseek(f, 0, SEEK_END);
  long sz = std::ftell(f);
  std::fseek(f, 0, SEEK_SET);
  std::vector<uint8_t> data(sz);
  if (std::fread(data.data(), 1, sz, f) != (size_t)sz) { std::fclose(f); return 1; }
  std::fclose(f);

  // --- headless Stella setup ---
  g_rom = data.data();
  g_romSize = (uInt32)data.size();
  // Force the bankswitch type via the filename-extension mechanism. Stella's
  // create() (StellaLIBRETRO::create) is monolithic — it builds the OSystem and
  // creates the console in one call — so there is no seam to inject a
  // settings().setValue("type", ...) before console creation from the adapter.
  // CartCreator, however, honours a valid mapper-ID file extension over
  // autodetect (Bankswitch::typeFromExtension). Stella reads the image from the
  // real file on disk (the in-memory g_rom is only a fallback for paths that
  // don't stat), so we copy the ROM to a temp file whose extension is the type
  // and point Stella at that. Deleted right after console creation.
  std::string romPath = rom;
  std::string tmpDir, tmpRom;
  if (!cartType.empty()) {
    char tmpl[] = "/tmp/morepork-stella-XXXXXX";
    if (const char* d = mkdtemp(tmpl)) {
      tmpDir = d;
      tmpRom = tmpDir + "/rom." + cartType;
      if (FILE* tf = std::fopen(tmpRom.c_str(), "wb")) {
        std::fwrite(data.data(), 1, data.size(), tf);
        std::fclose(tf);
        romPath = tmpRom;
      }
    }
  }
  libretro_rom_path = romPath;
  StellaLIBRETRO stella;
  stella.setROM(romPath.c_str(), data.data(), (uInt32)data.size());
  SettingsLIBRETRO cfg;
  cfg.console_format = spec;   // NTSC / PAL / PAL60 / SECAM / AUTO
  if (!stella.create(cfg, false)) {
    std::fprintf(stderr, "error: Stella create() failed\n");
    return 1;
  }
  // Console has been created; the forced-type temp file has served its purpose.
  if (!tmpRom.empty()) { ::unlink(tmpRom.c_str()); ::rmdir(tmpDir.c_str()); }

  Console& console = stella.osystem().console();

  // Report the resolved cartridge mapping (the detection linter consumes this).
  // Cartridge::name() is the class name ("CartridgeF8", "Cartridge2K", ...); the
  // ID is that with the "Cartridge" prefix stripped ("F8", "2K", "F8SC", ...).
  {
    std::string cn = console.cartridge().name();
    const std::string pfx = "Cartridge";
    if (cn.rfind(pfx, 0) == 0) cn = cn.substr(pfx.size());
    std::fprintf(stderr, "cartridge-type: %s\n", cn.c_str());
  }

  // Set the console panel switches to a known state (latching colour and
  // difficulty switches) so SWCHB reads are deterministic.
  Switches& sw = console.switches();
  sw.setTvColor((swchb & 0x08) != 0);
  sw.setLeftDifficultyA((swchb & 0x40) != 0);
  sw.setRightDifficultyA((swchb & 0x80) != 0);

  System& system = console.system();
  M6502& cpu = system.m6502();
  TIA& tia = console.tia();
  const uInt8* ram = system.m6532().getRAM().data();

  // --- morepork writer ---
  std::string header = jsonHeader(spec, romId(data), wantFrame);
  MoreporkWriter* w = morepork_writer_new(out, header.c_str(), header.size());
  if (!w) { std::fprintf(stderr, "error: morepork_writer_new failed\n"); return 1; }

  auto col = [&](const char* n) {
    int c = morepork_writer_find_field(w, n);
    if (c < 0) { std::fprintf(stderr, "error: field %s missing\n", n); std::exit(1); }
    return (size_t)c;
  };
  size_t cPC = col("pc"), cA = col("a"), cX = col("x"), cY = col("y"),
         cS = col("s"), cP = col("p"), cLine = col("line"), cClk = col("clock"),
         cRes = col("result"), cCode = col("code"), cObs = col("observed"),
         cExp = col("expected");

  // frameCount() is behind DEBUGGER_SUPPORT (absent in the libretro build), so
  // bound the run by an instruction budget (~30k instr/frame upper bound). The
  // real stop is the RESULT verdict.
  const long instrBudget = (long)maxFrames * 30000;
  long instrCount = 0;
  // Frame accounting via scanline wrap. A completed frame is when the TIA's
  // running scanline count drops (VSYNC restarts the field).
  int frameCount = 0;
  int prevLine = 0;
  bool resetHeld = holdReset > 0;
  if (resetHeld) sw.setReset(true);   // push RESET before the first frame
  int watchPcFrame = -1;              // first frame PC hit watchPc
  for (;;) {
    cpu.execute(1);              // step one instruction (advances TIA/RIOT)

    if (watchPc >= 0 && watchPcFrame < 0 && (int)cpu.gbPC() == watchPc)
      watchPcFrame = frameCount;

    morepork_writer_set_u16(w, cPC, cpu.gbPC());
    morepork_writer_set_u8(w, cA, cpu.gbA());
    morepork_writer_set_u8(w, cX, cpu.gbX());
    morepork_writer_set_u8(w, cY, cpu.gbY());
    morepork_writer_set_u8(w, cS, cpu.gbSP());
    morepork_writer_set_u8(w, cP, cpu.gbPS());
    morepork_writer_set_u16(w, cLine, (uint16_t)tia.scanlines());
    morepork_writer_set_u8(w, cClk, (uint8_t)tia.clocksThisLine());
    morepork_writer_set_u8(w, cRes, ram[0x00]);  // $80 RESULT
    morepork_writer_set_u8(w, cCode, ram[0x01]); // $81 CODE
    morepork_writer_set_u8(w, cObs, ram[0x02]);  // $82 OBSERVED
    morepork_writer_set_u8(w, cExp, ram[0x03]);  // $83 EXPECTED
    morepork_writer_finish_entry(w);

    int line = (int)tia.scanlines();
    if (line < prevLine) {           // scanline count dropped -> new frame
      ++frameCount;
      if (resetHeld && frameCount >= holdReset) {
        sw.setReset(false);          // release RESET
        resetHeld = false;
      }
      if (stopFrame > 0 && frameCount >= stopFrame) break;
    }
    prevLine = line;

    if (++instrCount >= instrBudget) break;
    if (!gameMode) {
      uInt8 r = ram[0x00];
      if (r == 0xA5 || r == 0x5A) break;  // terminal verdict
    }
  }
  if (watchPc >= 0)
    std::fprintf(stderr, "watchpc $%04X: %s (frame %d), frames run=%d\n",
                 watchPc, watchPcFrame >= 0 ? "REACHED" : "not reached",
                 watchPcFrame, frameCount);

  // --- final frame snapshot (GOLD modality) ---
  // Stella's TIA framebuffer stores raw TIA colour codes (the COLUxx byte),
  // exactly like the Gopher2600 adapter's pixels, so we embed them directly and
  // pair them with the SUITE's canonical NTSC palette for an oracle-independent
  // golden PNG. Width is fixed 160 (H_PIXEL); height is the last full frame's
  // rendered scanline count.
  if (wantFrame) {
    // We drive the CPU directly, so the frontend's frame-render step never
    // runs; publish the latest completed frame (front buffer) into myFramebuffer
    // ourselves. onFrameComplete() fills the front buffer during stepping.
    tia.renderToFrameBuffer();
    const uInt8* fb = tia.frameBuffer();
    uInt16 width = 160;
    uInt16 height = (uInt16)tia.frameBufferScanlinesLastFrame();
    if (fb && height > 0) {
      // Roll so row 0 = the top of the field (VSYNC start). Stella's buffer is
      // YStart-centred: framebuffer row 0 sits startLine() scanlines below the
      // field top, so rolling up by startLine() restores the field origin. This
      // makes the full field comparable across oracles.
      int anchorRow = ((int)height - (int)tia.startLine()) % (int)height;
      std::vector<uint8_t> rolled((size_t)width * height);
      int a = anchorRow % height; if (a < 0) a += height;
      for (int r = 0; r < height; r++) {
        int src = (a + r) % height;
        std::memcpy(&rolled[(size_t)r * width], fb + (size_t)src * width, width);
      }
      const uint8_t* pal = (spec == "SECAM")           ? canonicalSECAMPalette
                         : (spec.rfind("PAL", 0) == 0) ? canonicalPALPalette
                                                       : canonicalNTSCPalette;
      morepork_writer_mark_frame_indexed(w, width, height, 12.0f / 7.0f,
          pal, 256, rolled.data(), (size_t)width * (size_t)height);
    }
  }

  if (morepork_writer_close(w) != 0) {
    std::fprintf(stderr, "error: writer close failed\n");
    return 1;
  }
  return 0;
}
