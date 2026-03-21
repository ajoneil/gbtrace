//! Query conditions for filtering and searching trace entries.
//!
//! Conditions range from simple field comparisons to stateful transition
//! detection and Game Boy-specific semantic queries.

use crate::entry::TraceEntry;
use serde_json::Value;

/// A condition that can be evaluated against trace entries.
///
/// Some conditions are stateless (e.g. `FieldEquals`) and can be checked
/// against a single entry. Others are stateful (e.g. `FieldChanges`) and
/// require tracking the previous entry — use [`ConditionEvaluator`] for those.
#[derive(Debug, Clone)]
pub enum Condition {
    // --- Stateless: single-entry checks ---
    /// Field equals a specific value (string comparison on the display form).
    FieldEquals { field: String, value: String },

    // --- Stateful: require previous entry ---
    /// Field changed to any different value since the previous entry.
    FieldChanges { field: String },

    /// Field changed to a specific value (was something else before).
    FieldChangesTo { field: String, value: String },

    /// Field changed from a specific value (was that value, now isn't).
    FieldChangesFrom { field: String, value: String },

    // --- Semantic: Game Boy-specific conditions ---
    /// PPU enters the specified mode (0=HBlank, 1=VBlank, 2=OAM, 3=Drawing).
    /// Derived from STAT register bits 0-1. Requires `stat` field.
    PpuEntersMode(u8),

    /// LCD turns on (LCDC bit 7 transitions 0→1). Requires `lcdc` field.
    LcdTurnsOn,

    /// LCD turns off (LCDC bit 7 transitions 1→0). Requires `lcdc` field.
    LcdTurnsOff,

    /// Timer overflow: TIMA wraps to TMA value. Requires `tima` field.
    TimerOverflow,

    /// An interrupt fires (IF bit transitions 0→1).
    /// Bit index: 0=VBlank, 1=STAT, 2=Timer, 3=Serial, 4=Joypad.
    /// Requires `if_` field.
    InterruptFires(u8),

    // --- Compound ---
    /// All sub-conditions must match.
    All(Vec<Condition>),

    /// Any sub-condition must match.
    Any(Vec<Condition>),
}

impl Condition {
    /// Whether this condition requires state from the previous entry.
    pub fn is_stateful(&self) -> bool {
        match self {
            Condition::FieldEquals { .. } => false,
            Condition::FieldChanges { .. }
            | Condition::FieldChangesTo { .. }
            | Condition::FieldChangesFrom { .. }
            | Condition::PpuEntersMode(_)
            | Condition::LcdTurnsOn
            | Condition::LcdTurnsOff
            | Condition::TimerOverflow
            | Condition::InterruptFires(_) => true,
            Condition::All(cs) | Condition::Any(cs) => cs.iter().any(|c| c.is_stateful()),
        }
    }
}

/// Evaluates conditions against a stream of trace entries, tracking
/// state for transition-based conditions.
pub struct ConditionEvaluator {
    condition: Condition,
    prev: Option<TraceEntry>,
}

impl ConditionEvaluator {
    pub fn new(condition: Condition) -> Self {
        Self {
            condition,
            prev: None,
        }
    }

    /// Check whether the current entry matches the condition,
    /// given the tracked previous entry state.
    /// Call this for each entry in order.
    pub fn evaluate(&mut self, entry: &TraceEntry) -> bool {
        let result = eval_condition(&self.condition, entry, self.prev.as_ref());
        self.prev = Some(entry.clone());
        result
    }

    /// Reset the evaluator state (e.g. when starting a new trace).
    pub fn reset(&mut self) {
        self.prev = None;
    }
}

// ---------------------------------------------------------------------------
// Internal evaluation
// ---------------------------------------------------------------------------

fn eval_condition(cond: &Condition, entry: &TraceEntry, prev: Option<&TraceEntry>) -> bool {
    match cond {
        Condition::FieldEquals { field, value } => {
            entry_field_str(entry, field).as_deref() == Some(value.as_str())
        }

        Condition::FieldChanges { field } => {
            let cur = entry_field_str(entry, field);
            let prv = prev.and_then(|p| entry_field_str(p, field));
            cur.is_some() && cur != prv
        }

        Condition::FieldChangesTo { field, value } => {
            let cur = entry_field_str(entry, field);
            let prv = prev.and_then(|p| entry_field_str(p, field));
            cur.as_deref() == Some(value.as_str()) && prv.as_deref() != Some(value.as_str())
        }

        Condition::FieldChangesFrom { field, value } => {
            let cur = entry_field_str(entry, field);
            let prv = prev.and_then(|p| entry_field_str(p, field));
            prv.as_deref() == Some(value.as_str()) && cur.as_deref() != Some(value.as_str())
        }

        Condition::PpuEntersMode(mode) => {
            let cur_mode = entry_field_u8(entry, "stat").map(|s| s & 0x03);
            let prv_mode = prev.and_then(|p| entry_field_u8(p, "stat")).map(|s| s & 0x03);
            cur_mode == Some(*mode) && prv_mode != Some(*mode)
        }

        Condition::LcdTurnsOn => {
            bit_transitions(entry, prev, "lcdc", 7, false, true)
        }

        Condition::LcdTurnsOff => {
            bit_transitions(entry, prev, "lcdc", 7, true, false)
        }

        Condition::TimerOverflow => {
            // Detect when TIMA decreases (wraps from high value to TMA reload value)
            let cur = entry_field_u8(entry, "tima");
            let prv = prev.and_then(|p| entry_field_u8(p, "tima"));
            match (cur, prv) {
                (Some(c), Some(p)) => c < p && p > 0x80, // heuristic: large decrease = overflow
                _ => false,
            }
        }

        Condition::InterruptFires(bit) => {
            bit_transitions(entry, prev, "if_", *bit, false, true)
        }

        Condition::All(cs) => cs.iter().all(|c| eval_condition(c, entry, prev)),
        Condition::Any(cs) => cs.iter().any(|c| eval_condition(c, entry, prev)),
    }
}

/// Get a field value as its display string.
fn entry_field_str(entry: &TraceEntry, field: &str) -> Option<String> {
    entry.get(field).map(|v| match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => v.to_string(),
    })
}

/// Parse a hex string field (e.g. "0xFF") to a u8.
fn entry_field_u8(entry: &TraceEntry, field: &str) -> Option<u8> {
    entry.get(field).and_then(|v| match v {
        Value::String(s) => {
            let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
            u8::from_str_radix(s, 16).ok()
        }
        Value::Number(n) => n.as_u64().map(|n| n as u8),
        _ => None,
    })
}

/// Check if a specific bit in a hex field transitioned between two states.
fn bit_transitions(
    entry: &TraceEntry,
    prev: Option<&TraceEntry>,
    field: &str,
    bit: u8,
    from: bool,
    to: bool,
) -> bool {
    let cur_val = entry_field_u8(entry, field);
    let prv_val = prev.and_then(|p| entry_field_u8(p, field));
    match (cur_val, prv_val) {
        (Some(c), Some(p)) => {
            let cur_bit = (c >> bit) & 1 == 1;
            let prv_bit = (p >> bit) & 1 == 1;
            prv_bit == from && cur_bit == to
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Condition parsing from strings
// ---------------------------------------------------------------------------

/// Parse a condition from a human-readable string.
///
/// Supported formats:
/// - `field=value` — field equals value
/// - `field changes` — field changes to any value
/// - `field changes to value` — field transitions to specific value
/// - `field changes from value` — field transitions from specific value
/// - `ppu enters mode N` — PPU enters mode 0-3
/// - `lcd on` / `lcd off` — LCD turns on/off
/// - `timer overflow` — TIMA overflows
/// - `interrupt N` — interrupt bit N fires (0=vblank, 1=stat, 2=timer, 3=serial, 4=joypad)
pub fn parse_condition(s: &str) -> Result<Condition, String> {
    let s = s.trim();

    // Semantic conditions
    if let Some(rest) = s.strip_prefix("ppu enters mode ") {
        let mode: u8 = rest.trim().parse()
            .map_err(|_| format!("invalid PPU mode: {rest}"))?;
        if mode > 3 {
            return Err(format!("PPU mode must be 0-3, got {mode}"));
        }
        return Ok(Condition::PpuEntersMode(mode));
    }
    if s == "lcd on" { return Ok(Condition::LcdTurnsOn); }
    if s == "lcd off" { return Ok(Condition::LcdTurnsOff); }
    if s == "timer overflow" { return Ok(Condition::TimerOverflow); }
    if let Some(rest) = s.strip_prefix("interrupt ") {
        let bit: u8 = rest.trim().parse()
            .map_err(|_| format!("invalid interrupt bit: {rest}"))?;
        if bit > 4 {
            return Err(format!("interrupt bit must be 0-4, got {bit}"));
        }
        return Ok(Condition::InterruptFires(bit));
    }

    // "field changes to value"
    if let Some(rest) = s.strip_suffix(" changes") {
        return Ok(Condition::FieldChanges { field: rest.trim().to_string() });
    }
    if s.contains(" changes to ") {
        let parts: Vec<&str> = s.splitn(2, " changes to ").collect();
        return Ok(Condition::FieldChangesTo {
            field: parts[0].trim().to_string(),
            value: parts[1].trim().to_string(),
        });
    }
    if s.contains(" changes from ") {
        let parts: Vec<&str> = s.splitn(2, " changes from ").collect();
        return Ok(Condition::FieldChangesFrom {
            field: parts[0].trim().to_string(),
            value: parts[1].trim().to_string(),
        });
    }

    // field=value
    if let Some(eq) = s.find('=') {
        let field = s[..eq].trim().to_string();
        let value = s[eq + 1..].trim().to_string();
        if field.is_empty() || value.is_empty() {
            return Err(format!("invalid condition '{s}': field and value must be non-empty"));
        }
        return Ok(Condition::FieldEquals { field, value });
    }

    Err(format!("cannot parse condition: '{s}'"))
}
