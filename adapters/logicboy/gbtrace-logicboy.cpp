// gbtrace-logicboy: Adapter that uses LogicBoy (from metroboy) to produce
// .gbtrace files.
//
// LogicBoy is a fast logic-level Game Boy simulation derived from the
// gate-level GateBoy.  It runs at phase granularity (8 phases per T-cycle)
// but this adapter emits one trace entry per instruction boundary, matching
// the output format of the other gbtrace adapters.
//
// Usage:
//   gbtrace-logicboy --rom test.gb --profile cpu_basic.toml [--output trace.gbtrace]
//
// Build:
//   See Makefile in this directory.

#include "GateBoyLib/LogicBoy.h"
#include "metrolib/core/Blobs.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

// --- Field configuration ---

// Map of field name -> IO register address for fields read via peek().
static const std::unordered_map<std::string, unsigned short> IO_FIELD_ADDR = {
    {"lcdc", 0xFF40}, {"stat", 0xFF41}, {"scy",  0xFF42}, {"scx",  0xFF43},
    {"ly",   0xFF44}, {"lyc",  0xFF45}, {"wy",   0xFF4A}, {"wx",   0xFF4B},
    {"bgp",  0xFF47}, {"obp0", 0xFF48}, {"obp1", 0xFF49}, {"dma",  0xFF46},
    {"div",  0xFF04}, {"tima", 0xFF05}, {"tma",  0xFF06}, {"tac",  0xFF07},
    {"if_",  0xFF0F}, {"ie",   0xFFFF},
    {"sb",   0xFF01}, {"sc",   0xFF02},
};

// Read an IO register directly from LogicBoyState (avoids peek() which
// doesn't support some addresses like STAT, IF, IE).
static uint8_t read_io_reg(const LogicBoy& lb, unsigned short addr) {
    const auto& s = lb.lb_state;
    switch (addr) {
        case 0xFF00: return s.reg_joy;
        case 0xFF01: return s.reg_sb;
        case 0xFF02: return s.reg_sc;
        case 0xFF04: return (uint8_t)(s.reg_div >> 8);
        case 0xFF05: return s.reg_tima;
        case 0xFF06: return s.reg_tma;
        case 0xFF07: return s.reg_tac;
        case 0xFF0F: return s.reg_if;
        case 0xFF40: return s.reg_lcdc;
        case 0xFF41: return s.reg_stat;
        case 0xFF42: return s.reg_scy;
        case 0xFF43: return s.reg_scx;
        case 0xFF44: return s.reg_ly;
        case 0xFF45: return s.reg_lyc;
        case 0xFF46: return s.reg_dma;
        case 0xFF47: return s.reg_bgp;
        case 0xFF48: return s.reg_obp0;
        case 0xFF49: return s.reg_obp1;
        case 0xFF4A: return s.reg_wy;
        case 0xFF4B: return s.reg_wx;
        case 0xFFFF: return s.reg_ie;
        default: {
            GBResult r = lb.peek(addr);
            return r.is_ok() ? r.unwrap() : 0;
        }
    }
}

// CPU register fields read directly from the CpuState struct.
struct CpuField { enum Kind { REG8, REG16, IME } kind; int offset; };

static const std::unordered_map<std::string, CpuField> CPU_FIELDS = {
    {"a",   {CpuField::REG8,  0}},
    {"f",   {CpuField::REG8,  1}},
    {"b",   {CpuField::REG8,  2}},
    {"c",   {CpuField::REG8,  3}},
    {"d",   {CpuField::REG8,  4}},
    {"e",   {CpuField::REG8,  5}},
    {"h",   {CpuField::REG8,  6}},
    {"l",   {CpuField::REG8,  7}},
    {"pc",  {CpuField::REG16, 0}},
    {"sp",  {CpuField::REG16, 1}},
    {"ime", {CpuField::IME,   0}},
};

// --- Profile ---

struct Profile {
    std::string name;
    std::string trigger;
    std::vector<std::string> fields; // ordered, including "cy"
    std::unordered_map<std::string, unsigned short> memory; // name -> address
};

static Profile parse_profile(const std::string &path) {
    Profile prof;
    prof.trigger = "instruction";
    prof.fields.push_back("cy");

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

// --- Emitter setup ---

struct FieldEmitter {
    std::string name;
    enum Source { CPU_REG8, CPU_REG16, CPU_IME, IO_READ, OPCODE } source;
    unsigned short io_addr; // for IO_READ
};

static std::vector<FieldEmitter> g_emitters;

// Read an 8-bit CPU register by name from the CpuState.
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
    if (name == "pc") return reg.op_addr; // use op_addr = PC of current instruction
    if (name == "sp") return reg.sp;
    return 0;
}

static void build_emitters(const Profile &prof) {
    g_emitters.clear();
    for (const auto &field : prof.fields) {
        if (field == "cy") continue;

        FieldEmitter em;
        em.name = field;
        em.io_addr = 0;

        if (field == "op") {
            em.source = FieldEmitter::OPCODE;
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
        } else {
            std::fprintf(stderr, "Warning: unknown field '%s', skipping\n", field.c_str());
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
        "\"emulator\":\"logicboy\",\"emulator_version\":\"metroboy-git\","
        "\"rom_sha256\":\"%s\",\"model\":\"DMG\","
        "\"boot_rom\":\"%s\",\"profile\":\"%s\","
        "\"fields\":[",
        rom_sha256.c_str(), boot_rom_info.c_str(),
        prof.name.c_str());

    std::fprintf(out, "\"cy\"");
    for (const auto &em : g_emitters) {
        std::fprintf(out, ",\"%s\"", em.name.c_str());
    }

    std::fprintf(out, "],\"trigger\":\"%s\",\"cy_unit\":\"tcycle\"}\n",
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

static void emit_entry(FILE *out, LogicBoy &lb, uint64_t tcycle) {
    const CpuState &reg = lb.cpu.core.reg;

    std::fprintf(out, "{\"cy\":%llu", (unsigned long long)tcycle);

    for (const auto &em : g_emitters) {
        std::fprintf(out, ",\"%s\":", em.name.c_str());
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
            uint8_t val = read_io_reg(lb, em.io_addr);
            std::fprintf(out, "%d", val);
            break;
        }
        case FieldEmitter::OPCODE:
            std::fprintf(out, "%d", reg.op_next);
            break;
        }
    }

    std::fprintf(out, "}\n");
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
        "  --stop-serial-count <N> Stop on Nth occurrence (default: 1)\n",
        argv0);
}

int main(int argc, char *argv[]) {
    std::string rom_path;
    std::string profile_path;
    std::string output_path;
    int max_frames = 3000;
    std::vector<StopCondition> stop_conditions;
    unsigned char stop_serial_byte = 0;
    int stop_serial_count = 1;
    bool stop_serial_active = false;

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

    // Open output
    FILE *output = nullptr;
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

    // Load ROM into a blob
    blob cart_blob;
    if (!load_blob(rom_path.c_str(), cart_blob)) {
        std::fprintf(stderr, "Error: cannot load ROM '%s'\n", rom_path.c_str());
        return 1;
    }

    // Initialize LogicBoy
    LogicBoy lb;
    lb.reset();

    // Load ROM data into cart memory so LogicBoy can read it via the blob.
    // LogicBoy::reset() sets the CPU to post-boot state (PC=0x0100, etc.)
    // and the boot ROM is already applied via the default DMG boot ROM in
    // GateBoyMem::reset().

    // Write header
    std::string rom_hash = sha256_file(rom_path);
    write_header(output, profile, rom_hash, "skip");

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
    // LogicBoy runs at phase granularity (8 phases = 1 T-cycle).
    // We detect instruction boundaries by watching op_addr change:
    // when the CPU's op_addr differs from the previous value (and
    // op_state == 0), a new instruction has started.

    static constexpr int PHASES_PER_FRAME = 70224 * 8;  // 561792 phases
    int64_t total_phases = static_cast<int64_t>(max_frames) * PHASES_PER_FRAME;

    int prev_op_state = lb.cpu.core.reg.op_state;
    bool stopped_early = false;
    int stop_serial_seen = 0;
    bool prev_sc_high = false;
    int frames = 0;
    int64_t phase_count = 0;

    while (phase_count < total_phases) {
        lb.next_phase(cart_blob);
        phase_count++;

        const CpuState &reg = lb.cpu.core.reg;

        // Detect instruction boundary: op_state transitions to 0
        // (start of a new instruction's opcode fetch).
        if (reg.op_state == 0 && prev_op_state != 0) {
            uint64_t tcycle = lb.sys.gb_phase_total_old / 8;
            emit_entry(output, lb, tcycle);

            // Check serial stop condition
            if (stop_serial_active) {
                uint8_t sc_val = read_io_reg(lb, 0xFF02);
                bool sc_high = (sc_val & 0x80) != 0;
                if (sc_high && !prev_sc_high) {
                    uint8_t sb_val = read_io_reg(lb, 0xFF01);
                    if (sb_val == stop_serial_byte) {
                        stop_serial_seen++;
                        if (stop_serial_seen >= stop_serial_count) {
                            stopped_early = true;
                            break;
                        }
                    }
                }
                prev_sc_high = sc_high;
            }
        }

        prev_op_state = reg.op_state;

        // Check frame boundary for stop-when conditions
        if ((phase_count % PHASES_PER_FRAME) == 0) {
            frames++;
            for (const auto &cond : stop_conditions) {
                uint8_t val = read_io_reg(lb, cond.addr);
                if (val == cond.value) {
                    stopped_early = true;
                    break;
                }
            }
            if (stopped_early) break;
        }
    }

    std::fflush(output);
    if (output != stdout) {
        std::fclose(output);
    }

    if (stopped_early) {
        std::fprintf(stderr, "Stop condition met at frame %d, output written.\n", frames);
    } else {
        std::fprintf(stderr, "Traced %d frames, output written.\n", frames);
    }
    return 0;
}
