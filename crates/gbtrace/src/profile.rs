use crate::error::{Error, Result};
use crate::header::Trigger;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// A capture profile loaded from a TOML file.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub description: String,
    pub trigger: Trigger,
    /// Flattened, ordered list of field names to capture.
    /// `cy` is always implicitly included.
    pub fields: Vec<String>,
    /// Memory address reads: maps field name -> address.
    /// These are read via safe/peek memory access each instruction.
    pub memory: BTreeMap<String, u16>,
}

/// Raw TOML structure for deserialization.
#[derive(Deserialize)]
struct ProfileToml {
    profile: ProfileMeta,
    fields: FieldGroups,
}

#[derive(Deserialize)]
struct ProfileMeta {
    name: String,
    description: String,
    trigger: Trigger,
}

#[derive(Deserialize, Default)]
struct FieldGroups {
    #[serde(default)]
    cpu: Vec<String>,
    #[serde(default)]
    ppu: Vec<String>,
    #[serde(default)]
    timer: Vec<String>,
    #[serde(default)]
    interrupt: Vec<String>,
    #[serde(default)]
    serial: Vec<String>,
    #[serde(default)]
    pixel: Vec<String>,
    /// Arbitrary memory reads: name = "hex_address"
    #[serde(default)]
    memory: BTreeMap<String, String>,
}

/// All known field names in the spec.
const KNOWN_FIELDS: &[&str] = &[
    // timing
    "cy",
    // cpu
    "a", "f", "b", "c", "d", "e", "h", "l", "sp", "pc", "op", "ime",
    // ppu
    "lcdc", "stat", "ly", "lyc", "scy", "scx", "wy", "wx", "bgp", "obp0", "obp1", "dma",
    // pixel output
    "pix",
    // timer
    "div", "tima", "tma", "tac",
    // interrupt
    "if_", "ie",
    // serial
    "sb", "sc",
];

/// Native type of a trace field, used for Parquet column types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    UInt64,
    UInt16,
    UInt8,
    Bool,
    Str,
}

/// Return the native type for a known field name.
pub fn field_type(name: &str) -> FieldType {
    match name {
        "cy" => FieldType::UInt64,
        "pc" | "sp" => FieldType::UInt16,
        "ime" => FieldType::Bool,
        "pix" => FieldType::Str,
        _ => FieldType::UInt8,
    }
}

fn parse_hex_addr(s: &str) -> std::result::Result<u16, String> {
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u16::from_str_radix(s, 16).map_err(|_| format!("invalid hex address: {s}"))
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

        // Flatten field groups into ordered list, cy always first.
        let mut fields = Vec::new();
        let groups = [
            &raw.fields.cpu,
            &raw.fields.ppu,
            &raw.fields.timer,
            &raw.fields.interrupt,
            &raw.fields.serial,
            &raw.fields.pixel,
        ];

        for group in groups {
            for field in group {
                if field == "cy" || field == "_cy" {
                    continue; // already included
                }
                if !KNOWN_FIELDS.contains(&field.as_str()) {
                    return Err(Error::Profile(format!("unknown field: {field}")));
                }
                if fields.contains(field) {
                    return Err(Error::Profile(format!("duplicate field: {field}")));
                }
                fields.push(field.clone());
            }
        }

        // Parse memory address fields
        let mut memory = BTreeMap::new();
        for (name, addr_str) in &raw.fields.memory {
            if fields.contains(name) || KNOWN_FIELDS.contains(&name.as_str()) {
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
