use std::fs;
use std::path::PathBuf;
use std::process;

use clap::Parser;
use missingno_core::video::RawFrame;
use missingno_gb::cartridge::Cartridge;
use missingno_gb::system::ConsoleUi;
use missingno_gb::trace::{self, BootRom, Profile, TraceScope, Tracer, Trigger};
use missingno_gb::{Console, GameBoy};
use missingno_gbc::GameBoyColor;

#[derive(Parser)]
#[command(name = "morepork-missingno")]
struct Args {
    #[arg(long)]
    rom: PathBuf,

    #[arg(long)]
    profile: PathBuf,

    #[arg(long)]
    output: PathBuf,

    #[arg(long, default_value_t = 3000)]
    frames: u32,

    /// Run for exactly N T-cycles, then capture the screen and stop (gambatte
    /// tests: read the framebuffer after a fixed cycle budget, not N vblanks).
    #[arg(long)]
    until_tcycle: Option<u64>,

    /// Console model: dmg (original Game Boy) or cgb (Game Boy Color).
    #[arg(long, default_value = "dmg")]
    model: String,

    /// Stop when opcode at PC matches (hex, e.g. 40 for LD B,B)
    #[arg(long, value_parser = parse_hex_u8)]
    stop_opcode: Option<u8>,

    /// Stop when this byte is sent via serial (hex, e.g. 0A)
    #[arg(long, value_parser = parse_hex_u8)]
    stop_on_serial: Option<u8>,

    /// Number of serial byte matches before stopping
    #[arg(long, default_value_t = 1)]
    stop_serial_count: u32,

    /// Reference .pix file for screenshot matching
    #[arg(long)]
    reference: Option<PathBuf>,

    /// Extra frames to capture after stop condition
    #[arg(long, default_value_t = 0)]
    extra_frames: u32,

    /// Report last-frame audio activity to stderr as `AUDIO=0` / `AUDIO=1`
    /// (used by the gambatte `_outaudio` pass/fail check).
    #[arg(long, default_value_t = false)]
    report_audio: bool,

    /// Stop when memory ADDR equals VAL (hex, e.g. FF82=01) or ADDR!=VAL. Can be repeated.
    #[arg(long = "stop-when", value_parser = parse_stop_when)]
    stop_when: Vec<StopWhen>,
}

#[derive(Clone)]
struct StopWhen {
    addr: u16,
    value: u8,
    negate: bool,
}

fn parse_hex_u8(s: &str) -> Result<u8, String> {
    u8::from_str_radix(s, 16).map_err(|e| format!("invalid hex byte: {e}"))
}

fn parse_stop_when(s: &str) -> Result<StopWhen, String> {
    let (addr_s, val_s, negate) = if let Some((a, v)) = s.split_once("!=") {
        (a, v, true)
    } else if let Some((a, v)) = s.split_once('=') {
        (a, v, false)
    } else {
        return Err("expected ADDR=VAL or ADDR!=VAL (e.g. A000!=80)".to_string());
    };
    let addr = u16::from_str_radix(addr_s, 16).map_err(|e| format!("invalid address: {e}"))?;
    let value = u8::from_str_radix(val_s, 16).map_err(|e| format!("invalid value: {e}"))?;
    Ok(StopWhen { addr, value, negate })
}

/// Reference screenshots are raw RGB555 (160×144×3 bytes, each channel 0-31).
/// Comparing at the CGB's native 5-bit precision is expansion-neutral.
fn load_reference(path: &PathBuf) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("Failed to read reference {}: {e}", path.display()))
}

/// DMG shade index (0=lightest) → greyscale RGB555 channel value.
const GREY555: [u8; 4] = [31, 21, 10, 0];

/// Per-channel RGB555 match with a small tolerance, to absorb minor
/// 555→888 expansion / quantisation differences between emulators.
fn rgb555_match(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (*x as i16 - *y as i16).abs() <= 1)
}

/// Flatten the console's pre-resolution screen ([`ConsoleUi::raw_frame`]:
/// DMG shade indices, CGB RGB555 words) into the reference format — one byte
/// per 5-bit channel, row-major.
fn raw_frame_rgb555(frame: &RawFrame) -> Vec<u8> {
    match frame {
        RawFrame::Shade2 { pixels, .. } => pixels
            .iter()
            .flat_map(|&shade| {
                let v = GREY555[shade as usize];
                [v, v, v]
            })
            .collect(),
        RawFrame::Rgb555 { pixels, .. } => pixels
            .iter()
            .flat_map(|&p| {
                [(p & 0x1F) as u8, ((p >> 5) & 0x1F) as u8, ((p >> 10) & 0x1F) as u8]
            })
            .collect(),
        _ => unreachable!("GB consoles emit Shade2 or Rgb555 frames"),
    }
}

/// T-cycles in one DMG/CGB frame. Used as a time-based safety budget so a
/// ROM that disables the LCD (no frame is ever produced) still terminates
/// instead of spinning the frame-count loop forever — gbmicrotest toggle_lcdc
/// is the canonical offender. missingno's own test harness notes the same
/// trap and bounds by step count for the same reason.
const CYCLES_PER_FRAME: u64 = 70224;

/// Last-frame audio-activity check, matching gambatte's testrunner
/// convention (the final frame's samples either all match its first
/// sample → silent, or differ → audio). Tolerance accounts for APU
/// DC-offset drift.
fn last_frame_has_audio(samples: &[(f32, f32)], frames: u32) -> bool {
    if samples.is_empty() || frames == 0 {
        return false;
    }
    let per_frame = (samples.len() / frames as usize).max(1);
    let last = &samples[samples.len().saturating_sub(per_frame)..];
    let (l0, r0) = last[0];
    last.iter()
        .any(|&(l, r)| (l - l0).abs() > 0.005 || (r - r0).abs() > 0.005)
}

fn main() {
    let args = Args::parse();

    let rom_data = fs::read(&args.rom).unwrap_or_else(|e| {
        eprintln!("Error: failed to read ROM {}: {e}", args.rom.display());
        process::exit(1);
    });

    let cartridge = Cartridge::new(rom_data, None, None).unwrap_or_else(|e| {
        eprintln!("Error: {}: {e}", args.rom.display());
        process::exit(1);
    });

    let is_cgb = matches!(args.model.to_ascii_lowercase().as_str(), "cgb" | "gbc");
    if is_cgb {
        // missingno-gbc targets CPU-CGB-C (gambatte's cgb04c) — same model
        // gambatte's adapter reports, so cross-emulator CGB diffs line up.
        run(GameBoyColor::new(cartridge, None), &args);
    } else {
        run(GameBoy::new(cartridge, None), &args);
    }
}

fn run<M: ConsoleUi>(mut gb: Console<M>, args: &Args) {
    let profile = Profile::load(&args.profile).unwrap_or_else(|e| {
        eprintln!("Error: failed to load profile {}: {e}", args.profile.display());
        process::exit(1);
    });

    // The column set is authored from the console model's state schema —
    // the profile contributes the capture cadence. Full tier depth, matching
    // missingno's own CLI trace command: this is a reference capture.
    let mut tracer = Tracer::create(
        &args.output,
        &gb,
        profile.trigger.clone(),
        TraceScope::Full,
        BootRom::Skip,
        M::TRACE_MODEL_NAME,
    )
    .unwrap_or_else(|e| {
        eprintln!("Error: failed to create tracer: {e}");
        process::exit(1);
    });

    // Mark entry 0 as a frame boundary so the setup period is included.
    tracer.mark_frame().unwrap();

    // Discard any startup audio so `--report-audio` measures only the run.
    if args.report_audio {
        let _ = gb.drain_audio_samples();
    }

    let reference_pix = args.reference.as_ref().map(load_reference);
    let is_tcycle = tracer.trigger() == Trigger::Tcycle;

    let mut frame_count: u32 = 0;
    let mut stop_triggered = false;
    let mut remaining_extra: Option<u32> = None;
    let mut serial_match_count: u32 = 0;

    // Detect serial writes by watching SC bit 7 (transfer start)
    let mut prev_sc_high = (gb.peek_range(0xFF02, 1)[0] & 0x80) != 0;

    // Time-based safety budget: bounds the run even when the LCD never turns on
    // and `frame_count` can't advance. One frame of slack keeps it from ever
    // truncating a legitimate `--frames`-bounded run.
    let max_tcycles = (args.frames as u64)
        .saturating_add(1)
        .saturating_mul(CYCLES_PER_FRAME);
    let mut total_tcycles: u64 = 0;

    // Cycle-budget mode (gambatte hex/blank tests): the harness passes a budget
    // of N × 70224 dots. Sample after N real *frames* (vblanks), not N CPU
    // T-cycles — matching missingno's own `run_frames(N)`. A raw CPU-T-cycle
    // budget under-runs CGB double speed (a real frame is 2× the T-cycles), so
    // the `_ds_` result isn't on screen yet at the budget. At single speed the
    // two are identical (1 dot = 1 T-cycle).
    let sample_frames = args.until_tcycle.map(|b| (b / CYCLES_PER_FRAME) as u32);

    loop {
        // Cycle-budget mode: stop after the derived number of real frames and
        // snapshot the screen at that instant (see `sample_frames` above).
        if let Some(sf) = sample_frames {
            if frame_count >= sf {
                eprintln!("Frame budget reached ({frame_count} frames, {total_tcycles} cycles)");
                break;
            }
        }
        if frame_count >= args.frames {
            eprintln!("Frame limit reached ({} frames)", args.frames);
            break;
        }

        if total_tcycles >= max_tcycles {
            eprintln!("T-cycle limit reached ({total_tcycles} cycles; LCD likely off)");
            break;
        }

        if let Some(ref mut remaining) = remaining_extra {
            if *remaining == 0 {
                break;
            }
        }

        let (new_screen, tcycles) = if is_tcycle && !gb.speed_switch_in_progress() {
            // The shared dot-by-dot driver: captures at every T-cycle, pushes
            // pixels in the model's native encoding, marks frames, and resolves
            // STOP / VRAM-DMA holds at the boundary.
            let result = trace::step_instruction_tcycle(&mut gb, &mut tracer);
            (result.new_screen, result.tcycles as u64)
        } else {
            // During a CGB speed-switch blackout the tcycle driver can't advance
            // the frozen CPU; only `step()` drains it. Fall back to instruction
            // stepping for the blackout, then resume tcycle capture. (Also the
            // steady-state path for instruction-triggered profiles.)
            tracer.capture(&gb).unwrap();
            let result = gb.step();
            tracer.advance(result.tcycles);
            if result.new_screen {
                tracer.mark_frame().unwrap();
            }
            (result.new_screen, result.tcycles as u64)
        };
        total_tcycles += tcycles;

        if !stop_triggered {
            if let Some(opcode) = args.stop_opcode {
                let pc = gb.cpu().pc;
                if gb.peek_range(pc, 1)[0] == opcode {
                    eprintln!("Stop condition met: opcode 0x{opcode:02X} at PC=0x{pc:04X}");
                    stop_triggered = true;
                    remaining_extra = Some(args.extra_frames);
                }
            }

            for sw in &args.stop_when {
                let actual = gb.peek_range(sw.addr, 1)[0];
                let hit = if sw.negate { actual != sw.value } else { actual == sw.value };
                if hit {
                    let op = if sw.negate { "!=" } else { "==" };
                    eprintln!("Stop condition met: [0x{:04X}] {op} 0x{:02X}", sw.addr, sw.value);
                    stop_triggered = true;
                    remaining_extra = Some(args.extra_frames);
                    break;
                }
            }

            if let Some(serial_byte) = args.stop_on_serial {
                let sc_high = (gb.peek_range(0xFF02, 1)[0] & 0x80) != 0;
                if sc_high && !prev_sc_high {
                    let sb = gb.peek_range(0xFF01, 1)[0];
                    if sb == serial_byte {
                        serial_match_count += 1;
                        if serial_match_count >= args.stop_serial_count {
                            eprintln!(
                                "Stop condition met: serial byte 0x{serial_byte:02X} (count {serial_match_count})"
                            );
                            stop_triggered = true;
                            remaining_extra = Some(args.extra_frames);
                        }
                    }
                }
                prev_sc_high = sc_high;
            }
        }

        // Reference screenshot check runs on every frame boundary, even
        // after other stop conditions fire (the screen may not have
        // updated yet when serial/opcode triggers).
        if new_screen {
            if let Some(ref reference) = reference_pix {
                let current = raw_frame_rgb555(&M::raw_frame(&gb));
                if rgb555_match(&current, reference) {
                    if !stop_triggered {
                        stop_triggered = true;
                        remaining_extra = Some(args.extra_frames);
                    }
                    eprintln!("Reference match at frame {}", frame_count + 1);
                }
            }
        }

        if new_screen {
            frame_count += 1;
            if let Some(ref mut remaining) = remaining_extra {
                *remaining = remaining.saturating_sub(1);
            }
        }
    }

    // Cycle-budget mode: emit the full framebuffer at the budget as the trace's
    // final frame. The per-dot pix only holds the partial in-progress frame, so
    // we snapshot the whole screen (which still shows the persistent result) as
    // one frame — this is the screen the gambatte hex/blank check reads.
    if args.until_tcycle.is_some() {
        tracer.mark_frame().unwrap();
        match M::raw_frame(&gb) {
            RawFrame::Shade2 { pixels, .. } => {
                for shade in pixels {
                    tracer.push_pixel(shade);
                }
            }
            RawFrame::Rgb555 { pixels, .. } => {
                for word in pixels {
                    tracer.push_pixel_rgb555(word);
                }
            }
            _ => unreachable!("GB consoles emit Shade2 or Rgb555 frames"),
        }
        tracer.capture(&gb).unwrap();
    }

    if args.report_audio {
        let samples = gb.drain_audio_samples();
        let has_audio = last_frame_has_audio(&samples, frame_count.max(1));
        eprintln!("AUDIO={}", if has_audio { 1 } else { 0 });
    }

    tracer.finish().unwrap_or_else(|e| {
        eprintln!("Error finalizing trace: {e}");
        process::exit(1);
    });

    eprintln!("Trace written: {frame_count} frames");
}
