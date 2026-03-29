//! Snapshot payload definitions for typed snapshot records.
//!
//! Each snapshot type has a corresponding struct that can be serialized
//! to/from a byte payload. Payloads are compressed with zstd before
//! being written to the trace file.
//!
//! A complete set of snapshots (all types) is sufficient to restore
//! a Game Boy save state — no trace rows needed.

/// CPU state: registers + interrupt registers + internal state.
#[derive(Debug, Clone, Default)]
pub struct CpuSnapshot {
    // Registers
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    pub ime: bool,
    // Interrupt registers
    pub if_: u8,
    pub ie: u8,
    // Internal state
    /// 0=Running, 1=Halting, 2=Halted
    pub halt_state: u8,
    /// 0=None, 1=Pending (EI executed, IME set after next instruction), 2=Fired
    pub ei_delay: u8,
    /// HALT bug active (IME=0 HALT with pending interrupt, next PC increment skipped)
    pub halt_bug: bool,
}

impl CpuSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(18);
        out.push(self.a);
        out.push(self.f);
        out.push(self.b);
        out.push(self.c);
        out.push(self.d);
        out.push(self.e);
        out.push(self.h);
        out.push(self.l);
        out.extend_from_slice(&self.sp.to_le_bytes());
        out.extend_from_slice(&self.pc.to_le_bytes());
        out.push(self.ime as u8);
        out.push(self.if_);
        out.push(self.ie);
        out.push(self.halt_state);
        out.push(self.ei_delay);
        out.push(self.halt_bug as u8);
        out
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 18 { return None; }
        Some(Self {
            a: data[0], f: data[1], b: data[2], c: data[3],
            d: data[4], e: data[5], h: data[6], l: data[7],
            sp: u16::from_le_bytes([data[8], data[9]]),
            pc: u16::from_le_bytes([data[10], data[11]]),
            ime: data[12] != 0,
            if_: data[13],
            ie: data[14],
            halt_state: data[15],
            ei_delay: data[16],
            halt_bug: data[17] != 0,
        })
    }
}

/// PPU state: registers + timing internals.
#[derive(Debug, Clone, Default)]
pub struct PpuSnapshot {
    // Registers
    pub lcdc: u8,
    pub stat: u8,
    pub ly: u8,
    pub lyc: u8,
    pub scy: u8,
    pub scx: u8,
    pub wy: u8,
    pub wx: u8,
    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,
    pub dma: u8,
    // Timing internals
    /// Position within scanline (0-113 in M-cycles).
    pub dot_position: u8,
    /// Previous STAT interrupt line state for edge detection.
    pub stat_line_was_high: bool,
    /// Internal window Y counter (0-143).
    pub window_line_counter: u8,
}

impl PpuSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        vec![
            self.lcdc, self.stat, self.ly, self.lyc,
            self.scy, self.scx, self.wy, self.wx,
            self.bgp, self.obp0, self.obp1, self.dma,
            self.dot_position, self.stat_line_was_high as u8, self.window_line_counter,
        ]
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 15 { return None; }
        Some(Self {
            lcdc: data[0], stat: data[1], ly: data[2], lyc: data[3],
            scy: data[4], scx: data[5], wy: data[6], wx: data[7],
            bgp: data[8], obp0: data[9], obp1: data[10], dma: data[11],
            dot_position: data[12],
            stat_line_was_high: data[13] != 0,
            window_line_counter: data[14],
        })
    }
}

/// APU state: register values (including write-only) + internals.
#[derive(Debug, Clone, Default)]
pub struct ApuSnapshot {
    // Control registers
    pub master_vol: u8,   // NR50
    pub sound_pan: u8,    // NR51
    pub sound_on: u8,     // NR52

    // Channel 1 registers
    pub ch1_sweep: u8,    // NR10
    pub ch1_duty_len: u8, // NR11
    pub ch1_vol_env: u8,  // NR12
    pub ch1_freq_lo: u8,  // NR13 (write-only)
    pub ch1_freq_hi: u8,  // NR14 (partially write-only)

    // Channel 2 registers
    pub ch2_duty_len: u8, // NR21
    pub ch2_vol_env: u8,  // NR22
    pub ch2_freq_lo: u8,  // NR23 (write-only)
    pub ch2_freq_hi: u8,  // NR24 (partially write-only)

    // Channel 3 registers
    pub ch3_dac: u8,      // NR30
    pub ch3_len: u8,      // NR31 (write-only)
    pub ch3_vol: u8,      // NR32
    pub ch3_freq_lo: u8,  // NR33 (write-only)
    pub ch3_freq_hi: u8,  // NR34 (partially write-only)

    // Channel 4 registers
    pub ch4_len: u8,      // NR41 (write-only)
    pub ch4_vol_env: u8,  // NR42
    pub ch4_freq: u8,     // NR43
    pub ch4_control: u8,  // NR44 (partially write-only)

    // Internal state
    /// Frame sequencer step (0-7).
    pub frame_sequencer_step: u8,
    /// Previous DIV bit that clocks the frame sequencer.
    pub prev_div_apu_bit: bool,

    pub ch1_period: u16,
    pub ch1_envelope_timer: u8,
    pub ch1_sweep_timer: u8,
    pub ch1_sweep_enabled: bool,
    pub ch1_sweep_negate_used: bool,
    pub ch1_length_enabled: bool,

    pub ch2_period: u16,
    pub ch2_envelope_timer: u8,
    pub ch2_length_enabled: bool,

    pub ch3_period: u16,
    pub ch3_length_enabled: bool,

    pub ch4_envelope_timer: u8,
    pub ch4_length_enabled: bool,
}

impl ApuSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(40);
        // Control
        out.push(self.master_vol);
        out.push(self.sound_pan);
        out.push(self.sound_on);
        // Ch1 registers
        out.push(self.ch1_sweep);
        out.push(self.ch1_duty_len);
        out.push(self.ch1_vol_env);
        out.push(self.ch1_freq_lo);
        out.push(self.ch1_freq_hi);
        // Ch2 registers
        out.push(self.ch2_duty_len);
        out.push(self.ch2_vol_env);
        out.push(self.ch2_freq_lo);
        out.push(self.ch2_freq_hi);
        // Ch3 registers
        out.push(self.ch3_dac);
        out.push(self.ch3_len);
        out.push(self.ch3_vol);
        out.push(self.ch3_freq_lo);
        out.push(self.ch3_freq_hi);
        // Ch4 registers
        out.push(self.ch4_len);
        out.push(self.ch4_vol_env);
        out.push(self.ch4_freq);
        out.push(self.ch4_control);
        // Internals
        out.push(self.frame_sequencer_step);
        out.push(self.prev_div_apu_bit as u8);
        out.extend_from_slice(&self.ch1_period.to_le_bytes());
        out.push(self.ch1_envelope_timer);
        out.push(self.ch1_sweep_timer);
        out.push(self.ch1_sweep_enabled as u8);
        out.push(self.ch1_sweep_negate_used as u8);
        out.push(self.ch1_length_enabled as u8);
        out.extend_from_slice(&self.ch2_period.to_le_bytes());
        out.push(self.ch2_envelope_timer);
        out.push(self.ch2_length_enabled as u8);
        out.extend_from_slice(&self.ch3_period.to_le_bytes());
        out.push(self.ch3_length_enabled as u8);
        out.push(self.ch4_envelope_timer);
        out.push(self.ch4_length_enabled as u8);
        out
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 38 { return None; }
        let mut p = 0;
        let r = |p: &mut usize| -> u8 { let v = data[*p]; *p += 1; v };
        let rb = |p: &mut usize| -> bool { let v = data[*p] != 0; *p += 1; v };
        let r16 = |p: &mut usize| -> u16 {
            let v = u16::from_le_bytes([data[*p], data[*p + 1]]); *p += 2; v
        };
        Some(Self {
            master_vol: r(&mut p), sound_pan: r(&mut p), sound_on: r(&mut p),
            ch1_sweep: r(&mut p), ch1_duty_len: r(&mut p), ch1_vol_env: r(&mut p),
            ch1_freq_lo: r(&mut p), ch1_freq_hi: r(&mut p),
            ch2_duty_len: r(&mut p), ch2_vol_env: r(&mut p),
            ch2_freq_lo: r(&mut p), ch2_freq_hi: r(&mut p),
            ch3_dac: r(&mut p), ch3_len: r(&mut p), ch3_vol: r(&mut p),
            ch3_freq_lo: r(&mut p), ch3_freq_hi: r(&mut p),
            ch4_len: r(&mut p), ch4_vol_env: r(&mut p),
            ch4_freq: r(&mut p), ch4_control: r(&mut p),
            frame_sequencer_step: r(&mut p), prev_div_apu_bit: rb(&mut p),
            ch1_period: r16(&mut p), ch1_envelope_timer: r(&mut p),
            ch1_sweep_timer: r(&mut p), ch1_sweep_enabled: rb(&mut p),
            ch1_sweep_negate_used: rb(&mut p), ch1_length_enabled: rb(&mut p),
            ch2_period: r16(&mut p), ch2_envelope_timer: r(&mut p),
            ch2_length_enabled: rb(&mut p),
            ch3_period: r16(&mut p), ch3_length_enabled: rb(&mut p),
            ch4_envelope_timer: r(&mut p), ch4_length_enabled: rb(&mut p),
        })
    }
}

/// Timer state: registers + internals.
#[derive(Debug, Clone, Default)]
pub struct TimerSnapshot {
    // Registers
    pub div: u8,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    // Internals
    /// Full 16-bit internal counter (DIV exposes top 8 bits).
    pub internal_counter: u16,
    /// TIMA overflowed; TMA reload happens next M-cycle.
    pub overflow_pending: bool,
    /// TIMA is being reloaded from TMA this M-cycle.
    pub reloading: bool,
}

impl TimerSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8);
        out.push(self.div);
        out.push(self.tima);
        out.push(self.tma);
        out.push(self.tac);
        out.extend_from_slice(&self.internal_counter.to_le_bytes());
        out.push(self.overflow_pending as u8);
        out.push(self.reloading as u8);
        out
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 { return None; }
        Some(Self {
            div: data[0], tima: data[1], tma: data[2], tac: data[3],
            internal_counter: u16::from_le_bytes([data[4], data[5]]),
            overflow_pending: data[6] != 0,
            reloading: data[7] != 0,
        })
    }
}

/// DMA transfer state.
#[derive(Debug, Clone, Default)]
pub struct DmaSnapshot {
    pub active: bool,
    pub source: u16,
    pub byte_index: u8,
    pub delay_remaining: u8,
}

impl DmaSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5);
        out.push(self.active as u8);
        out.extend_from_slice(&self.source.to_le_bytes());
        out.push(self.byte_index);
        out.push(self.delay_remaining);
        out
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 5 { return None; }
        Some(Self {
            active: data[0] != 0,
            source: u16::from_le_bytes([data[1], data[2]]),
            byte_index: data[3],
            delay_remaining: data[4],
        })
    }
}

/// Serial transfer state.
#[derive(Debug, Clone, Default)]
pub struct SerialSnapshot {
    // Registers
    pub sb: u8,
    pub sc: u8,
    // Internals
    pub bits_remaining: u8,
    pub shift_clock: bool,
}

impl SerialSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        vec![self.sb, self.sc, self.bits_remaining, self.shift_clock as u8]
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 4 { return None; }
        Some(Self {
            sb: data[0], sc: data[1],
            bits_remaining: data[2],
            shift_clock: data[3] != 0,
        })
    }
}

/// Cartridge mapper state.
#[derive(Debug, Clone, Default)]
pub struct MbcSnapshot {
    /// MBC type identifier.
    pub mbc_type: String,
    pub rom_bank: u16,
    pub ram_bank: u8,
    pub ram_enabled: bool,
    /// MBC-specific mode (e.g. MBC1 mode 0/1).
    pub mode: u8,
}

impl MbcSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let type_bytes = self.mbc_type.as_bytes();
        out.push(type_bytes.len() as u8);
        out.extend_from_slice(type_bytes);
        out.extend_from_slice(&self.rom_bank.to_le_bytes());
        out.push(self.ram_bank);
        out.push(self.ram_enabled as u8);
        out.push(self.mode);
        out
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.is_empty() { return None; }
        let type_len = data[0] as usize;
        if data.len() < 1 + type_len + 5 { return None; }
        let mbc_type = std::str::from_utf8(&data[1..1 + type_len]).ok()?.to_string();
        let pos = 1 + type_len;
        Some(Self {
            mbc_type,
            rom_bank: u16::from_le_bytes([data[pos], data[pos + 1]]),
            ram_bank: data[pos + 2],
            ram_enabled: data[pos + 3] != 0,
            mode: data[pos + 4],
        })
    }
}

/// Memory snapshot payload.
///
/// Format: [num_regions: u8] then for each region:
///   [start_addr: u16 LE] [length: u16 LE] [data: u8 * length]
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub start: u16,
    pub data: Vec<u8>,
}

pub fn parse_memory_snapshot(payload: &[u8]) -> Option<Vec<MemoryRegion>> {
    if payload.is_empty() { return None; }
    let num_regions = payload[0] as usize;
    let mut pos = 1;
    let mut regions = Vec::with_capacity(num_regions);
    for _ in 0..num_regions {
        if pos + 4 > payload.len() { return None; }
        let start = u16::from_le_bytes([payload[pos], payload[pos + 1]]);
        let len = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]) as usize;
        pos += 4;
        if pos + len > payload.len() { return None; }
        regions.push(MemoryRegion {
            start,
            data: payload[pos..pos + len].to_vec(),
        });
        pos += len;
    }
    Some(regions)
}

pub fn build_memory_payload(regions: &[MemoryRegion]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(regions.len() as u8);
    for region in regions {
        out.extend_from_slice(&region.start.to_le_bytes());
        out.extend_from_slice(&(region.data.len() as u16).to_le_bytes());
        out.extend_from_slice(&region.data);
    }
    out
}
