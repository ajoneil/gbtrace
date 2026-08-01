//! The ColecoVision: Z80 + TI VDP (TMS9918A) + SN76489 PSG — the same
//! chip pair as the SG-1000 line, wrapped in a different machine (BIOS at
//! 0x0000, cartridge at 0x8000, test RAM at 0x7000, VDP interrupt on NMI).
//! A host for the TI VDP test suite's `.col` builds. The PSG has no
//! catalogue subsystem yet; adapters can carry PSG state as extension
//! fields until tests need it.

use super::{ExactPhrase, System};
use crate::hardware::{ti_vdp, z80};
use crate::profile::SubsystemDef;
use crate::query::Condition;

pub static SUBSYSTEMS: &[&SubsystemDef] = &[&z80::CPU, &ti_vdp::VDP];

/// Active display is lines 0-191; the frame interrupt rises entering
/// line 192 (0xC0).
static EXACT_PHRASES: &[ExactPhrase] = &[("vblank starts", || Condition::FieldChangesTo {
    field: "line".into(),
    value: "0xc0".into(),
})];

pub static COLECO: System = System {
    id: "coleco",
    isa: &super::Z80,
    subsystems: SUBSYSTEMS,
    exact_phrases: EXACT_PHRASES,
    numbered_phrases: &[],
    // The BIOS owns reset; cartridge entry depends on the BIOS's header
    // dispatch, so diff falls back to first-common-address alignment.
    entry_addrs: None,
};
