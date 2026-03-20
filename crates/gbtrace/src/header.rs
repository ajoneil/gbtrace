use serde::{Deserialize, Serialize};

/// How the boot ROM was handled for this trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BootRom {
    /// Boot ROM was skipped; initial state is post-boot.
    Skip,
    /// Emulator's built-in boot ROM was used.
    Builtin,
    /// A specific boot ROM was used, identified by SHA-256.
    #[serde(untagged)]
    Sha256(String),
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
