//! The Sega SG-1000: Z80 + TI VDP (TMS9918A) + SN76489 PSG. The first
//! Z80 system, and the natural host for TI VDP test ROMs — the cartridge
//! maps at 0x0000 with no BIOS in the way, so reset executes ROM
//! directly. The PSG has no catalogue subsystem yet; adapters can carry
//! PSG state as extension fields until tests need it.

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

pub static SG1000: System = System {
    id: "sg1000",
    isa: &super::Z80,
    subsystems: SUBSYSTEMS,
    exact_phrases: EXACT_PHRASES,
    numbered_phrases: &[],
    // The Z80 resets to 0x0000 with the cartridge mapped there, but the
    // entry instruction's length is ROM-dependent, so there is no fixed
    // second address; diff falls back to first-common-address alignment.
    entry_addrs: None,
};
