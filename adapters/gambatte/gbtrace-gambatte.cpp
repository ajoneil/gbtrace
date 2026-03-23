// gbtrace-gambatte: Adapter that uses libgambatte to produce .gbtrace files.
//
// Links against libgambatte (gambatte-speedrun) without any source modifications.
// Uses the public traceCallback API to capture per-instruction CPU state,
// and externalRead (peek) for IO registers (PPU, timer, interrupts).
//
// Usage:
//   gbtrace-gambatte --rom test.gb --profile cpu_basic.toml [--output trace.gbtrace]
//
// Build:
//   See Makefile in this directory.

#include <gambatte.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

// --- Field configuration ---

// Map of field name -> IO register address for fields read via externalRead.
// CPU register fields are read from the trace callback data array instead.
static const std::unordered_map<std::string, unsigned short> IO_FIELD_ADDR = {
    {"lcdc", 0xFF40}, {"stat", 0xFF41}, {"scy",  0xFF42}, {"scx",  0xFF43},
    {"ly",   0xFF44}, {"lyc",  0xFF45}, {"wy",   0xFF4A}, {"wx",   0xFF4B},
    {"bgp",  0xFF47}, {"obp0", 0xFF48}, {"obp1", 0xFF49}, {"dma",  0xFF46},
    {"div",  0xFF04}, {"tima", 0xFF05}, {"tma",  0xFF06}, {"tac",  0xFF07},
    {"if_",  0xFF0F}, {"ie",   0xFFFF},
    {"sb",   0xFF01}, {"sc",   0xFF02},
};

// Fields available from the trace callback data array.
// Maps field name -> (array index, is_16bit).
struct CallbackField { int index; bool is_16bit; };
static const std::unordered_map<std::string, CallbackField> CALLBACK_FIELDS = {
    {"pc", {1, true}},  {"sp", {2, true}},
    {"a",  {3, false}}, {"b",  {4, false}}, {"c",  {5, false}},
    {"d",  {6, false}}, {"e",  {7, false}}, {"f",  {8, false}},
    {"h",  {9, false}}, {"l",  {10, false}},
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

        // Track TOML sections
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
            // Memory field: name = "hex_address"
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
            auto start = val.find('[');
            auto end = val.find(']');
            if (start != std::string::npos && end != std::string::npos) {
                std::string inner = val.substr(start + 1, end - start - 1);
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

static FILE *g_output = nullptr;
static gambatte::GB *g_gb = nullptr;
static Profile g_profile;
static unsigned char g_stop_serial_byte = 0;
static int g_stop_serial_count = 1;  // stop after Nth occurrence
static int g_stop_serial_seen = 0;
static bool g_stop_serial_active = false;
static bool g_stop_serial_triggered = false;

// Pre-computed list of what to emit per entry, for fast callback execution.
struct FieldEmitter {
    std::string name;
    enum Source { CALLBACK_8, CALLBACK_16, IO_READ, IME, PIX } source;
    int cb_index;           // for CALLBACK_8/16
    unsigned short io_addr; // for IO_READ
};
static std::vector<FieldEmitter> g_emitters;
static bool g_has_pix = false;

// --- Pixel capture ---
// Gambatte fills video_buf as a 160x144 RGBA framebuffer during runFor().
// After each frame completes, we convert to a 2-bit shade string and emit
// it on the next trace entry. Pixels accumulate in g_pending_pix.
static gambatte::uint_least32_t *g_video_buf_ptr = nullptr;
static std::string g_pending_pix;

static inline char rgba_to_shade_char(gambatte::uint_least32_t rgba) {
    // Use red channel — gambatte's default greyscale palette
    unsigned r = rgba & 0xFF;
    if (r >= 0xC0) return '0';
    if (r >= 0x70) return '1';
    if (r >= 0x30) return '2';
    return '3';
}

static void capture_frame_pixels() {
    if (!g_video_buf_ptr) return;
    g_pending_pix.clear();
    g_pending_pix.reserve(160 * 144);
    for (int i = 0; i < 160 * 144; i++) {
        g_pending_pix += rgba_to_shade_char(g_video_buf_ptr[i]);
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
        FieldEmitter em;
        em.name = field;

        if (field == "ime") {
            // gambatte doesn't expose IME — skip rather than emit fake data
            std::fprintf(stderr, "Note: skipping 'ime' (not available in gambatte)\n");
            continue;
        } else if (field == "pix") {
            em.source = FieldEmitter::PIX;
            g_has_pix = true;
            g_emitters.push_back(em);
            continue;
        } else if (auto it = CALLBACK_FIELDS.find(field); it != CALLBACK_FIELDS.end()) {
            em.source = it->second.is_16bit ? FieldEmitter::CALLBACK_16 : FieldEmitter::CALLBACK_8;
            em.cb_index = it->second.index;
        } else if (auto it2 = IO_FIELD_ADDR.find(field); it2 != IO_FIELD_ADDR.end()) {
            em.source = FieldEmitter::IO_READ;
            em.io_addr = it2->second;
        } else if (auto it3 = prof.memory.find(field); it3 != prof.memory.end()) {
            em.source = FieldEmitter::IO_READ; // same mechanism — peek memory
            em.io_addr = it3->second;
        } else {
            std::fprintf(stderr, "Warning: unknown field '%s', skipping\n", field.c_str());
            continue;
        }
        g_emitters.push_back(em);
    }
}

// --- Formatting helpers ---

static inline void fput_u8(FILE *out, int val) {
    std::fprintf(out, "%d", val & 0xFF);
}

static inline void fput_u16(FILE *out, int val) {
    std::fprintf(out, "%d", val & 0xFFFF);
}

// --- Trace callback ---

// Cache for IO values — used to emit pre-execution state.
// The callback fires AFTER the instruction executes, so externalRead()
// gives post-execution values. We cache the IO values and emit them
// on the NEXT callback, giving pre-execution state for that instruction.
static std::unordered_map<unsigned short, unsigned char> g_io_cache;
static bool g_io_cache_valid = false;

static void trace_callback(void *data) {
    int *r = static_cast<int *>(data);

    // Read current IO values (post-execution of this instruction)
    std::unordered_map<unsigned short, unsigned char> io_now;
    for (const auto &em : g_emitters) {
        if (em.source == FieldEmitter::IO_READ) {
            io_now[em.io_addr] = g_gb->externalRead(em.io_addr);
        }
    }

    // Emit entry using cached IO values (pre-execution state)
    bool first = true;
    std::fprintf(g_output, "{");

    for (const auto &em : g_emitters) {
        if (!first) std::fprintf(g_output, ",");
        first = false;
        std::fprintf(g_output, "\"%s\":", em.name.c_str());
        switch (em.source) {
        case FieldEmitter::CALLBACK_8:
            fput_u8(g_output, r[em.cb_index]);
            break;
        case FieldEmitter::CALLBACK_16:
            fput_u16(g_output, r[em.cb_index]);
            break;
        case FieldEmitter::IO_READ:
            if (g_io_cache_valid) {
                fput_u8(g_output, g_io_cache[em.io_addr]);
            } else {
                // First instruction — no cached value, use current
                fput_u8(g_output, io_now[em.io_addr]);
            }
            break;
        case FieldEmitter::IME:
            // IME not available in gambatte — should be skipped by build_emitters
            break;
        case FieldEmitter::PIX: {
            std::fprintf(g_output, "\"%s\"", g_pending_pix.c_str());
            g_pending_pix.clear();
            break;
        }
        }
    }

    std::fprintf(g_output, "}\n");

    // Update cache for next callback
    g_io_cache = io_now;
    g_io_cache_valid = true;

    // Check serial stop condition: detect rising edge of SC bit 7
    if (g_stop_serial_active && !g_stop_serial_triggered) {
        static bool prev_sc_high = false;
        unsigned char sc = g_gb->externalRead(0xFF02);
        bool sc_high = (sc & 0x80) != 0;
        if (sc_high && !prev_sc_high) {
            unsigned char sb = g_gb->externalRead(0xFF01);
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

// --- Header ---

static void write_header(FILE *out, const Profile &prof,
                          const std::string &rom_sha256,
                          const std::string &model,
                          const std::string &boot_rom_info) {
    std::fprintf(out,
        "{\"_header\":true,\"format_version\":\"0.1.0\","
        "\"emulator\":\"gambatte-speedrun\",\"emulator_version\":\"r730+\","
        "\"rom_sha256\":\"%s\",\"model\":\"%s\","
        "\"boot_rom\":\"%s\",\"profile\":\"%s\","
        "\"fields\":[",
        rom_sha256.c_str(), model.c_str(), boot_rom_info.c_str(),
        prof.name.c_str());

    for (size_t i = 0; i < g_emitters.size(); i++) {
        if (i > 0) std::fprintf(out, ",");
        std::fprintf(out, "\"%s\"", g_emitters[i].name.c_str());
    }

    std::fprintf(out, "],\"trigger\":\"instruction\"}\n");
}

// --- Stop condition ---

struct StopCondition {
    unsigned short addr = 0;
    unsigned char value = 0;
    bool active = false;
};

static StopCondition parse_stop_when(const std::string &spec) {
    // Format: ADDR=VAL (hex), e.g. A000=80
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
        "  --output <path>      Output trace file (default: stdout)\n"
        "  --frames <n>         Stop after N frames (default: 3000)\n"
        "  --stop-when <A=V>    Stop when memory ADDR equals VAL (hex, e.g. A000=80)\n"
        "  --stop-on-serial <B> Stop when byte B (hex) is sent via serial (e.g. 0A for newline)\n"
        "  --stop-serial-count <N> Stop on Nth occurrence of serial byte (default: 1)\n"
        "  --model <model>      dmg or cgb (default: dmg)\n"
        "  --boot-rom <path>    Boot ROM file (default: skip boot)\n",
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
    unsigned load_flags = gambatte::GB::LoadFlag::NO_BIOS;
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
                load_flags |= gambatte::GB::LoadFlag::CGB_MODE;
            }
        } else if (arg == "--reference" && i + 1 < argc) {
            reference_path = argv[++i];
        } else if (arg == "--extra-frames" && i + 1 < argc) {
            extra_frames = std::atoi(argv[++i]);
        } else if (arg == "--help" || arg == "-h") {
            print_usage(argv[0]);
            return 0;
        }
    }

    // If a boot ROM is provided, don't skip BIOS
    if (!boot_rom_path.empty()) {
        load_flags &= ~gambatte::GB::LoadFlag::NO_BIOS;
    }

    if (rom_path.empty() || profile_path.empty()) {
        print_usage(argv[0]);
        return 1;
    }

    // Load profile
    g_profile = parse_profile(profile_path);
    build_emitters(g_profile);

    std::fprintf(stderr, "Profile: %s (%zu fields)\n",
                 g_profile.name.c_str(), g_profile.fields.size());

    // Open output
    if (output_path.empty() || output_path == "-") {
        g_output = stdout;
    } else {
        g_output = std::fopen(output_path.c_str(), "w");
        if (!g_output) {
            std::fprintf(stderr, "Error: cannot open %s for writing\n", output_path.c_str());
            return 1;
        }
    }

    static char output_buf[64 * 1024];
    std::setvbuf(g_output, output_buf, _IOFBF, sizeof(output_buf));

    // Init emulator
    gambatte::GB gb;
    g_gb = &gb;

    int load_result = gb.load(rom_path, load_flags);
    if (load_result != 0) {
        std::fprintf(stderr, "Error: failed to load ROM '%s' (error %d)\n",
                     rom_path.c_str(), load_result);
        return 1;
    }

    // Load boot ROM if provided
    std::string boot_rom_info = "skip";
    if (!boot_rom_path.empty()) {
        int bios_result = gb.loadBios(boot_rom_path);
        if (bios_result != 0) {
            std::fprintf(stderr, "Error: failed to load boot ROM '%s' (error %d)\n",
                         boot_rom_path.c_str(), bios_result);
            return 1;
        }
        boot_rom_info = sha256_file(boot_rom_path);
        std::fprintf(stderr, "Boot ROM: %s (sha256: %s)\n",
                     boot_rom_path.c_str(), boot_rom_info.c_str());
    }

    // Write header and set callback
    std::string rom_hash = sha256_file(rom_path);
    write_header(g_output, g_profile, rom_hash, model, boot_rom_info);
    gb.setTraceCallback(trace_callback);

    // Run
    static const std::size_t SAMPLES_PER_FRAME = 35112;
    std::vector<gambatte::uint_least32_t> video_buf(160 * 144, 0);
    std::vector<gambatte::uint_least32_t> audio_buf(SAMPLES_PER_FRAME * 2 + 2064, 0);

    for (const auto &cond : stop_conditions) {
        std::fprintf(stderr, "Stop condition: [0x%04X] == 0x%02X\n",
                     cond.addr, cond.value);
    }
    if (g_stop_serial_active) {
        std::fprintf(stderr, "Stop on serial byte: 0x%02X (after %d occurrence%s)\n",
                     g_stop_serial_byte, g_stop_serial_count,
                     g_stop_serial_count == 1 ? "" : "s");
    }

    // Set up pixel capture
    g_video_buf_ptr = video_buf.data();

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
        std::size_t samples = SAMPLES_PER_FRAME;
        std::ptrdiff_t result = gb.runFor(
            video_buf.data(), 160,
            audio_buf.data(), samples);
        if (result >= 0) {
            frames++;
            if (g_has_pix || has_reference) {
                capture_frame_pixels();
            }

            // Check reference match (always immediate stop — the frame we want is captured)
            if (has_reference && g_pending_pix == g_reference_pix) {
                std::fprintf(stderr, "Reference match at frame %d\n", frames);
                std::size_t s2 = SAMPLES_PER_FRAME;
                gb.runFor(video_buf.data(), 160, audio_buf.data(), s2);
                stopped_early = true;
                break;
            }

            // If we're in extra-frames countdown, just decrement
            if (remaining_extra >= 0) {
                if (remaining_extra == 0) {
                    stopped_early = true;
                    break;
                }
                remaining_extra--;
                continue;
            }

            // Check stop conditions — start countdown instead of breaking
            for (const auto &cond : stop_conditions) {
                if (gb.externalRead(cond.addr) == cond.value) {
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
        }
    }

    std::fflush(g_output);
    if (g_output != stdout) {
        std::fclose(g_output);
    }

    if (stopped_early) {
        std::fprintf(stderr, "Stop condition met at frame %d, output written.\n", frames);
    } else {
        std::fprintf(stderr, "Traced %d frames, output written.\n", frames);
    }
    return 0;
}
