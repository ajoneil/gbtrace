// gbtrace-mgba: Adapter that uses mGBA to produce .gbtrace files.
//
// Links against libmgba without any source modifications.
// Uses the mDebuggerModule callback API to capture per-instruction CPU state,
// and rawRead8 (peek) for IO registers.
//
// Usage:
//   gbtrace-mgba --rom test.gb --profile cpu_basic.toml --output trace.gbtrace
//
// Build:
//   See Makefile in this directory.

// Generated build flags (defines ENABLE_VFS etc.)
#include <mgba/flags.h>

#include <mgba/core/core.h>
#include <mgba/core/config.h>
#include <mgba/core/timing.h>
#include <mgba/debugger/debugger.h>
#include <mgba/gb/core.h>
#include <mgba/gb/interface.h>
#include <mgba/internal/gb/gb.h>
#include <mgba/internal/sm83/sm83.h>
#include <mgba-util/vfs.h>

#include "gbtrace.h"

#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// --- Field configuration ---

// Map of field name -> IO register address for fields read via rawRead8.
struct IOField { const char *name; unsigned short addr; };
static const struct IOField IO_FIELDS[] = {
    {"lcdc", 0xFF40}, {"stat", 0xFF41}, {"scy",  0xFF42}, {"scx",  0xFF43},
    {"ly",   0xFF44}, {"lyc",  0xFF45}, {"wy",   0xFF4A}, {"wx",   0xFF4B},
    {"bgp",  0xFF47}, {"obp0", 0xFF48}, {"obp1", 0xFF49}, {"dma",  0xFF46},
    {"div",  0xFF04}, {"tima", 0xFF05}, {"tma",  0xFF06}, {"tac",  0xFF07},
    {"if_",  0xFF0F}, {"ie",   0xFFFF},
    {"sb",   0xFF01}, {"sc",   0xFF02},
    /* APU registers */
    {"ch1_sweep", 0xFF10}, {"ch1_duty_len", 0xFF11}, {"ch1_vol_env", 0xFF12},
    {"ch1_freq_lo", 0xFF13}, {"ch1_freq_hi", 0xFF14},
    {"ch2_duty_len", 0xFF16}, {"ch2_vol_env", 0xFF17},
    {"ch2_freq_lo", 0xFF18}, {"ch2_freq_hi", 0xFF19},
    {"ch3_dac", 0xFF1A}, {"ch3_len", 0xFF1B}, {"ch3_vol", 0xFF1C},
    {"ch3_freq_lo", 0xFF1D}, {"ch3_freq_hi", 0xFF1E},
    {"ch4_len", 0xFF20}, {"ch4_vol_env", 0xFF21},
    {"ch4_freq", 0xFF22}, {"ch4_control", 0xFF23},
    {"master_vol", 0xFF24}, {"sound_pan", 0xFF25}, {"sound_on", 0xFF26},
    {NULL, 0}
};

// CPU register fields
static const char *REG8_FIELDS[] = {"a", "f", "b", "c", "d", "e", "h", "l", NULL};
static const char *REG16_FIELDS[] = {"pc", "sp", NULL};

static int find_io_addr(const char *name) {
    for (const struct IOField *f = IO_FIELDS; f->name; f++) {
        if (strcmp(f->name, name) == 0) return f->addr;
    }
    return -1;
}

static bool is_in_list(const char *name, const char **list) {
    for (; *list; list++) {
        if (strcmp(name, *list) == 0) return true;
    }
    return false;
}

// --- Profile (minimal TOML parser, matching other adapters) ---

#define MAX_FIELDS 128
#define MAX_NAME 64

#define MAX_MEMORY_FIELDS 16

struct MemoryField {
    char name[MAX_NAME];
    unsigned short addr;
};

struct Profile {
    char name[MAX_NAME];
    char trigger[MAX_NAME];
    char fields[MAX_FIELDS][MAX_NAME];
    int nfields;
    struct MemoryField memory[MAX_MEMORY_FIELDS];
    int nmemory;
};

static struct Profile load_profile(const char *path) {
    struct Profile prof = {0};

    GbtraceProfile *p = gbtrace_profile_load(path);
    if (!p) {
        fprintf(stderr, "Error: cannot load profile '%s'\n", path);
        exit(1);
    }

    strncpy(prof.name, gbtrace_profile_name(p), MAX_NAME - 1);
    strncpy(prof.trigger, gbtrace_profile_trigger(p), MAX_NAME - 1);

    size_t nfields = gbtrace_profile_num_fields(p);
    for (size_t i = 0; i < nfields && (int)i < MAX_FIELDS; i++) {
        strncpy(prof.fields[prof.nfields], gbtrace_profile_field_name(p, i), MAX_NAME - 1);
        prof.nfields++;
    }

    size_t nmem = gbtrace_profile_num_memory(p);
    for (size_t i = 0; i < nmem && (int)i < MAX_MEMORY_FIELDS; i++) {
        strncpy(prof.memory[prof.nmemory].name, gbtrace_profile_memory_name(p, i), MAX_NAME - 1);
        prof.memory[prof.nmemory].addr = gbtrace_profile_memory_addr(p, i);
        prof.nmemory++;
    }

    gbtrace_profile_free(p);
    return prof;
}

// --- Emitter configuration ---

enum EmitterSource { SRC_REG8, SRC_REG16, SRC_IO, SRC_IME, SRC_PIX };

struct FieldEmitter {
    char name[MAX_NAME];
    enum EmitterSource source;
    int io_addr; // for SRC_IO
};

static struct FieldEmitter g_emitters[MAX_FIELDS];
static int g_nemitters = 0;
static int g_has_pix = 0;
static uint32_t g_video_buf[160 * 144];
static char g_pending_pix[160 * 144 + 1];

static void capture_mgba_frame(void) {
    for (int i = 0; i < 160 * 144; i++) {
        uint32_t rgba = g_video_buf[i];
        unsigned r = (rgba >> 0) & 0xFF;
        char shade;
        if (r >= 0xC0) shade = '0';
        else if (r >= 0x70) shade = '1';
        else if (r >= 0x30) shade = '2';
        else shade = '3';
        g_pending_pix[i] = shade;
    }
    g_pending_pix[160 * 144] = '\0';
}

/* --- Reference matching --- */
static char g_reference_pix[160 * 144 + 1];
static int g_has_reference = 0;

static int load_reference(const char *path) {
    FILE *f = fopen(path, "rb");
    if (!f) return 0;
    size_t n = fread(g_reference_pix, 1, 160 * 144, f);
    fclose(f);
    g_reference_pix[n] = '\0';
    /* Strip trailing newlines */
    while (n > 0 && (g_reference_pix[n-1] == '\n' || g_reference_pix[n-1] == '\r')) {
        n--;
        g_reference_pix[n] = '\0';
    }
    return (int)n == 160 * 144;
}

static void build_emitters(const struct Profile *prof) {
    g_nemitters = 0;
    for (int i = 0; i < prof->nfields; i++) {
        const char *field = prof->fields[i];



        struct FieldEmitter *em = &g_emitters[g_nemitters];
        strncpy(em->name, field, MAX_NAME - 1);

        if (strcmp(field, "pix") == 0) {
            em->source = SRC_PIX;
            g_has_pix = 1;
            g_nemitters++;
            continue;
        } else if (strcmp(field, "ime") == 0) {
            em->source = SRC_IME;
        } else if (is_in_list(field, REG8_FIELDS)) {
            em->source = SRC_REG8;
        } else if (is_in_list(field, REG16_FIELDS)) {
            em->source = SRC_REG16;
        } else {
            int addr = find_io_addr(field);
            if (addr < 0) {
                // Check memory fields from profile
                for (int m = 0; m < prof->nmemory; m++) {
                    if (strcmp(field, prof->memory[m].name) == 0) {
                        addr = prof->memory[m].addr;
                        break;
                    }
                }
            }
            if (addr >= 0) {
                em->source = SRC_IO;
                em->io_addr = addr;
            } else {
                fprintf(stderr, "Warning: unknown field '%s', skipping\n", field);
                continue;
            }
        }
        g_nemitters++;
    }
}

// --- Globals ---

static struct Profile g_profile;
static struct mCore *g_core = NULL;

static unsigned char g_stop_serial_byte = 0;
static int g_stop_serial_count = 1;
static int g_stop_serial_seen = 0;
static int g_stop_serial_active = 0;
static int g_stop_serial_triggered = 0;
static int g_stop_opcode = -1;
static int g_stop_opcode_triggered = 0;

// --- FFI writer ---
static GbtraceWriter *g_writer = NULL;
static int g_writer_cols[MAX_FIELDS];
static int g_writer_ly_col = -1;

static int read_reg8(struct SM83Core *cpu, const char *name) {
    if (strcmp(name, "a") == 0) return cpu->a;
    if (strcmp(name, "f") == 0) return cpu->f.packed;
    if (strcmp(name, "b") == 0) return cpu->b;
    if (strcmp(name, "c") == 0) return cpu->c;
    if (strcmp(name, "d") == 0) return cpu->d;
    if (strcmp(name, "e") == 0) return cpu->e;
    if (strcmp(name, "h") == 0) return cpu->h;
    if (strcmp(name, "l") == 0) return cpu->l;
    return 0;
}

static int read_reg16(struct SM83Core *cpu, const char *name) {
    if (strcmp(name, "pc") == 0) return cpu->pc;
    if (strcmp(name, "sp") == 0) return cpu->sp;
    return 0;
}

// --- Debugger module for per-instruction tracing ---

struct TraceModule {
    struct mDebuggerModule d; // must be first
};

static void emit_entry(struct mCore *core) {
    struct SM83Core *cpu = core->cpu;

    // Gather ly and pix_len for boundary check
    uint8_t ly_val = 255;
    size_t pix_len = 0;
    if (g_writer_ly_col >= 0) {
        ly_val = core->rawRead8(core, 0xFF44, -1);
    }
    if (g_has_pix) {
        pix_len = strlen(g_pending_pix);
    }
    gbtrace_writer_check_boundary(g_writer, ly_val, pix_len);

    // Set all field values
    for (int i = 0; i < g_nemitters; i++) {
        int col = g_writer_cols[i];
        if (col < 0) continue;
        struct FieldEmitter *em = &g_emitters[i];
        switch (em->source) {
        case SRC_REG8:
            gbtrace_writer_set_u8(g_writer, col, read_reg8(cpu, em->name));
            break;
        case SRC_REG16:
            gbtrace_writer_set_u16(g_writer, col, read_reg16(cpu, em->name));
            break;
        case SRC_IO:
            if (em->io_addr >= 0xFF10 && em->io_addr <= 0xFF3F) {
                /* Read APU registers directly from memory.io[] to avoid
                   mGBA logging warnings for write-only registers. */
                struct GB *gb = (struct GB *) core->board;
                gbtrace_writer_set_u8(g_writer, col, gb->memory.io[em->io_addr & 0xFF]);
            } else {
                gbtrace_writer_set_u8(g_writer, col, core->rawRead8(core, em->io_addr, -1));
            }
            break;
        case SRC_IME:
            gbtrace_writer_set_bool(g_writer, col, cpu->irqPending);
            break;
        case SRC_PIX:
            gbtrace_writer_set_str(g_writer, col, g_pending_pix, strlen(g_pending_pix));
            g_pending_pix[0] = '\0';
            break;
        }
    }

    gbtrace_writer_finish_entry(g_writer);
}

static void check_stop_conditions(struct mCore *core) {
    struct SM83Core *cpu = core->cpu;

    /* Check opcode stop condition */
    if (g_stop_opcode >= 0 && !g_stop_opcode_triggered) {
        uint8_t op = core->rawRead8(core, cpu->pc, -1);
        if (op == (uint8_t)g_stop_opcode) {
            g_stop_opcode_triggered = 1;
        }
    }

    /* Check serial stop condition: detect rising edge of SC bit 7 */
    if (g_stop_serial_active && !g_stop_serial_triggered) {
        static int prev_sc_high = 0;
        unsigned char sc = core->rawRead8(core, 0xFF02, -1);
        int sc_high = (sc & 0x80) != 0;
        if (sc_high && !prev_sc_high) {
            unsigned char sb = core->rawRead8(core, 0xFF01, -1);
            if (sb == g_stop_serial_byte) {
                g_stop_serial_seen++;
                if (g_stop_serial_seen >= g_stop_serial_count) {
                    g_stop_serial_triggered = 1;
                }
            }
        }
        prev_sc_high = sc_high;
    }
}

static void trace_custom(struct mDebuggerModule *mod) {
    emit_entry(mod->p->core);
    check_stop_conditions(mod->p->core);
}

// --- SHA-256 ---

static char *sha256_file(const char *path) {
    static char result[128];
    char cmd[4096];
    snprintf(cmd, sizeof(cmd), "sha256sum \"%s\"", path);
    FILE *pipe = popen(cmd, "r");
    if (!pipe) return "unknown";
    if (fgets(result, sizeof(result), pipe)) {
        char *space = strchr(result, ' ');
        if (space) *space = '\0';
    }
    pclose(pipe);
    return result;
}

// --- Main ---

static void print_usage(const char *argv0) {
    fprintf(stderr,
        "Usage: %s --rom <file.gb> --profile <profile.toml> [options]\n"
        "\n"
        "Options:\n"
        "  --rom <path>         ROM file to run (required)\n"
        "  --profile <path>     Capture profile TOML file (required)\n"
        "  --output <path>      Output trace file (required)\n"
        "  --frames <n>         Stop after N frames (default: 3000)\n"
        "  --stop-when <A=V>    Stop when memory ADDR equals VAL (hex, e.g. A000=80)\n"
        "  --stop-on-serial <B> Stop when byte B (hex) is sent via serial (e.g. 0A for newline)\n"
        "  --stop-serial-count <N> Stop on Nth occurrence of serial byte (default: 1)\n"
        "  --model <model>      dmg or cgb (default: dmg)\n"
        "  --boot-rom <path>    Boot ROM file (default: skip boot)\n",
        argv0);
}

int main(int argc, char *argv[]) {
    const char *rom_path = NULL;
    const char *profile_path = NULL;
    const char *output_path = NULL;
    const char *boot_rom_path = NULL;
    const char *reference_path = NULL;
    int extra_frames = 0;
    int max_frames = 3000;
    const char *model = "DMG-B";
    struct { unsigned short addr; unsigned char value; } stop_conditions[16];
    int num_stop_conditions = 0;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--rom") == 0 && i + 1 < argc) {
            rom_path = argv[++i];
        } else if (strcmp(argv[i], "--profile") == 0 && i + 1 < argc) {
            profile_path = argv[++i];
        } else if (strcmp(argv[i], "--output") == 0 && i + 1 < argc) {
            output_path = argv[++i];
        } else if (strcmp(argv[i], "--frames") == 0 && i + 1 < argc) {
            max_frames = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--stop-when") == 0 && i + 1 < argc) {
            const char *spec = argv[++i];
            const char *eq = strchr(spec, '=');
            if (!eq) { fprintf(stderr, "Error: --stop-when format is ADDR=VAL (e.g. A000=80)\n"); return 1; }
            if (num_stop_conditions < 16) {
                stop_conditions[num_stop_conditions].addr = (unsigned short)strtoul(spec, NULL, 16);
                stop_conditions[num_stop_conditions].value = (unsigned char)strtoul(eq + 1, NULL, 16);
                num_stop_conditions++;
            }
        } else if (strcmp(argv[i], "--stop-on-serial") == 0 && i + 1 < argc) {
            g_stop_serial_byte = (unsigned char)strtoul(argv[++i], NULL, 16);
            g_stop_serial_active = 1;
        } else if (strcmp(argv[i], "--stop-serial-count") == 0 && i + 1 < argc) {
            g_stop_serial_count = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--boot-rom") == 0 && i + 1 < argc) {
            boot_rom_path = argv[++i];
        } else if (strcmp(argv[i], "--model") == 0 && i + 1 < argc) {
            const char *m = argv[++i];
            if (strcmp(m, "cgb") == 0 || strcmp(m, "CGB") == 0) {
                model = "CGB-E";
            }
        } else if (strcmp(argv[i], "--reference") == 0 && i + 1 < argc) {
            reference_path = argv[++i];
        } else if (strcmp(argv[i], "--extra-frames") == 0 && i + 1 < argc) {
            extra_frames = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--stop-opcode") == 0 && i + 1 < argc) {
            g_stop_opcode = (int)strtoul(argv[++i], NULL, 16);
        } else if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            print_usage(argv[0]);
            return 0;
        }
    }

    if (!rom_path || !profile_path) {
        print_usage(argv[0]);
        return 1;
    }

    if (!output_path) {
        fprintf(stderr, "Error: --output is required\n");
        print_usage(argv[0]);
        return 1;
    }

    // Load profile
    g_profile = load_profile(profile_path);
    build_emitters(&g_profile);
    fprintf(stderr, "Profile: %s (%d fields)\n", g_profile.name, g_profile.nfields);

    // Create core by auto-detecting from ROM file
    g_core = mCoreFind(rom_path);
    if (!g_core) {
        fprintf(stderr, "Error: failed to create core for '%s'\n", rom_path);
        return 1;
    }

    mCoreInitConfig(g_core, NULL);

    // Configure options
    if (!boot_rom_path) {
        mCoreConfigSetIntValue(&g_core->config, "skipBios", 1);
        mCoreConfigSetIntValue(&g_core->config, "useBios", 0);
    } else {
        mCoreConfigSetIntValue(&g_core->config, "skipBios", 0);
        mCoreConfigSetIntValue(&g_core->config, "useBios", 1);
    }

    // Force hardware model via config so auto-detect doesn't pick CGB for hybrid ROMs
    if (strcmp(model, "CGB-E") == 0) {
        mCoreConfigSetValue(&g_core->config, "gb.model", "CGB");
        mCoreConfigSetValue(&g_core->config, "cgb.model", "CGB");
    } else {
        mCoreConfigSetValue(&g_core->config, "gb.model", "DMG");
        mCoreConfigSetValue(&g_core->config, "cgb.model", "DMG");
    }

    g_core->init(g_core);

    // Set up video buffer (used for pixel capture when pix field is present)
    g_core->setVideoBuffer(g_core, g_video_buf, 160);

    if (!mCoreLoadFile(g_core, rom_path)) {
        fprintf(stderr, "Error: failed to load ROM '%s'\n", rom_path);
        return 1;
    }

    // Load boot ROM if provided
    const char *boot_rom_info = "skip";
    static char boot_hash[128];
    if (boot_rom_path) {
        struct VFile *bios = VFileOpen(boot_rom_path, O_RDONLY);
        if (!bios || !g_core->loadBIOS(g_core, bios, 0)) {
            fprintf(stderr, "Error: failed to load boot ROM '%s'\n", boot_rom_path);
            return 1;
        }
        strncpy(boot_hash, sha256_file(boot_rom_path), sizeof(boot_hash) - 1);
        boot_rom_info = boot_hash;
        fprintf(stderr, "Boot ROM: %s (sha256: %s)\n", boot_rom_path, boot_rom_info);
    }

    // Force the hardware model on the internal GB struct before reset,
    // since the config-based approach doesn't reliably override auto-detection.
    {
        struct GB *gb = (struct GB *) g_core->board;
        if (strcmp(model, "CGB-E") == 0) {
            gb->model = GB_MODEL_CGB;
        } else {
            gb->model = GB_MODEL_DMG;
        }
    }

    g_core->reset(g_core);

    // Init FFI writer
    char *rom_hash = sha256_file(rom_path);

    {
        char header_json[4096];
        int hpos = snprintf(header_json, sizeof(header_json),
            "{\"_header\":true,\"format_version\":\"0.1.0\","
            "\"emulator\":\"mgba\",\"emulator_version\":\"0.10.x\","
            "\"rom_sha256\":\"%s\",\"model\":\"%s\","
            "\"boot_rom\":\"%s\",\"profile\":\"%s\","
            "\"fields\":[",
            rom_hash, model, boot_rom_info, g_profile.name);
        for (int i = 0; i < g_nemitters; i++) {
            if (i > 0) hpos += snprintf(header_json + hpos, sizeof(header_json) - hpos, ",");
            hpos += snprintf(header_json + hpos, sizeof(header_json) - hpos,
                             "\"%s\"", g_emitters[i].name);
        }
        hpos += snprintf(header_json + hpos, sizeof(header_json) - hpos,
                         "],\"trigger\":\"instruction\"}");

        g_writer = gbtrace_writer_new(output_path, header_json, hpos);
        if (!g_writer) {
            fprintf(stderr, "Error: failed to create trace writer\n");
            return 1;
        }
    }

    // Cache column indices
    for (int i = 0; i < g_nemitters; i++) {
        g_writer_cols[i] = gbtrace_writer_find_field(g_writer, g_emitters[i].name);
    }
    g_writer_ly_col = gbtrace_writer_find_field(g_writer, "ly");

    /* Mark entry 0 as a frame boundary */
    gbtrace_writer_mark_frame(g_writer);

    fprintf(stderr, "Output: %s\n", output_path);

    // Emit the initial CPU state (the debugger callback misses the first
    // instruction because it's attached after reset)
    emit_entry(g_core);

    // Set up debugger with trace module
    struct mDebugger debugger;
    memset(&debugger, 0, sizeof(debugger));
    mDebuggerInit(&debugger);
    mDebuggerAttach(&debugger, g_core);

    struct TraceModule trace_mod;
    memset(&trace_mod, 0, sizeof(trace_mod));
    trace_mod.d.type = DEBUGGER_CUSTOM;
    trace_mod.d.custom = trace_custom;

    mDebuggerAttachModule(&debugger, &trace_mod.d);
    mDebuggerModuleSetNeedsCallback(&trace_mod.d);

    // Run
    for (int i = 0; i < num_stop_conditions; i++) {
        fprintf(stderr, "Stop condition: [0x%04X] == 0x%02X\n",
                stop_conditions[i].addr, stop_conditions[i].value);
    }
    if (g_stop_serial_active) {
        fprintf(stderr, "Stop on serial byte: 0x%02X (after %d occurrence%s)\n",
                g_stop_serial_byte, g_stop_serial_count,
                g_stop_serial_count == 1 ? "" : "s");
    }

    /* Load reference image */
    if (reference_path) {
        if (load_reference(reference_path)) {
            g_has_reference = 1;
            fprintf(stderr, "Reference: %s (%d pixels)\n", reference_path, 160 * 144);
        } else {
            fprintf(stderr, "Warning: could not load reference '%s'\n", reference_path);
        }
    }

    int frames = 0;
    int stopped_early = 0;
    int remaining_extra = -1;  /* -1 = not triggered yet */
    for (frames = 0; frames < max_frames; frames++) {
        mDebuggerRunFrame(&debugger);
        if (g_has_pix || g_has_reference) {
            capture_mgba_frame();
        }
        gbtrace_writer_mark_frame(g_writer);

        /* Check reference match (immediate stop) */
        if (g_has_reference && memcmp(g_pending_pix, g_reference_pix, 160 * 144) == 0) {
            fprintf(stderr, "Reference match at frame %d\n", frames + 1);
            mDebuggerRunFrame(&debugger);
            stopped_early = 1;
            frames++;
            break;
        }

        /* If in extra-frames countdown, just decrement */
        if (remaining_extra >= 0) {
            if (remaining_extra == 0) {
                stopped_early = 1;
                frames++;
                break;
            }
            remaining_extra--;
            continue;
        }

        /* Check stop conditions — start countdown */
        for (int sc = 0; sc < num_stop_conditions; sc++) {
            if (g_core->rawRead8(g_core, stop_conditions[sc].addr, -1) == stop_conditions[sc].value) {
                fprintf(stderr, "Stop condition met at frame %d, running %d extra frame%s\n",
                        frames + 1, extra_frames, extra_frames == 1 ? "" : "s");
                remaining_extra = extra_frames;
                break;
            }
        }
        if (remaining_extra >= 0 && remaining_extra == 0) {
            stopped_early = 1;
            frames++;
            break;
        }
        if (g_stop_serial_triggered) {
            fprintf(stderr, "Serial stop at frame %d, running %d extra frame%s\n",
                    frames + 1, extra_frames, extra_frames == 1 ? "" : "s");
            remaining_extra = extra_frames;
            if (remaining_extra == 0) {
                stopped_early = 1;
                frames++;
                break;
            }
        }
        if (g_stop_opcode_triggered) {
            fprintf(stderr, "Opcode stop at frame %d, running %d extra frame%s\n",
                    frames + 1, extra_frames, extra_frames == 1 ? "" : "s");
            remaining_extra = extra_frames;
            if (remaining_extra == 0) {
                stopped_early = 1;
                frames++;
                break;
            }
        }
    }

    gbtrace_writer_close(g_writer);
    g_writer = NULL;

    mDebuggerDetachModule(&debugger, &trace_mod.d);
    mDebuggerDeinit(&debugger);
    g_core->deinit(g_core);

    if (stopped_early) {
        fprintf(stderr, "Stop condition met at frame %d.\n", frames);
    }
    fprintf(stderr, "Traced %d frames, output written.\n", frames);
    return 0;
}
