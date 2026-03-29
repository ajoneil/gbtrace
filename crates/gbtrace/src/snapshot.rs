//! Snapshot payload definitions for typed snapshot records.
//!
//! Each snapshot type has a corresponding struct that can be serialized
//! to/from a byte payload. Payloads are compressed with zstd before
//! being written to the trace file.

/// CPU state beyond what's captured in per-cycle trace rows.
#[derive(Debug, Clone, Default)]
pub struct CpuSnapshot {
    /// 0=Running, 1=Halting, 2=Halted
    pub halt_state: u8,
    /// 0=None, 1=Pending (EI executed, IME set after next instruction), 2=Fired
    pub ei_delay: u8,
    /// HALT bug active (IME=0 HALT with pending interrupt, next PC increment skipped)
    pub halt_bug: bool,
}

impl CpuSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        vec![self.halt_state, self.ei_delay, self.halt_bug as u8]
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 3 { return None; }
        Some(Self {
            halt_state: data[0],
            ei_delay: data[1],
            halt_bug: data[2] != 0,
        })
    }
}

/// PPU timing state for mid-frame snapshots.
#[derive(Debug, Clone, Default)]
pub struct PpuTimingSnapshot {
    /// Position within scanline (0-113 in M-cycles).
    pub dot_position: u8,
    /// Previous STAT interrupt line state for edge detection.
    pub stat_line_was_high: bool,
    /// Internal window Y counter (0-143).
    pub window_line_counter: u8,
}

impl PpuTimingSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        vec![self.dot_position, self.stat_line_was_high as u8, self.window_line_counter]
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 3 { return None; }
        Some(Self {
            dot_position: data[0],
            stat_line_was_high: data[1] != 0,
            window_line_counter: data[2],
        })
    }
}

/// APU internal state not derivable from register reads.
#[derive(Debug, Clone, Default)]
pub struct ApuSnapshot {
    /// Frame sequencer step (0-7).
    pub frame_sequencer_step: u8,
    /// Previous DIV bit that clocks the frame sequencer.
    pub prev_div_apu_bit: bool,
    // Channel 1
    pub ch1_period: u16,
    pub ch1_envelope_timer: u8,
    pub ch1_sweep_timer: u8,
    pub ch1_sweep_enabled: bool,
    pub ch1_sweep_negate_used: bool,
    pub ch1_length_enabled: bool,
    // Channel 2
    pub ch2_period: u16,
    pub ch2_envelope_timer: u8,
    pub ch2_length_enabled: bool,
    // Channel 3
    pub ch3_period: u16,
    pub ch3_length_enabled: bool,
    // Channel 4
    pub ch4_envelope_timer: u8,
    pub ch4_length_enabled: bool,
}

impl ApuSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(20);
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
        if data.len() < 19 { return None; }
        let mut pos = 0;
        let read_u8 = |p: &mut usize| -> u8 { let v = data[*p]; *p += 1; v };
        let read_bool = |p: &mut usize| -> bool { let v = data[*p] != 0; *p += 1; v };
        let read_u16 = |p: &mut usize| -> u16 {
            let v = u16::from_le_bytes([data[*p], data[*p + 1]]);
            *p += 2;
            v
        };
        Some(Self {
            frame_sequencer_step: read_u8(&mut pos),
            prev_div_apu_bit: read_bool(&mut pos),
            ch1_period: read_u16(&mut pos),
            ch1_envelope_timer: read_u8(&mut pos),
            ch1_sweep_timer: read_u8(&mut pos),
            ch1_sweep_enabled: read_bool(&mut pos),
            ch1_sweep_negate_used: read_bool(&mut pos),
            ch1_length_enabled: read_bool(&mut pos),
            ch2_period: read_u16(&mut pos),
            ch2_envelope_timer: read_u8(&mut pos),
            ch2_length_enabled: read_bool(&mut pos),
            ch3_period: read_u16(&mut pos),
            ch3_length_enabled: read_bool(&mut pos),
            ch4_envelope_timer: read_u8(&mut pos),
            ch4_length_enabled: read_bool(&mut pos),
        })
    }
}

/// Timer internals.
#[derive(Debug, Clone, Default)]
pub struct TimerSnapshot {
    /// Full 16-bit internal counter (DIV exposes top 8 bits).
    pub internal_counter: u16,
    /// TIMA overflowed; TMA reload happens next M-cycle.
    pub overflow_pending: bool,
    /// TIMA is being reloaded from TMA this M-cycle.
    pub reloading: bool,
}

impl TimerSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4);
        out.extend_from_slice(&self.internal_counter.to_le_bytes());
        out.push(self.overflow_pending as u8);
        out.push(self.reloading as u8);
        out
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 4 { return None; }
        Some(Self {
            internal_counter: u16::from_le_bytes([data[0], data[1]]),
            overflow_pending: data[2] != 0,
            reloading: data[3] != 0,
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
    pub bits_remaining: u8,
    pub shift_clock: bool,
}

impl SerialSnapshot {
    pub fn to_bytes(&self) -> Vec<u8> {
        vec![self.bits_remaining, self.shift_clock as u8]
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 2 { return None; }
        Some(Self {
            bits_remaining: data[0],
            shift_clock: data[1] != 0,
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
