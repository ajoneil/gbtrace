// gbtrace-gateboy: Adapter that uses GateBoy (from metroboy) to produce
// .gbtrace files.
//
// GateBoy is a gate-level accurate Game Boy simulation.  It runs at phase
// granularity (8 phases per T-cycle) but this adapter emits one trace entry
// per instruction boundary, matching the output format of the other gbtrace
// adapters.
//
// The DMG boot ROM is built into GateBoy; the adapter runs it automatically
// and begins tracing at PC=0x0100.
//
// Usage:
//   gbtrace-gateboy --rom test.gb --profile cpu_basic.toml [--output trace.gbtrace]
//
// Build:
//   See Makefile in this directory.

#include "GateBoyLib/GateBoy.h"
#include "metrolib/core/Blobs.h"
#include "gbtrace.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

// --- Field configuration ---

// Read a value from GateBoy state.
//
// For IO registers, we read the raw gate-level DFF state via bit_pack()
// (NOT bit_pack_inv — gb_state.peek() uses bit_pack_inv which gives
// inverted values for registers stored in inverting DFFs).
// For RAM regions (VRAM, OAM, HRAM, etc.), we use GateBoy::peek() which
// reads directly from memory arrays.
static uint8_t read_reg(const GateBoy& gb, unsigned short addr) {
    const auto& s = gb.gb_state;

    switch (addr) {
        case 0xFF40: return (uint8_t)bit_pack(s.reg_lcdc);
        case 0xFF41: {
            // STAT must be reconstructed from multiple state sources:
            //   bit 7: always 1 (unused, reads high)
            //   bits 6-3: interrupt enable DFFs (stored inverted in reg_stat)
            //   bit 2: LYC coincidence flag (from RUPO latch, inverted)
            //   bits 1-0: PPU mode (from rendering latch, vblank, scan state)
            uint8_t ly = (uint8_t)bit_pack(s.reg_ly);
            bool vblank = ly >= 144;
            bool rendering = !(s.XYMU_RENDERING_LATCHn.state & 1);
            bool scanning = s.ACYL_SCANNINGp_odd.state & 1;

            uint8_t mode;
            if (vblank) {
                mode = 1;  // vblank
            } else if (rendering) {
                mode = 3;  // pixel transfer
            } else if (scanning) {
                mode = 2;  // OAM scan
            } else {
                mode = 0;  // hblank
            }

            uint8_t lyc_match = s.int_ctrl.RUPO_LYC_MATCHn.state ? 0 : 1;
            uint8_t enables = bit_pack(s.reg_stat) & 0x0F;

            return 0x80 | (enables << 3) | (lyc_match << 2) | mode;
        }
        case 0xFF42: return (uint8_t)bit_pack(s.reg_scy);
        case 0xFF43: return (uint8_t)bit_pack(s.reg_scx);
        case 0xFF44: return (uint8_t)bit_pack(s.reg_ly);
        case 0xFF45: return (uint8_t)bit_pack(s.reg_lyc);
        case 0xFF46: return (uint8_t)bit_pack(s.reg_dma);
        case 0xFF47: return (uint8_t)bit_pack(s.reg_bgp);
        case 0xFF48: return (uint8_t)bit_pack(s.reg_obp0);
        case 0xFF49: return (uint8_t)bit_pack(s.reg_obp1);
        case 0xFF4A: return (uint8_t)bit_pack(s.reg_wy);
        case 0xFF4B: return (uint8_t)bit_pack(s.reg_wx);
        case 0xFF04: return (uint8_t)(bit_pack(s.reg_div) >> 6);
        case 0xFF05: return (uint8_t)bit_pack(s.reg_tima);
        case 0xFF06: return (uint8_t)bit_pack(s.reg_tma);
        case 0xFF07: return (uint8_t)bit_pack(s.reg_tac);
        case 0xFF0F: return (uint8_t)bit_pack(s.reg_if);
        case 0xFFFF: return (uint8_t)bit_pack(s.reg_ie);
        // SB (0xFF01) and SC (0xFF02): serial is not simulated in GateBoy.
        case 0xFF01: return 0;
        case 0xFF02: return 0;
    }

    // RAM regions: peek() reads from memory arrays directly.
    GBResult r = gb.peek(addr);
    return r.is_ok() ? r.unwrap() : 0;
}

// --- Profile ---

struct Profile {
    std::string name;
    std::string trigger;
    std::vector<std::string> fields; // ordered
    std::unordered_map<std::string, unsigned short> memory; // name -> address
};

static Profile parse_profile(const std::string &path) {
    Profile prof;
    prof.trigger = "instruction";

    std::ifstream f(path);
    if (!f.is_open()) {
        std::fprintf(stderr, "Error: cannot open profile '%s'\n", path.c_str());
        std::exit(1);
    }

    auto trim = [](std::string &s) {
        while (!s.empty() && std::isspace(s.front())) s.erase(0, 1);
        while (!s.empty() && std::isspace(s.back())) s.pop_back();
    };
    auto strip_quotes = [](std::string &s) {
        if (s.size() >= 2 && s.front() == '"' && s.back() == '"')
            s = s.substr(1, s.size() - 2);
    };

    std::string line;
    bool in_memory_section = false;
    while (std::getline(f, line)) {
        auto hash = line.find('#');
        if (hash != std::string::npos) line = line.substr(0, hash);
        trim(line);

        if (line.size() >= 2 && line.front() == '[') {
            in_memory_section = (line == "[fields.memory]");
            continue;
        }

        auto eq = line.find('=');
        if (eq == std::string::npos) continue;

        std::string key = line.substr(0, eq);
        std::string val = line.substr(eq + 1);
        trim(key); trim(val);

        if (in_memory_section) {
            strip_quotes(val);
            unsigned long addr = std::strtoul(val.c_str(), nullptr, 16);
            prof.memory[key] = static_cast<unsigned short>(addr);
            prof.fields.push_back(key);
        } else if (key == "name") {
            strip_quotes(val);
            prof.name = val;
        } else if (key == "trigger") {
            strip_quotes(val);
            prof.trigger = val;
        } else if (!val.empty() && val.front() == '[') {
            // Handle multi-line arrays: accumulate lines until ']'
            std::string array_str = val;
            while (array_str.find(']') == std::string::npos && std::getline(f, line)) {
                auto h = line.find('#');
                if (h != std::string::npos) line = line.substr(0, h);
                trim(line);
                array_str += " " + line;
            }
            auto start = array_str.find('[');
            auto end = array_str.find(']');
            if (start != std::string::npos && end != std::string::npos) {
                std::string inner = array_str.substr(start + 1, end - start - 1);
                std::istringstream ss(inner);
                std::string token;
                while (std::getline(ss, token, ',')) {
                    trim(token); strip_quotes(token);
                    if (!token.empty() && token != "cy") {
                        prof.fields.push_back(token);
                    }
                }
            }
        }
    }

    return prof;
}

// --- Emitter setup ---

struct FieldEmitter {
    std::string name;
    enum Source { CPU_REG8, CPU_REG16, CPU_IME, IO_READ, PIX, PPU_U8, PPU_U16, PPU_BOOL } source;
    unsigned short io_addr; // for IO_READ
    // For PPU_U8/PPU_U16/PPU_BOOL: function pointer to read the value
    uint8_t (*read_ppu_u8)(const GateBoy &gb);
    uint16_t (*read_ppu_u16)();
    bool (*read_ppu_bool)(const GateBoy &gb);
};

static std::vector<FieldEmitter> g_emitters;
static bool g_has_pix = false;
static bool g_has_vram = false;

// --- Parquet direct writer (FFI) ---
static GbtraceWriter *g_parquet = nullptr;
// Column indices into the parquet writer, parallel to g_emitters
static std::vector<int> g_parquet_cols;
static int g_parquet_ly_col = -1;
static int g_parquet_vram_addr_col = -1;
static int g_parquet_vram_data_col = -1;

// --- VRAM write tracking ---
static uint16_t g_vram_write_addr = 0;
static uint8_t g_vram_write_data = 0;

// --- Pixel capture ---
// Uses GateBoy's pixel_callback in update_framebuffer() to capture each
// pixel at the exact moment it's pushed to the LCD.
// The callback fires every phase; we deduplicate by position since
// pix_count only advances once per real pixel push.
static std::string g_pix_buf;

// Separate frame buffer for reference comparison.
// Accumulates pixels by (x, y) position so we can compare the complete
// frame against a .pix reference without reconstructing from the trace.
static const int FRAME_PIXELS = 160 * 144;  // 23040
static std::string g_frame_ref_buf(FRAME_PIXELS, '0');
static std::string g_reference_pix;  // loaded from --reference file

// Pixel capture: detect pix_count changes between phases.
// When pix_count increments by 1, the pixel at the OLD position
// has been shifted to the LCD and its framebuffer value is final.
static int g_prev_pix_count = -1;

static bool g_captured_last_pixel = false;
static uint64_t g_total_pix_captured = 0;  // total pixels ever captured
static uint16_t g_frame_num = 0;           // increments every 23040 pixels

static void collect_pixel(GateBoy &gb) {
    int pix_count = bit_pack(gb.gb_state.pix_count);
    int old_pix_count = g_prev_pix_count;
    g_prev_pix_count = pix_count;

    if (old_pix_count < 0) return;

    int lcd_y = bit_pack(gb.gb_state.reg_ly);
    if (lcd_y < 0 || lcd_y >= 144) return;

    // Normal pixel shift: pix_count incremented by 1
    if (pix_count == old_pix_count + 1) {
        int lcd_x = old_pix_count - 8;
        if (lcd_x >= 0 && lcd_x < 160) {
            uint8_t fb_val = gb.mem.framebuffer[lcd_x + lcd_y * 160];
            char shade = '0' + (fb_val & 3);
            g_pix_buf += shade;
            g_frame_ref_buf[lcd_x + lcd_y * 160] = shade;
            g_total_pix_captured++;
        }
        g_captured_last_pixel = false;
    }

    // Last pixel (x=159): pix_count is at 167 and we haven't captured it yet.
    // The framebuffer has been written by update_framebuffer() at this point.
    if (pix_count == 167 && !g_captured_last_pixel) {
        uint8_t fb_val = gb.mem.framebuffer[159 + lcd_y * 160];
        char shade = '0' + (fb_val & 3);
        g_pix_buf += shade;
        g_frame_ref_buf[159 + lcd_y * 160] = shade;
        g_captured_last_pixel = true;
        g_total_pix_captured++;
    }

}

static bool load_reference(const std::string &path) {
    std::ifstream f(path, std::ios::binary);
    if (!f.is_open()) return false;
    g_reference_pix.assign(std::istreambuf_iterator<char>(f),
                           std::istreambuf_iterator<char>());
    // Strip trailing newline if present
    while (!g_reference_pix.empty() &&
           (g_reference_pix.back() == '\n' || g_reference_pix.back() == '\r'))
        g_reference_pix.pop_back();
    if ((int)g_reference_pix.size() != FRAME_PIXELS) {
        std::fprintf(stderr, "Warning: reference has %zu pixels, expected %d\n",
                     g_reference_pix.size(), FRAME_PIXELS);
        return false;
    }
    return true;
}

static bool check_frame_matches_reference() {
    if (g_reference_pix.empty()) return false;
    return g_frame_ref_buf == g_reference_pix;
}

static uint8_t read_cpu_reg8(const CpuState &reg, const std::string &name) {
    if (name == "a") return reg.a;
    if (name == "f") return reg.f;
    if (name == "b") return reg.b;
    if (name == "c") return reg.c;
    if (name == "d") return reg.d;
    if (name == "e") return reg.e;
    if (name == "h") return reg.h;
    if (name == "l") return reg.l;
    return 0;
}

static uint16_t read_cpu_reg16(const CpuState &reg, const std::string &name) {
    if (name == "pc") return reg.op_addr;
    if (name == "sp") return reg.sp;
    return 0;
}

static const std::unordered_map<std::string, unsigned short> IO_FIELD_ADDR = {
    {"lcdc", 0xFF40}, {"stat", 0xFF41}, {"scy",  0xFF42}, {"scx",  0xFF43},
    {"ly",   0xFF44}, {"lyc",  0xFF45}, {"wy",   0xFF4A}, {"wx",   0xFF4B},
    {"bgp",  0xFF47}, {"obp0", 0xFF48}, {"obp1", 0xFF49}, {"dma",  0xFF46},
    {"div",  0xFF04}, {"tima", 0xFF05}, {"tma",  0xFF06}, {"tac",  0xFF07},
    {"if_",  0xFF0F}, {"ie",   0xFFFF},
    {"sb",   0xFF01}, {"sc",   0xFF02},
};

// --- PPU internal readers ---
// Each returns a u8 from gate-level state via bit_pack().

// Sprite store: 10 sprites, each with x (8-bit), id (6-bit index), attr (4-bit flags)
#define SPRITE_X(N) [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.store_x##N); }
#define SPRITE_ID(N) [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.store_i##N); }
#define SPRITE_ATTR(N) [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.store_l##N); }

static const std::unordered_map<std::string, uint8_t(*)(const GateBoy &)> PPU_U8_READERS = {
    // Sprite store
    {"oam0_x", SPRITE_X(0)}, {"oam0_id", SPRITE_ID(0)}, {"oam0_attr", SPRITE_ATTR(0)},
    {"oam1_x", SPRITE_X(1)}, {"oam1_id", SPRITE_ID(1)}, {"oam1_attr", SPRITE_ATTR(1)},
    {"oam2_x", SPRITE_X(2)}, {"oam2_id", SPRITE_ID(2)}, {"oam2_attr", SPRITE_ATTR(2)},
    {"oam3_x", SPRITE_X(3)}, {"oam3_id", SPRITE_ID(3)}, {"oam3_attr", SPRITE_ATTR(3)},
    {"oam4_x", SPRITE_X(4)}, {"oam4_id", SPRITE_ID(4)}, {"oam4_attr", SPRITE_ATTR(4)},
    {"oam5_x", SPRITE_X(5)}, {"oam5_id", SPRITE_ID(5)}, {"oam5_attr", SPRITE_ATTR(5)},
    {"oam6_x", SPRITE_X(6)}, {"oam6_id", SPRITE_ID(6)}, {"oam6_attr", SPRITE_ATTR(6)},
    {"oam7_x", SPRITE_X(7)}, {"oam7_id", SPRITE_ID(7)}, {"oam7_attr", SPRITE_ATTR(7)},
    {"oam8_x", SPRITE_X(8)}, {"oam8_id", SPRITE_ID(8)}, {"oam8_attr", SPRITE_ATTR(8)},
    {"oam9_x", SPRITE_X(9)}, {"oam9_id", SPRITE_ID(9)}, {"oam9_attr", SPRITE_ATTR(9)},
    // Pixel FIFO
    {"bgw_fifo_a", [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.bgw_pipe_a); }},
    {"bgw_fifo_b", [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.bgw_pipe_b); }},
    {"spr_fifo_a", [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.spr_pipe_a); }},
    {"spr_fifo_b", [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.spr_pipe_b); }},
    {"mask_pipe",  [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.mask_pipe); }},
    {"pal_pipe",   [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.pal_pipe); }},
    // Fetcher state
    {"tfetch_state", [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.tfetch_counter); }},
    {"sfetch_state", [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.sfetch_counter_evn); }},
    {"tile_temp_a",  [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.tile_temp_a); }},
    {"tile_temp_b",  [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.tile_temp_b); }},
    // Counters
    {"pix_count",    [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.pix_count); }},
    {"sprite_count", [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.sprite_counter); }},
    {"scan_count",   [](const GateBoy &gb) -> uint8_t { return (uint8_t)bit_pack(gb.gb_state.scan_counter); }},
};

// Special u16 readers (not gate-level state, adapter-maintained counters)
static const std::unordered_map<std::string, uint16_t(*)()> PPU_U16_READERS = {
    {"frame_num", []() -> uint16_t { return g_frame_num; }},
};

static const std::unordered_map<std::string, bool(*)(const GateBoy &)> PPU_BOOL_READERS = {
    {"rendering", [](const GateBoy &gb) -> bool { return !(gb.gb_state.XYMU_RENDERING_LATCHn.state & 1); }},
    {"win_mode",  [](const GateBoy &gb) -> bool { return gb.gb_state.win_ctrl.PYNU_WIN_MODE_LATCHp.state != 0; }},
};

static void build_emitters(const Profile &prof) {
    g_emitters.clear();
    for (const auto &field : prof.fields) {
        if (field == "cy") continue;

        FieldEmitter em;
        em.name = field;
        em.io_addr = 0;

        if (field == "sb" || field == "sc") {
            std::fprintf(stderr, "Note: skipping '%s' (serial not simulated in GateBoy)\n",
                         field.c_str());
            continue;
        } else if (field == "pix") {
            em.source = FieldEmitter::PIX;
            g_has_pix = true;
            g_emitters.push_back(em);
            continue;
        } else if (field == "ime") {
            em.source = FieldEmitter::CPU_IME;
        } else if (field == "pc" || field == "sp") {
            em.source = FieldEmitter::CPU_REG16;
        } else if (field == "a" || field == "f" || field == "b" || field == "c" ||
                   field == "d" || field == "e" || field == "h" || field == "l") {
            em.source = FieldEmitter::CPU_REG8;
        } else if (auto it = IO_FIELD_ADDR.find(field); it != IO_FIELD_ADDR.end()) {
            em.source = FieldEmitter::IO_READ;
            em.io_addr = it->second;
        } else if (auto it2 = prof.memory.find(field); it2 != prof.memory.end()) {
            em.source = FieldEmitter::IO_READ;
            em.io_addr = it2->second;
        } else if (auto it3 = PPU_U8_READERS.find(field); it3 != PPU_U8_READERS.end()) {
            em.source = FieldEmitter::PPU_U8;
            em.read_ppu_u8 = it3->second;
        } else if (auto it3b = PPU_U16_READERS.find(field); it3b != PPU_U16_READERS.end()) {
            em.source = FieldEmitter::PPU_U16;
            em.read_ppu_u16 = it3b->second;
        } else if (auto it4 = PPU_BOOL_READERS.find(field); it4 != PPU_BOOL_READERS.end()) {
            em.source = FieldEmitter::PPU_BOOL;
            em.read_ppu_bool = it4->second;
        } else if (field == "vram_addr" || field == "vram_data") {
            // Handled separately via g_parquet_vram_addr_col/g_parquet_vram_data_col
            g_has_vram = true;
            continue;
        } else {
            std::fprintf(stderr, "Warning: unknown field '%s' (len=%zu), skipping\n", field.c_str(), field.size());
            continue;
        }
        g_emitters.push_back(em);
    }
}

// --- SHA-256 ---

static std::string sha256_file(const std::string &path) {
    std::string cmd = "sha256sum \"" + path + "\"";
    FILE *pipe = popen(cmd.c_str(), "r");
    if (!pipe) return "unknown";
    char buf[128];
    std::string result;
    if (std::fgets(buf, sizeof(buf), pipe)) {
        result = buf;
        auto space = result.find(' ');
        if (space != std::string::npos)
            result = result.substr(0, space);
    }
    pclose(pipe);
    return result;
}

// --- Header ---

static void write_header(FILE *out, const Profile &prof,
                          const std::string &rom_sha256,
                          const std::string &boot_rom_info) {
    std::fprintf(out,
        "{\"_header\":true,\"format_version\":\"0.1.0\","
        "\"emulator\":\"gateboy\",\"emulator_version\":\"metroboy-git\","
        "\"rom_sha256\":\"%s\",\"model\":\"DMG\","
        "\"boot_rom\":\"%s\",\"profile\":\"%s\","
        "\"fields\":[",
        rom_sha256.c_str(), boot_rom_info.c_str(),
        prof.name.c_str());

    for (size_t i = 0; i < g_emitters.size(); i++) {
        if (i > 0) std::fprintf(out, ",");
        std::fprintf(out, "\"%s\"", g_emitters[i].name.c_str());
    }

    std::fprintf(out, "],\"trigger\":\"%s\"}\n",
                 prof.trigger.c_str());
}

// --- Stop conditions ---

struct StopCondition {
    unsigned short addr = 0;
    unsigned char value = 0;
};

static StopCondition parse_stop_when(const std::string &spec) {
    auto eq = spec.find('=');
    if (eq == std::string::npos) {
        std::fprintf(stderr, "Error: --stop-when format is ADDR=VAL (e.g. A000=80)\n");
        std::exit(1);
    }
    StopCondition cond;
    cond.addr = static_cast<unsigned short>(
        std::strtoul(spec.substr(0, eq).c_str(), nullptr, 16));
    cond.value = static_cast<unsigned char>(
        std::strtoul(spec.substr(eq + 1).c_str(), nullptr, 16));
    return cond;
}

// --- Emit one trace entry ---

static void emit_entry(FILE *out, GateBoy &gb) {
    const CpuState &reg = gb.cpu.core.reg;

    bool first = true;
    std::fprintf(out, "{");

    for (const auto &em : g_emitters) {
        if (!first) std::fprintf(out, ",");
        first = false;
        std::fprintf(out, "\"%s\":", em.name.c_str());
        switch (em.source) {
        case FieldEmitter::CPU_REG8:
            std::fprintf(out, "%d", read_cpu_reg8(reg, em.name));
            break;
        case FieldEmitter::CPU_REG16:
            std::fprintf(out, "%d", read_cpu_reg16(reg, em.name));
            break;
        case FieldEmitter::CPU_IME:
            std::fprintf(out, "%s", reg.ime ? "true" : "false");
            break;
        case FieldEmitter::IO_READ: {
            uint8_t val = read_reg(gb, em.io_addr);
            std::fprintf(out, "%d", val);
            break;
        }
        case FieldEmitter::PIX:
            std::fprintf(out, "\"%s\"", g_pix_buf.c_str());
            g_pix_buf.clear();
            break;
        case FieldEmitter::PPU_U8:
            std::fprintf(out, "%d", em.read_ppu_u8(gb));
            break;
        case FieldEmitter::PPU_U16:
            std::fprintf(out, "%d", em.read_ppu_u16());
            break;
        case FieldEmitter::PPU_BOOL:
            std::fprintf(out, "%s", em.read_ppu_bool(gb) ? "true" : "false");
            break;
        }
    }

    std::fprintf(out, "}\n");
}

static void emit_entry_parquet(GateBoy &gb) {
    const CpuState &reg = gb.cpu.core.reg;

    // Gather ly and pix_len for boundary check
    uint8_t ly_val = 255;
    size_t pix_len = 0;
    if (g_parquet_ly_col >= 0) {
        ly_val = (uint8_t)bit_pack(gb.gb_state.reg_ly);
    }
    if (g_has_pix) {
        pix_len = g_pix_buf.size();
    }
    gbtrace_writer_check_boundary(g_parquet, ly_val, pix_len);

    // Set all field values
    for (size_t i = 0; i < g_emitters.size(); i++) {
        int col = g_parquet_cols[i];
        if (col < 0) continue;
        const auto &em = g_emitters[i];
        switch (em.source) {
        case FieldEmitter::CPU_REG8:
            gbtrace_writer_set_u8(g_parquet, col, read_cpu_reg8(reg, em.name));
            break;
        case FieldEmitter::CPU_REG16:
            gbtrace_writer_set_u16(g_parquet, col, read_cpu_reg16(reg, em.name));
            break;
        case FieldEmitter::CPU_IME:
            gbtrace_writer_set_bool(g_parquet, col, reg.ime);
            break;
        case FieldEmitter::IO_READ:
            gbtrace_writer_set_u8(g_parquet, col, read_reg(gb, em.io_addr));
            break;
        case FieldEmitter::PIX:
            gbtrace_writer_set_str(g_parquet, col,
                                   g_pix_buf.c_str(), g_pix_buf.size());
            g_pix_buf.clear();
            break;
        case FieldEmitter::PPU_U8:
            gbtrace_writer_set_u8(g_parquet, col, em.read_ppu_u8(gb));
            break;
        case FieldEmitter::PPU_U16:
            gbtrace_writer_set_u16(g_parquet, col, em.read_ppu_u16());
            break;
        case FieldEmitter::PPU_BOOL:
            gbtrace_writer_set_bool(g_parquet, col, em.read_ppu_bool(gb));
            break;
        }
    }

    // VRAM write fields
    if (g_parquet_vram_addr_col >= 0) {
        gbtrace_writer_set_u16(g_parquet, g_parquet_vram_addr_col, g_vram_write_addr);
    }
    if (g_parquet_vram_data_col >= 0) {
        gbtrace_writer_set_u8(g_parquet, g_parquet_vram_data_col, g_vram_write_data);
    }
    g_vram_write_addr = 0;
    g_vram_write_data = 0;

    gbtrace_writer_finish_entry(g_parquet);
}

// --- Main ---

static void print_usage(const char *argv0) {
    std::fprintf(stderr,
        "Usage: %s --rom <file.gb> --profile <profile.toml> [options]\n"
        "\n"
        "Options:\n"
        "  --rom <path>            ROM file to run (required)\n"
        "  --profile <path>        Capture profile TOML file (required)\n"
        "  --output <path>         Output trace file (default: stdout)\n"
        "  --frames <n>            Stop after N frames (default: 3000)\n"
        "  --stop-when <A=V>       Stop when memory ADDR equals VAL (hex)\n"
        "  --stop-on-serial <B>    Stop when byte B (hex) is sent via serial\n"
        "  --stop-serial-count <N> Stop on Nth occurrence (default: 1)\n"
        "  --reference <path>      Stop when framebuffer matches .pix reference\n"
        "  --no-fastboot           Run the built-in boot ROM instead of fastbooting\n",
        argv0);
}

int main(int argc, char *argv[]) {
    std::string rom_path;
    std::string profile_path;
    std::string output_path;
    std::string reference_path;
    int max_frames = 3000;
    std::vector<StopCondition> stop_conditions;
    unsigned char stop_serial_byte = 0;
    int stop_serial_count = 1;
    bool stop_serial_active = false;
    int extra_frames = 0;
    int stop_opcode = -1;
    bool fastboot = true;

    for (int i = 1; i < argc; i++) {
        std::string arg = argv[i];
        if (arg == "--rom" && i + 1 < argc) {
            rom_path = argv[++i];
        } else if (arg == "--profile" && i + 1 < argc) {
            profile_path = argv[++i];
        } else if (arg == "--output" && i + 1 < argc) {
            output_path = argv[++i];
        } else if (arg == "--frames" && i + 1 < argc) {
            max_frames = std::atoi(argv[++i]);
        } else if (arg == "--stop-when" && i + 1 < argc) {
            stop_conditions.push_back(parse_stop_when(argv[++i]));
        } else if (arg == "--stop-on-serial" && i + 1 < argc) {
            stop_serial_byte = static_cast<unsigned char>(
                std::strtoul(argv[++i], nullptr, 16));
            stop_serial_active = true;
        } else if (arg == "--stop-serial-count" && i + 1 < argc) {
            stop_serial_count = std::atoi(argv[++i]);
        } else if (arg == "--reference" && i + 1 < argc) {
            reference_path = argv[++i];
        } else if (arg == "--extra-frames" && i + 1 < argc) {
            extra_frames = std::atoi(argv[++i]);
        } else if (arg == "--stop-opcode" && i + 1 < argc) {
            stop_opcode = static_cast<int>(std::strtoul(argv[++i], nullptr, 16));
        } else if (arg == "--no-fastboot") {
            fastboot = false;
        } else if (arg == "--help" || arg == "-h") {
            print_usage(argv[0]);
            return 0;
        }
    }

    if (rom_path.empty() || profile_path.empty()) {
        print_usage(argv[0]);
        return 1;
    }

    // Load profile
    Profile profile = parse_profile(profile_path);
    build_emitters(profile);

    std::fprintf(stderr, "Profile: %s (%zu fields)\n",
                 profile.name.c_str(), profile.fields.size());

    // Load reference image if provided
    bool has_reference = false;
    if (!reference_path.empty()) {
        if (load_reference(reference_path)) {
            has_reference = true;
            std::fprintf(stderr, "Reference: %s (%d pixels)\n",
                         reference_path.c_str(), FRAME_PIXELS);
        } else {
            std::fprintf(stderr, "Warning: could not load reference '%s'\n",
                         reference_path.c_str());
        }
    }

    // Detect parquet output mode from file extension
    bool parquet_mode = false;
    if (output_path.size() >= 8 &&
        output_path.substr(output_path.size() - 8) == ".parquet") {
        parquet_mode = true;
    }

    // Open output (JSONL mode only; parquet mode opens via FFI after header is built)
    FILE *output = nullptr;
    if (!parquet_mode) {
        if (output_path.empty() || output_path == "-") {
            output = stdout;
        } else {
            output = std::fopen(output_path.c_str(), "w");
            if (!output) {
                std::fprintf(stderr, "Error: cannot open %s for writing\n",
                             output_path.c_str());
                return 1;
            }
        }
        static char output_buf[64 * 1024];
        std::setvbuf(output, output_buf, _IOFBF, sizeof(output_buf));
    }

    // Load ROM into a blob
    blob cart_blob;
    if (!load_blob(rom_path.c_str(), cart_blob)) {
        std::fprintf(stderr, "Error: cannot load ROM '%s'\n", rom_path.c_str());
        return 1;
    }

    // Initialize GateBoy
    GateBoy gb;
    std::string boot_rom_info;

    if (fastboot) {
        gb.reset();
        boot_rom_info = "skip";
    } else {
        // Run the built-in boot ROM to completion (PC reaches 0x0100).
        // The DMG boot ROM scrolls the Nintendo logo which takes several
        // frames (~1M+ T-cycles). Budget generously.
        gb.poweron(false);

        static constexpr int PHASES_PER_TCYCLE_BOOT = 2;
        static constexpr int64_t MAX_BOOT_PHASES = 20000000;  // ~10M T-cycles

        std::fprintf(stderr, "Running boot ROM...\n");
        bool boot_complete = false;
        for (int64_t i = 0; i < MAX_BOOT_PHASES; i++) {
            gb.next_phase(cart_blob);
            if ((i % PHASES_PER_TCYCLE_BOOT) == (PHASES_PER_TCYCLE_BOOT - 1)) {
                uint16_t pc = gb.cpu.core.reg.op_addr;
                if (pc == 0x0100) {
                    std::fprintf(stderr, "Boot ROM complete at phase %lld (%lld T-cycles)\n",
                                 (long long)(i + 1), (long long)((i + 1) / 2));
                    boot_complete = true;
                    break;
                }
            }
        }

        if (!boot_complete) {
            std::fprintf(stderr, "Error: boot ROM did not reach PC=0x0100 within %lld phases.\n"
                                 "Does the ROM have a valid Nintendo logo?\n",
                         (long long)MAX_BOOT_PHASES);
            return 1;
        }

        boot_rom_info = "built-in";
    }

    // Write header / init parquet writer
    std::string rom_hash = sha256_file(rom_path);

    if (parquet_mode) {
        // Build header JSON for the FFI writer
        // Build complete field list including fields handled separately
        std::vector<std::string> all_fields;
        for (const auto &em : g_emitters) all_fields.push_back(em.name);
        if (g_has_vram) {
            all_fields.push_back("vram_addr");
            all_fields.push_back("vram_data");
        }

        std::string header_json = "{\"_header\":true,\"format_version\":\"0.1.0\","
            "\"emulator\":\"gateboy\",\"emulator_version\":\"metroboy-git\","
            "\"rom_sha256\":\"" + rom_hash + "\",\"model\":\"DMG\","
            "\"boot_rom\":\"" + boot_rom_info + "\",\"profile\":\"" + profile.name + "\","
            "\"fields\":[";
        for (size_t i = 0; i < all_fields.size(); i++) {
            if (i > 0) header_json += ",";
            header_json += "\"" + all_fields[i] + "\"";
        }
        header_json += "],\"trigger\":\"" + profile.trigger + "\"}";

        g_parquet = gbtrace_writer_new(
            output_path.c_str(), header_json.c_str(), header_json.size());
        if (!g_parquet) {
            std::fprintf(stderr, "Error: failed to create parquet writer\n");
            return 1;
        }

        // Cache column indices
        g_parquet_cols.resize(g_emitters.size());
        for (size_t i = 0; i < g_emitters.size(); i++) {
            g_parquet_cols[i] = gbtrace_writer_find_field(
                g_parquet, g_emitters[i].name.c_str());
        }
        g_parquet_ly_col = gbtrace_writer_find_field(g_parquet, "ly");
        g_parquet_vram_addr_col = gbtrace_writer_find_field(g_parquet, "vram_addr");
        g_parquet_vram_data_col = gbtrace_writer_find_field(g_parquet, "vram_data");
        g_has_vram = (g_parquet_vram_addr_col >= 0);

        // Mark entry 0 as a frame boundary
        gbtrace_writer_mark_frame(g_parquet);

        std::fprintf(stderr, "Output: parquet (direct write)\n");
    } else {
        write_header(output, profile, rom_hash, boot_rom_info.c_str());
    }

    // Print stop conditions
    for (const auto &cond : stop_conditions) {
        std::fprintf(stderr, "Stop condition: [0x%04X] == 0x%02X\n",
                     cond.addr, cond.value);
    }
    if (stop_serial_active) {
        std::fprintf(stderr, "Stop on serial byte: 0x%02X (after %d occurrence%s)\n",
                     stop_serial_byte, stop_serial_count,
                     stop_serial_count == 1 ? "" : "s");
    }

    // Run simulation
    //
    // GateBoy runs at phase granularity (8 phases per T-cycle).
    // Emission mode depends on profile trigger:
    //   "tcycle"      — emit every T-cycle (every 8 phases)
    //   "instruction" — emit at instruction boundaries

    static constexpr int PHASES_PER_FRAME = 70224 * 2;  // 140448 phases (2 phases per T-cycle)
    static constexpr int PHASES_PER_TCYCLE = 2;  // 8 phases per M-cycle, 2 phases per T-cycle
    int64_t total_phases = static_cast<int64_t>(max_frames) * PHASES_PER_FRAME;

    bool tcycle_mode = (profile.trigger == "tcycle");


    uint16_t prev_op_addr = gb.cpu.core.reg.op_addr;
    int prev_op_state = gb.cpu.core.reg.op_state;
    bool stopped_early = false;
    bool stop_triggered = false;
    int remaining_extra = -1;  // -1 = not triggered yet
    int stop_serial_seen = 0;
    bool prev_sc_high = false;
    int frames = 0;
    int64_t phase_count = 0;
    g_total_pix_captured = 0;
    g_frame_num = 0;

    while (phase_count < total_phases) {
        gb.next_phase(cart_blob);
        phase_count++;

        // Collect pixel output from this phase (if pix_count incremented)
        if (g_has_pix) {
            collect_pixel(gb);
        }

        // Detect VRAM writes from bus signals (check at T-cycle boundary)
        if (g_has_vram && (phase_count % PHASES_PER_TCYCLE) == 0) {
            const auto &s = gb.gb_state;
            // APOV_CPU_WRp is high when the CPU is writing
            if (s.cpu_signals.APOV_CPU_WRp.state & BIT_DATA) {
                uint16_t addr = (uint16_t)bit_pack(s.cpu_abus);
                if (addr >= 0x8000 && addr <= 0x9FFF) {
                    g_vram_write_addr = addr;
                    g_vram_write_data = (uint8_t)bit_pack(s.cpu_dbus);
                }
            }
        }

        const CpuState &reg = gb.cpu.core.reg;

        bool should_emit;
        if (tcycle_mode) {
            should_emit = (phase_count % PHASES_PER_TCYCLE) == 0;
        } else {
            should_emit = (reg.op_state == 0 && prev_op_state != 0)
                       || (reg.op_state == 0 && reg.op_addr != prev_op_addr);
        }

        // Detect instruction boundaries (regardless of emit mode)
        bool at_instr_boundary = (reg.op_state == 0 && prev_op_state != 0)
                              || (reg.op_state == 0 && reg.op_addr != prev_op_addr);

        if (should_emit) {
            if (g_parquet) {
                emit_entry_parquet(gb);
            } else {
                emit_entry(output, gb);
            }
        }

        // Check stop conditions at instruction boundaries only
        if (at_instr_boundary && !stop_triggered) {
                // Check stop-when conditions
                for (const auto &cond : stop_conditions) {
                    uint8_t val = read_reg(gb, cond.addr);
                    if (val == cond.value) {
                        std::fprintf(stderr, "Stop condition met at frame %d, running %d extra frame%s\n",
                                     frames, extra_frames, extra_frames == 1 ? "" : "s");
                        stop_triggered = true;
                        remaining_extra = extra_frames;
                        break;
                    }
                }

                // Check opcode stop condition
                // Read from cart_blob for ROM addresses to avoid peek() errors
                if (!stop_triggered && stop_opcode >= 0) {
                    uint16_t addr = reg.op_addr;
                    int opval = -1;
                    if (addr < (int)cart_blob.size()) {
                        opval = cart_blob.data()[addr];
                    } else {
                        GBResult r = gb.peek(addr);
                        if (r.is_ok()) opval = r.unwrap();
                    }
                    if (opval == stop_opcode) {
                        std::fprintf(stderr, "Opcode stop at frame %d, running %d extra frame%s\n",
                                     frames, extra_frames, extra_frames == 1 ? "" : "s");
                        stop_triggered = true;
                        remaining_extra = extra_frames;
                    }
                }

                // Check serial stop condition
                if (!stop_triggered && stop_serial_active) {
                    uint8_t sc_val = read_reg(gb, 0xFF02);
                    bool sc_high = (sc_val & 0x80) != 0;
                    if (sc_high && !prev_sc_high) {
                        uint8_t sb_val = read_reg(gb, 0xFF01);
                        if (sb_val == stop_serial_byte) {
                            stop_serial_seen++;
                            if (stop_serial_seen >= stop_serial_count) {
                                std::fprintf(stderr, "Serial stop at frame %d, running %d extra frame%s\n",
                                             frames, extra_frames, extra_frames == 1 ? "" : "s");
                                stop_triggered = true;
                                remaining_extra = extra_frames;
                            }
                        }
                    }
                    prev_sc_high = sc_high;
                }
        }

        prev_op_state = reg.op_state;
        prev_op_addr = reg.op_addr;

        // Detect LCD frame boundary from MEDA_VSYNC_OUTn data bit.
        // Bit 0 goes high at vsync start (scanline 144 — last visible pixel done).
        // We mark the frame at the RISING edge (vsync starting) so the boundary
        // sits between the last pixel of scanline 143 and the vblank period.
        // The next frame's first pixel (scanline 0) comes after vblank ends.
        {
            bool vsync = gb.gb_state.lcd.MEDA_VSYNC_OUTn.state & 1;
            static bool prev_vsync = false;
            if (!prev_vsync && vsync) {
                // VSYNC started — previous frame is complete
                g_frame_num++;
                if (g_parquet) {
                    gbtrace_writer_mark_frame(g_parquet);
                }
                // If a stop was requested, the LCD frame is now complete
                if (stop_triggered) {
                    stopped_early = true;
                    break;
                }
            }
            prev_vsync = vsync;
        }

        // Track frame boundaries for --frames limit
        if ((phase_count % PHASES_PER_FRAME) == 0) {
            frames++;

            // Check reference match at phase-counter boundary.
            // Don't stop immediately — continue until the next VSYNC
            // so the last LCD frame is complete.
            if (has_reference && !stop_triggered && check_frame_matches_reference()) {
                std::fprintf(stderr, "Reference match at frame %d, finishing LCD frame...\n", frames);
                stop_triggered = true;
                // remaining_extra not used here — we stop at next VSYNC instead
            }

            // Extra-frames countdown at frame boundaries
            if (remaining_extra >= 0) {
                if (remaining_extra == 0) {
                    stopped_early = true;
                    break;
                }
                remaining_extra--;
            }
        }
    }

    if (g_parquet) {
        gbtrace_writer_close(g_parquet);
        g_parquet = nullptr;
    } else {
        std::fflush(output);
        if (output != stdout) {
            std::fclose(output);
        }
    }

    if (stopped_early) {
        std::fprintf(stderr, "Stop condition met at frame %d, output written.\n", frames);
    } else {
        std::fprintf(stderr, "Traced %d frames, output written.\n", frames);
    }


    return 0;
}
