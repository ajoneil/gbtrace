use serde::{Deserialize, Serialize};

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
    Tcycle,
    Scanline,
    Frame,
    Custom,
}

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
        if !self.fields.contains(&"cy".to_string()) {
            return Err(crate::error::Error::InvalidHeader(
                "fields must contain 'cy'".into(),
            ));
        }
        Ok(())
    }
}
