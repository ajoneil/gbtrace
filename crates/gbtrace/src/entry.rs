use serde_json::Value;
use std::collections::BTreeMap;

/// A single trace entry — one row of emulator state.
///
/// Fields are stored as a ordered map of field name → JSON value.
/// The `cy` field is always present as an integer; register fields
/// are hex strings like `"0x0F"`.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceEntry {
    fields: BTreeMap<String, Value>,
}

impl TraceEntry {
    pub fn new() -> Self {
        Self {
            fields: BTreeMap::new(),
        }
    }

    /// Set the cycle count.
    pub fn set_cy(&mut self, cy: u64) {
        self.fields
            .insert("cy".to_string(), Value::Number(cy.into()));
    }

    /// Get the cycle count, if present.
    pub fn cy(&self) -> Option<u64> {
        self.fields.get("cy").and_then(|v| v.as_u64())
    }

    /// Set an 8-bit hex field (e.g. `"a"`, `"f"`, `"lcdc"`).
    pub fn set_u8(&mut self, name: impl Into<String>, val: u8) {
        self.fields
            .insert(name.into(), Value::String(format!("0x{val:02X}")));
    }

    /// Set a 16-bit hex field (e.g. `"pc"`, `"sp"`).
    pub fn set_u16(&mut self, name: impl Into<String>, val: u16) {
        self.fields
            .insert(name.into(), Value::String(format!("0x{val:04X}")));
    }

    /// Set a boolean field (e.g. `"ime"`).
    pub fn set_bool(&mut self, name: impl Into<String>, val: bool) {
        self.fields.insert(name.into(), Value::Bool(val));
    }

    /// Get a field value by name.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.get(name)
    }

    /// Serialize to a JSON object.
    pub fn to_json_value(&self) -> Value {
        Value::Object(
            self.fields
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
    }

    /// Deserialize from a JSON object.
    pub fn from_json_value(value: &Value) -> Option<Self> {
        let obj = value.as_object()?;
        let fields = obj
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Some(Self { fields })
    }
}

impl Default for TraceEntry {
    fn default() -> Self {
        Self::new()
    }
}
