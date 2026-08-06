//! morepork-openmsx: a morepork adapter driving openMSX as an MSX1 oracle
//! for the TI VDP (TMS9918A) test suite. openMSX's VDP is its own lineage —
//! independent of MAME's tms9928a — which is the point of having it.
//!
//! openMSX is driven headlessly (SDL_VIDEODRIVER=offscreen) over its stdio
//! control channel (`openmsx -control stdio`, an XML-framed Tcl console).
//! The C-BIOS machines make this ROM-free: `C-BIOS_MSX1_JP` boots the
//! suite's `.mx1` builds on an open-source BIOS with a TMS9918A (NTSC).
//!
//! The capture mechanism is richer than the MAME gdbstub one because
//! openMSX exposes the VDP through debuggables:
//!
//! 1. A breakpoint at the cartridge's INIT vector (read from the "AB"
//!    header) installs a per-instruction `debug set_condition` hook that
//!    logs CPU registers *plus VDP state* — the eight write registers, the
//!    status register, and the internal address/latch/read-ahead machinery
//!    (`VRAM pointer`, `VDP register latch status`, `VDP data latch
//!    value`) — one line per instruction, entirely in-process.
//! 2. A watchpoint on the RESULT byte ($E000, $A5 PASS / $5A FAIL) stops
//!    the trace at the verdict.
//! 3. `screenshot -raw` captures the rendered frame, reverse-mapped to TI
//!    VDP colour indices against openMSX's calibrated palette.
//!
//! The CLI mirrors the mame adapter's Go-flag surface:
//!
//! ```text
//! morepork-openmsx -rom sanity.mx1 -out trace.morepork
//! ```

mod openmsx_palette;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use morepork::format::write::MoreporkWriter;
use morepork::header::TraceHeader;
use morepork::snapshot::IndexedFrame;
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// --- trace layout ---
//
// One log line per instruction, decimal columns in this order. The two
// VRAM-pointer bytes are logged separately (little-endian) and combined
// into the catalogue's u16 `addr` on the Rust side.

#[derive(Clone, Copy)]
enum Kind {
    U8,
    U16,
    Bool,
}

/// Trace fields in file order; `addr` consumes two log columns.
static FIELDS: &[(&str, Kind)] = &[
    ("pc", Kind::U16), ("sp", Kind::U16),
    ("a", Kind::U8), ("f", Kind::U8), ("b", Kind::U8), ("c", Kind::U8),
    ("d", Kind::U8), ("e", Kind::U8), ("h", Kind::U8), ("l", Kind::U8),
    ("ix", Kind::U16), ("iy", Kind::U16),
    ("reg0", Kind::U8), ("reg1", Kind::U8), ("reg2", Kind::U8), ("reg3", Kind::U8),
    ("reg4", Kind::U8), ("reg5", Kind::U8), ("reg6", Kind::U8), ("reg7", Kind::U8),
    ("status", Kind::U8),
    ("addr", Kind::U16),
    ("latch", Kind::Bool),
    ("buffer", Kind::U8),
];

/// The Tcl expression producing one log line, matching `FIELDS` (with
/// `addr` as its two bytes).
const LINE_TCL: &str = concat!(
    "[reg pc] [reg sp] [reg a] [reg f] [reg b] [reg c] [reg d] [reg e] ",
    "[reg h] [reg l] [reg ix] [reg iy] ",
    "[debug read {VDP regs} 0] [debug read {VDP regs} 1] ",
    "[debug read {VDP regs} 2] [debug read {VDP regs} 3] ",
    "[debug read {VDP regs} 4] [debug read {VDP regs} 5] ",
    "[debug read {VDP regs} 6] [debug read {VDP regs} 7] ",
    "[debug read {VDP status regs} 0] ",
    "[debug read {VRAM pointer} 0] [debug read {VRAM pointer} 1] ",
    "[debug read {VDP register latch status} 0] ",
    "[debug read {VDP data latch value} 0]",
);

/// Log columns: every field is one column except `addr`, which is two.
const LOG_COLS: usize = 25;

// --- openMSX control channel ---

struct OpenMsx {
    child: Child,
    lines: mpsc::Receiver<String>,
}

impl OpenMsx {
    fn launch(machine: &str, rom: &str) -> Result<Self> {
        let mut child = Command::new("openmsx")
            .args(["-control", "stdio", "-machine", machine, "-carta", rom])
            .env("SDL_VIDEODRIVER", "offscreen")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("launch openmsx: {e}"))?;
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut o = OpenMsx { child, lines };
        o.write("<openmsx-control>\n")?;
        Ok(o)
    }

    fn write(&mut self, s: &str) -> Result<()> {
        let stdin = self.child.stdin.as_mut().ok_or("openmsx stdin closed")?;
        stdin.write_all(s.as_bytes())?;
        stdin.flush()?;
        Ok(())
    }

    /// Run a Tcl command, returning Ok(body) or Err on a `nok` reply.
    fn cmd(&mut self, tcl: &str) -> Result<String> {
        let escaped = tcl
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        self.write(&format!("<command>{escaped}</command>\n"))?;
        let mut buf = String::new();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| format!("openmsx reply timeout for: {tcl}"))?;
            buf.push_str(&self.lines.recv_timeout(remaining).map_err(|_| "openmsx exited or reply timeout")?);
            buf.push('\n');
            if let Some(start) = buf.find("<reply result=\"") {
                let rest = &buf[start + 15..];
                let Some(quote) = rest.find('"') else { continue };
                let result = &rest[..quote];
                let body = if let Some(open_end) = rest.find("/>") {
                    if rest[..open_end].find('>').is_none() {
                        // self-closing <reply result="ok"/>
                        String::new()
                    } else {
                        match rest.find("</reply>") {
                            Some(end) => rest[rest.find('>').unwrap() + 1..end].to_string(),
                            None => continue,
                        }
                    }
                } else {
                    match rest.find("</reply>") {
                        Some(end) => rest[rest.find('>').unwrap() + 1..end].to_string(),
                        None => continue,
                    }
                };
                let body = unescape(&body);
                return if result == "ok" {
                    Ok(body.trim().to_string())
                } else {
                    Err(format!("openmsx: {} -> {}", tcl, body.trim()).into())
                };
            }
        }
    }
}

impl Drop for OpenMsx {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#x0a;", "\n")
        .replace("&amp;", "&")
}

// --- CLI (matches the mame adapter's Go-flag surface) ---

struct Args {
    rom: String,
    out: String,
    machine: String,
    spec: String,
    frames: i64,
    frame: bool,
}

fn usage() -> ! {
    eprintln!(
        "usage: morepork-openmsx [flags]\n\
         \x20 -rom <path>       MSX cartridge (.mx1, \"AB\" header at 0x4000)\n\
         \x20 -out <path>       output .morepork path (default trace.morepork)\n\
         \x20 -machine <name>   openMSX machine (default C-BIOS_MSX1_JP)\n\
         \x20 -spec NTSC        TV spec (C-BIOS_MSX1_JP is NTSC-only)\n\
         \x20 -frames <n>       cap: emulated seconds = max(5, frames/50 + 3) (default 30)\n\
         \x20 -frame[=bool]     capture a final frame snapshot (default true)"
    );
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut args = Args {
        rom: String::new(),
        out: "trace.morepork".into(),
        machine: "C-BIOS_MSX1_JP".into(),
        spec: "NTSC".into(),
        frames: 30,
        frame: true,
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
            "rom" => args.rom = value,
            "out" => args.out = value,
            "machine" => args.machine = value,
            "spec" => args.spec = value,
            "frames" => args.frames = value.parse().unwrap_or_else(|_| usage()),
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
    if !args.spec.eq_ignore_ascii_case("NTSC") {
        eprintln!("error: C-BIOS_MSX1_JP is NTSC-only (TMS9918A); -spec {} is not supported", args.spec);
        std::process::exit(1);
    }
    if let Err(e) = run(&args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

// --- capture ---

fn run(args: &Args) -> Result<()> {
    let rom_bytes = std::fs::read(&args.rom)?;
    if rom_bytes.len() < 4 || &rom_bytes[0..2] != b"AB" {
        return Err("not an MSX cartridge: missing \"AB\" header".into());
    }
    let init = u16::from(rom_bytes[2]) | u16::from(rom_bytes[3]) << 8;
    let rom_sha = hex_encode(&Sha256::digest(&rom_bytes));

    let trace_log = tempfile::Builder::new().prefix("morepork-openmsx-").suffix(".log").tempfile()?;

    let mut o = OpenMsx::launch(&args.machine, &args.rom)?;
    let version = o.cmd("openmsx_info version").unwrap_or_else(|_| "unknown".into());
    o.cmd(if args.frame { "set renderer SDLGL-PP" } else { "set renderer none" })?;
    o.cmd("set throttle off")?;

    // Hooks go in before power-on so the capture is deterministic. The
    // INIT breakpoint logs its own instruction, then installs the
    // per-instruction condition; the RESULT watchpoint tears the trace
    // down at the verdict.
    o.cmd(&format!(
        "set ::fh [open {} w]\n\
         proc ::mp_line {{}} {{ puts $::fh \"{LINE_TCL}\" }}\n\
         debug set_bp 0x{init:04x} {{}} {{ ::mp_line; set ::cond [debug set_condition 1 {{::mp_line}}] }}\n\
         debug set_watchpoint write_mem 0xE000 \
           {{($::wp_last_value == 0xa5) || ($::wp_last_value == 0x5a)}} \
           {{ if {{[info exists ::cond]}} {{ debug remove_condition $::cond; unset ::cond }}\n\
              close $::fh\n\
              set ::verdict $::wp_last_value }}",
        trace_log.path().display()
    ))?;
    o.cmd("set power on")?;

    // Wait for the verdict, capped in emulated seconds (throttle is off,
    // so the cap is reached in moments of real time if the ROM never
    // latches a verdict). Frames convert at 50 Hz: the emulated MSX1 is
    // a PAL machine, and a 60 Hz conversion undercuts long frame-counted
    // ROMs (vram/retention's 3600 waited frames = 72 emulated seconds).
    let cap_seconds = std::cmp::max(5, args.frames / 50 + 3) as f64;
    // The wall deadline must cover the emulated budget: the tracing
    // callbacks run openMSX well below realtime (observed ~0.35x), so
    // allow 5 wall-seconds per emulated second, floor 120.
    let deadline = Instant::now()
        + Duration::from_secs(std::cmp::max(120, cap_seconds as u64 * 5));
    let verdict_seen = loop {
        if o.cmd("info exists ::verdict")? == "1" {
            break true;
        }
        let time: f64 = o.cmd("machine_info time")?.parse().unwrap_or(0.0);
        if time > cap_seconds {
            break false;
        }
        if Instant::now() > deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if !verdict_seen {
        // Tear the trace down so the file is flushed and parseable.
        o.cmd("if {[info exists ::cond]} { debug remove_condition $::cond; unset ::cond }; catch { close $::fh }")?;
        let emu: f64 = o.cmd("machine_info time")?.parse().unwrap_or(0.0);
        eprintln!(
            "warning: no verdict (cap {cap_seconds}s emulated, reached {emu:.0}s); \
             capturing state as-is"
        );
    }
    let block = o.cmd(
        "list [debug read memory 0xE000] [debug read memory 0xE001] \
              [debug read memory 0xE002] [debug read memory 0xE003]",
    )?;
    let mut verdict = [0u8; 4];
    for (slot, v) in verdict.iter_mut().zip(block.split_whitespace()) {
        *slot = v.parse().unwrap_or(0);
    }

    // Let the readout render (unthrottled: a real moment is many emulated
    // frames), then capture the frame.
    let frame = if args.frame {
        std::thread::sleep(Duration::from_millis(300));
        match capture_frame(&mut o) {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("warning: frame capture failed: {e}");
                None
            }
        }
    } else {
        None
    };
    drop(o);

    write_trace(&args.out, &args.machine, &version, &rom_sha, trace_log.path(), verdict, frame.as_ref())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// --- frame capture ---

struct FrameData {
    pixels: Vec<u8>, // 256x192 TI VDP colour indices
}

/// `screenshot -raw` writes the rendered MSX frame with borders — 320×240,
/// or 640×480 when the renderer's scale factor doubles it (normalized here
/// by sampling every other pixel). The TMS9918A active area is the centred
/// 256×192 at (32,24), located empirically with an all-white VRAM fill.
fn capture_frame(o: &mut OpenMsx) -> Result<FrameData> {
    let shot = tempfile::Builder::new().prefix("morepork-openmsx-").suffix(".png").tempfile()?;
    o.cmd(&format!("screenshot -raw {}", shot.path().display()))?;

    let decoder = png::Decoder::new(std::fs::File::open(shot.path())?);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => return Err(format!("unexpected screenshot colour type {other:?}").into()),
    };
    let scale = if w >= 640 { 2 } else { 1 };
    let (sw, sh) = (w / scale, h / scale);
    if sw < 320 || sh < 240 {
        return Err(format!("unexpected screenshot geometry {w}x{h}").into());
    }

    let mut exact: HashMap<u32, u8> = HashMap::new();
    for (colour, rgb) in openmsx_palette::OPENMSX_TI_VDP.iter().enumerate().skip(1) {
        exact.entry(rgb_key(rgb[0], rgb[1], rgb[2])).or_insert(colour as u8);
    }
    const VIS_W: usize = 256;
    const VIS_H: usize = 192;
    let (x0, y0) = (32, 24);
    let mut pixels = vec![0u8; VIS_W * VIS_H];
    let mut cache: HashMap<u32, u8> = HashMap::new();
    for y in 0..VIS_H {
        for x in 0..VIS_W {
            let i = ((y0 + y) * scale * w + (x0 + x) * scale) * channels;
            let (r, g, b) = (buf[i], buf[i + 1], buf[i + 2]);
            let key = rgb_key(r, g, b);
            let idx = *cache.entry(key).or_insert_with(|| {
                exact.get(&key).copied().unwrap_or_else(|| nearest(r, g, b))
            });
            pixels[y * VIS_W + x] = idx;
        }
    }
    Ok(FrameData { pixels })
}

fn rgb_key(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

/// Closest openMSX-rendered TI VDP colour (1-15) — the safety net for
/// GL stacks that shade slightly differently than the calibrated table.
fn nearest(r: u8, g: u8, b: u8) -> u8 {
    let mut best = 1u8;
    let mut best_d = i32::MAX;
    for (colour, e) in openmsx_palette::OPENMSX_TI_VDP.iter().enumerate().skip(1) {
        let dr = i32::from(r) - i32::from(e[0]);
        let dg = i32::from(g) - i32::from(e[1]);
        let db = i32::from(b) - i32::from(e[2]);
        let d = dr * dr + dg * dg + db * db;
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

// --- trace writing ---

fn write_trace(
    out: &str,
    machine: &str,
    version: &str,
    rom_sha: &str,
    log: &Path,
    verdict: [u8; 4],
    frame: Option<&FrameData>,
) -> Result<()> {
    let text = std::fs::read_to_string(log)?;
    let mut entries: Vec<Vec<u64>> = Vec::new();
    for line in text.lines() {
        let cols: Vec<u64> = line.split_whitespace().filter_map(|c| c.parse().ok()).collect();
        if cols.len() != LOG_COLS {
            continue;
        }
        entries.push(cols);
    }
    if entries.is_empty() {
        return Err("no trace entries (cartridge INIT never reached?)".into());
    }

    let mut fields: Vec<String> = FIELDS.iter().map(|(n, _)| n.to_string()).collect();
    fields.extend(["result", "code", "observed", "expected"].map(String::from));
    let mut header_json = serde_json::json!({
        "_header": true, "format_version": "0.1.0",
        "emulator": "openmsx", "emulator_version": version, "rom_sha256": rom_sha,
        "system": "msx1", "model": machine, "profile": "tier1",
        "fields": fields, "trigger": "instruction",
    });
    if frame.is_some() {
        header_json["pix_format"] = serde_json::json!("indexed8");
    }
    let header: TraceHeader = serde_json::from_value(header_json)?;
    let mut writer = MoreporkWriter::create(out, &header, &[])?;

    let last = entries.len() - 1;
    let nfields = FIELDS.len();
    for (i, entry) in entries.iter().enumerate() {
        let mut log_col = 0;
        for (field_col, (name, kind)) in FIELDS.iter().enumerate() {
            let value = if *name == "addr" {
                let v = entry[log_col] | entry[log_col + 1] << 8;
                log_col += 2;
                v
            } else {
                let v = entry[log_col];
                log_col += 1;
                v
            };
            match kind {
                Kind::U8 => writer.set_u8(field_col, value as u8),
                Kind::U16 => writer.set_u16(field_col, value as u16),
                Kind::Bool => writer.set_bool(field_col, value != 0),
            }
        }
        let block = if i == last { verdict } else { [0; 4] };
        for (offset, value) in block.into_iter().enumerate() {
            writer.set_u8(nfields + offset, value);
        }
        writer.finish_entry()?;
    }
    if let Some(fr) = frame {
        let snapshot = IndexedFrame {
            width: 256,
            height: 192,
            pixel_aspect: 8.0 / 7.0,
            palette: openmsx_palette::TI_VDP.to_vec(),
            pixels: fr.pixels.clone(),
        };
        writer.mark_frame(Some(&snapshot.to_bytes()))?;
    }
    writer.finish()?;
    Ok(())
}
