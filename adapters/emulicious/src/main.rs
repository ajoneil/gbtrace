//! morepork-emulicious: a morepork adapter driving Emulicious as a
//! **verdict-level** oracle for the TI VDP (TMS9918A) test suite, on both
//! SG-1000 (`.sg`) and MSX1 (`.mx1`, booted on Emulicious's bundled
//! C-BIOS). Emulicious's VDP is Calindro's own lineage — independent of
//! MAME's and openMSX's — so its pass/fail vote is a genuine third
//! opinion on both machines.
//!
//! Why verdict-level and not an instruction trace: Emulicious's only
//! programmatic surface is its DAP remote-debugging server (`-remotedebug
//! <port>`), and each DAP request costs ~40ms (the server synchronizes
//! with the emulation thread per request), so per-instruction stepping
//! tops out around 2-12 instructions/second — unusable for capture.
//! Address breakpoints are also out of reach remotely: DAP function
//! breakpoints resolve debug symbols only, and no symfile format we tried
//! resolves for plain-ROM loads. What *is* fast is free-running at
//! `-turbo` speed while polling memory with `evaluate` (which works while
//! running): the adapter runs to the verdict, pauses, and captures the
//! RESULT block plus the final CPU state and the `va`/`scanline` VDP
//! internals as a single-entry trace.
//!
//! ```text
//! morepork-emulicious -rom sanity.sg  -out trace.morepork   # sg1000
//! morepork-emulicious -rom sanity.mx1 -out trace.morepork   # msx1
//! ```

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use morepork::format::write::MoreporkWriter;
use morepork::header::TraceHeader;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const DEFAULT_JAR: &str = "/usr/share/emulicious-bin/Emulicious.jar";

// --- DAP client ---

struct Dap {
    stream: TcpStream,
    incoming: mpsc::Receiver<Value>,
    events: std::collections::VecDeque<Value>,
    seq: u64,
}

impl Dap {
    fn connect(port: u16) -> Result<Self> {
        let mut stream = None;
        for _ in 0..300 {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
                stream = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let stream = stream.ok_or("Emulicious DAP server never listened")?;
        let reader = stream.try_clone()?;
        let (tx, incoming) = mpsc::channel();
        std::thread::spawn(move || pump(reader, tx));
        Ok(Dap { stream, incoming, events: Default::default(), seq: 0 })
    }

    /// Send a request and wait for its response, queue-passing any events
    /// seen on the way (the caller polls them separately if it cares).
    fn request(&mut self, command: &str, arguments: Value) -> Result<Value> {
        self.seq += 1;
        let body = json!({
            "seq": self.seq, "type": "request",
            "command": command, "arguments": arguments,
        })
        .to_string();
        write!(self.stream, "Content-Length: {}\r\n\r\n{body}", body.len())?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| format!("timeout waiting for {command} response"))?;
            let msg = self
                .incoming
                .recv_timeout(remaining)
                .map_err(|_| format!("Emulicious exited or {command} response timeout"))?;
            if msg["type"] == "response" && msg["request_seq"] == json!(self.seq) {
                return Ok(msg);
            }
            if msg["type"] == "event" {
                self.events.push_back(msg);
            }
        }
    }

    /// Wait for a named event, buffering others seen on the way.
    fn wait_event(&mut self, name: &str, timeout: Duration) -> Option<Value> {
        if let Some(pos) = self.events.iter().position(|e| e["event"] == json!(name)) {
            return self.events.remove(pos);
        }
        let deadline = Instant::now() + timeout;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            let Ok(msg) = self.incoming.recv_timeout(remaining) else { break };
            if msg["type"] == "event" {
                if msg["event"] == json!(name) {
                    return Some(msg);
                }
                self.events.push_back(msg);
            }
        }
        None
    }

    /// Evaluate an Emulicious expression, returning the printed result.
    fn eval(&mut self, expression: &str) -> Result<String> {
        let r = self.request("evaluate", json!({"expression": expression, "context": "repl"}))?;
        if r["success"] == json!(true) {
            Ok(r["body"]["result"].as_str().unwrap_or_default().to_string())
        } else {
            Err(format!("evaluate {expression}: {}", r["message"]).into())
        }
    }

    /// Evaluate an expression of the form `name = $HEX ...` to a number.
    fn eval_u16(&mut self, expression: &str) -> Result<u16> {
        let printed = self.eval(expression)?;
        parse_hex_result(&printed)
            .ok_or_else(|| format!("unparseable evaluate result: {printed:?}").into())
    }
}

fn pump(mut reader: TcpStream, tx: mpsc::Sender<Value>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        loop {
            let Some(header_end) = find(&buf, b"\r\n\r\n") else { break };
            let header = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let Some(length) = header
                .lines()
                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().ok()))
                .flatten()
            else {
                return;
            };
            let start = header_end + 4;
            if buf.len() < start + length {
                break;
            }
            if let Ok(msg) = serde_json::from_slice::<Value>(&buf[start..start + length]) {
                if tx.send(msg).is_err() {
                    return;
                }
            }
            buf.drain(..start + length);
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Parse `pc = $4181` / `sp = $DFF0 (_RAM_DFF0_)` / `@$e000 = $A5` to the
/// hex value. The value is introduced by the *last* `$` — expressions like
/// `@$e000` echo their own `$` first.
fn parse_hex_result(printed: &str) -> Option<u16> {
    let hex = printed.rsplit('$').next()?;
    let digits: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    u16::from_str_radix(&digits, 16).ok()
}

// --- systems ---

struct SysDef {
    id: &'static str,    // morepork header `system`
    model: &'static str, // header `model`
    result_addr: u16,    // RESULT block base
    extension: &'static str, // ROM extension selecting this row
}

static SYSTEMS: &[SysDef] = &[
    SysDef { id: "sg1000", model: "NTSC", result_addr: 0xC000, extension: "sg" },
    SysDef { id: "msx1", model: "C-BIOS", result_addr: 0xE000, extension: "mx1" },
];

// --- CLI (mame-adapter conventions) ---

struct Args {
    rom: String,
    out: String,
    system: String,
    spec: String,
    frames: i64,
    jar: String,
}

fn usage() -> ! {
    eprintln!(
        "usage: morepork-emulicious [flags]\n\
         \x20 -rom <path>       ROM (.sg for sg1000, .mx1 for msx1)\n\
         \x20 -out <path>       output .morepork path (default trace.morepork)\n\
         \x20 -system <id>      sg1000 or msx1 (default: from the ROM extension)\n\
         \x20 -spec NTSC        TV spec (NTSC only)\n\
         \x20 -frames <n>       verdict deadline: real seconds = max(10, frames/30) (default 30)\n\
         \x20 -jar <path>       Emulicious.jar (default {DEFAULT_JAR})"
    );
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut args = Args {
        rom: String::new(),
        out: "trace.morepork".into(),
        system: String::new(),
        spec: "NTSC".into(),
        frames: 30,
        jar: DEFAULT_JAR.into(),
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
            "system" => args.system = value,
            "spec" => args.spec = value,
            "frames" => args.frames = value.parse().unwrap_or_else(|_| usage()),
            "jar" => args.jar = value,
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
        eprintln!("error: -spec {} is not supported (NTSC only)", args.spec);
        std::process::exit(1);
    }
    let system = if args.system.is_empty() {
        let ext = args.rom.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
        match SYSTEMS.iter().find(|s| s.extension == ext) {
            Some(s) => s,
            None => {
                eprintln!("error: cannot infer -system from extension {ext:?} (use -system sg1000|msx1)");
                std::process::exit(2);
            }
        }
    } else {
        match SYSTEMS.iter().find(|s| s.id == args.system) {
            Some(s) => s,
            None => {
                eprintln!("error: unknown -system {:?} (sg1000, msx1)", args.system);
                std::process::exit(2);
            }
        }
    };
    if let Err(e) = run(system, &args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

// --- capture ---

struct Emulicious(Option<Child>);

impl Drop for Emulicious {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn run(sys: &SysDef, args: &Args) -> Result<()> {
    let rom_bytes = std::fs::read(&args.rom)?;
    let rom_sha = hex_encode(&Sha256::digest(&rom_bytes));
    let rom_abs = std::fs::canonicalize(&args.rom)?;

    let port = {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.local_addr()?.port()
    };
    let child = Command::new("java")
        .args(["-jar", &args.jar, "-remotedebug", &port.to_string(), "-turbo"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("launch emulicious: {e}"))?;
    let _guard = Emulicious(Some(child));

    let mut d = Dap::connect(port)?;
    d.request("initialize", json!({"adapterID": "morepork", "clientID": "morepork"}))?;
    let launch = d.request(
        "launch",
        json!({"program": rom_abs.to_string_lossy(), "stopOnEntry": true}),
    )?;
    if launch["success"] != json!(true) {
        return Err(format!("emulicious launch failed: {}", launch["message"]).into());
    }
    // stopOnEntry parks the machine at reset; wait for that stop before
    // continuing so the run is deterministic regardless of JVM startup
    // timing (continuing before the entry stop registers is silently lost).
    d.wait_event("initialized", Duration::from_secs(10));
    if d.wait_event("stopped", Duration::from_secs(15)).is_none() {
        return Err("no entry stop from Emulicious after launch".into());
    }
    let cont = d.request("continue", json!({"threadId": 1}))?;
    if cont["success"] != json!(true) {
        return Err(format!("continue failed: {}", cont["message"]).into());
    }

    // Free-run at -turbo speed, polling the RESULT byte live (evaluate
    // works while running). This is the only fast surface Emulicious has.
    let verdict_expr = format!("@${:04x}", sys.result_addr);
    let deadline = Instant::now() + Duration::from_secs(std::cmp::max(10, args.frames / 30) as u64);
    let mut verdict_seen = false;
    while Instant::now() < deadline {
        if let Ok(v) = d.eval_u16(&verdict_expr) {
            if v == 0xA5 || v == 0x5A {
                verdict_seen = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !verdict_seen {
        eprintln!("warning: no verdict before the deadline; capturing state as-is");
    }
    d.request("pause", json!({"threadId": 1}))?;
    std::thread::sleep(Duration::from_millis(200));

    // Final state: registers, the VDP address/beam internals, the RESULT block.
    let mut state = HashMap::new();
    for expr in ["pc", "sp", "af", "bc", "de", "hl", "ix", "iy", "va", "scanline"] {
        state.insert(expr, d.eval_u16(expr)?);
    }
    let mut block = [0u8; 4];
    for (i, slot) in block.iter_mut().enumerate() {
        *slot = d.eval_u16(&format!("@${:04x}", sys.result_addr + i as u16))? as u8;
    }
    let _ = d.request("disconnect", json!({"terminateDebuggee": true}));

    let version = emulicious_version(&args.jar);
    write_trace(&args.out, sys, &version, &rom_sha, &state, block)
}

/// The release date heading at the top of WhatsNew.txt beside the jar.
fn emulicious_version(jar: &str) -> String {
    let whatsnew = std::path::Path::new(jar).with_file_name("WhatsNew.txt");
    std::fs::read_to_string(whatsnew)
        .ok()
        .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('=')).map(String::from))
        .unwrap_or_else(|| "unknown".into())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// --- trace writing ---

/// A single-entry trace: the machine state at the verdict, plus the RESULT
/// block. Emulicious contributes an independent pass/fail vote, not an
/// instruction stream (see the module docs for why).
fn write_trace(
    out: &str,
    sys: &SysDef,
    version: &str,
    rom_sha: &str,
    state: &HashMap<&str, u16>,
    block: [u8; 4],
) -> Result<()> {
    let fields = [
        "pc", "sp", "a", "f", "b", "c", "d", "e", "h", "l", "ix", "iy",
        "addr", "line", "result", "code", "observed", "expected",
    ];
    let header_json = json!({
        "_header": true, "format_version": "0.1.0",
        "emulator": "emulicious", "emulator_version": version, "rom_sha256": rom_sha,
        "system": sys.id, "model": sys.model, "profile": "tier1",
        "fields": fields, "trigger": "instruction",
    });
    let header: TraceHeader = serde_json::from_value(header_json)?;
    let mut writer = MoreporkWriter::create(out, &header, &[])?;

    let get = |k: &str| state.get(k).copied().unwrap_or(0);
    let pairs = [("a", "f", get("af")), ("b", "c", get("bc")), ("d", "e", get("de")), ("h", "l", get("hl"))];
    let mut byte_of = HashMap::new();
    for (hi, lo, v) in pairs {
        byte_of.insert(hi, (v >> 8) as u8);
        byte_of.insert(lo, v as u8);
    }
    for (col, name) in fields.iter().enumerate() {
        match *name {
            "pc" | "sp" | "ix" | "iy" => writer.set_u16(col, get(name)),
            "addr" => writer.set_u16(col, get("va")),
            "line" => writer.set_u16(col, get("scanline")),
            "a" | "f" | "b" | "c" | "d" | "e" | "h" | "l" => writer.set_u8(col, byte_of[name]),
            "result" => writer.set_u8(col, block[0]),
            "code" => writer.set_u8(col, block[1]),
            "observed" => writer.set_u8(col, block[2]),
            "expected" => writer.set_u8(col, block[3]),
            _ => unreachable!(),
        }
    }
    writer.finish_entry()?;
    writer.finish()?;
    Ok(())
}
