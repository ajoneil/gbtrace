//! The NMOS 6502 register/flag vocabulary shared by every family built on the
//! core (the NES's 2A03, the VCS's 6507). Instruction decode is not authored
//! here — the render path decodes through `missingno_core`'s shared
//! `InstructionSet` (see [`crate::disasm`]).

use crate::profile::{FieldDef, FieldType, Layer, SubsystemDef};
use crate::system::{FlagDef, field};

/// The 6502 register file, shared by every family carrying this core.
pub static CPU: SubsystemDef = SubsystemDef {
    name: "cpu",
    layers: &[
        (Layer::Registers, &[
            field!("pc", u16),
            field!("a", u8),
            field!("x", u8),
            field!("y", u8),
            field!("s", u8),
            field!("p", u8, dict),
        ]),
        (Layer::Internal, &[
            field!("rdy", bool),
        ]),
        (Layer::Timing, &[
            // u16: one instruction can stall the CPU far past a u8 delta
            // (NES OAM DMA freezes it 513+ cycles; VCS WSYNC parks it
            // for the rest of the scanline).
            field!("cycles", u16),
        ]),
    ],
};

/// 6502 status flags in P. B (bit 4) only exists in pushed copies of P,
/// so it is not part of the vocabulary.
pub static FLAGS: &[FlagDef] = &[
    FlagDef { names: &["n", "negative"], field: "p", bit: 7 },
    FlagDef { names: &["v", "overflow"], field: "p", bit: 6 },
    FlagDef { names: &["d", "decimal"], field: "p", bit: 3 },
    FlagDef { names: &["i", "interrupt"], field: "p", bit: 2 },
    FlagDef { names: &["z", "zero"], field: "p", bit: 1 },
    FlagDef { names: &["c", "carry"], field: "p", bit: 0 },
];
