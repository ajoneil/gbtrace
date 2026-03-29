use std::fs;
use std::path::PathBuf;
use std::process;

use clap::Parser;
use missingno_gb::{GameBoy, cartridge::Cartridge};
use missingno_gb::cpu::mcycle::DotAction;
use missingno_gb::trace::{Tracer, Trigger, Profile, BootRom};

#[derive(Parser)]
#[command(name = "gbtrace-missingno")]
struct Args {
    #[arg(long)]
    rom: PathBuf,

    #[arg(long)]
    profile: PathBuf,

    #[arg(long)]
    output: PathBuf,

    #[arg(long, default_value_t = 3000)]
    frames: u32,

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

    /// Stop when memory ADDR equals VAL (hex, e.g. FF82=01). Can be repeated.
    #[arg(long = "stop-when", value_parser = parse_stop_when)]
    stop_when: Vec<(u16, u8)>,
}

fn parse_hex_u8(s: &str) -> Result<u8, String> {
    u8::from_str_radix(s, 16).map_err(|e| format!("invalid hex byte: {e}"))
}

fn parse_stop_when(s: &str) -> Result<(u16, u8), String> {
    let (addr_s, val_s) = s.split_once('=')
        .ok_or_else(|| "expected ADDR=VAL (e.g. FF82=01)".to_string())?;
    let addr = u16::from_str_radix(addr_s, 16)
        .map_err(|e| format!("invalid address: {e}"))?;
    let val = u8::from_str_radix(val_s, 16)
        .map_err(|e| format!("invalid value: {e}"))?;
    Ok((addr, val))
}

fn load_reference(path: &PathBuf) -> Vec<u8> {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read reference {}: {e}", path.display()));
    content.bytes().filter_map(|b| {
        if b >= b'0' && b <= b'3' { Some(b - b'0') } else { None }
    }).collect()
}

fn framebuffer_to_pix(gb: &GameBoy) -> Vec<u8> {
    let fb = gb.screen().front();
    let mut pix = Vec::with_capacity(160 * 144);
    for y in 0..144 {
        for x in 0..160 {
            pix.push(fb.pixels[y][x].0);
        }
    }
    pix
}

fn main() {
    let args = Args::parse();

    let rom_data = fs::read(&args.rom)
        .unwrap_or_else(|e| {
            eprintln!("Error: failed to read ROM {}: {e}", args.rom.display());
            process::exit(1);
        });

    let cartridge = Cartridge::new(rom_data, None);
    let mut gb = GameBoy::new(cartridge, None); // skip boot ROM

    let profile = Profile::load(&args.profile)
        .unwrap_or_else(|e| {
            eprintln!("Error: failed to load profile {}: {e}", args.profile.display());
            process::exit(1);
        });

    let mut tracer = Tracer::create(
        &args.output,
        &profile,
        &gb,
        BootRom::Skip,
    ).unwrap_or_else(|e| {
        eprintln!("Error: failed to create tracer: {e}");
        process::exit(1);
    });

    // Mark entry 0 as a frame boundary so the setup period is included.
    tracer.mark_frame().unwrap();

    let reference_pix = args.reference.as_ref().map(load_reference);
    let is_tcycle = tracer.trigger() == Trigger::Tcycle;

    let mut frame_count: u32 = 0;
    let mut stop_triggered = false;
    let mut remaining_extra: Option<u32> = None;
    let mut serial_match_count: u32 = 0;

    // Detect serial writes by watching SC bit 7 (transfer start)
    let mut prev_sc_high = (gb.peek(0xFF02) & 0x80) != 0;

    loop {
        // Check frame limit
        if frame_count >= args.frames {
            eprintln!("Frame limit reached ({} frames)", args.frames);
            break;
        }

        // Extra frames countdown
        if let Some(ref mut remaining) = remaining_extra {
            if *remaining == 0 { break; }
        }

        let new_screen = if is_tcycle {
            step_tcycle(&mut gb, &mut tracer)
        } else {
            step_instruction(&mut gb, &mut tracer)
        };

        // Check stop conditions
        if !stop_triggered {
            // Opcode check
            if let Some(opcode) = args.stop_opcode {
                let pc = gb.cpu().program_counter;
                if gb.peek(pc) == opcode {
                    eprintln!("Stop condition met: opcode 0x{:02X} at PC=0x{:04X}", opcode, pc);
                    stop_triggered = true;
                    remaining_extra = Some(args.extra_frames);
                }
            }

            // Memory watch check
            for &(addr, val) in &args.stop_when {
                if gb.peek(addr) == val {
                    eprintln!("Stop condition met: [0x{:04X}] == 0x{:02X}", addr, val);
                    stop_triggered = true;
                    remaining_extra = Some(args.extra_frames);
                    break;
                }
            }

            // Serial check: detect rising edge of SC bit 7
            if let Some(serial_byte) = args.stop_on_serial {
                let sc_high = (gb.peek(0xFF02) & 0x80) != 0;
                if sc_high && !prev_sc_high {
                    let sb = gb.peek(0xFF01);
                    if sb == serial_byte {
                        serial_match_count += 1;
                        if serial_match_count >= args.stop_serial_count {
                            eprintln!("Stop condition met: serial byte 0x{:02X} (count {})",
                                serial_byte, serial_match_count);
                            stop_triggered = true;
                            remaining_extra = Some(args.extra_frames);
                        }
                    }
                }
                prev_sc_high = sc_high;
            }

        }

        // Reference screenshot check runs on every frame boundary,
        // even after other stop conditions fire (the screen may not
        // have updated yet when serial/opcode triggers).
        if new_screen {
            if let Some(ref reference) = reference_pix {
                let current = framebuffer_to_pix(&gb);
                if current == *reference {
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

    tracer.finish().unwrap_or_else(|e| {
        eprintln!("Error finalizing trace: {e}");
        process::exit(1);
    });

    eprintln!("Trace written: {} frames", frame_count);
}

/// Step one instruction via T-cycle phases, capturing at each dot.
fn step_tcycle(gb: &mut GameBoy, tracer: &mut Tracer) -> bool {
    let mut new_screen = false;

    gb.cpu_mut().take_instruction_boundary();

    loop {
        let rise = gb.step_phase();
        new_screen |= rise.new_screen;
        if let Some(pixel) = rise.pixel {
            tracer.push_pixel(pixel.shade);
        }

        let fall = gb.step_phase();
        new_screen |= fall.new_screen;
        if let Some(pixel) = fall.pixel {
            tracer.push_pixel(pixel.shade);
        }

        // Detect bus writes from the dot action (writes happen on fall)
        if let DotAction::Write { address, value } = gb.last_dot_action() {
            if (0x8000..=0x9FFF).contains(address) {
                tracer.push_vram_write(*address, *value);
            }
            if (0xFF10..=0xFF3F).contains(address) {
                tracer.push_apu_write(*address, *value);
            }
        }

        if rise.new_screen || fall.new_screen {
            tracer.mark_frame().unwrap();
        }

        tracer.capture(gb).unwrap();
        tracer.advance_dot();

        if gb.cpu().at_instruction_boundary() {
            break;
        }
    }

    new_screen
}

/// Step one instruction, capture once.
fn step_instruction(gb: &mut GameBoy, tracer: &mut Tracer) -> bool {
    tracer.capture(gb).unwrap();
    let result = gb.step();
    tracer.advance(result.dots);

    if result.new_screen {
        tracer.mark_frame().unwrap();
    }

    result.new_screen
}
