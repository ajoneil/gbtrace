//! System registry.
//!
//! Self-describing trace headers carry everything a reader needs for
//! info/query/diff/table work; what remains system-specific is vocabulary
//! and behaviour that cannot be data in the file: the default field
//! catalogue behind profiles, flag names, semantic query phrases, the
//! instruction decoder, and diff-alignment hints. Each machine contributes
//! one [`System`] here; systems that share silicon share an [`Isa`] (the
//! Game Boy's DMG/CGB share `sm83`; the NES and VCS share `6502`), with
//! the shared chip's vocabulary hosted in [`crate::hardware`]. See
//! `docs/multi-system.md`.

use crate::hardware::{mos6502, z80};
use crate::profile::SubsystemDef;
use crate::query::Condition;

pub mod gb;
pub mod nes;
pub mod sg1000;
pub mod vcs;

/// Field-catalogue construction shorthand shared by the family catalogues.
macro_rules! field {
    ($name:expr, u8) => {
        FieldDef {
            name: $name,
            field_type: FieldType::UInt8,
            nullable: false,
            dictionary: false,
        }
    };
    ($name:expr, u8, dict) => {
        FieldDef {
            name: $name,
            field_type: FieldType::UInt8,
            nullable: false,
            dictionary: true,
        }
    };
    ($name:expr, u16) => {
        FieldDef {
            name: $name,
            field_type: FieldType::UInt16,
            nullable: false,
            dictionary: false,
        }
    };
    ($name:expr, u16, nullable) => {
        FieldDef {
            name: $name,
            field_type: FieldType::UInt16,
            nullable: true,
            dictionary: false,
        }
    };
    ($name:expr, u8, nullable) => {
        FieldDef {
            name: $name,
            field_type: FieldType::UInt8,
            nullable: true,
            dictionary: false,
        }
    };
    ($name:expr, bool) => {
        FieldDef {
            name: $name,
            field_type: FieldType::Bool,
            nullable: false,
            dictionary: true,
        }
    };
    ($name:expr, str, nullable) => {
        FieldDef {
            name: $name,
            field_type: FieldType::Str,
            nullable: true,
            dictionary: false,
        }
    };
}
pub(crate) use field;

/// A named CPU flag: which field holds it and at which bit. The first name
/// is canonical (single letter); the rest are accepted aliases.
pub struct FlagDef {
    pub names: &'static [&'static str],
    pub field: &'static str,
    pub bit: u8,
}

/// An instruction-set architecture: the decode/flag vocabulary shared by
/// every system built on it. The concrete disassembler lives on each
/// [`System`] (it closes over a system-specific ROM-offset mapping); the
/// ISA carries the flag vocabulary that `flag …` queries and the viewer use.
pub struct Isa {
    /// Identifier stored in the trace header (`"sm83"`, `"6502"`).
    pub id: &'static str,

    /// Flag vocabulary for `flag …` queries and viewer flag rendering,
    /// in display order (high bit first).
    pub flags: &'static [FlagDef],
}

/// A semantic phrase that is exactly one fixed string (`"lcd on"`),
/// desugaring to a generic [`Condition`].
pub type ExactPhrase = (&'static str, fn() -> Condition);

/// A semantic phrase of the form `<prefix><number>` (`"interrupt 2"`),
/// with an inclusive maximum for the number.
pub type NumberedPhrase = (&'static str, u8, fn(u8) -> Condition);

/// A system: the machine-specific vocabulary and behaviour behind
/// profiles, queries, disassembly, and diff alignment. Its [`Isa`] carries
/// the decode/flag vocabulary shared with sibling systems.
pub struct System {
    /// Identifier stored in the trace header and profile (`"dmg"`, `"cgb"`,
    /// `"nes"`, `"vcs"`).
    pub id: &'static str,

    /// The instruction-set architecture this system runs. Provides the flag
    /// vocabulary shared across systems on the same ISA.
    pub isa: &'static Isa,

    /// Default field catalogue: validates profiles and types legacy traces
    /// whose headers predate `field_defs`.
    pub subsystems: &'static [&'static SubsystemDef],

    /// Semantic query phrases that are exactly one fixed string.
    pub exact_phrases: &'static [ExactPhrase],

    /// Semantic query phrases carrying a number.
    pub numbered_phrases: &'static [NumberedPhrase],

    /// Diff-alignment hint: the address every trace of this system reaches
    /// at program entry, and the address of the entry's second instruction
    /// (proves execution continued past it). GB: cartridge entry
    /// 0x0100/0x0101.
    pub entry_addrs: Option<(u16, u16)>,
}

impl System {
    /// Look up a field definition by name across this system's subsystems.
    pub fn lookup_field(&self, name: &str) -> Option<&'static crate::profile::FieldDef> {
        self.subsystems
            .iter()
            .flat_map(|s| s.all_fields())
            .find(|f| f.name == name)
    }

    /// Which of this family's subsystems and layers a field belongs to.
    pub fn field_group(&self, name: &str) -> Option<(&'static str, &'static str)> {
        use crate::profile::Layer;
        for subsystem in self.subsystems {
            for (layer, fields) in subsystem.layers {
                if fields.iter().any(|f| f.name == name) {
                    let layer_name = match layer {
                        Layer::Registers => "registers",
                        Layer::Internal => "internal",
                        Layer::Writes => "writes",
                        Layer::Output => "output",
                        Layer::Timing => "timing",
                    };
                    return Some((subsystem.name, layer_name));
                }
            }
        }
        None
    }
}

/// The Sharp SM83 (Game Boy), the NMOS 6502 (the NES's 2A03, the VCS's
/// 6507), and the Zilog Z80 (the SG-1000). The flag vocabulary lives with
/// each ISA's home module.
pub static SM83: Isa = Isa {
    id: "sm83",
    flags: gb::FLAGS,
};
pub static MOS6502: Isa = Isa {
    id: "6502",
    flags: mos6502::FLAGS,
};
pub static Z80: Isa = Isa {
    id: "z80",
    flags: z80::FLAGS,
};

/// Every registered ISA.
pub static ISAS: &[&Isa] = &[&SM83, &MOS6502, &Z80];

/// Look up an ISA by id.
pub fn isa(id: &str) -> Option<&'static Isa> {
    ISAS.iter().copied().find(|i| i.id == id)
}

/// Every registered system. `dmg` first — it is also the fallback for
/// traces whose headers predate the `system` field.
pub static SYSTEMS: &[&System] = &[&gb::DMG, &gb::CGB, &nes::NES, &vcs::VCS, &sg1000::SG1000];

/// Look up a system by id.
pub fn system(id: &str) -> Option<&'static System> {
    SYSTEMS.iter().copied().find(|s| s.id == id)
}
