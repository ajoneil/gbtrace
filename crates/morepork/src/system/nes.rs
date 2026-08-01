//! The NES family. NTSC/PAL and clones are models within this family, not
//! separate families.
//!
//! The catalogue starts with the state missingno-nes exposes publicly
//! (CPU registers, PPU control/mask and beam position) and grows with its
//! tracer; adapters can carry anything else as extension fields.

use super::{ExactPhrase, System, field};
use crate::hardware::mos6502;
use crate::profile::{FieldDef, FieldType, Layer, SubsystemDef};
use crate::query::Condition;

pub static PPU: SubsystemDef = SubsystemDef {
    name: "ppu",
    layers: &[(
        Layer::Registers,
        &[
            field!("control", u8, dict),
            field!("mask", u8, dict),
            field!("line", u16),
            field!("dot", u16),
        ],
    )],
};

pub static SUBSYSTEMS: &[&SubsystemDef] = &[&mos6502::CPU, &PPU];

/// NTSC vblank begins on scanline 241 (0xF1).
static EXACT_PHRASES: &[ExactPhrase] = &[("vblank starts", || Condition::FieldChangesTo {
    field: "line".into(),
    value: "0xf1".into(),
})];

pub static NES: System = System {
    id: "nes",
    isa: &super::MOS6502,
    subsystems: SUBSYSTEMS,
    exact_phrases: EXACT_PHRASES,
    numbered_phrases: &[],
    // The reset vector is ROM-dependent, so there is no fixed entry
    // address; diff falls back to first-common-address alignment.
    entry_addrs: None,
};
