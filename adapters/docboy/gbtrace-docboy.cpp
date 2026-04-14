// gbtrace-docboy: Adapter that uses DocBoy to produce .gbtrace files.
//
// Links against libdocboy with the trace API patch applied.
// Uses the debugger tick callback to capture state at every T-cycle,
// and the register getter API for CPU register access.
//
// DocBoy does not require a boot ROM — with ENABLE_BOOTROM=OFF it
// initializes to post-boot state (similar to Gambatte's NO_BIOS mode).
//
// Usage:
//   gbtrace-docboy --rom test.gb --profile cpu_basic.toml --output trace.gbtrace
//
// Build:
//   See Makefile in this directory.

#include "docboy/core/core.h"
#include "docboy/gameboy/gameboy.h"
#include "docboy/debugger/backend.h"
#include "docboy/common/specs.h"
#include "docboy/lcd/appearance.h"

#include "gbtrace.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

// --- Field configuration ---

// IO register addresses read via DebuggerBackend::read_memory().
static const std::unordered_map<std::string, unsigned short> IO_FIELD_ADDR = {
    {"lcdc", 0xFF40}, {"stat", 0xFF41}, {"scy",  0xFF42}, {"scx",  0xFF43},
    {"ly",   0xFF44}, {"lyc",  0xFF45}, {"wy",   0xFF4A}, {"wx",   0xFF4B},
    {"bgp",  0xFF47}, {"obp0", 0xFF48}, {"obp1", 0xFF49}, {"dma",  0xFF46},
    {"div",  0xFF04}, {"tima", 0xFF05}, {"tma",  0xFF06}, {"tac",  0xFF07},
    {"if_",  0xFF0F}, {"ie",   0xFFFF},
    {"sb",   0xFF01}, {"sc",   0xFF02},
    // APU registers
    {"ch1_sweep", 0xFF10}, {"ch1_duty_len", 0xFF11}, {"ch1_vol_env", 0xFF12},
    {"ch1_freq_lo", 0xFF13}, {"ch1_freq_hi", 0xFF14},
    {"ch2_duty_len", 0xFF16}, {"ch2_vol_env", 0xFF17},
    {"ch2_freq_lo", 0xFF18}, {"ch2_freq_hi", 0xFF19},
    {"ch3_dac", 0xFF1A}, {"ch3_len", 0xFF1B}, {"ch3_vol", 0xFF1C},
    {"ch3_freq_lo", 0xFF1D}, {"ch3_freq_hi", 0xFF1E},
    {"ch4_len", 0xFF20}, {"ch4_vol_env", 0xFF21},
    {"ch4_freq", 0xFF22}, {"ch4_control", 0xFF23},
    {"master_vol", 0xFF24}, {"sound_pan", 0xFF25}, {"sound_on", 0xFF26},
};

// --- Profile ---

struct Profile {
    std::string name;
    std::string trigger;
    std::vector<std::string> fields;
    std::unordered_map<std::string, unsigned short> memory;
};

static Profile load_profile(const std::string &path) {
    GbtraceProfile *p = gbtrace_profile_load(path.c_str());
    if (!p) {
        std::fprintf(stderr, "Error: cannot load profile '%s'\n", path.c_str());
        std::exit(1);
    }
    Profile prof;
    prof.name = gbtrace_profile_name(p);
    prof.trigger = gbtrace_profile_trigger(p);
    size_t nfields = gbtrace_profile_num_fields(p);
    for (size_t i = 0; i < nfields; i++)
        prof.fields.push_back(gbtrace_profile_field_name(p, i));
    size_t nmem = gbtrace_profile_num_memory(p);
    for (size_t i = 0; i < nmem; i++)
        prof.memory[gbtrace_profile_memory_name(p, i)] = gbtrace_profile_memory_addr(p, i);
    gbtrace_profile_free(p);
    return prof;
}

// --- Field emitters ---

struct FieldEmitter {
    std::string name;
    enum Source { CPU_REG8, CPU_REG16, IO_READ, IME, PIX } source;
    unsigned short io_addr;
};

static std::vector<FieldEmitter> g_emitters;
static bool g_has_pix = false;

static void build_emitters(const Profile &prof) {
    g_emitters.clear();
    static const std::unordered_map<std::string, bool> REG16 = {{"pc", true}, {"sp", true}};
    static const std::unordered_map<std::string, bool> REG8 = {
        {"a", true}, {"f", true}, {"b", true}, {"c", true},
        {"d", true}, {"e", true}, {"h", true}, {"l", true},
    };

    for (const auto &field : prof.fields) {
        FieldEmitter em;
        em.name = field;
        em.io_addr = 0;

        if (field == "pix") {
            em.source = FieldEmitter::PIX;
            g_has_pix = true;
        } else if (field == "ime") {
            em.source = FieldEmitter::IME;
        } else if (REG16.count(field)) {
            em.source = FieldEmitter::CPU_REG16;
        } else if (REG8.count(field)) {
            em.source = FieldEmitter::CPU_REG8;
        } else if (auto it = IO_FIELD_ADDR.find(field); it != IO_FIELD_ADDR.end()) {
            em.source = FieldEmitter::IO_READ;
            em.io_addr = it->second;
        } else if (auto it2 = prof.memory.find(field); it2 != prof.memory.end()) {
            em.source = FieldEmitter::IO_READ;
            em.io_addr = it2->second;
        } else {
            std::fprintf(stderr, "Warning: unknown field '%s', skipping\n", field.c_str());
            continue;
        }
        g_emitters.push_back(em);
    }
}

// --- Globals ---

static GbtraceWriter *g_writer = nullptr;
static std::vector<int> g_writer_cols;
static int g_writer_ly_col = -1;
static DebuggerBackend *g_debugger = nullptr;

// Pixel capture
static std::string g_pending_pix;
static std::string g_reference_pix;

static inline char rgb565_to_shade(uint16_t pixel) {
    // Extract red channel (bits 15:11), scale to 8-bit
    unsigned r = ((pixel >> 11) & 0x1F) * 255 / 31;
    if (r >= 0xC0) return '0';
    if (r >= 0x70) return '1';
    if (r >= 0x30) return '2';
    return '3';
}

// --- Cached CPU register snapshot ---
// Read all registers once per entry to avoid repeated calls through the
// debugger accessor layer (matters at ~70K entries per frame).

struct CpuSnapshot {
    uint16_t af, bc, de, hl, sp, pc;
    bool ime;
};

static CpuSnapshot g_cpu_snap;

static void snapshot_cpu() {
    g_cpu_snap.af = g_debugger->get_af();
    g_cpu_snap.bc = g_debugger->get_bc();
    g_cpu_snap.de = g_debugger->get_de();
    g_cpu_snap.hl = g_debugger->get_hl();
    g_cpu_snap.sp = g_debugger->get_sp();
    g_cpu_snap.pc = g_debugger->get_core().gb.cpu.pc;
    g_cpu_snap.ime = g_debugger->get_ime();
}

static inline uint8_t read_cpu_reg8(const std::string &name) {
    if (name == "a") return (g_cpu_snap.af >> 8) & 0xFF;
    if (name == "f") return g_cpu_snap.af & 0xFF;
    if (name == "b") return (g_cpu_snap.bc >> 8) & 0xFF;
    if (name == "c") return g_cpu_snap.bc & 0xFF;
    if (name == "d") return (g_cpu_snap.de >> 8) & 0xFF;
    if (name == "e") return g_cpu_snap.de & 0xFF;
    if (name == "h") return (g_cpu_snap.hl >> 8) & 0xFF;
    if (name == "l") return g_cpu_snap.hl & 0xFF;
    return 0;
}

static inline uint16_t read_cpu_reg16(const std::string &name) {
    if (name == "pc") return g_cpu_snap.pc;
    if (name == "sp") return g_cpu_snap.sp;
    return 0;
}

// --- Emit one trace entry ---

static void emit_entry() {
    snapshot_cpu();

    uint8_t ly_val = 255;
    size_t pix_len = 0;
    if (g_writer_ly_col >= 0) {
        ly_val = g_debugger->read_memory(0xFF44);
    }
    if (g_has_pix) {
        pix_len = g_pending_pix.size();
    }
    gbtrace_writer_check_boundary(g_writer, ly_val, pix_len);

    for (size_t i = 0; i < g_emitters.size(); i++) {
        int col = g_writer_cols[i];
        if (col < 0) continue;
        const auto &em = g_emitters[i];
        switch (em.source) {
        case FieldEmitter::CPU_REG8:
            gbtrace_writer_set_u8(g_writer, col, read_cpu_reg8(em.name));
            break;
        case FieldEmitter::CPU_REG16:
            gbtrace_writer_set_u16(g_writer, col, read_cpu_reg16(em.name));
            break;
        case FieldEmitter::IO_READ:
            gbtrace_writer_set_u8(g_writer, col, g_debugger->read_memory(em.io_addr));
            break;
        case FieldEmitter::IME:
            gbtrace_writer_set_bool(g_writer, col, g_cpu_snap.ime);
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
    unsigned short addr = 0;
    unsigned char value = 0;
    bool negate = false;
};

static StopCondition parse_stop_when(const std::string &spec) {
    auto neq = spec.find("!=");
    auto eq = spec.find('=');
    if (eq == std::string::npos) {
        std::fprintf(stderr, "Error: --stop-when format is ADDR=VAL or ADDR!=VAL\n");
        std::exit(1);
    }
    StopCondition cond;
    bool is_negate = (neq != std::string::npos && neq < eq);
    cond.addr = static_cast<unsigned short>(std::strtoul(spec.substr(0, is_negate ? neq : eq).c_str(), nullptr, 16));
    cond.value = static_cast<unsigned char>(std::strtoul(spec.substr(eq + 1).c_str(), nullptr, 16));
    cond.negate = is_negate;
    return cond;
}

// --- Reference matching ---

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

// --- Main ---

int main(int argc, char *argv[]) {
    std::string rom_path;
    std::string profile_path;
    std::string output_path;
    std::string reference_path;
    int max_frames = 3000;
    int extra_frames = 0;
    int stop_opcode = -1;
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
        } else if (arg == "--stop-opcode" && i + 1 < argc) {
            stop_opcode = static_cast<int>(std::strtoul(argv[++i], nullptr, 16));
        } else if (arg == "--reference" && i + 1 < argc) {
            reference_path = argv[++i];
        } else if (arg == "--extra-frames" && i + 1 < argc) {
            extra_frames = std::atoi(argv[++i]);
        } else if (arg == "--model" && i + 1 < argc) {
            ++i; // ignore — DocBoy is DMG-only for now
        } else if (arg == "--boot-rom" && i + 1 < argc) {
            ++i; // ignore — no boot ROM support
        } else if (arg == "--stop-on-serial" && i + 1 < argc) {
            ++i; // TODO: serial stop not yet implemented
        } else if (arg == "--stop-serial-count" && i + 1 < argc) {
            ++i;
        }
    }

    if (rom_path.empty() || profile_path.empty() || output_path.empty()) {
        std::fprintf(stderr,
            "Usage: gbtrace-docboy --rom <file.gb> --profile <profile.toml> --output <path> [options]\n");
        return 1;
    }

    // Load profile
    Profile profile = load_profile(profile_path);
    build_emitters(profile);
    std::fprintf(stderr, "Profile: %s (%zu fields)\n",
                 profile.name.c_str(), g_emitters.size());

    // Init DocBoy (heap-allocated — GameBoy struct is too large for the stack)
    auto gb = std::make_unique<GameBoy>();

    // Set DMG greyscale palette — DocBoy's default Appearance is zero-initialized
    // (all black), which breaks screenshot comparison.
    Appearance grey_palette;
    grey_palette.default_color = 0xFFFF;
    grey_palette.palette = {0xFFFF, 0xAD55, 0x52AA, 0x0000};
    gb->lcd.set_appearance(grey_palette);

    Core core(*gb);
    core.load_rom(rom_path);

    // Attach debugger for tick callback and register access
    DebuggerBackend debugger(core);
    core.attach_debugger(debugger);
    g_debugger = &debugger;

    // Init FFI writer
    std::string rom_hash = sha256_file(rom_path);
    std::string header_json = "{\"_header\":true,\"format_version\":\"0.1.0\","
        "\"emulator\":\"docboy\",\"emulator_version\":\"git\","
        "\"rom_sha256\":\"" + rom_hash + "\",\"model\":\"DMG-B\","
        "\"boot_rom\":\"skip\",\"profile\":\"" + profile.name + "\","
        "\"fields\":[";
    for (size_t i = 0; i < g_emitters.size(); i++) {
        if (i > 0) header_json += ",";
        header_json += "\"" + g_emitters[i].name + "\"";
    }
    header_json += "],\"trigger\":\"tcycle\"}";

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

    // Install tick callback — fires at every T-cycle
    debugger.set_tick_callback([](uint64_t) {
        emit_entry();
    });

    // Set continue command so the debugger doesn't block
    debugger.proceed();

    // Run simulation
    int frames = 0;
    bool stopped_early = false;
    int remaining_extra = -1;
    bool stop_opcode_triggered = false;
    uint8_t prev_ppu_mode = 0;

    while (frames < max_frames) {
        try {
            core.cycle();
        } catch (const std::runtime_error &) {
            // DocBoy throws on undefined opcodes (e.g. 0xED used by wilbertpol
            // tests as a stop signal). Treat as opcode stop if one is configured,
            // otherwise stop immediately. Core state is invalid after this.
            if (stop_opcode >= 0) {
                stop_opcode_triggered = true;
            }
            stopped_early = true;
            break;
        }

        // Check opcode stop per M-cycle (must catch it before PC moves on)
        if (!stop_opcode_triggered && stop_opcode >= 0) {
            uint8_t opval = debugger.read_memory(gb->cpu.pc);
            if (opval == static_cast<uint8_t>(stop_opcode)) {
                stop_opcode_triggered = true;
            }
        }

        // Detect VBlank edge (mode transitions to 1)
        uint8_t ppu_mode = gb->ppu.stat.mode;
        if (ppu_mode == Specs::Ppu::Modes::VBLANK && prev_ppu_mode != Specs::Ppu::Modes::VBLANK) {
            frames++;

            // Capture pixels
            if (g_has_pix || has_reference) {
                const PixelRgb565 *pixels = gb->lcd.get_pixels();
                g_pending_pix.clear();
                g_pending_pix.reserve(160 * 144);
                for (int j = 0; j < 160 * 144; j++) {
                    g_pending_pix += rgb565_to_shade(pixels[j]);
                }
            }
            gbtrace_writer_mark_frame(g_writer);

            // Check reference match
            if (has_reference && g_pending_pix == g_reference_pix) {
                std::fprintf(stderr, "Reference match at frame %d\n", frames);
                // Run one more frame to capture final state
                while (gb->ppu.stat.mode == Specs::Ppu::Modes::VBLANK) core.cycle();
                while (gb->ppu.stat.mode != Specs::Ppu::Modes::VBLANK) core.cycle();
                stopped_early = true;
                break;
            }

            // Extra-frames countdown
            if (remaining_extra >= 0) {
                if (remaining_extra == 0) {
                    stopped_early = true;
                    break;
                }
                remaining_extra--;
                prev_ppu_mode = ppu_mode;
                continue;
            }

            // Check stop conditions (memory watches)
            for (const auto &cond : stop_conditions) {
                uint8_t val = debugger.read_memory(cond.addr);
                bool match = (val == cond.value);
                if (cond.negate ? !match : match) {
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

            // Start extra-frames countdown on opcode trigger
            if (stop_opcode_triggered) {
                std::fprintf(stderr, "Opcode stop at frame %d, running %d extra frame%s\n",
                             frames, extra_frames, extra_frames == 1 ? "" : "s");
                remaining_extra = extra_frames;
                if (remaining_extra == 0) {
                    stopped_early = true;
                    break;
                }
            }
        }
        prev_ppu_mode = ppu_mode;
    }

    gbtrace_writer_close(g_writer);
    g_writer = nullptr;
    g_debugger = nullptr;

    if (stopped_early) {
        std::fprintf(stderr, "Stop condition met at frame %d.\n", frames);
    }
    std::fprintf(stderr, "Traced %d frames, output written.\n", frames);
    return 0;
}
