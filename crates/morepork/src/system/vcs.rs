//! The Atari 2600 (VCS) family. NTSC/PAL are models within this family.
//!
//! The VCS is the stress test of the per-frame-dimensions model: there is
//! no hardware frame, only the software's sync pattern, so frame height is
//! emergent and every `frame` snapshot carries its own dimensions
//! (`snapshot::IndexedFrame`). The catalogue starts with the state
//! missingno-vcs exposes publicly (the 6507's register file, the TIA beam
//! position, RIOT timer and ports); adapters can carry anything else as
//! extension fields.

use super::{System, field};
use crate::hardware::mos6502;
use crate::profile::{FieldDef, FieldType, Layer, SubsystemDef};

pub static TIA: SubsystemDef = SubsystemDef {
    name: "tia",
    layers: &[(
        Layer::Registers,
        &[
            // Scanline within the frame — emergent height, so u16.
            field!("line", u16),
            // Colour clock within the line (0-227).
            field!("clock", u8),
        ],
    )],
};

pub static RIOT: SubsystemDef = SubsystemDef {
    name: "riot",
    layers: &[(
        Layer::Registers,
        &[
            field!("timer", u8),
            field!("port_a", u8, dict),
            field!("port_b", u8, dict),
        ],
    )],
};

pub static SUBSYSTEMS: &[&SubsystemDef] = &[&mos6502::CPU, &TIA, &RIOT];

pub static VCS: System = System {
    id: "vcs",
    isa: &super::MOS6502,
    subsystems: SUBSYSTEMS,
    exact_phrases: &[],
    numbered_phrases: &[],
    // The reset vector is ROM-dependent; diff falls back to
    // first-common-address alignment.
    entry_addrs: None,
};
