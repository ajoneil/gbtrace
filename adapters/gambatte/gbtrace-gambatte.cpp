// gbtrace-gambatte: Adapter that uses libgambatte to produce .gbtrace files.
//
// Links against libgambatte (gambatte-speedrun) without any source modifications.
// Uses the public traceCallback API to capture per-instruction CPU state.
//
// Usage:
//   gbtrace-gambatte --rom test.gb [--output trace.gbtrace] [--frames 3000] [--model dmg]
//
// Build:
//   See Makefile in this directory.

#include <gambatte.h>

#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

// --- Globals for trace callback context ---
// (traceCallback is a raw function pointer with void* data, so we use globals)

static FILE *g_output = nullptr;
static gambatte::GB *g_gb = nullptr;

// Convert a sample offset to T-cycles.
// In normal speed: 1 sample = 4 T-cycles.
// In CGB double speed: 1 sample = 8 T-cycles, but the Game Boy still runs
// at the same wall clock rate, so we always multiply by 4 for the trace
// (the spec uses T-cycles at 4.194304 MHz).
static inline unsigned long long samples_to_tcycles(unsigned long long samples) {
    return samples * 4;
}

static std::string hex8(int val) {
    char buf[8];
    std::snprintf(buf, sizeof(buf), "0x%02X", val & 0xFF);
    return buf;
}

static std::string hex16(int val) {
    char buf[8];
    std::snprintf(buf, sizeof(buf), "0x%04X", val & 0xFFFF);
    return buf;
}

// Trace callback — fired before each instruction.
// The void* points to an array of values:
//   [0] = cycleOffset (sample-based)
//   [1] = PC, [2] = SP
//   [3] = A, [4] = B, [5] = C, [6] = D, [7] = E, [8] = F, [9] = H, [10] = L
//   [11] = prefetched (bool)
//   [12] = opcode << 16 | operandHigh << 8 | operandLow
//   [13] = LY
static void trace_callback(void *data) {
    int *r = static_cast<int *>(data);

    long long cycle_offset = static_cast<long long>(r[0]);
    unsigned long long total_samples = g_gb->timeNow() + cycle_offset;
    unsigned long long tcycles = samples_to_tcycles(total_samples);

    int pc = r[1];
    int sp = r[2];
    int a  = r[3];
    int b  = r[4];
    int c  = r[5];
    int d  = r[6];
    int e  = r[7];
    int f  = r[8];
    int h  = r[9];
    int l  = r[10];
    int opcode = (r[12] >> 16) & 0xFF;

    std::fprintf(g_output,
        "{\"cy\":%llu,\"pc\":\"%s\",\"sp\":\"%s\","
        "\"a\":\"%s\",\"f\":\"%s\","
        "\"b\":\"%s\",\"c\":\"%s\","
        "\"d\":\"%s\",\"e\":\"%s\","
        "\"h\":\"%s\",\"l\":\"%s\","
        "\"op\":\"%s\"}\n",
        tcycles,
        hex16(pc).c_str(), hex16(sp).c_str(),
        hex8(a).c_str(), hex8(f).c_str(),
        hex8(b).c_str(), hex8(c).c_str(),
        hex8(d).c_str(), hex8(e).c_str(),
        hex8(h).c_str(), hex8(l).c_str(),
        hex8(opcode).c_str());
}

// Compute SHA-256 of a file (shelling out to sha256sum for simplicity).
static std::string sha256_file(const std::string &path) {
    std::string cmd = "sha256sum \"" + path + "\"";
    FILE *pipe = popen(cmd.c_str(), "r");
    if (!pipe) return "unknown";
    char buf[128];
    std::string result;
    if (std::fgets(buf, sizeof(buf), pipe)) {
        result = buf;
        // sha256sum outputs "hash  filename\n"
        auto space = result.find(' ');
        if (space != std::string::npos)
            result = result.substr(0, space);
    }
    pclose(pipe);
    return result;
}

static void write_header(FILE *out, const std::string &rom_sha256,
                          const std::string &model) {
    std::fprintf(out,
        "{\"_header\":true,\"format_version\":\"0.1.0\","
        "\"emulator\":\"gambatte-speedrun\",\"emulator_version\":\"r730+\","
        "\"rom_sha256\":\"%s\",\"model\":\"%s\","
        "\"boot_rom\":\"skip\",\"profile\":\"cpu_basic\","
        "\"fields\":[\"cy\",\"pc\",\"sp\",\"a\",\"f\",\"b\",\"c\",\"d\",\"e\",\"h\",\"l\",\"op\"],"
        "\"trigger\":\"instruction\"}\n",
        rom_sha256.c_str(), model.c_str());
}

static void print_usage(const char *argv0) {
    std::fprintf(stderr,
        "Usage: %s --rom <file.gb> [options]\n"
        "\n"
        "Options:\n"
        "  --rom <path>       ROM file to run (required)\n"
        "  --output <path>    Output trace file (default: stdout)\n"
        "  --frames <n>       Stop after N frames (default: 3000)\n"
        "  --model <model>    dmg or cgb (default: dmg)\n",
        argv0);
}

int main(int argc, char *argv[]) {
    std::string rom_path;
    std::string output_path;
    int max_frames = 3000;
    std::string model = "DMG-B";
    unsigned load_flags = gambatte::GB::LoadFlag::NO_BIOS;

    // Parse args
    for (int i = 1; i < argc; i++) {
        std::string arg = argv[i];
        if (arg == "--rom" && i + 1 < argc) {
            rom_path = argv[++i];
        } else if (arg == "--output" && i + 1 < argc) {
            output_path = argv[++i];
        } else if (arg == "--frames" && i + 1 < argc) {
            max_frames = std::atoi(argv[++i]);
        } else if (arg == "--model" && i + 1 < argc) {
            std::string m = argv[++i];
            if (m == "cgb" || m == "CGB") {
                model = "CGB-E";
                load_flags |= gambatte::GB::LoadFlag::CGB_MODE;
            }
        } else if (arg == "--help" || arg == "-h") {
            print_usage(argv[0]);
            return 0;
        }
    }

    if (rom_path.empty()) {
        print_usage(argv[0]);
        return 1;
    }

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

    // Set up a 64KB write buffer for performance
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

    // Write header
    std::string rom_hash = sha256_file(rom_path);
    write_header(g_output, rom_hash, model);

    // Set trace callback
    gb.setTraceCallback(trace_callback);

    // Run emulation frame by frame
    // runFor takes audio samples; Game Boy produces 35112 samples per frame
    // (at 2097152 Hz / ~59.73 fps)
    static const std::size_t SAMPLES_PER_FRAME = 35112;
    std::vector<gambatte::uint_least32_t> video_buf(160 * 144, 0);
    std::vector<gambatte::uint_least32_t> audio_buf(SAMPLES_PER_FRAME * 2 + 2064, 0);

    int frames = 0;
    while (frames < max_frames) {
        std::size_t samples = SAMPLES_PER_FRAME;
        std::ptrdiff_t result = gb.runFor(
            video_buf.data(), 160,
            audio_buf.data(), samples);

        if (result >= 0) {
            frames++;
        }
    }

    // Cleanup
    std::fflush(g_output);
    if (g_output != stdout) {
        std::fclose(g_output);
    }

    std::fprintf(stderr, "Traced %d frames, output written.\n", frames);
    return 0;
}
