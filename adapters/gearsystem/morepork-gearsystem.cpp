// morepork-gearsystem: a morepork adapter for the Gearsystem emulator in
// SG-1000 mode, a second trace oracle for the TI VDP test suite's `.sg`
// builds alongside MAME's sg1000 driver.
//
// Same embedding shape as the gearcoleco adapter (the cores share an
// author and API family — so this is a semi-independent lineage: a
// different VDP implementation, but related craftsmanship; MAME remains
// the fully-independent vote). Gearsystem's Video keeps the TMS-era
// debug state private, so a small checked-in patch
// (gearsystem-trace-api.patch) ports Gearcoleco's own upstream accessors
// across: GetStatusReg (side-effect-free), GetBufferReg, GetAddressReg,
// GetLatch, GetRenderLine, GetCycleCounter.
//
// The adapter drives Processor::RunInstruction directly (mirroring
// RunToVBlank's tick loop — Gearsystem ticks video+audio only), logs one
// entry per instruction with the RESULT block sampled live, and copies
// the frame straight from the framebuffer, which holds raw TMS colour
// indices in TMS9918 modes. No BIOS: SG-1000 carts boot at $0000.
//
//   morepork-gearsystem -rom test.sg -out trace.morepork

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "GearsystemCore.h"
#include "Processor.h"
#include "Video.h"
#include "Memory.h"
#include "Audio.h"
#include "Cartridge.h"

#include "morepork.h"

// The core's logger routes through this frontend-owned flag (part of the
// Gear-family MCP debug plumbing); we are the frontend.
bool g_mcp_stdio_mode = false;

// RESULT convention (missingno-ti-vdp-tests include/result.inc, sg1000 shim).
static const uint16_t kResultAddr = 0xC000;

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
  h += "\"emulator\":\"gearsystem\",";
  h += "\"emulator_version\":\"" GEARSYSTEM_PIN "\",";
  h += "\"rom_sha256\":\"" + romSha + "\",";
  h += "\"system\":\"sg1000\",";
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

int main(int argc, char** argv) {
  const char* rom = nullptr;
  const char* out = "trace.morepork";
  std::string spec = "NTSC";
  int maxFrames = 30;
  bool wantFrame = true;
  for (int i = 1; i < argc; i++) {
    std::string a = argv[i];
    auto next = [&]() { return (i + 1 < argc) ? argv[++i] : ""; };
    if (a == "-rom") rom = next();
    else if (a == "-out") out = next();
    else if (a == "-spec") spec = next();
    else if (a == "-frames") maxFrames = std::atoi(next());
    else if (a == "-frame") wantFrame = true;
    else if (a == "-frame=false" || a == "-frame=0") wantFrame = false;
    else if (a == "-frame=true" || a == "-frame=1") wantFrame = true;
    else {
      std::fprintf(stderr,
                   "usage: morepork-gearsystem -rom test.sg"
                   " [-out trace.morepork] [-spec NTSC] [-frames N] [-frame=false]\n");
      return 2;
    }
  }
  if (!rom) {
    std::fprintf(stderr, "error: -rom is required\n");
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

  GearsystemCore core;
  core.Init(GS_PIXEL_RGBA8888);
  if (!core.LoadROM(rom)) {
    std::fprintf(stderr, "error: failed to load ROM %s\n", rom);
    return 1;
  }
  if (!core.GetCartridge()->IsSG1000()) {
    std::fprintf(stderr, "error: %s did not detect as an SG-1000 cartridge\n", rom);
    return 1;
  }
  Memory* mem = core.GetMemory();
  Processor* proc = core.GetProcessor();
  Video* video = core.GetVideo();
  Audio* audio = core.GetAudio();
  Processor::ProcessorState* st = proc->GetState();

  std::string header = jsonHeader(spec, romId(romBytes), wantFrame);
  MoreporkWriter* w = morepork_writer_new(out, header.c_str(), header.size());
  if (!w) { std::fprintf(stderr, "error: writer_new failed\n"); return 1; }
  int col[kNumFields];
  for (size_t i = 0; i < kNumFields; i++) col[i] = morepork_writer_find_field(w, kFields[i]);
  size_t c = 0;
  auto u8f = [&](uint8_t v) { morepork_writer_set_u8(w, col[c++], v); };
  auto u16f = [&](uint16_t v) { morepork_writer_set_u16(w, col[c++], v); };
  auto boolf = [&](bool v) { morepork_writer_set_bool(w, col[c++], v); };

  // Drive the core's own tick loop (RunToVBlank's body — video and audio
  // only in Gearsystem) one instruction at a time.
  const int capFrames = std::max(2, maxFrames / 60) * 60;
  const long capInstructions = 200000000L;
  std::vector<int16_t> sampleBuffer(GS_AUDIO_BUFFER_SIZE);
  int sampleCount = 0;
  int framesDone = 0;
  bool verdict = false;
  for (long n = 0; n < capInstructions; n++) {
    uint8_t result = mem->Read(kResultAddr);
    uint8_t* vr = video->GetRegisters();
    c = 0;
    u16f(st->PC->GetValue());
    u16f(st->SP->GetValue());
    uint16_t af = st->AF->GetValue(), bc = st->BC->GetValue();
    uint16_t de = st->DE->GetValue(), hl = st->HL->GetValue();
    u8f(af >> 8); u8f(af & 0xFF);
    u8f(bc >> 8); u8f(bc & 0xFF);
    u8f(de >> 8); u8f(de & 0xFF);
    u8f(hl >> 8); u8f(hl & 0xFF);
    u16f(st->IX->GetValue());
    u16f(st->IY->GetValue());
    u16f(st->WZ->GetValue());
    uint16_t af2 = st->AF2->GetValue(), bc2 = st->BC2->GetValue();
    uint16_t de2 = st->DE2->GetValue(), hl2 = st->HL2->GetValue();
    u8f(af2 >> 8); u8f(af2 & 0xFF);
    u8f(bc2 >> 8); u8f(bc2 & 0xFF);
    u8f(de2 >> 8); u8f(de2 & 0xFF);
    u8f(hl2 >> 8); u8f(hl2 & 0xFF);
    u8f(*st->I);
    u8f(*st->R);
    u8f((uint8_t)*st->InterruptMode);
    boolf(*st->IFF1);
    boolf(*st->IFF2);
    boolf(*st->Halt);
    for (int i = 0; i < 8; i++) u8f(vr[i]);
    u8f(video->GetStatusReg());
    u16f(video->GetAddressReg());
    // Catalogue semantics: latch set after the first control byte;
    // GetLatch() returns m_bFirstByteInSequence, the inverse.
    boolf(!video->GetLatch());
    u8f(video->GetBufferReg());
    u16f((uint16_t)video->GetRenderLine());
    u16f((uint16_t)video->GetCycleCounter());
    u8f(result);
    u8f(mem->Read(kResultAddr + 1));
    u8f(mem->Read(kResultAddr + 2));
    u8f(mem->Read(kResultAddr + 3));
    morepork_writer_finish_entry(w);

    if (result == 0xA5 || result == 0x5A) { verdict = true; break; }
    if (framesDone >= capFrames) break;

    unsigned int clocks = proc->RunInstruction();
    if (video->Tick(clocks)) {
      framesDone++;
      audio->EndFrame(sampleBuffer.data(), &sampleCount);
    }
    audio->Tick(clocks);
  }
  if (!verdict)
    std::fprintf(stderr, "warning: no verdict within %d frames; trace ends at the budget\n", capFrames);

  if (wantFrame) {
    // Let the readout render, then grab the colour-index framebuffer.
    for (int extra = 0; extra < 30;) {
      unsigned int clocks = proc->RunInstruction();
      if (video->Tick(clocks)) {
        extra++;
        audio->EndFrame(sampleBuffer.data(), &sampleCount);
      }
      audio->Tick(clocks);
    }
    if (!video->IsSG1000Mode()) {
      std::fprintf(stderr, "warning: VDP left TMS9918 mode; skipping frame snapshot\n");
    } else {
      uint16_t* fb = video->GetFrameBuffer();
      std::vector<uint8_t> pixels(GS_RESOLUTION_SMS_WIDTH * GS_RESOLUTION_SMS_HEIGHT);
      for (size_t i = 0; i < pixels.size(); i++) pixels[i] = (uint8_t)(fb[i] & 0x0F);
      morepork_writer_mark_frame_indexed(
          w, GS_RESOLUTION_SMS_WIDTH, GS_RESOLUTION_SMS_HEIGHT, 8.0f / 7.0f,
          kTiVdpPalette, 16, pixels.data(), pixels.size());
    }
  }

  if (morepork_writer_close(w) != 0) {
    std::fprintf(stderr, "error: writer close failed\n");
    return 1;
  }
  return 0;
}
