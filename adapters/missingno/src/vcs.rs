//! morepork-missingno-vcs — drive missingno's Atari VCS machine under the
//! suite's VCS adapter contract (the same CLI as adapters/stella and
//! adapters/gopher2600):
//!
//!     morepork-missingno-vcs -rom test.a26 -out trace.morepork \
//!         -spec PAL -frames 30 [-frame|-frame=false] [-type F8] [-swchb 0x48]
//!
//! One trace entry per instruction carrying the 6507 register file, beam
//! position, RIOT ports, and the suite's verdict bytes (zero page $80..$83:
//! RESULT/CODE/OBSERVED/EXPECTED). Tracing stops when RESULT holds a
//! terminal verdict ($A5 PASS / $5A FAIL); the result screen is given two
//! more frames to draw and a single indexed frame snapshot is embedded —
//! raw TIA colour bytes against the suite's canonical region palette, so
//! `morepork render` produces the same golden PNG as every other oracle.

use std::process::ExitCode;

use missingno_vcs::console::{Frame, Vcs};
use missingno_vcs::tia::VISIBLE_CLOCKS;
use missingno_vcs::{CartType, TvStandard};
use morepork::format::write::MoreporkWriter;
use morepork::header::{PixFormat, TraceHeader, Trigger};
use morepork::snapshot::IndexedFrame;
use sha2::{Digest, Sha256};

mod vcs_palette;

const PIXEL_ASPECT: f32 = 12.0 / 7.0;
// Bounds a single step_frame call; a PAL field is 312 lines.
const FRAME_LINE_BUDGET: usize = 400;
// Instruction safety cap: a ROM that neither syncs nor verdicts still halts.
const MAX_INSTRUCTIONS: u64 = 50_000_000;

// Suite mapper IDs (the .mapping sidecar vocabulary, shared with the
// Stella adapter's -type flag) <-> missingno board types.
const CART_TYPES: &[(&str, CartType)] = &[
    ("2K", CartType::Plain2K),
    ("4K", CartType::Plain4K),
    ("F8", CartType::F8),
    ("F8SC", CartType::F8Sc),
    ("F6", CartType::F6),
    ("F6SC", CartType::F6Sc),
    ("F4", CartType::F4),
    ("F4SC", CartType::F4Sc),
    ("FA", CartType::Fa),
    ("E0", CartType::E0),
    ("E7", CartType::E7),
    ("CV", CartType::Cv),
    ("UA", CartType::Ua),
    ("3F", CartType::ThreeF),
    ("FE", CartType::Fe),
    ("DPC", CartType::Dpc),
    ("AR", CartType::Ar),
    ("F0", CartType::F0),
    ("JANE", CartType::Jane),
    ("WF8", CartType::Wf8),
    ("WD", CartType::Wd),
    ("FC", CartType::Fc),
    ("0FA0", CartType::ZeroFa0),
    ("03E0", CartType::Zero3E0),
    ("3E", CartType::ThreeE),
    ("3E+", CartType::ThreeEPlus),
    ("EF", CartType::Ef),
    ("DF", CartType::Df),
    ("BF", CartType::Bf),
    ("SB", CartType::Sb),
    ("0840", CartType::Zero840),
    ("X07", CartType::X07),
    ("MDM", CartType::Mdm),
];

struct Args {
    rom: String,
    out: String,
    spec: String,
    frames: u32,
    frame: bool,
    cart_type: Option<CartType>,
    swchb: u8,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        rom: String::new(),
        out: "trace.morepork".into(),
        spec: "NTSC".into(),
        frames: 30,
        frame: true,
        cart_type: None,
        swchb: 0x48,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next().ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "-rom" => args.rom = value("-rom")?,
            "-out" => args.out = value("-out")?,
            "-spec" => args.spec = value("-spec")?,
            "-frames" => {
                args.frames = value("-frames")?
                    .parse()
                    .map_err(|e| format!("-frames: {e}"))?
            }
            "-frame" | "-frame=true" => args.frame = true,
            "-frame=false" => args.frame = false,
            "-type" | "-mapping" => {
                let id = value("-type")?.to_uppercase();
                if id != "AUTO" {
                    args.cart_type = Some(
                        CART_TYPES
                            .iter()
                            .find(|(name, _)| *name == id)
                            .ok_or_else(|| format!("unknown cartridge type '{id}'"))?
                            .1,
                    );
                }
            }
            "-swchb" => {
                let v = value("-swchb")?;
                let v = v.strip_prefix("0x").unwrap_or(&v);
                args.swchb =
                    u8::from_str_radix(v, 16).map_err(|e| format!("-swchb: {e}"))?;
            }
            other => return Err(format!("unknown flag '{other}'")),
        }
    }
    if args.rom.is_empty() {
        return Err("usage: morepork-missingno-vcs -rom <file.a26> -out <trace> \
                    -spec <NTSC|PAL|SECAM> -frames <n> [-frame] [-type <ID>] \
                    [-swchb <hex>]"
            .into());
    }
    Ok(args)
}

fn spec_to_standard(spec: &str) -> Result<TvStandard, String> {
    match spec.to_uppercase().as_str() {
        "NTSC" => Ok(TvStandard::Ntsc),
        "PAL" | "PAL60" => Ok(TvStandard::Pal),
        "SECAM" => Ok(TvStandard::Secam),
        other => Err(format!("unknown spec '{other}'")),
    }
}

fn canonical_palette(standard: TvStandard) -> &'static [[u8; 3]; 256] {
    match standard {
        TvStandard::Ntsc => &vcs_palette::CANONICAL_NTSC,
        TvStandard::Pal => &vcs_palette::CANONICAL_PAL,
        TvStandard::Secam => &vcs_palette::CANONICAL_SECAM,
    }
}

/// Run to the next instruction boundary, returning the CPU cycles
/// consumed (WSYNC parks the CPU, so one store can span most of a line).
fn step_instruction_counted(vcs: &mut Vcs) -> u16 {
    let mut cycles = 0u16;
    while vcs.cpu.at_instruction_boundary() && !vcs.cpu.halted() {
        vcs.step_cpu_cycle();
        cycles += 1;
    }
    while !vcs.cpu.at_instruction_boundary() && !vcs.cpu.halted() {
        vcs.step_cpu_cycle();
        cycles += 1;
    }
    cycles
}

// Field order in every trace entry; the four verdict bytes mirror the
// suite convention at zero page $80..$83.
const FIELDS: &[&str] = &[
    "pc", "a", "x", "y", "s", "p", "rdy", "cycles", "line", "clock", "timer",
    "port_a", "port_b", "result", "code", "observed", "expected",
];

fn capture(writer: &mut MoreporkWriter, vcs: &Vcs, cycles: u16) -> Result<(), morepork::Error> {
    for (col, field) in FIELDS.iter().enumerate() {
        match *field {
            "pc" => writer.set_u16(col, vcs.cpu.pc),
            "a" => writer.set_u8(col, vcs.cpu.a),
            "x" => writer.set_u8(col, vcs.cpu.x),
            "y" => writer.set_u8(col, vcs.cpu.y),
            "s" => writer.set_u8(col, vcs.cpu.s),
            "p" => writer.set_u8(col, vcs.cpu.p),
            "rdy" => writer.set_bool(col, vcs.cpu.rdy),
            "cycles" => writer.set_u16(col, cycles),
            "line" => writer.set_u16(col, vcs.scanline() as u16),
            "clock" => writer.set_u8(col, vcs.tia.beam() as u8),
            "timer" => writer.set_u8(col, vcs.peek(0x284)),
            "port_a" => writer.set_u8(col, vcs.peek(0x280)),
            "port_b" => writer.set_u8(col, vcs.peek(0x282)),
            "result" => writer.set_u8(col, vcs.peek(0x0080)),
            "code" => writer.set_u8(col, vcs.peek(0x0081)),
            "observed" => writer.set_u8(col, vcs.peek(0x0082)),
            "expected" => writer.set_u8(col, vcs.peek(0x0083)),
            _ => unreachable!(),
        }
    }
    writer.finish_entry()
}

fn run(args: &Args) -> Result<(), String> {
    let rom = std::fs::read(&args.rom).map_err(|e| format!("{}: {e}", args.rom))?;
    let standard = spec_to_standard(&args.spec)?;

    let mut vcs = Vcs::new(&rom, standard, args.cart_type)
        .map_err(|e| format!("cartridge: {e:?}"))?;
    // Console switches from the SWCHB byte (bit3 colour, bit6/7 difficulty).
    vcs.set_color_mode(args.swchb & 0x08 != 0);
    vcs.set_difficulty(0, args.swchb & 0x40 != 0);
    vcs.set_difficulty(1, args.swchb & 0x80 != 0);

    // Detection-linter line, mirroring the stella/gopher adapters. The
    // inference itself is missingno's; this reports the by-size rule
    // (the superchip signature check stays internal).
    let detected = args.cart_type.or(match rom.len() {
        0x800 => Some(CartType::Plain2K),
        0x1000 => Some(CartType::Plain4K),
        0x2000 => Some(CartType::F8),
        0x4000 => Some(CartType::F6),
        0x8000 => Some(CartType::F4),
        _ => None,
    });
    let id = detected
        .and_then(|d| CART_TYPES.iter().find(|(_, t)| *t == d).map(|(n, _)| *n))
        .unwrap_or("?");
    eprintln!("cartridge-type: {id}");

    let mut hasher = Sha256::new();
    hasher.update(&rom);
    let rom_sha256 = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    let header = TraceHeader {
        _header: true,
        format_version: "0.1.0".into(),
        emulator: "missingno".into(),
        emulator_version: env!("CARGO_PKG_VERSION").into(),
        rom_sha256,
        system: "vcs".into(),
        model: standard_name(standard).into(),
        profile: "vcs-suite".into(),
        fields: FIELDS.iter().map(|f| f.to_string()).collect(),
        trigger: Trigger::Instruction,
        pix_format: PixFormat::Indexed8,
        ..Default::default()
    };
    let mut writer =
        MoreporkWriter::create(&args.out, &header, &[]).map_err(|e| e.to_string())?;

    let mut last_frame: Option<Frame> = None;
    let mut frames_done: u32 = 0;
    let mut verdict = false;
    let mut instructions: u64 = 0;
    while frames_done < args.frames && instructions < MAX_INSTRUCTIONS {
        let cycles = step_instruction_counted(&mut vcs);
        instructions += 1;
        capture(&mut writer, &vcs, cycles).map_err(|e| e.to_string())?;
        if let Some(frame) = vcs.take_frame() {
            frames_done += 1;
            last_frame = Some(frame);
        }
        let result = vcs.peek(0x0080);
        if result == 0xA5 || result == 0x5A {
            verdict = true;
            break;
        }
        if cycles == 0 {
            break; // JAM: the CPU is parked for good
        }
    }

    if args.frame {
        // A SELF test publishes its verdict before the pass/fail screen
        // renders: give it two more frames to draw.
        if verdict {
            for _ in 0..2 {
                match vcs.step_frame(FRAME_LINE_BUDGET) {
                    Some(frame) => last_frame = Some(frame),
                    None => break,
                }
            }
        }
        if let Some(frame) = &last_frame {
            let palette = canonical_palette(standard);
            let snapshot = IndexedFrame {
                width: VISIBLE_CLOCKS as u16,
                height: frame.lines.len() as u16,
                pixel_aspect: PIXEL_ASPECT,
                palette: palette.to_vec(),
                // Raw TIA colour bytes: the canonical table is indexed by
                // the full byte (odd entries black, bit 0 ignored).
                pixels: frame.lines.iter().flatten().copied().collect(),
            }
            .to_bytes();
            writer
                .mark_frame(Some(&snapshot))
                .map_err(|e| e.to_string())?;
        }
    }

    writer.finish().map_err(|e| e.to_string())?;
    eprintln!(
        "trace written: {frames_done} frames, {instructions} instructions, verdict={verdict}"
    );
    Ok(())
}

fn standard_name(standard: TvStandard) -> &'static str {
    match standard {
        TvStandard::Ntsc => "NTSC",
        TvStandard::Pal => "PAL",
        TvStandard::Secam => "SECAM",
    }
}

fn main() -> ExitCode {
    match parse_args().and_then(|args| run(&args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("morepork-missingno-vcs: {err}");
            ExitCode::FAILURE
        }
    }
}
