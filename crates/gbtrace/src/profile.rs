use crate::error::{Error, Result};
use crate::header::Trigger;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Field metadata
// ---------------------------------------------------------------------------

/// Native type of a trace field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    UInt64,
    UInt16,
    UInt8,
    Bool,
    Str,
}

/// Complete metadata for a single trace field.
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: &'static str,
    pub field_type: FieldType,
    pub nullable: bool,
    pub dictionary: bool,
}

// ---------------------------------------------------------------------------
// Subsystem / layer definitions
// ---------------------------------------------------------------------------

/// A capture layer within a subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Registers,
    Internal,
    Writes,
    Output,
    Timing,
}

impl Layer {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "registers" => Some(Layer::Registers),
            "internal" => Some(Layer::Internal),
            "writes" => Some(Layer::Writes),
            "output" => Some(Layer::Output),
            "timing" => Some(Layer::Timing),
            _ => None,
        }
    }
}

/// A hardware subsystem definition with its available layers.
pub struct SubsystemDef {
    pub name: &'static str,
    pub layers: &'static [(Layer, &'static [FieldDef])],
}

impl SubsystemDef {
    /// Get all fields for the given layers.
    fn fields_for_layers(&self, layers: &[Layer]) -> Vec<&'static FieldDef> {
        self.layers
            .iter()
            .filter(|(l, _)| layers.contains(l))
            .flat_map(|(_, fields)| fields.iter())
            .collect()
    }

    /// Get all fields across all layers.
    fn all_fields(&self) -> Vec<&'static FieldDef> {
        self.layers
            .iter()
            .flat_map(|(_, fields)| fields.iter())
            .collect()
    }

    /// Get the available layer names for this subsystem.
    fn available_layers(&self) -> Vec<Layer> {
        self.layers.iter().map(|(l, _)| *l).collect()
    }
}

// ---------------------------------------------------------------------------
// Field definitions — Game Boy hardware
// ---------------------------------------------------------------------------

macro_rules! field {
    ($name:expr, u8) => {
        FieldDef { name: $name, field_type: FieldType::UInt8, nullable: false, dictionary: false }
    };
    ($name:expr, u8, dict) => {
        FieldDef { name: $name, field_type: FieldType::UInt8, nullable: false, dictionary: true }
    };
    ($name:expr, u16) => {
        FieldDef { name: $name, field_type: FieldType::UInt16, nullable: false, dictionary: false }
    };
    ($name:expr, u16, nullable) => {
        FieldDef { name: $name, field_type: FieldType::UInt16, nullable: true, dictionary: false }
    };
    ($name:expr, u8, nullable) => {
        FieldDef { name: $name, field_type: FieldType::UInt8, nullable: true, dictionary: false }
    };
    ($name:expr, bool) => {
        FieldDef { name: $name, field_type: FieldType::Bool, nullable: false, dictionary: true }
    };
    ($name:expr, str, nullable) => {
        FieldDef { name: $name, field_type: FieldType::Str, nullable: true, dictionary: false }
    };
}

pub static CPU: SubsystemDef = SubsystemDef {
    name: "cpu",
    layers: &[
        (Layer::Registers, &[
            field!("pc", u16),
            field!("sp", u16),
            field!("a", u8),
            field!("f", u8, dict),
            field!("b", u8),
            field!("c", u8),
            field!("d", u8),
            field!("e", u8),
            field!("h", u8),
            field!("l", u8),
            field!("ime", bool),
        ]),
        (Layer::Timing, &[
            field!("mcycles", u8),
            field!("tcycles", u8),
        ]),
    ],
};

pub static PPU: SubsystemDef = SubsystemDef {
    name: "ppu",
    layers: &[
        (Layer::Registers, &[
            field!("lcdc", u8, dict),
            field!("stat", u8, dict),
            field!("ly", u8),
            field!("lyc", u8),
            field!("scy", u8),
            field!("scx", u8),
            field!("wy", u8),
            field!("wx", u8),
            field!("bgp", u8, dict),
            field!("obp0", u8, dict),
            field!("obp1", u8, dict),
            field!("dma", u8),
        ]),
        (Layer::Internal, &[
            // sprite store (10 sprites × 3 fields)
            field!("oam0_x", u8), field!("oam0_id", u8), field!("oam0_attr", u8),
            field!("oam1_x", u8), field!("oam1_id", u8), field!("oam1_attr", u8),
            field!("oam2_x", u8), field!("oam2_id", u8), field!("oam2_attr", u8),
            field!("oam3_x", u8), field!("oam3_id", u8), field!("oam3_attr", u8),
            field!("oam4_x", u8), field!("oam4_id", u8), field!("oam4_attr", u8),
            field!("oam5_x", u8), field!("oam5_id", u8), field!("oam5_attr", u8),
            field!("oam6_x", u8), field!("oam6_id", u8), field!("oam6_attr", u8),
            field!("oam7_x", u8), field!("oam7_id", u8), field!("oam7_attr", u8),
            field!("oam8_x", u8), field!("oam8_id", u8), field!("oam8_attr", u8),
            field!("oam9_x", u8), field!("oam9_id", u8), field!("oam9_attr", u8),
            // pixel FIFO
            field!("bgw_fifo_a", u8), field!("bgw_fifo_b", u8),
            field!("spr_fifo_a", u8), field!("spr_fifo_b", u8),
            field!("mask_pipe", u8), field!("pal_pipe", u8),
            // fetcher
            field!("tfetch_state", u8, dict), field!("sfetch_state", u8, dict),
            field!("tile_temp_a", u8), field!("tile_temp_b", u8),
            // counters/flags
            field!("pix_count", u8), field!("sprite_count", u8), field!("scan_count", u8),
            field!("rendering", bool), field!("win_mode", bool),
        ]),
        (Layer::Writes, &[
            field!("vram_addr", u16, nullable),
            field!("vram_data", u8, nullable),
        ]),
        (Layer::Output, &[
            field!("pix", str, nullable),
            field!("pix_x", u8),
        ]),
    ],
};

pub static APU: SubsystemDef = SubsystemDef {
    name: "apu",
    layers: &[
        (Layer::Registers, &[
            // Channel 1 — square with sweep
            field!("nr10", u8), field!("nr11", u8), field!("nr12", u8),
            field!("nr13", u8), field!("nr14", u8),
            // Channel 2 — square
            field!("nr21", u8), field!("nr22", u8),
            field!("nr23", u8), field!("nr24", u8),
            // Channel 3 — wave
            field!("nr30", u8), field!("nr31", u8), field!("nr32", u8),
            field!("nr33", u8), field!("nr34", u8),
            // Channel 4 — noise
            field!("nr41", u8), field!("nr42", u8),
            field!("nr43", u8), field!("nr44", u8),
            // Control
            field!("nr50", u8), field!("nr51", u8), field!("nr52", u8),
        ]),
        (Layer::Internal, &[
            // TODO: channel counters, envelope state, LFSR, wave position, etc.
            // These will be defined as Missingno's APU crate exposes them.
        ]),
        (Layer::Writes, &[
            field!("wave_addr", u16, nullable),
            field!("wave_data", u8, nullable),
        ]),
    ],
};

pub static TIMER: SubsystemDef = SubsystemDef {
    name: "timer",
    layers: &[
        (Layer::Registers, &[
            field!("div", u8),
            field!("tima", u8),
            field!("tma", u8),
            field!("tac", u8, dict),
        ]),
    ],
};

pub static INTERRUPT: SubsystemDef = SubsystemDef {
    name: "interrupt",
    layers: &[
        (Layer::Registers, &[
            field!("if_", u8),
            field!("ie", u8),
        ]),
    ],
};

pub static SERIAL: SubsystemDef = SubsystemDef {
    name: "serial",
    layers: &[
        (Layer::Registers, &[
            field!("sb", u8),
            field!("sc", u8),
        ]),
    ],
};

/// All subsystems in field order.
pub static ALL_SUBSYSTEMS: &[&SubsystemDef] = &[
    &CPU, &PPU, &APU, &TIMER, &INTERRUPT, &SERIAL,
];

// ---------------------------------------------------------------------------
// Field lookup helpers
// ---------------------------------------------------------------------------

/// Look up a field definition by name across all subsystems.
pub fn lookup_field(name: &str) -> Option<&'static FieldDef> {
    ALL_SUBSYSTEMS.iter()
        .flat_map(|s| s.all_fields())
        .find(|f| f.name == name)
}

/// Return the native type for a field name.
/// Falls back to UInt8 for unknown fields (e.g. memory reads).
pub fn field_type(name: &str) -> FieldType {
    lookup_field(name).map(|f| f.field_type).unwrap_or(FieldType::UInt8)
}

/// Whether a field should be nullable.
pub fn field_nullable(name: &str) -> bool {
    lookup_field(name).map(|f| f.nullable).unwrap_or(false)
}

/// Whether a field should use dictionary encoding.
pub fn field_dictionary(name: &str) -> bool {
    lookup_field(name).map(|f| f.dictionary).unwrap_or(false)
}

/// Check if a name is a known built-in field.
pub fn is_known_field(name: &str) -> bool {
    lookup_field(name).is_some()
}

// ---------------------------------------------------------------------------
// Profile
// ---------------------------------------------------------------------------

/// A capture profile loaded from a TOML file.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub description: String,
    pub trigger: Trigger,
    /// Flattened, ordered list of field names to capture.
    pub fields: Vec<String>,
    /// Memory address reads: maps field name -> address.
    pub memory: BTreeMap<String, u16>,
}

// ---------------------------------------------------------------------------
// TOML deserialization
// ---------------------------------------------------------------------------

/// Raw TOML structure for deserialization.
#[derive(Deserialize)]
struct ProfileToml {
    profile: ProfileMeta,
    fields: FieldGroupsToml,
}

#[derive(Deserialize)]
struct ProfileMeta {
    name: String,
    description: String,
    trigger: Trigger,
}

/// Subsystem layer selection in TOML.
///
/// Each subsystem can be:
/// - `true` or `"all"` — all layers
/// - `"registers"` — a single layer
/// - `["registers", "internal"]` — multiple layers
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum LayerSelection {
    Bool(bool),
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Deserialize, Default)]
struct FieldGroupsToml {
    #[serde(default)]
    cpu: Option<LayerSelection>,
    #[serde(default)]
    ppu: Option<LayerSelection>,
    #[serde(default)]
    apu: Option<LayerSelection>,
    #[serde(default)]
    timer: Option<LayerSelection>,
    #[serde(default)]
    interrupt: Option<LayerSelection>,
    #[serde(default)]
    serial: Option<LayerSelection>,
    /// Arbitrary memory reads: name = "hex_address"
    #[serde(default)]
    memory: BTreeMap<String, String>,
}

fn parse_hex_addr(s: &str) -> std::result::Result<u16, String> {
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u16::from_str_radix(s, 16).map_err(|_| format!("invalid hex address: {s}"))
}

fn resolve_layers(
    selection: &LayerSelection,
    subsystem: &SubsystemDef,
) -> std::result::Result<Vec<Layer>, String> {
    match selection {
        LayerSelection::Bool(true) => Ok(subsystem.available_layers()),
        LayerSelection::Bool(false) => Ok(vec![]),
        LayerSelection::Single(s) if s == "all" => Ok(subsystem.available_layers()),
        LayerSelection::Single(s) => {
            let layer = Layer::from_str(s).ok_or_else(|| {
                format!("unknown layer '{s}' for subsystem '{}'", subsystem.name)
            })?;
            if !subsystem.available_layers().contains(&layer) {
                return Err(format!(
                    "subsystem '{}' does not have layer '{s}'",
                    subsystem.name
                ));
            }
            Ok(vec![layer])
        }
        LayerSelection::Multiple(layers) => {
            let mut result = Vec::new();
            for s in layers {
                if s == "all" {
                    return Ok(subsystem.available_layers());
                }
                let layer = Layer::from_str(s).ok_or_else(|| {
                    format!("unknown layer '{s}' for subsystem '{}'", subsystem.name)
                })?;
                if !subsystem.available_layers().contains(&layer) {
                    return Err(format!(
                        "subsystem '{}' does not have layer '{s}'",
                        subsystem.name
                    ));
                }
                if !result.contains(&layer) {
                    result.push(layer);
                }
            }
            Ok(result)
        }
    }
}

impl Profile {
    /// Load a profile from a TOML file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Self::parse(&contents)
    }

    /// Parse a profile from a TOML string.
    pub fn parse(toml_str: &str) -> Result<Self> {
        let raw: ProfileToml = toml::from_str(toml_str)?;

        let mut fields = Vec::new();

        // Resolve each subsystem's layer selection into fields.
        let subsystem_selections: &[(&SubsystemDef, &Option<LayerSelection>)] = &[
            (&CPU, &raw.fields.cpu),
            (&PPU, &raw.fields.ppu),
            (&APU, &raw.fields.apu),
            (&TIMER, &raw.fields.timer),
            (&INTERRUPT, &raw.fields.interrupt),
            (&SERIAL, &raw.fields.serial),
        ];

        for (subsystem, selection) in subsystem_selections {
            if let Some(sel) = selection {
                let layers = resolve_layers(sel, subsystem)
                    .map_err(Error::Profile)?;
                for field_def in subsystem.fields_for_layers(&layers) {
                    if fields.contains(&field_def.name.to_string()) {
                        return Err(Error::Profile(format!(
                            "duplicate field: {}",
                            field_def.name
                        )));
                    }
                    fields.push(field_def.name.to_string());
                }
            }
        }

        // Parse memory address fields
        let mut memory = BTreeMap::new();
        for (name, addr_str) in &raw.fields.memory {
            if fields.contains(name) || is_known_field(name) {
                return Err(Error::Profile(format!(
                    "memory field '{name}' conflicts with a built-in field"
                )));
            }
            let addr = parse_hex_addr(addr_str)
                .map_err(|e| Error::Profile(format!("memory field '{name}': {e}")))?;
            fields.push(name.clone());
            memory.insert(name.clone(), addr);
        }

        Ok(Profile {
            name: raw.profile.name,
            description: raw.profile.description,
            trigger: raw.profile.trigger,
            fields,
            memory,
        })
    }
}
