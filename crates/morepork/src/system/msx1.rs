//! The MSX1: Z80 + TI VDP (TMS9918A/9929A) + AY-3-8910 PSG — the same
//! CPU/VDP pair as the SG-1000 line behind the MSX's slot machinery (BIOS
//! in slot 0, cartridge at 0x4000 with the "AB" header, test RAM at
//! 0xE000, VDP ports 0x98/0x99). A host for the TI VDP test suite's
//! `.mx1` builds. The PSG has no catalogue subsystem yet; adapters can
//! carry PSG state as extension fields until tests need it.

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

pub static MSX1: System = System {
    id: "msx1",
    isa: &super::Z80,
    subsystems: SUBSYSTEMS,
    exact_phrases: EXACT_PHRASES,
    numbered_phrases: &[],
    // The BIOS owns reset and calls the cartridge's INIT vector, so there
    // is no fixed entry; diff falls back to first-common-address alignment.
    entry_addrs: None,
};
