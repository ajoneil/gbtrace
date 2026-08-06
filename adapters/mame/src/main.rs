//! morepork-mame: a morepork adapter driving MAME as an independent-lineage
//! behavioural oracle — the a2600 driver for the VCS suite, the sg1000 driver
//! for the TI VDP (TMS9918A) suite. Links the morepork core crate directly
//! and writes native `.morepork`.
//!
//! MAME is not linkable like the Stella/Gopher2600 adapters, so this drives
//! it headlessly via its gdbstub debugger. For speed it does NOT single-step
//! over the wire (that was ~19s/ROM); instead it uses the GDB remote
//! `monitor` command (qRcmd) to install MAME's own debugger `trace` command
//! (which logs every instruction at full emulation speed) plus a watchpoint
//! on the RESULT byte, then `continue`s to the verdict (~250ms/ROM).
//!
//! The CLI keeps the original Go adapter's flag surface (single-dash,
//! `-name value` and `-name=value`) because the suite scripts hard-code it:
//!
//! ```text
//! morepork-mame -rom test.bin -out trace.morepork -spec NTSC -frames 30
//! morepork-mame -system sg1000 -rom sanity.sg -out trace.morepork
//! ```

mod mame_palette;
mod ti_vdp_palette;
#[allow(dead_code)] // SECAM is rejected before capture; the table rides along
mod vcs_palette;

use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use morepork::format::write::MoreporkWriter;
use morepork::header::TraceHeader;
use morepork::snapshot::IndexedFrame;
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// --- system definitions ---
//
// Everything system-shaped in one table: the MAME machine, the CPU device
// tag the debugger's `trace` wants, the tracelog register format, and the
// RESULT-block address of the suite's verdict convention. Both suites share
// the verdict values ($A5 PASS / $5A FAIL, then CODE/OBSERVED/EXPECTED).

struct FieldSpec {
    name: &'static str,
    wide: bool, // u16 field; otherwise u8
}

const fn f8(name: &'static str) -> FieldSpec {
    FieldSpec { name, wide: false }
}
const fn f16(name: &'static str) -> FieldSpec {
    FieldSpec { name, wide: true }
}

struct SysDef {
    cli: &'static str,         // -system value selecting this row
    id: &'static str,          // morepork header `system`
    model: Option<&'static str>, // header `model` override; None = the -spec value
    cpu_tag: &'static str,     // MAME device tag for `trace` (a2600/coleco: maincpu, sg1000/sc3000: z80)
    result_addr: &'static str, // RESULT block base, hex without 0x
    trace_fmt: &'static str,   // tracelog format string ("R" + one column per field)
    trace_syms: &'static str,  // tracelog symbols, validated against installed MAME
    cpu_fields: &'static [FieldSpec],
    machine: fn(&str) -> Result<&'static str>,
}

fn vcs_machine(spec: &str) -> Result<&'static str> {
    // MAME has no SECAM Atari 2600 machine (only a2600 / a2600p), so it
    // cannot capture a real SECAM field. Reject rather than silently emit
    // an a2600 (NTSC-geometry) frame tagged SECAM.
    if spec.eq_ignore_ascii_case("SECAM") {
        return Err("MAME has no SECAM 2600 driver; capture -spec SECAM with the stella or gopher2600 adapter".into());
    }
    Ok(if spec == "PAL" { "a2600p" } else { "a2600" })
}

/// The TI VDP machines are NTSC TMS9918A drivers; TMS9929A (PAL) capture
/// would need different machines, and the suite treats PAL as provisional
/// anyway.
fn ntsc_only(machine: &'static str, spec: &str) -> Result<&'static str> {
    if !spec.eq_ignore_ascii_case("NTSC") {
        return Err(format!("MAME {machine} capture is NTSC-only (TMS9918A); -spec {spec} is not supported").into());
    }
    Ok(machine)
}

fn sg1000_machine(spec: &str) -> Result<&'static str> {
    ntsc_only("sg1000", spec)
}

fn sc3000_machine(spec: &str) -> Result<&'static str> {
    ntsc_only("sc3000", spec)
}

fn coleco_machine(spec: &str) -> Result<&'static str> {
    ntsc_only("coleco", spec)
}

/// The Z80 register columns shared by every TI VDP machine row.
static Z80_FIELDS: &[FieldSpec] = &[
    f16("pc"), f16("sp"), f8("a"), f8("f"), f8("b"), f8("c"),
    f8("d"), f8("e"), f8("h"), f8("l"), f16("ix"), f16("iy"),
];
const Z80_TRACE_FMT: &str = "R%04X %04X %02X %02X %02X %02X %02X %02X %02X %02X %04X %04X";
const Z80_TRACE_SYMS: &str = "pc,sp,a,f,b,c,d,e,h,l,ix,iy";

static SYSTEMS: &[SysDef] = &[
    SysDef {
        cli: "vcs",
        id: "vcs",
        model: None,
        cpu_tag: "maincpu",
        result_addr: "80",
        trace_fmt: "R%04X %02X %02X %02X %02X %02X",
        trace_syms: "pc,a,x,y,sp,p", // register symbol is `sp`, not `s`
        cpu_fields: &[f16("pc"), f8("a"), f8("x"), f8("y"), f8("s"), f8("p")],
        machine: vcs_machine,
    },
    SysDef {
        cli: "sg1000",
        id: "sg1000",
        model: None,
        cpu_tag: "z80",
        result_addr: "c000",
        trace_fmt: Z80_TRACE_FMT,
        trace_syms: Z80_TRACE_SYMS,
        cpu_fields: Z80_FIELDS,
        machine: sg1000_machine,
    },
    // The SC-3000 is the SG-1000's keyboard-computer sibling: identical
    // Z80 + TI VDP envelope, so it captures as the `sg1000` system with
    // the machine carried in `model`.
    SysDef {
        cli: "sc3000",
        id: "sg1000",
        model: Some("SC-3000"),
        cpu_tag: "z80",
        result_addr: "c000",
        trace_fmt: Z80_TRACE_FMT,
        trace_syms: Z80_TRACE_SYMS,
        cpu_fields: Z80_FIELDS,
        machine: sc3000_machine,
    },
    // MAME's coleco driver needs the ColecoVision BIOS romset (coleco.zip)
    // on the -rompath; the suite's `.col` builds sit at 0x8000 with the
    // RESULT block at 0x7000.
    SysDef {
        cli: "coleco",
        id: "coleco",
        model: None,
        cpu_tag: "maincpu",
        result_addr: "7000",
        trace_fmt: Z80_TRACE_FMT,
        trace_syms: Z80_TRACE_SYMS,
        cpu_fields: Z80_FIELDS,
        machine: coleco_machine,
    },
];

// --- GDB remote client ---

struct Gdb {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Gdb {
    fn connect(port: u16) -> Result<Self> {
        for _ in 0..100 {
            if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
                let reader = BufReader::new(stream.try_clone()?);
                let mut g = Gdb { stream, reader };
                g.stream.write_all(b"+")?;
                return Ok(g);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(format!("gdbstub never listened on {port}").into())
    }

    fn send(&mut self, body: &str) {
        let sum: u32 = body.bytes().map(u32::from).sum();
        let _ = write!(self.stream, "${body}#{:02x}", sum & 0xff);
    }

    fn recv(&mut self) -> String {
        let mut byte = [0u8; 1];
        loop {
            match self.reader.read_exact(&mut byte) {
                Ok(()) if byte[0] == b'$' => break,
                Ok(()) => continue,
                Err(_) => return String::new(),
            }
        }
        let mut body = Vec::with_capacity(64);
        loop {
            if self.reader.read_exact(&mut byte).is_err() {
                return String::new();
            }
            if byte[0] == b'#' {
                break;
            }
            body.push(byte[0]);
        }
        let mut checksum = [0u8; 2];
        let _ = self.reader.read_exact(&mut checksum);
        let _ = self.stream.write_all(b"+");
        String::from_utf8_lossy(&body).into_owned()
    }

    fn cmd(&mut self, body: &str) -> String {
        self.send(body);
        self.recv()
    }

    /// Run a MAME debugger console command via the GDB `monitor` (qRcmd) escape.
    fn mon(&mut self, command: &str) -> String {
        let hexed: String = command.bytes().map(|b| format!("{b:02x}")).collect();
        let resp = self.cmd(&format!("qRcmd,{hexed}"));
        match hex_decode(&resp) {
            Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            None => resp,
        }
    }
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

// --- MAME process management ---

/// Kills the whole process group (negative pid), then reaps. MAME paused at
/// the watchpoint never reaches -seconds_to_run, so this is the only thing
/// that stops it; the group covers any child the launcher forks.
struct MameProcess(Option<Child>);

impl MameProcess {
    fn spawn(mut cmd: Command) -> Result<Self> {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        let child = cmd.spawn().map_err(|e| format!("launch mame: {e}"))?;
        Ok(MameProcess(Some(child)))
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.0.take() {
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for MameProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

// --- CLI (Go-flag-compatible: -name value, -name=value, bare bools) ---

struct Args {
    system: String,
    rom: String,
    out: String,
    spec: String,
    frames: i64,
    port: u16,
    swchb: i64,
    frame: bool,
    rompath: String,
}

fn usage() -> ! {
    eprintln!(
        "usage: morepork-mame [flags]\n\
         \x20 -system vcs|sg1000|sc3000|coleco   target system (default vcs)\n\
         \x20 -rom <path>          ROM (.bin/.a26 for vcs; .sg/.col for the TI VDP machines)\n\
         \x20 -out <path>          output .morepork path (default trace.morepork)\n\
         \x20 -spec NTSC|PAL       TV spec (vcs: a2600 vs a2600p; TI VDP machines: NTSC only)\n\
         \x20 -frames <n>          cap: seconds_to_run = max(2, frames/60) (default 30)\n\
         \x20 -port <n>            gdbstub port (0 = auto-pick, default)\n\
         \x20 -swchb <n>           vcs console switches (default 0x48)\n\
         \x20 -frame[=bool]        capture a final frame snapshot (default true)\n\
         \x20 -rompath <dir>       MAME rompath for machines needing BIOS romsets (coleco)"
    );
    std::process::exit(2);
}

fn parse_int(s: &str) -> Option<i64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn parse_args() -> Args {
    let mut args = Args {
        system: "vcs".into(),
        rom: String::new(),
        out: "trace.morepork".into(),
        spec: "NTSC".into(),
        frames: 30,
        port: 0,
        swchb: 0x48,
        frame: true,
        rompath: String::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let Some(stripped) = arg.strip_prefix('-') else {
            eprintln!("error: unexpected argument {arg:?}");
            usage();
        };
        let stripped = stripped.strip_prefix('-').unwrap_or(stripped);
        let (name, inline) = match stripped.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (stripped, None),
        };
        // Bools take an optional inline value; everything else consumes one.
        if name == "frame" {
            args.frame = inline.as_deref().map_or(true, |v| v == "true" || v == "1");
            continue;
        }
        if matches!(name, "h" | "help") {
            usage();
        }
        let value = inline.or_else(|| it.next()).unwrap_or_else(|| {
            eprintln!("error: flag -{name} needs a value");
            usage();
        });
        match name {
            "system" => args.system = value,
            "rom" => args.rom = value,
            "out" => args.out = value,
            "spec" => args.spec = value,
            "rompath" => args.rompath = value,
            "frames" => args.frames = parse_int(&value).unwrap_or_else(|| usage()),
            "port" => args.port = parse_int(&value).unwrap_or_else(|| usage()) as u16,
            "swchb" => args.swchb = parse_int(&value).unwrap_or_else(|| usage()),
            _ => {
                eprintln!("error: unknown flag -{name}");
                usage();
            }
        }
    }
    args
}

fn main() {
    let args = parse_args();
    if args.rom.is_empty() {
        eprintln!("error: -rom is required");
        std::process::exit(2);
    }
    let Some(sys) = SYSTEMS.iter().find(|s| s.cli == args.system) else {
        eprintln!("error: unknown -system {:?} (vcs, sg1000, sc3000, coleco)", args.system);
        std::process::exit(2);
    };
    if let Err(e) = run(sys, &args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

// --- capture ---

fn run(sys: &SysDef, args: &Args) -> Result<()> {
    let machine = (sys.machine)(&args.spec)?;
    let rom_bytes = std::fs::read(&args.rom)?;
    let rom_sha = hex_encode(&Sha256::digest(&rom_bytes));

    // Pick a free ephemeral gdbstub port so rapid successive runs don't
    // collide on a fixed port left in TIME_WAIT (the source of transient
    // "gdbstub never listened" failures).
    let port = if args.port == 0 {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("pick free port: {e}"))?;
        listener.local_addr()?.port()
    } else {
        args.port
    };

    let lua = tempfile::Builder::new().prefix("morepork-mame-").suffix(".lua").tempfile()?;
    if sys.id == "vcs" {
        // Console switches exist only on the 2600.
        std::fs::write(lua.path(), switch_lua(args.swchb))?;
    }
    let cfg = tempfile::Builder::new().prefix("morepork-mame-cfg-").tempdir()?;
    let trace_log = tempfile::Builder::new().prefix("morepork-mame-trace-").suffix(".log").tempfile()?;
    let cfg_path = cfg.path().to_string_lossy().into_owned();

    let seconds = std::cmp::max(2, args.frames / 60);
    let lua_path = lua.path().to_string_lossy().into_owned();
    let port_arg = port.to_string();
    let seconds_arg = seconds.to_string();
    let mut cmd = Command::new("mame");
    cmd.args([
        machine, "-cart", args.rom.as_str(),
        "-video", "none", "-sound", "none", "-nothrottle",
        "-autoboot_script", lua_path.as_str(), "-autoboot_delay", "0",
        "-cfg_directory", cfg_path.as_str(), "-snapshot_directory", cfg_path.as_str(),
        "-nvram_directory", cfg_path.as_str(),
        "-debug", "-debugger", "gdbstub", "-debugger_port", port_arg.as_str(),
        "-seconds_to_run", seconds_arg.as_str(),
    ])
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    if !args.rompath.is_empty() {
        cmd.args(["-rompath", &args.rompath]);
    }
    let mut mame = MameProcess::spawn(cmd)?;

    let mut g = Gdb::connect(port)?;
    // handshake (MAME's gdbstub answers `g`/monitor only after these)
    g.cmd("qSupported");
    g.cmd("qXfer:features:read:target.xml:0,3fc");

    // install a full-speed per-instruction trace (noloop = don't collapse
    // loops; one invalid register symbol makes the whole tracelog fall back
    // to plain disassembly silently) and a watchpoint on the RESULT verdict.
    g.mon(&format!(
        "trace {},{},noloop,{{tracelog \"{}\\n\",{}}}",
        trace_log.path().display(),
        sys.cpu_tag,
        sys.trace_fmt,
        sys.trace_syms
    ));
    g.mon(&format!("wpset 0x{},1,w,{{(wpdata==0xa5)||(wpdata==0x5a)}}", sys.result_addr));
    // The wall-clock read timeout must cover the whole emulated budget:
    // per-instruction tracing runs MAME well below realtime (observed
    // ~0.5x on sg1000), so scale generously. MAME's own -seconds_to_run
    // exit closes the stream and unblocks the read long before this cap
    // when the ROM never latches a verdict.
    let wall = std::cmp::max(30, seconds as u64 * 12);
    g.stream.set_read_timeout(Some(Duration::from_secs(wall)))?;
    g.cmd("c"); // run full-speed to the verdict (or seconds_to_run)
    // read the RESULT bytes at the stop (per-instruction memory in the trace
    // format breaks tracelog, so we grab the final verdict here).
    let (res, code, obs, exp) = parse_mem(&g.cmd(&format!("m{},4", sys.result_addr)));
    g.mon("trace off"); // flush the trace file
    mame.kill(); // free the port before pass 2

    // A second, gdbstub-free headless pass captures the final frame via Lua
    // (gdbstub exposes no pixels). Best-effort: a frame is nice-to-have.
    let frame = if args.frame {
        match capture_frame(sys, machine, args) {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("warning: frame capture failed: {e}");
                None
            }
        }
    } else {
        None
    };

    let model = sys.model.unwrap_or(&args.spec);
    write_trace(&args.out, sys, model, &rom_sha, trace_log.path(), (res, code, obs, exp), frame.as_ref())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_mem(h: &str) -> (u8, u8, u8, u8) {
    let byte = |i: usize| {
        h.get(i..i + 2)
            .and_then(|s| u8::from_str_radix(s, 16).ok())
            .unwrap_or(0)
    };
    (byte(0), byte(2), byte(4), byte(6))
}

/// Sets the a2600 console switches (best-effort; see README).
fn switch_lua(swchb: i64) -> String {
    format!(
        r#"
local v = {swchb}
local function apply()
  local swb = manager.machine.ioport.ports[":SWB"]
  if not swb then return end
  local function set(name, bit)
    local f = swb.fields[name]
    if f then f:set_value(((v >> bit) & 1) ~= 0 and f.mask or 0) end
  end
  set("TV Type", 3)
  set("Left Diff. Switch", 6)
  set("Right Diff. Switch", 7)
end
apply()
emu.register_prestart(apply)
emu.register_frame_done(apply)
"#
    )
}

// --- frame capture (second headless pass) ---

struct FrameData {
    width: usize,
    height: usize,
    pixels: Vec<u8>,          // canonical-palette indices (TIA codes / TI VDP colours)
    palette: Vec<[u8; 3]>,    // RGB triples stamped into the frame snapshot
    aspect: f32,              // display pixel aspect at the system's dot clock
}

/// Runs the ROM for a few frames, then dumps the screen's pixels (ARGB32) to
/// a file and exits.
fn frame_lua(target: i64, dump: &Path) -> String {
    format!(
        r#"
local target = {target}
local n = 0
emu.register_frame_done(function()
  n = n + 1
  if n < target then return end
  local s = manager.machine.screens:at(1)
  local ok, px = pcall(function() return s:pixels() end)
  if ok then
    local f = io.open("{}", "wb"); f:write(px); f:close()
    io.stderr:write(string.format("GBFRAME %d %d\n", s.width, s.height))
  end
  manager.machine:exit()
end)
"#,
        dump.display()
    )
}

/// Launches a second headless MAME, dumps the last frame's pixels, and
/// reverse-maps each RGB to a canonical palette index (TIA colour code /
/// TI VDP colour) so the frame is oracle-independent like the other adapters'.
fn capture_frame(sys: &SysDef, machine: &str, args: &Args) -> Result<FrameData> {
    let (rom, spec, max_frames) = (args.rom.as_str(), args.spec.as_str(), args.frames);
    let dump = tempfile::Builder::new().prefix("morepork-mame-px-").suffix(".bin").tempfile()?;
    let lua = tempfile::Builder::new().prefix("morepork-mame-frame-").suffix(".lua").tempfile()?;
    // Redirect MAME's cfg/snapshot/nvram output to a temp dir so it never
    // litters the working directory (cfg/, snap/, nvram/).
    let home = tempfile::Builder::new().prefix("morepork-mame-home-").tempdir()?;
    let home_path = home.path().to_string_lossy().into_owned();

    let target = std::cmp::max(max_frames, 8); // let a static image settle
    std::fs::write(lua.path(), frame_lua(target, dump.path()))?;
    let seconds = target / 60 + 2;

    let lua_path = lua.path().to_string_lossy().into_owned();
    let seconds_arg = seconds.to_string();
    let mut cmd = Command::new("mame");
    cmd.args([
        machine, "-cart", rom,
        "-video", "none", "-sound", "none", "-nothrottle",
        "-autoboot_script", lua_path.as_str(), "-autoboot_delay", "0",
        "-cfg_directory", home_path.as_str(), "-snapshot_directory", home_path.as_str(),
        "-nvram_directory", home_path.as_str(),
        "-seconds_to_run", seconds_arg.as_str(),
    ])
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    if !args.rompath.is_empty() {
        cmd.args(["-rompath", &args.rompath]);
    }
    let mut mame = MameProcess::spawn(cmd)?;

    // Drain stderr on a thread (the pipe must not fill), wait max 30s.
    let stderr = mame.0.as_mut().and_then(|c| c.stderr.take());
    let drain = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        match mame.0.as_mut().map(|c| c.try_wait()) {
            Some(Ok(Some(_))) | None => break,
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    mame.kill();
    let errors = drain.join().unwrap_or_default();

    let (mut w, mut h) = (0usize, 0usize);
    for line in errors.lines() {
        if let Some(rest) = line.strip_prefix("GBFRAME ") {
            let mut parts = rest.split_whitespace();
            w = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            h = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }
    if w == 0 || h == 0 {
        let first = errors.lines().next().unwrap_or("");
        return Err(format!("no frame dumped ({first})").into());
    }
    let argb = std::fs::read(dump.path())?;
    if argb.len() < w * h * 4 {
        return Err(format!("short pixel dump: {} < {}", argb.len(), w * h * 4).into());
    }

    match sys.id {
        "vcs" => map_vcs_frame(w, h, &argb, spec),
        _ => map_ti_vdp_frame(w, h, &argb),
    }
}

/// MAME pixels() is ARGB32 little-endian: bytes b,g,r,a.
fn rgb_at(argb: &[u8], i: usize) -> (u8, u8, u8) {
    (argb[i * 4 + 2], argb[i * 4 + 1], argb[i * 4])
}

fn rgb_key(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

/// Crops MAME's a2600 screen (176 wide = 160 visible + 8px borders) to the
/// canonical 160-wide visible area and reverse-maps each RGB to a TIA colour
/// code against MAME's calibrated palette FOR THIS REGION (a2600p's PAL
/// palette differs from a2600's NTSC one). Exact match; nearest is a safety
/// net — MAME has no anti-aliasing, so its pixels hit the table exactly.
/// (Vertical alignment vs the full-field golden is done in the GOLD compare
/// step — MAME exposes no VSYNC position.)
fn map_vcs_frame(w: usize, h: usize, argb: &[u8], spec: &str) -> Result<FrameData> {
    let pal_region = spec.to_uppercase().starts_with("PAL");
    let mame_pal = if pal_region { &mame_palette::MAME_PAL } else { &mame_palette::MAME_NTSC };

    const VIS: usize = 160;
    let x0 = w.saturating_sub(VIS) / 2;
    let cw = VIS.min(w);

    let mut exact: HashMap<u32, u8> = HashMap::new();
    for (code, rgb) in mame_pal.iter().enumerate() {
        // prefer the lowest code (code 0 for black)
        exact.entry(rgb_key(rgb[0], rgb[1], rgb[2])).or_insert(code as u8);
    }
    let mut pixels = vec![0u8; cw * h];
    let mut cache: HashMap<u32, u8> = HashMap::new();
    for y in 0..h {
        for cx in 0..cw {
            let (r, g, b) = rgb_at(argb, y * w + x0 + cx);
            let key = rgb_key(r, g, b);
            let idx = *cache.entry(key).or_insert_with(|| {
                exact.get(&key).copied().unwrap_or_else(|| nearest_mame(mame_pal, r, g, b))
            });
            pixels[y * cw + cx] = idx;
        }
    }
    let canonical = if pal_region { &vcs_palette::CANONICAL_PAL } else { &vcs_palette::CANONICAL_NTSC };
    Ok(FrameData {
        width: cw,
        height: h,
        pixels,
        palette: canonical.to_vec(),
        aspect: 12.0 / 7.0,
    })
}

/// Maps an RGB triple to the TIA colour code whose MAME palette entry is
/// closest (squared distance) — a safety net for the exact map. Only even
/// indices are real colours (odd = black).
fn nearest_mame(pal: &[[u8; 3]; 256], r: u8, g: u8, b: u8) -> u8 {
    let mut best = 0u8;
    let mut best_d = i32::MAX;
    for code in (0..256).step_by(2) {
        let e = pal[code];
        let d = distance(e, r, g, b);
        if d < best_d {
            best_d = d;
            best = code as u8;
            if d == 0 {
                break;
            }
        }
    }
    best
}

/// Crops MAME's sg1000 screen to the TMS9918A active area and reverse-maps
/// each RGB to a TI VDP colour index. Verified against MAME 0.288: the
/// screen is 280×216 with the 256×192 active area centred at (12,12), and
/// its rendered RGBs match the canonical palette exactly (nearest_ti_vdp is
/// a safety net). Index 0 (transparent) renders as the backdrop, so the map
/// covers colours 1-15 only and the frame never contains a 0.
fn map_ti_vdp_frame(w: usize, h: usize, argb: &[u8]) -> Result<FrameData> {
    const VIS_W: usize = 256;
    const VIS_H: usize = 192;
    if w < VIS_W || h < VIS_H {
        return Err(format!("screen {w}x{h} smaller than the {VIS_W}x{VIS_H} active area").into());
    }
    let (x0, y0) = ((w - VIS_W) / 2, (h - VIS_H) / 2);

    let mut exact: HashMap<u32, u8> = HashMap::new();
    for (colour, rgb) in ti_vdp_palette::TI_VDP.iter().enumerate().skip(1) {
        // colour 1 (black) wins the 0/1 duplicate
        exact.entry(rgb_key(rgb[0], rgb[1], rgb[2])).or_insert(colour as u8);
    }
    let mut pixels = vec![0u8; VIS_W * VIS_H];
    let mut cache: HashMap<u32, u8> = HashMap::new();
    for y in 0..VIS_H {
        for x in 0..VIS_W {
            let (r, g, b) = rgb_at(argb, (y0 + y) * w + x0 + x);
            let key = rgb_key(r, g, b);
            let idx = *cache.entry(key).or_insert_with(|| {
                exact.get(&key).copied().unwrap_or_else(|| nearest_ti_vdp(r, g, b))
            });
            pixels[y * VIS_W + x] = idx;
        }
    }
    Ok(FrameData {
        width: VIS_W,
        height: VIS_H,
        pixels,
        palette: ti_vdp_palette::TI_VDP.to_vec(),
        aspect: 8.0 / 7.0,
    })
}

/// Maps an RGB triple to the closest TI VDP colour (1-15).
fn nearest_ti_vdp(r: u8, g: u8, b: u8) -> u8 {
    let mut best = 1u8;
    let mut best_d = i32::MAX;
    for (colour, e) in ti_vdp_palette::TI_VDP.iter().enumerate().skip(1) {
        let d = distance(*e, r, g, b);
        if d < best_d {
            best_d = d;
            best = colour as u8;
            if d == 0 {
                break;
            }
        }
    }
    best
}

fn distance(e: [u8; 3], r: u8, g: u8, b: u8) -> i32 {
    let dr = i32::from(r) - i32::from(e[0]);
    let dg = i32::from(g) - i32::from(e[1]);
    let db = i32::from(b) - i32::from(e[2]);
    dr * dr + dg * dg + db * db
}

// --- trace writing ---

/// Parses the MAME trace log ("R" + one register column per cpu_field) into
/// a native .morepork. The RESULT bytes are placed on the final (verdict)
/// entry.
fn write_trace(
    out: &str,
    sys: &SysDef,
    model: &str,
    rom_sha: &str,
    log: &Path,
    verdict: (u8, u8, u8, u8),
    frame: Option<&FrameData>,
) -> Result<()> {
    let text = std::fs::read_to_string(log)?;
    let ncols = sys.cpu_fields.len();
    let hx = |s: &str| u64::from_str_radix(s, 16).unwrap_or(0);
    let mut entries: Vec<Vec<u64>> = Vec::new();
    for line in text.lines() {
        if !line.starts_with('R') {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() != ncols {
            continue;
        }
        let mut vals = Vec::with_capacity(ncols);
        vals.push(hx(&cols[0][1..]));
        vals.extend(cols[1..].iter().map(|c| hx(c)));
        entries.push(vals);
    }
    if entries.is_empty() {
        return Err("no trace entries (empty MAME trace log)".into());
    }

    let mut fields: Vec<String> = sys.cpu_fields.iter().map(|f| f.name.to_string()).collect();
    fields.extend(["result", "code", "observed", "expected"].map(String::from));
    let mut header_json = serde_json::json!({
        "_header": true, "format_version": "0.1.0",
        "emulator": "mame", "emulator_version": "adapter", "rom_sha256": rom_sha,
        "system": sys.id, "model": model, "profile": "tier1",
        "fields": fields, "trigger": "instruction",
    });
    if frame.is_some() {
        header_json["pix_format"] = serde_json::json!("indexed8");
    }
    let header: TraceHeader = serde_json::from_value(header_json)?;
    let mut writer = MoreporkWriter::create(out, &header, &[])?;

    let (res, code, obs, exp) = verdict;
    let last = entries.len() - 1;
    for (i, entry) in entries.iter().enumerate() {
        for (col, field) in sys.cpu_fields.iter().enumerate() {
            if field.wide {
                writer.set_u16(col, entry[col] as u16);
            } else {
                writer.set_u8(col, entry[col] as u8);
            }
        }
        let block = if i == last { [res, code, obs, exp] } else { [0; 4] };
        for (offset, value) in block.into_iter().enumerate() {
            writer.set_u8(ncols + offset, value);
        }
        writer.finish_entry()?;
    }
    if let Some(fr) = frame {
        if fr.width > 0 && fr.height > 0 && !fr.pixels.is_empty() && !fr.palette.is_empty() {
            let snapshot = IndexedFrame {
                width: fr.width as u16,
                height: fr.height as u16,
                pixel_aspect: fr.aspect,
                palette: fr.palette.clone(),
                pixels: fr.pixels.clone(),
            };
            writer.mark_frame(Some(&snapshot.to_bytes()))?;
        }
    }
    writer.finish()?;
    Ok(())
}
