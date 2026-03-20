use crate::error::{Error, Result};
use crate::header::Trigger;
use serde::Deserialize;
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
}

/// All known field names in the spec.
const KNOWN_FIELDS: &[&str] = &[
    // timing
    "cy",
    // cpu
    "a", "f", "b", "c", "d", "e", "h", "l", "sp", "pc", "op", "ime",
    // ppu
    "lcdc", "stat", "ly", "lyc", "scy", "scx", "wy", "wx", "bgp", "obp0", "obp1", "dma",
    // timer
    "div", "tima", "tma", "tac",
    // interrupt
    "if_", "ie",
    // serial
    "sb", "sc",
];

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
        let mut fields = vec!["cy".to_string()];
        let groups = [
            &raw.fields.cpu,
            &raw.fields.ppu,
            &raw.fields.timer,
            &raw.fields.interrupt,
            &raw.fields.serial,
        ];

        for group in groups {
            for field in group {
                if field == "cy" {
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

        Ok(Profile {
            name: raw.profile.name,
            description: raw.profile.description,
            trigger: raw.profile.trigger,
            fields,
        })
    }
}
