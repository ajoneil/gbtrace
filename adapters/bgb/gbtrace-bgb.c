// gbtrace-bgb: Adapter that uses BGB to produce .gbtrace files.
//
// BGB is a closed-source Windows Game Boy emulator.  This adapter runs it
// under Wine in headless mode, using a per-instruction breakpoint with a
// debug-message format string to emit register/IO state.  A named pipe
// (FIFO) replaces BGB's debugmsg.txt so the trace is converted to native
// .gbtrace on the fly via the FFI writer — no intermediate files.
//
// BGB is downloaded automatically on first use (not redistributable).
//
// Usage:
//   gbtrace-bgb --rom test.gb --profile cpu_basic.toml --output trace.gbtrace
//
// Build:
//   See Makefile in this directory.

#define _POSIX_C_SOURCE 200809L
#define _DEFAULT_SOURCE

#include "gbtrace.h"

#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>
#include <errno.h>

// ── Field configuration ─────────────────────────────────────────────

// Map field name → IO register address for fields read via %($FFxx)%.
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

// BGB emits 16-bit register pairs (AF, BC, DE, HL) which we split into
// individual 8-bit fields during parsing.

static int find_io_addr(const char *name) {
    for (const struct IOField *f = IO_FIELDS; f->name; f++)
        if (strcmp(f->name, name) == 0) return f->addr;
    return -1;
}

// ── Profile loading ─────────────────────────────────────────────────

#define MAX_FIELDS 128
#define MAX_NAME 64
#define MAX_MEMORY_FIELDS 16

struct MemoryField { char name[MAX_NAME]; unsigned short addr; };

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
    if (!p) { fprintf(stderr, "Error: cannot load profile '%s'\n", path); exit(1); }

    strncpy(prof.name, gbtrace_profile_name(p), MAX_NAME - 1);
    strncpy(prof.trigger, gbtrace_profile_trigger(p), MAX_NAME - 1);

    size_t nf = gbtrace_profile_num_fields(p);
    for (size_t i = 0; i < nf && (int)i < MAX_FIELDS; i++) {
        strncpy(prof.fields[prof.nfields], gbtrace_profile_field_name(p, i), MAX_NAME - 1);
        prof.nfields++;
    }
    size_t nm = gbtrace_profile_num_memory(p);
    for (size_t i = 0; i < nm && (int)i < MAX_MEMORY_FIELDS; i++) {
        strncpy(prof.memory[prof.nmemory].name, gbtrace_profile_memory_name(p, i), MAX_NAME - 1);
        prof.memory[prof.nmemory].addr = gbtrace_profile_memory_addr(p, i);
        prof.nmemory++;
    }
    gbtrace_profile_free(p);
    return prof;
}

// ── Emitter setup ───────────────────────────────────────────────────

// How a field is sourced from BGB's debug message output.
enum EmitterSource {
    SRC_AF_HI,  // A register  (high byte of %AF%)
    SRC_AF_LO,  // F register  (low byte of %AF%)
    SRC_BC_HI, SRC_BC_LO,
    SRC_DE_HI, SRC_DE_LO,
    SRC_HL_HI, SRC_HL_LO,
    SRC_PC,     // %PC% (16-bit)
    SRC_SP,     // %SP% (16-bit)
    SRC_IME,    // %IME%
    SRC_IO,     // %($FFxx)% — position in output determined at build time
    SRC_SKIP,   // field not available from BGB
};

struct FieldEmitter {
    char name[MAX_NAME];
    enum EmitterSource source;
    int output_index;   // index into the space-separated output tokens
    int io_addr;        // for SRC_IO: the memory address
};

static struct FieldEmitter g_emitters[MAX_FIELDS];
static int g_nemitters = 0;

// Which 16-bit pairs / IO addrs are actually needed (to build the format string).
static bool g_need_af = false, g_need_bc = false;
static bool g_need_de = false, g_need_hl = false;
static bool g_need_pc = false, g_need_sp = false;
static bool g_need_ime = false;

struct IOSlot { unsigned short addr; int output_index; };
static struct IOSlot g_io_slots[MAX_FIELDS];
static int g_nio_slots = 0;

// Return the output token index for an IO address, adding a new slot if needed.
static int io_slot_for(unsigned short addr) {
    for (int i = 0; i < g_nio_slots; i++)
        if (g_io_slots[i].addr == addr) return g_io_slots[i].output_index;
    // Will be assigned after we know the base offset
    g_io_slots[g_nio_slots].addr = addr;
    g_io_slots[g_nio_slots].output_index = -1; // placeholder
    return g_nio_slots++;
}

static bool is_reg_field(const char *name, const char *reg) {
    return strcmp(name, reg) == 0;
}

static void plan_emitters(const struct Profile *prof) {
    g_nemitters = 0;
    g_nio_slots = 0;
    g_need_af = g_need_bc = g_need_de = g_need_hl = false;
    g_need_pc = g_need_sp = g_need_ime = false;

    // Pre-seed IO slots with memory fields so they get priority in the
    // format string (BGB has a 127-char limit and these are essential
    // for pass/fail detection in test suites like gbmicrotest).
    for (int m = 0; m < prof->nmemory; m++) {
        io_slot_for(prof->memory[m].addr);
    }

    for (int i = 0; i < prof->nfields; i++) {
        const char *field = prof->fields[i];
        struct FieldEmitter *em = &g_emitters[g_nemitters];
        strncpy(em->name, field, MAX_NAME - 1);
        em->output_index = -1;

        if (strcmp(field, "pix") == 0) {
            // Pixel capture not supported via BGB debug messages
            fprintf(stderr, "Warning: field 'pix' not supported by BGB adapter, skipping\n");
            em->source = SRC_SKIP;
        } else if (is_reg_field(field, "a"))   { em->source = SRC_AF_HI; g_need_af = true; }
        else if (is_reg_field(field, "f"))      { em->source = SRC_AF_LO; g_need_af = true; }
        else if (is_reg_field(field, "b"))      { em->source = SRC_BC_HI; g_need_bc = true; }
        else if (is_reg_field(field, "c"))      { em->source = SRC_BC_LO; g_need_bc = true; }
        else if (is_reg_field(field, "d"))      { em->source = SRC_DE_HI; g_need_de = true; }
        else if (is_reg_field(field, "e"))      { em->source = SRC_DE_LO; g_need_de = true; }
        else if (is_reg_field(field, "h"))      { em->source = SRC_HL_HI; g_need_hl = true; }
        else if (is_reg_field(field, "l"))      { em->source = SRC_HL_LO; g_need_hl = true; }
        else if (is_reg_field(field, "pc"))     { em->source = SRC_PC; g_need_pc = true; }
        else if (is_reg_field(field, "sp"))     { em->source = SRC_SP; g_need_sp = true; }
        else if (is_reg_field(field, "ime"))    { em->source = SRC_IME; g_need_ime = true; }
        else {
            // Check IO fields
            int addr = find_io_addr(field);
            if (addr < 0) {
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
                io_slot_for((unsigned short)addr);
            } else {
                fprintf(stderr, "Warning: field '%s' not available in BGB adapter, skipping\n", field);
                em->source = SRC_SKIP;
            }
        }
        g_nemitters++;
    }
}

// BGB has a 127-char limit per debug message.  To capture more fields we
// use multiple `any` breakpoints (comma-separated in -br), each prefixed
// with a letter (A, B, C, ...).  Each instruction produces N lines of
// output which the parser reassembles into one entry.
//
// The -br arg looks like:  any///A %PC% ...,any///B %($FF40)% ...
// Token indices are global across all lines.

#define MAX_BR_LINES 8
#define BR_MSG_LIMIT 127  // max chars per debug message (empirical)

static char g_br_lines[MAX_BR_LINES][256]; // format strings per line
static int  g_br_ntokens[MAX_BR_LINES];    // tokens per line (excluding prefix)
static int  g_num_br_lines = 0;

// Append a token expression to the current breakpoint line, starting a new
// line if it won't fit.  Returns the global token index assigned.
static int g_total_tokens = 0;

static void br_ensure_line(void) {
    if (g_num_br_lines == 0) {
        g_br_lines[0][0] = '\0';
        g_br_ntokens[0] = 0;
        g_num_br_lines = 1;
    }
}

static int br_append(const char *expr) {
    br_ensure_line();
    int line = g_num_br_lines - 1;
    int cur_len = (int)strlen(g_br_lines[line]);
    // +2 for prefix letter and space on empty line, +1 for space separator
    int prefix_overhead = (cur_len == 0) ? 2 : 0;
    int sep = (cur_len > 0 && !prefix_overhead) ? 1 : 0;
    int needed = prefix_overhead + sep + (int)strlen(expr);

    if (cur_len + needed > BR_MSG_LIMIT) {
        // Start a new line
        if (g_num_br_lines >= MAX_BR_LINES) return -1; // out of lines
        line = g_num_br_lines++;
        g_br_lines[line][0] = '\0';
        g_br_ntokens[line] = 0;
        cur_len = 0;
        prefix_overhead = 2;
        sep = 0;
    }

    char *buf = g_br_lines[line];
    int pos = cur_len;
    if (prefix_overhead) {
        // Write "X " prefix (A, B, C, ...)
        buf[pos++] = 'A' + line;
        buf[pos++] = ' ';
    }
    if (sep) buf[pos++] = ' ';
    strcpy(buf + pos, expr);
    g_br_ntokens[line]++;
    return g_total_tokens++;
}

// Stop condition token indices (checked during parsing to know when to stop).
static int g_stop_token_indices[16];
static int g_num_stop_tokens = 0;

// Build the complete -br argument (comma-separated breakpoints) and assign
// output_index to each emitter.  Returns total token count.
// Stop conditions are appended as boolean expressions (%($ADDR)=VAL%)
// that the parser checks to detect when to stop and kill BGB.
struct StopCond { unsigned short addr; unsigned char value; int negate; };

static int build_format_strings(const struct StopCond *stops, int nstops) {
    g_num_br_lines = 0;
    g_total_tokens = 0;
    g_num_stop_tokens = 0;

    // CPU register tokens
    int idx_pc = -1, idx_sp = -1, idx_af = -1, idx_bc = -1;
    int idx_de = -1, idx_hl = -1, idx_ime = -1;

    if (g_need_pc)  idx_pc  = br_append("%PC%");
    if (g_need_sp)  idx_sp  = br_append("%SP%");
    if (g_need_af)  idx_af  = br_append("%AF%");
    if (g_need_bc)  idx_bc  = br_append("%BC%");
    if (g_need_de)  idx_de  = br_append("%DE%");
    if (g_need_hl)  idx_hl  = br_append("%HL%");
    if (g_need_ime) idx_ime = br_append("%IME%");

    // IO register tokens
    for (int i = 0; i < g_nio_slots; i++) {
        char expr[20];
        snprintf(expr, sizeof(expr), "%%($%04X)%%", g_io_slots[i].addr);
        int idx = br_append(expr);
        if (idx < 0) {
            fprintf(stderr, "Warning: ran out of BGB breakpoint lines, "
                    "dropping IO field $%04X and beyond\n", g_io_slots[i].addr);
            break;
        }
        g_io_slots[i].output_index = idx;
    }

    // Stop condition boolean tokens — appended as %($ADDR)=VAL% expressions.
    // These evaluate to 0 or 1; the parser stops when any is non-zero.
    for (int i = 0; i < nstops && i < 16; i++) {
        char expr[32];
        if (stops[i].negate) {
            snprintf(expr, sizeof(expr), "%%(($%04X)=%02X)=0%%",
                     stops[i].addr, stops[i].value);
        } else {
            snprintf(expr, sizeof(expr), "%%($%04X)=%02X%%",
                     stops[i].addr, stops[i].value);
        }
        int idx = br_append(expr);
        if (idx >= 0) {
            g_stop_token_indices[g_num_stop_tokens++] = idx;
        }
    }

    // Assign output_index to each emitter
    for (int i = 0; i < g_nemitters; i++) {
        struct FieldEmitter *em = &g_emitters[i];
        switch (em->source) {
        case SRC_PC:    em->output_index = idx_pc; break;
        case SRC_SP:    em->output_index = idx_sp; break;
        case SRC_AF_HI: case SRC_AF_LO: em->output_index = idx_af; break;
        case SRC_BC_HI: case SRC_BC_LO: em->output_index = idx_bc; break;
        case SRC_DE_HI: case SRC_DE_LO: em->output_index = idx_de; break;
        case SRC_HL_HI: case SRC_HL_LO: em->output_index = idx_hl; break;
        case SRC_IME:   em->output_index = idx_ime; break;
        case SRC_IO: {
            for (int s = 0; s < g_nio_slots; s++) {
                if (g_io_slots[s].addr == (unsigned short)em->io_addr) {
                    em->output_index = g_io_slots[s].output_index;
                    break;
                }
            }
            if (em->output_index < 0) em->source = SRC_SKIP;
            break;
        }
        case SRC_SKIP: break;
        }
    }

    return g_total_tokens;
}

// Assemble the -br argument: "any///A ...,any///B ...,..."
static void build_br_arg(char *buf, size_t bufsz) {
    int pos = 0;
    for (int i = 0; i < g_num_br_lines; i++) {
        if (i > 0) pos += snprintf(buf + pos, bufsz - pos, ",");
        pos += snprintf(buf + pos, bufsz - pos, "any///%s", g_br_lines[i]);
    }
}

// ── SHA-256 ─────────────────────────────────────────────────────────

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

// ── BGB download ────────────────────────────────────────────────────

static const char *BGB_URL = "https://bgb.bircd.org/bgb.zip";

static void ensure_bgb(const char *adapter_dir) {
    char exe_path[4096];
    snprintf(exe_path, sizeof(exe_path), "%s/bgb.exe", adapter_dir);

    struct stat st;
    if (stat(exe_path, &st) == 0) return; // already present

    fprintf(stderr, "Downloading BGB from %s ...\n", BGB_URL);
    char cmd[4096];
    snprintf(cmd, sizeof(cmd),
        "cd \"%s\" && curl -fsSL -o bgb.zip '%s' && unzip -oq bgb.zip bgb.exe bgb.ini && rm -f bgb.zip",
        adapter_dir, BGB_URL);
    int rc = system(cmd);
    if (rc != 0) {
        fprintf(stderr, "Error: failed to download BGB\n");
        exit(1);
    }
    // Enable debug message file output in bgb.ini
    snprintf(cmd, sizeof(cmd),
        "sed -i 's/DebugMsgFile=0/DebugMsgFile=1/' \"%s/bgb.ini\"",
        adapter_dir);
    system(cmd);
}

// ── Line parsing ────────────────────────────────────────────────────

// Parse space-separated hex tokens from a BGB debug message line.
// Tokens are stored as raw unsigned long values.
#define MAX_TOKENS 64

static int parse_line(const char *line, unsigned long *tokens, int max) {
    int n = 0;
    const char *p = line;
    while (*p && n < max) {
        while (*p == ' ' || *p == '\t') p++;
        if (!*p || *p == '\n') break;
        char *end;
        tokens[n] = strtoul(p, &end, 16);
        if (end == p) break;
        n++;
        p = end;
    }
    return n;
}

// ── Main ────────────────────────────────────────────────────────────

static void print_usage(const char *argv0) {
    fprintf(stderr,
        "Usage: %s --rom <file.gb> --profile <profile.toml> --output <out.gbtrace> [options]\n"
        "\n"
        "Options:\n"
        "  --rom <path>           ROM file (required)\n"
        "  --profile <path>       Capture profile TOML (required)\n"
        "  --output <path>        Output .gbtrace file (required)\n"
        "  --frames <n>           Max frames (default: 3000)\n"
        "  --stop-when <A=V>      Stop when memory ADDR equals VAL (hex)\n"
        "  --stop-on-serial <B>   Stop when serial byte B (hex) is sent\n"
        "  --stop-serial-count <N> Nth occurrence (default: 1)\n"
        "  --model <model>        dmg or cgb (default: dmg)\n"
        "  --boot-rom <path>      Boot ROM file (default: skip)\n"
        "  --reference <path>     Reference .pix file (screenshot match)\n"
        "  --extra-frames <n>     Extra frames after stop (default: 0)\n",
        argv0);
}

static volatile sig_atomic_t g_child_pid = 0;

static void cleanup_child(int sig) {
    (void)sig;
    if (g_child_pid > 0) kill(g_child_pid, SIGTERM);
}

int main(int argc, char *argv[]) {
    const char *rom_path = NULL;
    const char *profile_path = NULL;
    const char *output_path = NULL;
    const char *boot_rom_path = NULL;
    const char *reference_path = NULL;
    int max_frames = 3000;
    int extra_frames = 0;
    const char *model = "DMG-B";

    // Stop conditions — detected in the adapter by checking boolean tokens
    struct StopCond stop_conds[16];
    int num_stop_conds = 0;
    unsigned char stop_serial_byte = 0;
    int stop_serial_active = 0;

    int stop_serial_count = 1;

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
            const char *neq = strstr(spec, "!=");
            const char *eq = strchr(spec, '=');
            if (eq && num_stop_conds < 16) {
                int is_neg = (neq && neq < eq);
                stop_conds[num_stop_conds].addr = (unsigned short)strtoul(spec, NULL, 16);
                stop_conds[num_stop_conds].value = (unsigned char)strtoul(eq + 1, NULL, 16);
                stop_conds[num_stop_conds].negate = is_neg;
                num_stop_conds++;
            }
        } else if (strcmp(argv[i], "--stop-on-serial") == 0 && i + 1 < argc) {
            stop_serial_byte = (unsigned char)strtoul(argv[++i], NULL, 16);
            stop_serial_active = 1;
        } else if (strcmp(argv[i], "--stop-serial-count") == 0 && i + 1 < argc) {
            stop_serial_count = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--model") == 0 && i + 1 < argc) {
            const char *m = argv[++i];
            if (strcmp(m, "cgb") == 0 || strcmp(m, "CGB") == 0) model = "CGB-E";
        } else if (strcmp(argv[i], "--boot-rom") == 0 && i + 1 < argc) {
            boot_rom_path = argv[++i];
        } else if (strcmp(argv[i], "--reference") == 0 && i + 1 < argc) {
            reference_path = argv[++i];
        } else if (strcmp(argv[i], "--extra-frames") == 0 && i + 1 < argc) {
            extra_frames = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--help") == 0) {
            print_usage(argv[0]); return 0;
        }
    }

    if (!rom_path || !profile_path || !output_path) {
        print_usage(argv[0]); return 1;
    }

    // Not yet implemented via BGB
    (void)reference_path; (void)max_frames; (void)extra_frames;
    (void)stop_serial_byte; (void)stop_serial_active; (void)stop_serial_count;

    // Determine adapter directory (where bgb.exe lives)
    char adapter_dir[4096];
    {
        // argv[0] might be relative or absolute
        char *last_slash = strrchr(argv[0], '/');
        if (last_slash) {
            size_t len = last_slash - argv[0];
            memcpy(adapter_dir, argv[0], len);
            adapter_dir[len] = '\0';
        } else {
            strcpy(adapter_dir, ".");
        }
    }

    // Ensure BGB is downloaded
    ensure_bgb(adapter_dir);

    // Load profile and plan emitters
    struct Profile prof = load_profile(profile_path);
    plan_emitters(&prof);

    int ntokens = build_format_strings(stop_conds, num_stop_conds);
    fprintf(stderr, "Profile: %s (%d fields, %d BGB tokens across %d line%s)\n",
            prof.name, prof.nfields, ntokens, g_num_br_lines,
            g_num_br_lines == 1 ? "" : "s");
    for (int i = 0; i < g_num_br_lines; i++)
        fprintf(stderr, "  Line %c: %s\n", 'A' + i, g_br_lines[i]);

    // Build header JSON
    char *rom_hash = sha256_file(rom_path);
    const char *boot_info = "skip";
    static char boot_hash[128];
    if (boot_rom_path) {
        strncpy(boot_hash, sha256_file(boot_rom_path), sizeof(boot_hash) - 1);
        boot_info = boot_hash;
    }

    char header_json[4096];
    int hpos = snprintf(header_json, sizeof(header_json),
        "{\"_header\":true,\"format_version\":\"0.1.0\","
        "\"emulator\":\"bgb\",\"emulator_version\":\"1.6.4\","
        "\"rom_sha256\":\"%s\",\"model\":\"%s\","
        "\"boot_rom\":\"%s\",\"profile\":\"%s\","
        "\"fields\":[",
        rom_hash, model, boot_info, prof.name);
    int first_field = 1;
    for (int i = 0; i < g_nemitters; i++) {
        if (g_emitters[i].source == SRC_SKIP) continue;
        if (!first_field) hpos += snprintf(header_json + hpos, sizeof(header_json) - hpos, ",");
        hpos += snprintf(header_json + hpos, sizeof(header_json) - hpos,
                         "\"%s\"", g_emitters[i].name);
        first_field = 0;
    }
    hpos += snprintf(header_json + hpos, sizeof(header_json) - hpos,
                     "],\"trigger\":\"instruction\"}");

    // Create writer
    GbtraceWriter *writer = gbtrace_writer_new(output_path, header_json, hpos);
    if (!writer) {
        fprintf(stderr, "Error: failed to create trace writer\n");
        return 1;
    }

    // Cache column indices
    int writer_cols[MAX_FIELDS];
    for (int i = 0; i < g_nemitters; i++) {
        if (g_emitters[i].source == SRC_SKIP) {
            writer_cols[i] = -1;
        } else {
            writer_cols[i] = gbtrace_writer_find_field(writer, g_emitters[i].name);
        }
    }
    int ly_col = gbtrace_writer_find_field(writer, "ly");
    gbtrace_writer_mark_frame(writer);

    // Create named pipe for debugmsg.txt
    char fifo_path[4096];
    snprintf(fifo_path, sizeof(fifo_path), "%s/debugmsg.txt", adapter_dir);
    unlink(fifo_path);
    if (mkfifo(fifo_path, 0600) != 0) {
        fprintf(stderr, "Error: mkfifo(%s): %s\n", fifo_path, strerror(errno));
        gbtrace_writer_close(writer);
        return 1;
    }

    // Build BGB command line
    char bgb_br[2048];
    build_br_arg(bgb_br, sizeof(bgb_br));

    // Convert ROM path to Wine Z: drive path
    char wine_rom[4096];
    {
        const char *abs_rom = rom_path;
        char resolved[4096];
        if (rom_path[0] != '/') {
            if (!realpath(rom_path, resolved)) {
                fprintf(stderr, "Error: cannot resolve ROM path '%s'\n", rom_path);
                unlink(fifo_path);
                gbtrace_writer_close(writer);
                return 1;
            }
            abs_rom = resolved;
        }
        snprintf(wine_rom, sizeof(wine_rom), "Z:%s", abs_rom);
        for (char *p = wine_rom + 2; *p; p++)
            if (*p == '/') *p = '\\';
    }

    // Fork BGB process
    pid_t pid = fork();
    if (pid < 0) {
        fprintf(stderr, "Error: fork failed\n");
        unlink(fifo_path);
        gbtrace_writer_close(writer);
        return 1;
    }

    if (pid == 0) {
        // Child: run BGB under xvfb-run + wine
        // Redirect stdout/stderr to /dev/null
        freopen("/dev/null", "w", stdout);
        freopen("/dev/null", "w", stderr);

        // Change to adapter directory so BGB finds its ini and writes debugmsg.txt there
        if (chdir(adapter_dir) != 0) _exit(1);

        execlp("xvfb-run", "xvfb-run", "-a",
               "wine", "./bgb.exe", "-headless", "-runfast",
               "-br", bgb_br,
               "-rom", wine_rom,
               NULL);
        _exit(1);
    }

    // Parent: read from the FIFO and write trace entries
    g_child_pid = pid;
    signal(SIGTERM, cleanup_child);
    signal(SIGINT, cleanup_child);

    FILE *fifo = fopen(fifo_path, "r");
    if (!fifo) {
        fprintf(stderr, "Error: cannot open FIFO %s: %s\n", fifo_path, strerror(errno));
        kill(pid, SIGTERM);
        waitpid(pid, NULL, 0);
        unlink(fifo_path);
        gbtrace_writer_close(writer);
        return 1;
    }

    // Cache LY token index for frame boundary detection
    int ly_token_idx = -1;
    if (ly_col >= 0) {
        for (int i = 0; i < g_nemitters; i++) {
            if (strcmp(g_emitters[i].name, "ly") == 0 && g_emitters[i].output_index >= 0) {
                ly_token_idx = g_emitters[i].output_index;
                break;
            }
        }
    }

    // Compute the base token offset for each breakpoint line.
    // BGB outputs lines in reverse order (last breakpoint first), so
    // line C (index 2) comes first, then B (1), then A (0).
    int line_base[MAX_BR_LINES];
    {
        int base = 0;
        for (int i = 0; i < g_num_br_lines; i++) {
            line_base[i] = base;
            base += g_br_ntokens[i];
        }
    }

    char line[4096];
    unsigned long all_tokens[MAX_TOKENS]; // accumulated across all lines
    long entry_count = 0;
    uint8_t prev_ly = 0;
    int lines_collected = 0;

    while (fgets(line, sizeof(line), fifo)) {
        // Identify which line this is by the prefix letter
        if (line[0] < 'A' || line[0] >= 'A' + g_num_br_lines || line[1] != ' ')
            continue; // malformed

        int line_idx = line[0] - 'A';
        unsigned long tokens[MAX_TOKENS];
        int nt = parse_line(line + 2, tokens, MAX_TOKENS); // skip "X " prefix
        if (nt < g_br_ntokens[line_idx]) continue; // malformed

        // Copy into the global token array at the right offset
        int base = line_base[line_idx];
        for (int t = 0; t < g_br_ntokens[line_idx]; t++)
            all_tokens[base + t] = tokens[t];
        lines_collected++;

        // Emit entry once we've collected all lines for this instruction
        if (lines_collected < g_num_br_lines) continue;
        lines_collected = 0;

        // Check LY for frame boundary
        if (ly_token_idx >= 0) {
            uint8_t ly_val = (uint8_t)all_tokens[ly_token_idx];
            if (ly_val == 0 && prev_ly != 0 && entry_count > 0) {
                gbtrace_writer_mark_frame(writer);
            }
            prev_ly = ly_val;
        }

        // Emit fields
        for (int i = 0; i < g_nemitters; i++) {
            int col = writer_cols[i];
            if (col < 0) continue;
            struct FieldEmitter *em = &g_emitters[i];
            if (em->output_index < 0) continue;
            unsigned long val = all_tokens[em->output_index];

            switch (em->source) {
            case SRC_PC:
            case SRC_SP:
                gbtrace_writer_set_u16(writer, col, (uint16_t)val);
                break;
            case SRC_AF_HI: gbtrace_writer_set_u8(writer, col, (uint8_t)(val >> 8)); break;
            case SRC_AF_LO: gbtrace_writer_set_u8(writer, col, (uint8_t)(val & 0xFF)); break;
            case SRC_BC_HI: gbtrace_writer_set_u8(writer, col, (uint8_t)(val >> 8)); break;
            case SRC_BC_LO: gbtrace_writer_set_u8(writer, col, (uint8_t)(val & 0xFF)); break;
            case SRC_DE_HI: gbtrace_writer_set_u8(writer, col, (uint8_t)(val >> 8)); break;
            case SRC_DE_LO: gbtrace_writer_set_u8(writer, col, (uint8_t)(val & 0xFF)); break;
            case SRC_HL_HI: gbtrace_writer_set_u8(writer, col, (uint8_t)(val >> 8)); break;
            case SRC_HL_LO: gbtrace_writer_set_u8(writer, col, (uint8_t)(val & 0xFF)); break;
            case SRC_IME:   gbtrace_writer_set_bool(writer, col, val != 0); break;
            case SRC_IO:    gbtrace_writer_set_u8(writer, col, (uint8_t)val); break;
            case SRC_SKIP:  break;
            }
        }

        gbtrace_writer_finish_entry(writer);
        entry_count++;

        // Check software stop conditions.  Skip the first few entries to
        // avoid false triggers from uninitialised memory (BGB starts HRAM
        // at 0xFF which can match --stop-when conditions immediately).
        if (entry_count > 2 && g_num_stop_tokens > 0) {
            bool should_stop = false;
            for (int s = 0; s < g_num_stop_tokens; s++) {
                int idx = g_stop_token_indices[s];
                if (idx >= 0 && all_tokens[idx] != 0) {
                    should_stop = true;
                    fprintf(stderr, "Stop condition met at entry %ld\n",
                            entry_count);
                    break;
                }
            }
            if (should_stop) break;
        }
    }

    fclose(fifo);

    // Kill BGB (it may still be running if we stopped due to a condition)
    kill(pid, SIGTERM);
    int status = 0;
    waitpid(pid, &status, 0);
    g_child_pid = 0;

    // Clean up
    unlink(fifo_path);
    gbtrace_writer_close(writer);

    fprintf(stderr, "Traced %ld entries, output written to %s\n", entry_count, output_path);
    return 0;
}
