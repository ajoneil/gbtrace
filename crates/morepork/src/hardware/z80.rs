//! The Zilog Z80 register/flag vocabulary shared by every system carrying
//! the chip (the SG-1000 today; ColecoVision, MSX, and the SMS's
//! derivative core to follow). Register terminology mirrors missingno's
//! `crates/hardware/z80` `Cpu` struct — the shadow set is `a_`…`l_`, and
//! `wz` is the internal address latch. Instruction decode is not authored
//! here — the render path decodes through `missingno_core`'s shared
//! `InstructionSet` (see [`crate::disasm`]) once the z80 hardware crate
//! implements it; until then Z80 traces disassemble as hex.

use crate::profile::{FieldDef, FieldType, Layer, SubsystemDef};
use crate::system::{FlagDef, field};

/// The Z80 register file, shared by every system carrying this core.
pub static CPU: SubsystemDef = SubsystemDef {
    name: "cpu",
    layers: &[
        (Layer::Registers, &[
            field!("pc", u16),
            field!("op_addr", u16),
            field!("sp", u16),
            field!("a", u8),
            field!("f", u8, dict),
            field!("b", u8),
            field!("c", u8),
            field!("d", u8),
            field!("e", u8),
            field!("h", u8),
            field!("l", u8),
            field!("ix", u16),
            field!("iy", u16),
            field!("wz", u16),
            field!("a_", u8),
            field!("f_", u8, dict),
            field!("b_", u8),
            field!("c_", u8),
            field!("d_", u8),
            field!("e_", u8),
            field!("h_", u8),
            field!("l_", u8),
            field!("i", u8),
            field!("r", u8),
        ]),
        (Layer::Internal, &[
            field!("im", u8, dict),
            field!("iff1", bool),
            field!("iff2", bool),
            field!("halted", bool),
        ]),
        (Layer::Timing, &[
            // T-states retired by the entry. The longest instruction is 23
            // T-states (block ops iterate as repeated instructions), so u8.
            field!("cycles", u8),
        ]),
    ],
};

/// Z80 status flags in F, high bit first. X and Y are the undocumented
/// copy-of-result bits 3 and 5 — part of the vocabulary because exercising
/// them is exactly what Z80 test ROMs do.
pub static FLAGS: &[FlagDef] = &[
    FlagDef {
        names: &["s", "sign"],
        field: "f",
        bit: 7,
    },
    FlagDef {
        names: &["z", "zero"],
        field: "f",
        bit: 6,
    },
    FlagDef {
        names: &["y"],
        field: "f",
        bit: 5,
    },
    FlagDef {
        names: &["h", "half"],
        field: "f",
        bit: 4,
    },
    FlagDef {
        names: &["x"],
        field: "f",
        bit: 3,
    },
    FlagDef {
        names: &["p", "pv", "parity", "overflow"],
        field: "f",
        bit: 2,
    },
    FlagDef {
        names: &["n", "sub", "subtract"],
        field: "f",
        bit: 1,
    },
    FlagDef {
        names: &["c", "carry"],
        field: "f",
        bit: 0,
    },
];
