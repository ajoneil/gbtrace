//! The Texas Instruments TMS9918A video display processor — "ti-vdp" in
//! missingno terms — shared by the SG-1000, ColecoVision, and MSX. The
//! SMS VDP descends from it but is its own chip (11 registers, CRAM,
//! counter ports) and gets its own module when SMS lands.
//!
//! The catalogue covers the chip's architectural state: the eight
//! write-only registers and readable status register, the internal
//! address/latch/read-ahead machinery, and the beam position (which the
//! TMS9918 exposes to software not at all — no counter ports, unlike its
//! SMS descendant — but every emulator models, and VDP tests live by).

use crate::profile::{FieldDef, FieldType, Layer, SubsystemDef};
use crate::system::field;

pub static VDP: SubsystemDef = SubsystemDef {
    name: "vdp",
    layers: &[
        (Layer::Registers, &[
            field!("reg0", u8, dict),
            field!("reg1", u8, dict),
            field!("reg2", u8, dict),
            field!("reg3", u8, dict),
            field!("reg4", u8, dict),
            field!("reg5", u8, dict),
            field!("reg6", u8, dict),
            field!("reg7", u8, dict),
            field!("status", u8, dict),
            // Beam position: NTSC is 262 lines of 342 dots.
            field!("line", u16),
            field!("dot", u16),
        ]),
        (Layer::Internal, &[
            // VRAM address register (14-bit).
            field!("addr", u16),
            // Control-port write phase: set after the first byte of an
            // address/register write, cleared by the second.
            field!("latch", bool),
            // Data-port read-ahead buffer.
            field!("buffer", u8),
        ]),
    ],
};
