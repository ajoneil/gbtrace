// gbtrace-mgba: Adapter that uses mGBA to produce .gbtrace files.
//
// Links against libmgba without any source modifications.
// Uses the mDebuggerModule callback API to capture per-instruction CPU state,
// and rawRead8 (peek) for IO registers.
//
// Usage:
//   gbtrace-mgba --rom test.gb --profile cpu_basic.toml [--output trace.gbtrace]
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
#include <mgba/internal/gb/gb.h>
#include <mgba/internal/sm83/sm83.h>
#include <mgba-util/vfs.h>

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

#define MAX_FIELDS 32
#define MAX_NAME 64

struct Profile {
    char name[MAX_NAME];
    char trigger[MAX_NAME];
    char fields[MAX_FIELDS][MAX_NAME];
    int nfields; // includes "cy" at index 0
};

static void trim(char *s) {
    while (*s && (*s == ' ' || *s == '\t')) memmove(s, s+1, strlen(s));
    char *end = s + strlen(s) - 1;
    while (end > s && (*end == ' ' || *end == '\t' || *end == '\n' || *end == '\r')) *end-- = '\0';
}

static void strip_quotes(char *s) {
    size_t len = strlen(s);
    if (len >= 2 && s[0] == '"' && s[len-1] == '"') {
        memmove(s, s+1, len-2);
        s[len-2] = '\0';
    }
}

static struct Profile parse_profile(const char *path) {
    struct Profile prof = {0};
    strcpy(prof.trigger, "instruction");
    strcpy(prof.fields[0], "cy");
    prof.nfields = 1;

    FILE *f = fopen(path, "r");
    if (!f) { fprintf(stderr, "Error: cannot open profile '%s'\n", path); exit(1); }

    char line[1024];
    while (fgets(line, sizeof(line), f)) {
        // Strip comments
        char *hash = strchr(line, '#');
        if (hash) *hash = '\0';

        char *eq = strchr(line, '=');
        if (!eq) continue;

        *eq = '\0';
        char *key = line;
        char *val = eq + 1;
        trim(key); trim(val);

        if (strcmp(key, "name") == 0) {
            strip_quotes(val);
            strncpy(prof.name, val, MAX_NAME - 1);
        } else if (strcmp(key, "trigger") == 0) {
            strip_quotes(val);
            strncpy(prof.trigger, val, MAX_NAME - 1);
        } else if (val[0] == '[') {
            // Parse array: ["f1", "f2", ...]
            char *start = strchr(val, '[');
            char *end = strchr(val, ']');
            if (start && end) {
                *end = '\0';
                char *tok = strtok(start + 1, ",");
                while (tok && prof.nfields < MAX_FIELDS) {
                    trim(tok); strip_quotes(tok);
                    if (tok[0] && strcmp(tok, "cy") != 0) {
                        strncpy(prof.fields[prof.nfields], tok, MAX_NAME - 1);
                        prof.nfields++;
                    }
                    tok = strtok(NULL, ",");
                }
            }
        }
    }
    fclose(f);
    return prof;
}

// --- Emitter configuration ---

enum EmitterSource { SRC_REG8, SRC_REG16, SRC_IO, SRC_OPCODE, SRC_IME };

struct FieldEmitter {
    char name[MAX_NAME];
    enum EmitterSource source;
    int io_addr; // for SRC_IO
};

static struct FieldEmitter g_emitters[MAX_FIELDS];
static int g_nemitters = 0;

static void build_emitters(const struct Profile *prof) {
    g_nemitters = 0;
    for (int i = 0; i < prof->nfields; i++) {
        const char *field = prof->fields[i];
        if (strcmp(field, "cy") == 0) continue;

        struct FieldEmitter *em = &g_emitters[g_nemitters];
        strncpy(em->name, field, MAX_NAME - 1);

        if (strcmp(field, "op") == 0) {
            em->source = SRC_OPCODE;
        } else if (strcmp(field, "ime") == 0) {
            em->source = SRC_IME;
        } else if (is_in_list(field, REG8_FIELDS)) {
            em->source = SRC_REG8;
        } else if (is_in_list(field, REG16_FIELDS)) {
            em->source = SRC_REG16;
        } else {
            int addr = find_io_addr(field);
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

static FILE *g_output = NULL;
static struct Profile g_profile;
static struct mCore *g_core = NULL;

// --- Formatting helpers ---

static inline void fput_hex8(FILE *out, int val) {
    fprintf(out, "\"0x%02X\"", val & 0xFF);
}

static inline void fput_hex16(FILE *out, int val) {
    fprintf(out, "\"0x%04X\"", val & 0xFFFF);
}

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
    struct GB *gb = core->board;

    // Cycle count: globalCycles + cpu->cycles gives 8MHz ticks; /2 = T-cycles
    uint64_t ticks = gb->timing.globalCycles + (uint64_t)cpu->cycles;
    uint64_t tcycles = ticks / 2;

    fprintf(g_output, "{\"cy\":%llu", (unsigned long long)tcycles);

    for (int i = 0; i < g_nemitters; i++) {
        struct FieldEmitter *em = &g_emitters[i];
        fprintf(g_output, ",\"%s\":", em->name);
        switch (em->source) {
        case SRC_REG8:
            fput_hex8(g_output, read_reg8(cpu, em->name));
            break;
        case SRC_REG16:
            fput_hex16(g_output, read_reg16(cpu, em->name));
            break;
        case SRC_IO:
            fput_hex8(g_output, core->rawRead8(core, em->io_addr, -1));
            break;
        case SRC_OPCODE:
            fput_hex8(g_output, core->rawRead8(core, cpu->pc, -1));
            break;
        case SRC_IME:
            fprintf(g_output, cpu->irqPending ? "true" : "false");
            break;
        }
    }

    fprintf(g_output, "}\n");
}

static void trace_custom(struct mDebuggerModule *mod) {
    emit_entry(mod->p->core);
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

// --- Header ---

static void write_header(FILE *out, const struct Profile *prof,
                          const char *rom_sha256, const char *model,
                          const char *boot_rom_info) {
    fprintf(out,
        "{\"_header\":true,\"format_version\":\"0.1.0\","
        "\"emulator\":\"mgba\",\"emulator_version\":\"0.10.x\","
        "\"rom_sha256\":\"%s\",\"model\":\"%s\","
        "\"boot_rom\":\"%s\",\"profile\":\"%s\","
        "\"fields\":[",
        rom_sha256, model, boot_rom_info, prof->name);

    for (int i = 0; i < prof->nfields; i++) {
        if (i > 0) fprintf(out, ",");
        fprintf(out, "\"%s\"", prof->fields[i]);
    }

    fprintf(out, "],\"trigger\":\"%s\",\"cy_unit\":\"tcycle\"}\n", prof->trigger);
}

// --- Main ---

static void print_usage(const char *argv0) {
    fprintf(stderr,
        "Usage: %s --rom <file.gb> --profile <profile.toml> [options]\n"
        "\n"
        "Options:\n"
        "  --rom <path>         ROM file to run (required)\n"
        "  --profile <path>     Capture profile TOML file (required)\n"
        "  --output <path>      Output trace file (default: stdout)\n"
        "  --frames <n>         Stop after N frames (default: 3000)\n"
        "  --model <model>      dmg or cgb (default: dmg)\n"
        "  --boot-rom <path>    Boot ROM file (default: skip boot)\n",
        argv0);
}

int main(int argc, char *argv[]) {
    const char *rom_path = NULL;
    const char *profile_path = NULL;
    const char *output_path = NULL;
    const char *boot_rom_path = NULL;
    int max_frames = 3000;
    const char *model = "DMG-B";

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--rom") == 0 && i + 1 < argc) {
            rom_path = argv[++i];
        } else if (strcmp(argv[i], "--profile") == 0 && i + 1 < argc) {
            profile_path = argv[++i];
        } else if (strcmp(argv[i], "--output") == 0 && i + 1 < argc) {
            output_path = argv[++i];
        } else if (strcmp(argv[i], "--frames") == 0 && i + 1 < argc) {
            max_frames = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--boot-rom") == 0 && i + 1 < argc) {
            boot_rom_path = argv[++i];
        } else if (strcmp(argv[i], "--model") == 0 && i + 1 < argc) {
            const char *m = argv[++i];
            if (strcmp(m, "cgb") == 0 || strcmp(m, "CGB") == 0) {
                model = "CGB-E";
            }
        } else if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            print_usage(argv[0]);
            return 0;
        }
    }

    if (!rom_path || !profile_path) {
        print_usage(argv[0]);
        return 1;
    }

    // Load profile
    g_profile = parse_profile(profile_path);
    build_emitters(&g_profile);
    fprintf(stderr, "Profile: %s (%d fields)\n", g_profile.name, g_profile.nfields);

    // Open output
    if (!output_path || strcmp(output_path, "-") == 0) {
        g_output = stdout;
    } else {
        g_output = fopen(output_path, "w");
        if (!g_output) {
            fprintf(stderr, "Error: cannot open %s for writing\n", output_path);
            return 1;
        }
    }

    static char output_buf[64 * 1024];
    setvbuf(g_output, output_buf, _IOFBF, sizeof(output_buf));

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

    // Set up a dummy video buffer (required even headless)
    static uint32_t video_buf[160 * 144];
    g_core->setVideoBuffer(g_core, video_buf, 160);

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

    g_core->reset(g_core);

    // Write header
    char *rom_hash = sha256_file(rom_path);
    write_header(g_output, &g_profile, rom_hash, model, boot_rom_info);

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
    for (int frame = 0; frame < max_frames; frame++) {
        mDebuggerRunFrame(&debugger);
    }

    fflush(g_output);
    if (g_output != stdout) {
        fclose(g_output);
    }

    mDebuggerDetachModule(&debugger, &trace_mod.d);
    mDebuggerDeinit(&debugger);
    g_core->deinit(g_core);

    fprintf(stderr, "Traced %d frames, output written.\n", max_frames);
    return 0;
}
