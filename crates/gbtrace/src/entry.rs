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

    /// Set an 8-bit field (e.g. `"a"`, `"f"`, `"lcdc"`).
    pub fn set_u8(&mut self, name: impl Into<String>, val: u8) {
        self.fields
            .insert(name.into(), Value::Number((val as u64).into()));
    }

    /// Set a 16-bit field (e.g. `"pc"`, `"sp"`).
    pub fn set_u16(&mut self, name: impl Into<String>, val: u16) {
        self.fields
            .insert(name.into(), Value::Number((val as u64).into()));
    }

    /// Set a boolean field (e.g. `"ime"`).
    pub fn set_bool(&mut self, name: impl Into<String>, val: bool) {
        self.fields.insert(name.into(), Value::Bool(val));
    }

    /// Get a field value by name.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.get(name)
    }

    /// Get a field as u8 (works for both numeric and legacy hex string values).
    pub fn get_u8(&self, name: &str) -> Option<u8> {
        self.fields.get(name).and_then(|v| match v {
            Value::Number(n) => n.as_u64().map(|n| n as u8),
            Value::String(s) => {
                let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
                u8::from_str_radix(s, 16).ok()
            }
            _ => None,
        })
    }

    /// Get a field as u16 (works for both numeric and legacy hex string values).
    pub fn get_u16(&self, name: &str) -> Option<u16> {
        self.fields.get(name).and_then(|v| match v {
            Value::Number(n) => n.as_u64().map(|n| n as u16),
            Value::String(s) => {
                let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
                u16::from_str_radix(s, 16).ok()
            }
            _ => None,
        })
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
