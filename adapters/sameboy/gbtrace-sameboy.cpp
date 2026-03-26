// gbtrace-sameboy: Adapter that uses SameBoy to produce .gbtrace files.
//
// Links against libsameboy without any source modifications.
// Uses the public GB_set_execution_callback API to capture per-instruction
// CPU state, and GB_safe_read_memory (peek) for IO registers.
//
// Usage:
//   gbtrace-sameboy --rom test.gb --profile cpu_basic.toml --output trace.gbtrace
//
// Build:
//   See Makefile in this directory.

// Include C++ headers first to avoid conflicts with SameBoy's `internal` macro
// (defs.h redefines `internal` as a visibility attribute, which clashes with
// std::ios_base::internal). Also, debugger.h uses `new` as a parameter name.
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <unistd.h>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

#include "gbtrace.h"

// We define GB_INTERNAL to get full struct access (ime, cycles_since_run, etc.)
#define GB_INTERNAL
// Avoid C++ keyword conflict in debugger.h
#define new new_value

extern "C" {
#include <gb.h>
#include <memory.h>
}

#undef new

// --- Field configuration ---

// Map of field name -> IO register address for fields read via GB_safe_read_memory.
static const std::unordered_map<std::string, unsigned short> IO_FIELD_ADDR = {
    {"lcdc", 0xFF40}, {"stat", 0xFF41}, {"scy",  0xFF42}, {"scx",  0xFF43},
    {"ly",   0xFF44}, {"lyc",  0xFF45}, {"wy",   0xFF4A}, {"wx",   0xFF4B},
    {"bgp",  0xFF47}, {"obp0", 0xFF48}, {"obp1", 0xFF49}, {"dma",  0xFF46},
    {"div",  0xFF04}, {"tima", 0xFF05}, {"tma",  0xFF06}, {"tac",  0xFF07},
    {"if_",  0xFF0F}, {"ie",   0xFFFF},
    {"sb",   0xFF01}, {"sc",   0xFF02},
};

// CPU register fields: maps field name -> register enum + is_16bit.
struct RegisterField {
    enum Reg { AF, BC, DE, HL, SP, PC,
               A, F, B, C, D, E, H, L };
    Reg reg;
    bool is_16bit;
};

static const std::unordered_map<std::string, RegisterField> REGISTER_FIELDS = {
    {"pc", {RegisterField::PC, true}},  {"sp", {RegisterField::SP, true}},
    {"a",  {RegisterField::A, false}},  {"f",  {RegisterField::F, false}},
    {"b",  {RegisterField::B, false}},  {"c",  {RegisterField::C, false}},
    {"d",  {RegisterField::D, false}},  {"e",  {RegisterField::E, false}},
    {"h",  {RegisterField::H, false}},  {"l",  {RegisterField::L, false}},
};

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

    // Minimal TOML parser — enough for our profile format.
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

// --- Globals for trace callback context ---

static GB_gameboy_t *g_gb = nullptr;
static Profile g_profile;
static uint64_t g_total_8mhz_ticks = 0; // needed for frame timing

static unsigned char g_stop_serial_byte = 0;
static int g_stop_serial_count = 1;
static int g_stop_serial_seen = 0;
static bool g_stop_serial_active = false;
static bool g_stop_serial_triggered = false;
static int g_stop_opcode = -1;
static bool g_stop_opcode_triggered = false;

// --- FFI writer ---
static GbtraceWriter *g_writer = nullptr;
static std::vector<int> g_writer_cols;
static int g_writer_ly_col = -1;

// Pre-computed list of what to emit per entry.
struct FieldEmitter {
    std::string name;
    enum Source { REGISTER_8, REGISTER_16, IO_READ, IME, PIX } source;
    RegisterField::Reg reg; // for REGISTER_8/16
    unsigned short io_addr; // for IO_READ
};
static std::vector<FieldEmitter> g_emitters;
static bool g_has_pix = false;
static uint32_t g_pixel_buf[160 * 144];
static std::string g_pending_pix;

static inline char rgba_to_shade(uint32_t rgba) {
    unsigned r = (rgba >> 0) & 0xFF;
    if (r >= 0xC0) return '0';
    if (r >= 0x70) return '1';
    if (r >= 0x30) return '2';
    return '3';
}

static void capture_sameboy_frame() {
    g_pending_pix.clear();
    g_pending_pix.reserve(160 * 144);
    for (int i = 0; i < 160 * 144; i++) {
        g_pending_pix += rgba_to_shade(g_pixel_buf[i]);
    }
}

// --- Reference matching ---
static std::string g_reference_pix;

static bool load_reference(const std::string &path) {
    std::ifstream f(path, std::ios::binary);
    if (!f.is_open()) return false;
    g_reference_pix.assign(std::istreambuf_iterator<char>(f),
                           std::istreambuf_iterator<char>());
    while (!g_reference_pix.empty() &&
           (g_reference_pix.back() == '\n' || g_reference_pix.back() == '\r'))
        g_reference_pix.pop_back();
    return (int)g_reference_pix.size() == 160 * 144;
}

static void build_emitters(const Profile &prof) {
    g_emitters.clear();
    for (const auto &field : prof.fields) {
        if (field == "cy") continue;

        FieldEmitter em;
        em.name = field;

        if (field == "pix") {
            em.source = FieldEmitter::PIX;
            g_has_pix = true;
            g_emitters.push_back(em);
            continue;
        } else if (field == "ime") {
            em.source = FieldEmitter::IME;
        } else if (auto it = REGISTER_FIELDS.find(field); it != REGISTER_FIELDS.end()) {
            em.source = it->second.is_16bit ? FieldEmitter::REGISTER_16 : FieldEmitter::REGISTER_8;
            em.reg = it->second.reg;
        } else if (auto it2 = IO_FIELD_ADDR.find(field); it2 != IO_FIELD_ADDR.end()) {
            em.source = FieldEmitter::IO_READ;
            em.io_addr = it2->second;
        } else if (auto it3 = prof.memory.find(field); it3 != prof.memory.end()) {
            em.source = FieldEmitter::IO_READ;
            em.io_addr = it3->second;
        } else {
            std::fprintf(stderr, "Warning: unknown field '%s', skipping\n", field.c_str());
            continue;
        }
        g_emitters.push_back(em);
    }
}

// Read a register value from the emulator.
static inline int read_reg(GB_gameboy_t *gb, RegisterField::Reg reg) {
    GB_registers_t *regs = GB_get_registers(gb);
    switch (reg) {
    case RegisterField::AF: return regs->af;
    case RegisterField::BC: return regs->bc;
    case RegisterField::DE: return regs->de;
    case RegisterField::HL: return regs->hl;
    case RegisterField::SP: return regs->sp;
    case RegisterField::PC: return regs->pc;
    case RegisterField::A:  return regs->a;
    case RegisterField::F:  return regs->f;
    case RegisterField::B:  return regs->b;
    case RegisterField::C:  return regs->c;
    case RegisterField::D:  return regs->d;
    case RegisterField::E:  return regs->e;
    case RegisterField::H:  return regs->h;
    case RegisterField::L:  return regs->l;
    }
    return 0;
}

// --- Emit entry ---

static void emit_entry(GB_gameboy_t *gb, uint16_t address) {
    // Gather ly and pix_len for boundary check
    uint8_t ly_val = 255;
    size_t pix_len = 0;
    if (g_writer_ly_col >= 0) {
        ly_val = GB_safe_read_memory(gb, 0xFF44);
    }
    if (g_has_pix) {
        pix_len = g_pending_pix.size();
    }
    gbtrace_writer_check_boundary(g_writer, ly_val, pix_len);

    // Set all field values
    for (size_t i = 0; i < g_emitters.size(); i++) {
        int col = g_writer_cols[i];
        if (col < 0) continue;
        const auto &em = g_emitters[i];
        switch (em.source) {
        case FieldEmitter::REGISTER_8:
            gbtrace_writer_set_u8(g_writer, col, read_reg(gb, em.reg));
            break;
        case FieldEmitter::REGISTER_16:
            if (em.reg == RegisterField::PC)
                gbtrace_writer_set_u16(g_writer, col, address);
            else
                gbtrace_writer_set_u16(g_writer, col, read_reg(gb, em.reg));
            break;
        case FieldEmitter::IO_READ:
            gbtrace_writer_set_u8(g_writer, col, GB_safe_read_memory(gb, em.io_addr));
            break;
        case FieldEmitter::IME:
            gbtrace_writer_set_bool(g_writer, col, gb->ime);
            break;
        case FieldEmitter::PIX:
            gbtrace_writer_set_str(g_writer, col,
                                   g_pending_pix.c_str(), g_pending_pix.size());
            g_pending_pix.clear();
            break;
        }
    }

    gbtrace_writer_finish_entry(g_writer);
}

// --- Trace callback ---

static void exec_callback(GB_gameboy_t *gb, uint16_t address, uint8_t opcode) {
    emit_entry(gb, address);

    // Check opcode stop condition
    if (g_stop_opcode >= 0 && !g_stop_opcode_triggered) {
        if (opcode == static_cast<uint8_t>(g_stop_opcode)) {
            g_stop_opcode_triggered = true;
        }
    }

    // Check serial stop condition: detect rising edge of SC bit 7
    if (g_stop_serial_active && !g_stop_serial_triggered) {
        static bool prev_sc_high = false;
        unsigned char sc = GB_safe_read_memory(gb, 0xFF02);
        bool sc_high = (sc & 0x80) != 0;
        if (sc_high && !prev_sc_high) {
            unsigned char sb = GB_safe_read_memory(gb, 0xFF01);
            if (sb == g_stop_serial_byte) {
                g_stop_serial_seen++;
                if (g_stop_serial_seen >= g_stop_serial_count) {
                    g_stop_serial_triggered = true;
                }
            }
        }
        prev_sc_high = sc_high;
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

// --- Stop condition ---

struct StopCondition {
    unsigned short addr;
    unsigned char value;
    bool active = false;
};

static StopCondition parse_stop_when(const std::string &spec) {
    auto eq = spec.find('=');
    if (eq == std::string::npos) {
        std::fprintf(stderr, "Error: --stop-when format is ADDR=VAL (e.g. A000=80)\n");
        std::exit(1);
    }
    StopCondition cond;
    cond.addr = static_cast<unsigned short>(std::strtoul(spec.substr(0, eq).c_str(), nullptr, 16));
    cond.value = static_cast<unsigned char>(std::strtoul(spec.substr(eq + 1).c_str(), nullptr, 16));
    cond.active = true;
    return cond;
}

// --- Main ---

static void print_usage(const char *argv0) {
    std::fprintf(stderr,
        "Usage: %s --rom <file.gb> --profile <profile.toml> [options]\n"
        "\n"
        "Options:\n"
        "  --rom <path>         ROM file to run (required)\n"
        "  --profile <path>     Capture profile TOML file (required)\n"
        "  --output <path>      Output trace file (required)\n"
        "  --frames <n>         Stop after N frames (default: 3000)\n"
        "  --stop-when <A=V>    Stop when memory ADDR equals VAL (hex, e.g. A000=80)\n"
        "  --stop-on-serial <HH>  Stop when serial byte HH is sent (hex)\n"
        "  --stop-serial-count <n> Require n serial matches before stopping (default: 1)\n"
        "  --model <model>      dmg or cgb (default: dmg)\n"
        "  --boot-rom <path>    Boot ROM file (default: boot_roms/<model>_boot.bin)\n",
        argv0);
}

int main(int argc, char *argv[]) {
    std::string rom_path;
    std::string profile_path;
    std::string output_path;
    std::string boot_rom_path;
    int max_frames = 3000;
    std::string model = "DMG-B";
    std::string reference_path;
    int extra_frames = 0;
    GB_model_t gb_model = GB_MODEL_DMG_B;
    std::vector<StopCondition> stop_conditions;

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
            g_stop_serial_byte = static_cast<unsigned char>(
                std::strtoul(argv[++i], nullptr, 16));
            g_stop_serial_active = true;
        } else if (arg == "--stop-serial-count" && i + 1 < argc) {
            g_stop_serial_count = std::atoi(argv[++i]);
        } else if (arg == "--boot-rom" && i + 1 < argc) {
            boot_rom_path = argv[++i];
        } else if (arg == "--model" && i + 1 < argc) {
            std::string m = argv[++i];
            if (m == "cgb" || m == "CGB") {
                model = "CGB-E";
                gb_model = GB_MODEL_CGB_E;
            }
        } else if (arg == "--reference" && i + 1 < argc) {
            reference_path = argv[++i];
        } else if (arg == "--extra-frames" && i + 1 < argc) {
            extra_frames = std::atoi(argv[++i]);
        } else if (arg == "--stop-opcode" && i + 1 < argc) {
            g_stop_opcode = static_cast<int>(std::strtoul(argv[++i], nullptr, 16));
        } else if (arg == "--help" || arg == "-h") {
            print_usage(argv[0]);
            return 0;
        }
    }

    if (rom_path.empty() || profile_path.empty() || output_path.empty()) {
        print_usage(argv[0]);
        return 1;
    }

    // Default boot ROM: resolve relative to executable location
    if (boot_rom_path.empty()) {
        // Find directory containing the executable
        std::string exe_dir;
        char exe_buf[4096];
        ssize_t len = readlink("/proc/self/exe", exe_buf, sizeof(exe_buf) - 1);
        if (len > 0) {
            exe_buf[len] = '\0';
            exe_dir = exe_buf;
            auto slash = exe_dir.rfind('/');
            if (slash != std::string::npos)
                exe_dir = exe_dir.substr(0, slash);
        } else {
            exe_dir = ".";
        }

        std::string boot_name = (gb_model == GB_MODEL_CGB_E) ? "cgb_boot.bin" : "dmg_boot.bin";
        boot_rom_path = exe_dir + "/boot_roms/" + boot_name;
    }

    // Load profile
    g_profile = parse_profile(profile_path);
    build_emitters(g_profile);

    std::fprintf(stderr, "Profile: %s (%zu fields)\n",
                 g_profile.name.c_str(), g_profile.fields.size());

    // Init emulator
    g_gb = GB_init(GB_alloc(), gb_model);

    // Load boot ROM
    int bios_result = GB_load_boot_rom(g_gb, boot_rom_path.c_str());
    if (bios_result != 0) {
        std::fprintf(stderr, "Error: failed to load boot ROM '%s' (error %d)\n",
                     boot_rom_path.c_str(), bios_result);
        return 1;
    }
    std::string boot_rom_info = sha256_file(boot_rom_path);
    std::fprintf(stderr, "Boot ROM: %s (sha256: %s)\n",
                 boot_rom_path.c_str(), boot_rom_info.c_str());

    int load_result = GB_load_rom(g_gb, rom_path.c_str());
    if (load_result != 0) {
        std::fprintf(stderr, "Error: failed to load ROM '%s' (error %d)\n",
                     rom_path.c_str(), load_result);
        return 1;
    }

    // Optimizations for trace generation
    bool need_pixels = g_has_pix || !reference_path.empty();
    if (!need_pixels) {
        GB_set_rendering_disabled(g_gb, true);
    } else {
        GB_set_pixels_output(g_gb, g_pixel_buf);
        GB_set_color_correction_mode(g_gb, GB_COLOR_CORRECTION_DISABLED);
        // Set RGB encode callback so pixel buffer gets standard 0xRRGGBB values
        GB_set_rgb_encode_callback(g_gb, [](GB_gameboy_t *, uint8_t r, uint8_t g, uint8_t b) -> uint32_t {
            return (uint32_t)r | ((uint32_t)g << 8) | ((uint32_t)b << 16) | 0xFF000000u;
        });
    }
    GB_set_turbo_mode(g_gb, true, true);

    // Run boot ROM without tracing — advance until PC reaches 0x0100
    std::fprintf(stderr, "Running boot ROM (no trace)...\n");
    while (true) {
        unsigned ticks = GB_run(g_gb);
        g_total_8mhz_ticks += ticks;
        GB_registers_t *regs = GB_get_registers(g_gb);
        if (regs->pc >= 0x0100) break;
    }
    std::fprintf(stderr, "Boot complete at cycle %llu\n",
                 (unsigned long long)(g_total_8mhz_ticks / 2));

    // Reset cycle origin so traces start at cy=0 post-boot
    g_total_8mhz_ticks = 0;

    // Init FFI writer
    std::string rom_hash = sha256_file(rom_path);

    // Build header JSON for the FFI writer
    std::string header_json = "{\"_header\":true,\"format_version\":\"0.1.0\","
        "\"emulator\":\"sameboy\",\"emulator_version\":\"0.16.x\","
        "\"rom_sha256\":\"" + rom_hash + "\",\"model\":\"" + model + "\","
        "\"boot_rom\":\"" + boot_rom_info + "\",\"profile\":\"" + g_profile.name + "\","
        "\"fields\":[";
    for (size_t i = 0; i < g_emitters.size(); i++) {
        if (i > 0) header_json += ",";
        header_json += "\"" + g_emitters[i].name + "\"";
    }
    header_json += "],\"trigger\":\"instruction\"}";

    g_writer = gbtrace_writer_new(
        output_path.c_str(), header_json.c_str(), header_json.size());
    if (!g_writer) {
        std::fprintf(stderr, "Error: failed to create writer for '%s'\n",
                     output_path.c_str());
        return 1;
    }

    // Cache column indices
    g_writer_cols.resize(g_emitters.size());
    for (size_t i = 0; i < g_emitters.size(); i++) {
        g_writer_cols[i] = gbtrace_writer_find_field(
            g_writer, g_emitters[i].name.c_str());
    }
    g_writer_ly_col = gbtrace_writer_find_field(g_writer, "ly");

    // Mark entry 0 as a frame boundary
    gbtrace_writer_mark_frame(g_writer);

    GB_set_execution_callback(g_gb, exec_callback);

    // Run: GB_run executes one CPU step and returns 8MHz ticks consumed.
    for (const auto &cond : stop_conditions) {
        std::fprintf(stderr, "Stop condition: [0x%04X] == 0x%02X\n",
                     cond.addr, cond.value);
    }
    if (g_stop_serial_active) {
        std::fprintf(stderr, "Serial stop: byte=0x%02X count=%d\n",
                     g_stop_serial_byte, g_stop_serial_count);
    }

    // Load reference image
    bool has_reference = false;
    if (!reference_path.empty()) {
        if (load_reference(reference_path)) {
            has_reference = true;
            std::fprintf(stderr, "Reference: %s (%d pixels)\n",
                         reference_path.c_str(), 160 * 144);
        } else {
            std::fprintf(stderr, "Warning: could not load reference '%s'\n",
                         reference_path.c_str());
        }
    }

    int frames = 0;
    bool stopped_early = false;
    int remaining_extra = -1;  // -1 = not triggered yet
    while (frames < max_frames) {
        unsigned ticks = GB_run(g_gb);
        g_total_8mhz_ticks += ticks;
        if (g_gb->vblank_just_occured) {
            frames++;
            if (g_has_pix || has_reference) {
                capture_sameboy_frame();
            }
            gbtrace_writer_mark_frame(g_writer);

            // Check reference match (immediate stop)
            if (has_reference && g_pending_pix == g_reference_pix) {
                std::fprintf(stderr, "Reference match at frame %d\n", frames);
                while (true) {
                    GB_run(g_gb);
                    if (g_gb->vblank_just_occured) break;
                }
                stopped_early = true;
                break;
            }

            // If in extra-frames countdown, just decrement
            if (remaining_extra >= 0) {
                if (remaining_extra == 0) {
                    stopped_early = true;
                    break;
                }
                remaining_extra--;
                continue;
            }

            // Check stop conditions — start countdown
            for (const auto &cond : stop_conditions) {
                if (GB_safe_read_memory(g_gb, cond.addr) == cond.value) {
                    std::fprintf(stderr, "Stop condition met at frame %d, running %d extra frame%s\n",
                                 frames, extra_frames, extra_frames == 1 ? "" : "s");
                    remaining_extra = extra_frames;
                    break;
                }
            }
            if (remaining_extra >= 0 && remaining_extra == 0) {
                stopped_early = true;
                break;
            }
            if (g_stop_serial_triggered) {
                std::fprintf(stderr, "Serial stop at frame %d, running %d extra frame%s\n",
                             frames, extra_frames, extra_frames == 1 ? "" : "s");
                remaining_extra = extra_frames;
                if (remaining_extra == 0) {
                    stopped_early = true;
                    break;
                }
            }
            if (g_stop_opcode_triggered) {
                std::fprintf(stderr, "Opcode stop at frame %d, running %d extra frame%s\n",
                             frames, extra_frames, extra_frames == 1 ? "" : "s");
                remaining_extra = extra_frames;
                if (remaining_extra == 0) {
                    stopped_early = true;
                    break;
                }
            }
        }
    }

    gbtrace_writer_close(g_writer);
    g_writer = nullptr;

    GB_free(g_gb);
    GB_dealloc(g_gb);

    if (stopped_early) {
        std::fprintf(stderr, "Stop condition met at frame %d.\n", frames);
    }
    std::fprintf(stderr, "Traced %d frames, output written.\n", frames);
    return 0;
}
