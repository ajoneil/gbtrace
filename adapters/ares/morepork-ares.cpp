// morepork-ares: a morepork adapter embedding ares as an independent-lineage
// trace oracle for the TI VDP test suite — the ColecoVision core today, with
// the SG-1000 and MSX cores staged to follow (all three link into this
// binary's build).
//
// ares embeds as a static library (cmake target `ares` + `mia` for pak
// construction) with a thin ares::Platform frontend. No source patches:
// the CPU's per-instruction debugger hook routes tracer notifications to
// Platform::log, and enabling the instruction tracer (`setTerminal(true)`)
// gives us a synchronous callback at every instruction boundary, on the
// CPU's own cothread — where we ignore the disassembly text and read the
// core's public state directly (`ares::ColecoVision::cpu` is a Z80 with
// public registers; `vdp` is a TMS9918 with public io/vram). The eight
// write registers and status are reconstructed from ares' decomposed io
// fields exactly as `TMS9918::register`/status read decode them.
//
// The frame comes from Platform::video, reverse-mapped against a palette
// built at runtime by calling the core's own colour pipeline (vdp.color),
// so no calibration table can drift.
//
//   morepork-ares -system coleco -rom test.col -bios colecovision.rom -out trace.morepork

// The TMS9918's io/dac/sprite/background state is protected in ares; the
// adapter only reads it for tracing. The access-specifier override is
// confined to this TU and does not change struct layout on the compilers
// we build with — it keeps the embed patch-free.
#define protected public
#include <ares/ares.hpp>
#include <cv/cv.hpp>
#undef protected
#include <mia/mia.hpp>

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>
#include <unordered_map>

#include "morepork.h"

// RESULT convention (missingno-ti-vdp-tests include/result.inc, coleco shim).
static const uint16_t kResultAddr = 0x7000;

// Canonical TI VDP palette stamped into frame snapshots (matches the other
// TI VDP adapters): comparisons happen in colour-index space.
static const uint8_t kTiVdpPalette[16 * 3] = {
    0, 0, 0,       0, 0, 0,       33, 200, 66,   94, 220, 120,
    84, 85, 237,   125, 118, 252, 212, 82, 77,   66, 235, 245,
    252, 85, 84,   255, 121, 120, 212, 193, 84,  230, 206, 128,
    33, 176, 59,   201, 91, 186,  204, 204, 204, 255, 255, 255,
};

static const char* kFields[] = {
    "pc", "sp", "a", "f", "b", "c", "d", "e", "h", "l", "ix", "iy", "wz",
    "a_", "f_", "b_", "c_", "d_", "e_", "h_", "l_", "i", "r",
    "im", "iff1", "iff2", "halted",
    "reg0", "reg1", "reg2", "reg3", "reg4", "reg5", "reg6", "reg7",
    "status", "addr", "latch", "buffer", "line", "dot",
    "result", "code", "observed", "expected",
};
static const size_t kNumFields = sizeof(kFields) / sizeof(kFields[0]);

static std::string jsonHeader(const std::string& spec, const std::string& romSha,
                              bool withFrame) {
  std::string h = "{";
  h += "\"_header\":true,";
  h += "\"format_version\":\"0.1.0\",";
  h += "\"emulator\":\"ares\",";
  h += "\"emulator_version\":\"" ARES_PIN "\",";
  h += "\"rom_sha256\":\"" + romSha + "\",";
  h += "\"system\":\"coleco\",";
  h += "\"model\":\"" + spec + "\",";
  h += "\"profile\":\"tier1\",";
  h += "\"fields\":[";
  for (size_t i = 0; i < kNumFields; i++) {
    if (i) h += ",";
    h += std::string("\"") + kFields[i] + "\"";
  }
  h += "],";
  if (withFrame) h += "\"pix_format\":\"indexed8\",";
  h += "\"trigger\":\"instruction\"";
  h += "}";
  return h;
}

// FNV-1a ROM id, hex-encoded — the stella-adapter convention when no
// crypto library is linked.
static std::string romId(const std::vector<uint8_t>& rom) {
  uint64_t h = 1469598103934665603ULL;
  for (uint8_t b : rom) { h ^= b; h *= 1099511628211ULL; }
  char buf[17];
  std::snprintf(buf, sizeof(buf), "%016llx", (unsigned long long)h);
  return std::string(buf);
}

// --- capture state shared with the platform callbacks ---

struct Capture {
  MoreporkWriter* writer = nullptr;
  int col[64];
  long instructions = 0;
  long capInstructions = 200000000L;
  bool verdict = false;
  bool tracing = true;
  // last frame delivered by Platform::video
  std::vector<uint32_t> frame;
  uint32_t frameWidth = 0, frameHeight = 0;
} cap;

using namespace ares::ColecoVision;

static uint8_t resultByte(int offset) {
  // CV RAM is 1KB at $6000-$7FFF, mirrored; $7000 maps to offset 0.
  return cpu.ram[(kResultAddr + offset) & 0x3FF];
}

// Reconstruct the eight TMS9918 write registers from ares' decomposed io
// fields (the exact inverse of TMS9918::register in io.cpp) and the status
// byte from the flag state.
static void vdpRegisters(uint8_t out[8]) {
  out[0] = (uint8_t)vdp.dac.io.externalSync | ((uint8_t)vdp.io.videoMode.bit(2) << 1);
  out[1] = (uint8_t)vdp.sprite.io.zoom
         | ((uint8_t)vdp.sprite.io.size << 1)
         | ((uint8_t)vdp.io.videoMode.bit(1) << 3)
         | ((uint8_t)vdp.io.videoMode.bit(0) << 4)
         | ((uint8_t)vdp.irqFrame.enable << 5)
         | ((uint8_t)vdp.dac.io.displayEnable << 6)
         | ((uint8_t)vdp.io.vramMode << 7);
  out[2] = (uint8_t)vdp.background.io.nameTableAddress;
  out[3] = (uint8_t)vdp.background.io.colorTableAddress;
  out[4] = (uint8_t)vdp.background.io.patternTableAddress;
  out[5] = (uint8_t)vdp.sprite.io.attributeTableAddress;
  out[6] = (uint8_t)vdp.sprite.io.patternTableAddress;
  out[7] = ((uint8_t)vdp.dac.io.colorForeground << 4) | (uint8_t)vdp.dac.io.colorBackground;
}

static uint8_t vdpStatus() {
  return ((uint8_t)vdp.irqFrame.pending << 7)
       | ((uint8_t)vdp.sprite.io.overflow << 6)
       | ((uint8_t)vdp.sprite.io.collision << 5)
       | (uint8_t)vdp.sprite.io.overflowIndex;
}

static void logInstruction() {
  if (!cap.tracing || cap.verdict) return;
  if (cap.instructions++ > cap.capInstructions) { cap.tracing = false; return; }
  MoreporkWriter* w = cap.writer;
  size_t c = 0;
  auto u8f = [&](uint8_t v) { morepork_writer_set_u8(w, cap.col[c++], v); };
  auto u16f = [&](uint16_t v) { morepork_writer_set_u16(w, cap.col[c++], v); };
  auto boolf = [&](bool v) { morepork_writer_set_bool(w, cap.col[c++], v); };

  u16f(cpu.PC);
  u16f(cpu.SP);
  u8f(cpu.af.byte.hi); u8f(cpu.af.byte.lo);
  u8f(cpu.bc.byte.hi); u8f(cpu.bc.byte.lo);
  u8f(cpu.de.byte.hi); u8f(cpu.de.byte.lo);
  u8f(cpu.hl.byte.hi); u8f(cpu.hl.byte.lo);
  u16f(cpu.ix.word);
  u16f(cpu.iy.word);
  u16f(cpu.wz.word);
  u8f(cpu.af_.byte.hi); u8f(cpu.af_.byte.lo);
  u8f(cpu.bc_.byte.hi); u8f(cpu.bc_.byte.lo);
  u8f(cpu.de_.byte.hi); u8f(cpu.de_.byte.lo);
  u8f(cpu.hl_.byte.hi); u8f(cpu.hl_.byte.lo);
  u8f(cpu.ir.byte.hi);
  u8f(cpu.ir.byte.lo);
  u8f((uint8_t)cpu.IM);
  boolf((bool)cpu.IFF1);
  boolf((bool)cpu.IFF2);
  boolf((bool)cpu.HALT);
  uint8_t regs[8];
  vdpRegisters(regs);
  for (int i = 0; i < 8; i++) u8f(regs[i]);
  u8f(vdpStatus());
  // controlValue is the live address pointer; controlLatch is the
  // write-phase flag (set after the first control byte).
  u16f((uint16_t)(vdp.io.controlValue & 0x3FFF));
  boolf((bool)vdp.io.controlLatch);
  u8f((uint8_t)vdp.io.vramLatch);
  u16f((uint16_t)vdp.io.vcounter);
  u16f((uint16_t)vdp.io.hcounter);
  uint8_t result = resultByte(0);
  u8f(result);
  u8f(resultByte(1));
  u8f(resultByte(2));
  u8f(resultByte(3));
  morepork_writer_finish_entry(w);

  if (result == 0xA5 || result == 0x5A) cap.verdict = true;
}

// --- ares platform ---

struct MoreporkPlatform : ares::Platform {
  std::shared_ptr<mia::Pak> gamePak;
  std::shared_ptr<mia::Pak> systemPak;

  auto pak(ares::Node::Object node) -> std::shared_ptr<vfs::directory> override {
    if (node->name() == "ColecoVision") return systemPak->pak;
    if (node->name() == "ColecoVision Cartridge") return gamePak->pak;
    return {};
  }

  auto log(ares::Node::Debugger::Tracer::Tracer, nall::string_view) -> void override {
    logInstruction();
  }

  auto video(ares::Node::Video::Screen, const u32* data, u32 pitch, u32 width, u32 height) -> void override {
    cap.frameWidth = width;
    cap.frameHeight = height;
    cap.frame.resize((size_t)width * height);
    for (u32 y = 0; y < height; y++)
      std::memcpy(cap.frame.data() + (size_t)y * width,
                  (const u8*)data + (size_t)y * pitch, width * sizeof(u32));
  }
};

int main(int argc, char** argv) {
  const char* rom = nullptr;
  const char* bios = nullptr;
  const char* out = "trace.morepork";
  std::string spec = "NTSC";
  std::string system = "coleco";
  int maxFrames = 30;
  bool wantFrame = true;
  for (int i = 1; i < argc; i++) {
    std::string a = argv[i];
    auto next = [&]() { return (i + 1 < argc) ? argv[++i] : ""; };
    if (a == "-rom") rom = next();
    else if (a == "-bios") bios = next();
    else if (a == "-out") out = next();
    else if (a == "-spec") spec = next();
    else if (a == "-system") system = next();
    else if (a == "-frames") maxFrames = std::atoi(next());
    else if (a == "-frame") wantFrame = true;
    else if (a == "-frame=false" || a == "-frame=0") wantFrame = false;
    else if (a == "-frame=true" || a == "-frame=1") wantFrame = true;
    else {
      std::fprintf(stderr,
                   "usage: morepork-ares -system coleco -rom test.col -bios colecovision.rom"
                   " [-out trace.morepork] [-spec NTSC] [-frames N] [-frame=false]\n");
      return 2;
    }
  }
  if (!rom || !bios) {
    std::fprintf(stderr, "error: -rom and -bios are required\n");
    return 2;
  }
  if (system != "coleco") {
    std::fprintf(stderr, "error: -system %s not yet wired (coleco only; sg1000/msx1 staged)\n", system.c_str());
    return 2;
  }
  if (spec != "NTSC") {
    std::fprintf(stderr, "error: -spec %s is not supported (NTSC only)\n", spec.c_str());
    return 1;
  }

  std::vector<uint8_t> romBytes;
  {
    FILE* f = std::fopen(rom, "rb");
    if (!f) { std::fprintf(stderr, "error: cannot open %s\n", rom); return 1; }
    std::fseek(f, 0, SEEK_END);
    long size = std::ftell(f);
    std::fseek(f, 0, SEEK_SET);
    romBytes.resize(size);
    if (std::fread(romBytes.data(), 1, size, f) != (size_t)size) {
      std::fclose(f);
      std::fprintf(stderr, "error: short read on %s\n", rom);
      return 1;
    }
    std::fclose(f);
  }

  static MoreporkPlatform platform;
  ares::platform = &platform;

  platform.gamePak = mia::Medium::create("ColecoVision");
  if (platform.gamePak->load(nall::string{rom}) != successful) {
    std::fprintf(stderr, "error: mia failed to load ROM %s\n", rom);
    return 1;
  }
  platform.systemPak = mia::System::create("ColecoVision");
  if (platform.systemPak->load(nall::string{bios}) != successful) {
    std::fprintf(stderr, "error: mia failed to load BIOS %s\n", bios);
    return 1;
  }

  ares::Node::System root;
  if (!ares::ColecoVision::load(root, "[Coleco] ColecoVision (NTSC)")) {
    std::fprintf(stderr, "error: ares failed to load the ColecoVision system\n");
    return 1;
  }
  if (auto port = root->find<ares::Node::Port>("Cartridge Slot")) {
    port->allocate();
    port->connect();
  }

  // Enable the CPU's per-instruction tracer; notifications land in
  // Platform::log, where the trace entries are written.
  bool tracerFound = false;
  for (auto tracer : ares::Node::enumerate<ares::Node::Debugger::Tracer::Instruction>(root)) {
    if (tracer->component() == "CPU") {
      tracer->setTerminal(true);
      tracerFound = true;
    }
  }
  if (!tracerFound) {
    std::fprintf(stderr, "error: no CPU instruction tracer found in the node tree\n");
    return 1;
  }

  std::string header = jsonHeader(spec, romId(romBytes), wantFrame);
  cap.writer = morepork_writer_new(out, header.c_str(), header.size());
  if (!cap.writer) { std::fprintf(stderr, "error: writer_new failed\n"); return 1; }
  for (size_t i = 0; i < kNumFields; i++)
    cap.col[i] = morepork_writer_find_field(cap.writer, kFields[i]);

  root->power(false);

  const int capFrames = std::max(2, maxFrames / 60) * 60;
  for (int frames = 0; frames < capFrames && !cap.verdict; frames++) root->run();
  if (!cap.verdict)
    std::fprintf(stderr, "warning: no verdict within %d frames; trace ends at the budget\n", capFrames);

  if (wantFrame) {
    // Let the readout render, tracing off.
    cap.tracing = false;
    for (int extra = 0; extra < 30; extra++) root->run();
    if (cap.frameWidth > 0) {
      // Build the exact reverse map from the core's own colour pipeline:
      // Screen palette entries are vdp.color() n64 RGB48 collapsed to RGB24.
      std::unordered_map<uint32_t, uint8_t> exact;
      for (uint32_t i = 0; i < 16; i++) {
        n64 c = vdp.color(i);
        uint32_t key = (uint32_t)(c >> 40 & 0xFF) << 16
                     | (uint32_t)(c >> 24 & 0xFF) << 8
                     | (uint32_t)(c >> 8 & 0xFF);
        if (!exact.count(key)) exact[key] = (uint8_t)i;
      }
      const uint32_t w = cap.frameWidth, h = cap.frameHeight;
      const uint32_t x0 = w > 256 ? (w - 256) / 2 : 0;
      const uint32_t y0 = h > 192 ? (h - 192) / 2 : 0;
      std::vector<uint8_t> pixels(256 * 192, 0);
      bool ok = w >= 256 && h >= 192;
      if (ok) {
        for (uint32_t y = 0; y < 192; y++) {
          for (uint32_t x = 0; x < 256; x++) {
            uint32_t rgb = cap.frame[(size_t)(y0 + y) * w + x0 + x] & 0xFFFFFF;
            auto hit = exact.find(rgb);
            uint8_t idx = 1;
            if (hit != exact.end()) {
              idx = hit->second;
            } else {
              // nearest, colours 1-15
              int best = INT32_MAX;
              for (uint32_t i = 1; i < 16; i++) {
                n64 cc = vdp.color(i);
                int dr = (int)(rgb >> 16 & 0xFF) - (int)(cc >> 40 & 0xFF);
                int dg = (int)(rgb >> 8 & 0xFF) - (int)(cc >> 24 & 0xFF);
                int db = (int)(rgb & 0xFF) - (int)(cc >> 8 & 0xFF);
                int d = dr * dr + dg * dg + db * db;
                if (d < best) { best = d; idx = (uint8_t)i; }
              }
            }
            pixels[y * 256 + x] = idx;
          }
        }
        morepork_writer_mark_frame_indexed(
            cap.writer, 256, 192, 8.0f / 7.0f,
            kTiVdpPalette, 16, pixels.data(), pixels.size());
      } else {
        std::fprintf(stderr, "warning: unexpected frame geometry %ux%u; skipping frame\n", w, h);
      }
    } else {
      std::fprintf(stderr, "warning: no frame delivered; skipping frame snapshot\n");
    }
  }

  if (morepork_writer_close(cap.writer) != 0) {
    std::fprintf(stderr, "error: writer close failed\n");
    return 1;
  }
  return 0;
}
