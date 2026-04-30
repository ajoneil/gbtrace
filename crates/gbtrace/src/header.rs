use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::profile::FieldType;

/// How the boot ROM was handled for this trace.
///
/// Serializes as a plain string:
/// - `"skip"` — no boot ROM, post-boot state was set manually
/// - `"builtin"` — emulator's built-in boot ROM was used
/// - `"stripped:<original>"` — boot entries were removed post-capture
/// - `"<sha256>"` — a specific boot ROM was used, identified by hash
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootRom {
    /// Boot ROM was skipped; initial state is post-boot.
    Skip,
    /// Emulator's built-in boot ROM was used.
    Builtin,
    /// Boot ROM was used but entries were stripped post-capture.
    /// Contains the original boot_rom value (e.g. the SHA-256 hash).
    Stripped(String),
    /// A specific boot ROM was used, identified by SHA-256.
    Sha256(String),
}

impl BootRom {
    /// Return the stripped variant, preserving the original boot ROM info.
    pub fn to_stripped(&self) -> Self {
        match self {
            BootRom::Skip => BootRom::Skip, // already no boot data
            BootRom::Builtin => BootRom::Stripped("builtin".to_string()),
            BootRom::Stripped(_) => self.clone(), // already stripped
            BootRom::Sha256(hash) => BootRom::Stripped(hash.clone()),
        }
    }
}

impl Serialize for BootRom {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            BootRom::Skip => serializer.serialize_str("skip"),
            BootRom::Builtin => serializer.serialize_str("builtin"),
            BootRom::Stripped(original) => {
                serializer.serialize_str(&format!("stripped:{original}"))
            }
            BootRom::Sha256(hash) => serializer.serialize_str(hash),
        }
    }
}

impl<'de> Deserialize<'de> for BootRom {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "skip" => BootRom::Skip,
            "builtin" => BootRom::Builtin,
            _ if s.starts_with("stripped:") => {
                BootRom::Stripped(s[9..].to_string())
            }
            _ => BootRom::Sha256(s),
        })
    }
}

/// When trace entries are emitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    Instruction,
    Mcycle,
    Tcycle,
    Scanline,
    Frame,
    Custom,
}

/// Adapter-defined field with its type metadata, declared in the trace
/// header. Used for non-standard fields (emulator-internal debug state)
/// that aren't part of the built-in field catalogue. Readers consult
/// `TraceHeader::extension_fields` to resolve types for these names.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionField {
    /// Native type of the field's value.
    #[serde(rename = "type")]
    pub field_type: FieldType,
    /// Whether the column is nullable.
    #[serde(default, skip_serializing_if = "is_false")]
    pub nullable: bool,
    /// Human-readable description (what the field captures).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Emulator that defined the field (e.g. "missingno"). Allows
    /// downstream tooling to attribute extension semantics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

fn is_false(b: &bool) -> bool { !b }

/// The header line of a `.gbtrace` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceHeader {
    /// Always `true`. Identifies this line as the header.
    pub _header: bool,

    /// Spec version (semver).
    pub format_version: String,

    /// Emulator identifier (lowercase, no spaces).
    pub emulator: String,

    /// Emulator version string.
    pub emulator_version: String,

    /// SHA-256 hex digest of the ROM file.
    pub rom_sha256: String,

    /// Hardware model identifier (e.g. "DMG-B", "CGB-E").
    pub model: String,

    /// How the boot ROM was handled.
    pub boot_rom: BootRom,

    /// Name of the capture profile used.
    pub profile: String,

    /// Ordered list of field names present in each state entry.
    pub fields: Vec<String>,

    /// When entries are emitted.
    pub trigger: Trigger,

    /// Adapter-defined extension fields. Maps field name → type metadata
    /// for fields that aren't in the built-in catalogue. Adapters declare
    /// these at trace-creation time so the writer / reader can construct
    /// appropriate column buffers and a downstream consumer can resolve
    /// types without having to know about adapter-internal fields.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extension_fields: BTreeMap<String, ExtensionField>,

    /// Optional freeform notes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl TraceHeader {
    /// Validate header invariants.
    pub fn validate(&self) -> crate::error::Result<()> {
        if !self._header {
            return Err(crate::error::Error::InvalidHeader(
                "_header must be true".into(),
            ));
        }
        if self.fields.is_empty() {
            return Err(crate::error::Error::InvalidHeader(
                "fields must not be empty".into(),
            ));
        }
        // Reject extension fields that shadow built-in field names —
        // type resolution would be ambiguous (the built-in catalogue and
        // the header would each claim a type, with no clear winner).
        for name in self.extension_fields.keys() {
            if crate::profile::is_known_field(name) {
                return Err(crate::error::Error::InvalidHeader(format!(
                    "extension field '{name}' shadows a built-in field"
                )));
            }
        }
        Ok(())
    }

    /// Resolve a field's type — built-in fields use the static catalogue;
    /// extension fields use `extension_fields`; unknown names fall back
    /// to `UInt8`.
    pub fn resolve_field_type(&self, name: &str) -> FieldType {
        if let Some(def) = crate::profile::lookup_field(name) {
            return def.field_type;
        }
        if let Some(ext) = self.extension_fields.get(name) {
            return ext.field_type;
        }
        FieldType::UInt8
    }

    /// Resolve a field's nullability.
    pub fn resolve_field_nullable(&self, name: &str) -> bool {
        if let Some(def) = crate::profile::lookup_field(name) {
            return def.nullable;
        }
        if let Some(ext) = self.extension_fields.get(name) {
            return ext.nullable;
        }
        false
    }
}
