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

    /// CPU flag is set. Derived from F register bits.
    /// Flag: Z=7, N=6, H=5, C=4.
    /// Requires `f` field.
    FlagSet(u8),

    /// CPU flag is clear. Requires `f` field.
    FlagClear(u8),

    /// CPU flag transitions to set (was clear, now set). Requires `f` field.
    FlagBecomesSet(u8),

    /// CPU flag transitions to clear (was set, now clear). Requires `f` field.
    FlagBecomesClear(u8),

    /// Bitwise-AND test: `(field & mask) != 0`. Generalises per-bit
    /// queries; e.g., `if_ & 0x02` matches whenever the STAT IRQ bit is
    /// set, irrespective of other IF bits.
    FieldBitMask { field: String, mask: u64 },

    /// Bitwise-AND equality test: `(field & mask) == value`. E.g.,
    /// `stat & 0x03 = 1` matches the VBlank PPU mode robustly to other
    /// STAT bits.
    FieldBitMaskEquals { field: String, mask: u64, value: u64 },

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
            Condition::FieldEquals { .. }
            | Condition::FlagSet(_)
            | Condition::FlagClear(_)
            | Condition::FieldBitMask { .. }
            | Condition::FieldBitMaskEquals { .. } => false,
            Condition::FieldChanges { .. }
            | Condition::FieldChangesTo { .. }
            | Condition::FieldChangesFrom { .. }
            | Condition::PpuEntersMode(_)
            | Condition::LcdTurnsOn
            | Condition::LcdTurnsOff
            | Condition::TimerOverflow
            | Condition::InterruptFires(_)
            | Condition::FlagBecomesSet(_)
            | Condition::FlagBecomesClear(_) => true,
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
            match entry.get(field) {
                Some(Value::Number(n)) => {
                    // Compare numerically: parse the condition value as hex or decimal
                    if let Some(target) = parse_number(value) {
                        n.as_u64() == Some(target)
                    } else {
                        false
                    }
                }
                Some(v) => entry_field_str_raw(v) == *value,
                None => false,
            }
        }

        Condition::FieldChanges { field } => {
            let cur = entry_field_str(entry, field);
            let prv = prev.and_then(|p| entry_field_str(p, field));
            cur.is_some() && cur != prv
        }

        Condition::FieldChangesTo { field, value } => {
            let matches_val = |e: &TraceEntry| field_matches_value(e, field, value);
            matches_val(entry) && prev.map_or(true, |p| !matches_val(p))
        }

        Condition::FieldChangesFrom { field, value } => {
            let matches_val = |e: &TraceEntry| field_matches_value(e, field, value);
            prev.map_or(false, |p| matches_val(p)) && !matches_val(entry)
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

        Condition::FlagSet(bit) => {
            entry_field_u8(entry, "f").map_or(false, |f| (f >> bit) & 1 == 1)
        }

        Condition::FlagClear(bit) => {
            entry_field_u8(entry, "f").map_or(false, |f| (f >> bit) & 1 == 0)
        }

        Condition::FlagBecomesSet(bit) => {
            bit_transitions(entry, prev, "f", *bit, false, true)
        }

        Condition::FlagBecomesClear(bit) => {
            bit_transitions(entry, prev, "f", *bit, true, false)
        }

        Condition::FieldBitMask { field, mask } => {
            entry.get(field).and_then(|v| v.as_u64()).map_or(false, |n| (n & mask) != 0)
        }

        Condition::FieldBitMaskEquals { field, mask, value } => {
            entry.get(field).and_then(|v| v.as_u64()).map_or(false, |n| (n & mask) == *value)
        }

        Condition::All(cs) => cs.iter().all(|c| eval_condition(c, entry, prev)),
        Condition::Any(cs) => cs.iter().any(|c| eval_condition(c, entry, prev)),
    }
}

/// Get a field value as its raw string representation.
fn entry_field_str(entry: &TraceEntry, field: &str) -> Option<String> {
    entry.get(field).map(|v| entry_field_str_raw(v))
}

fn entry_field_str_raw(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => v.to_string(),
    }
}

/// Parse a number from user input. Always treats as hex (with or without 0x prefix).
pub fn parse_number(s: &str) -> Option<u64> {
    let s = s.trim();
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u64::from_str_radix(hex, 16).ok()
}

/// Check if a field in an entry matches a value string (numeric or string comparison).
fn field_matches_value(entry: &TraceEntry, field: &str, value: &str) -> bool {
    match entry.get(field) {
        Some(Value::Number(n)) => {
            if let Some(target) = parse_number(value) {
                n.as_u64() == Some(target)
            } else {
                false
            }
        }
        Some(v) => entry_field_str_raw(v) == *value,
        None => false,
    }
}

fn entry_field_u8(entry: &TraceEntry, field: &str) -> Option<u8> {
    entry.get_u8(field)
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
/// Map a CPU flag name to its bit position in the F register.
fn flag_bit(name: &str) -> Result<u8, String> {
    match name.to_lowercase().as_str() {
        "z" | "zero" => Ok(7),
        "n" | "sub" | "subtract" => Ok(6),
        "h" | "half" | "halfcarry" => Ok(5),
        "c" | "carry" => Ok(4),
        _ => Err(format!("unknown flag '{name}': expected z, n, h, or c")),
    }
}

pub fn parse_condition(s: &str) -> Result<Condition, String> {
    let s = s.trim();

    // Flag conditions: "flag z set", "flag c clear", "flag z becomes set", etc.
    if let Some(rest) = s.strip_prefix("flag ") {
        let rest = rest.trim();
        // "flag z becomes set" / "flag z becomes clear"
        if let Some(inner) = rest.strip_suffix(" becomes set") {
            return Ok(Condition::FlagBecomesSet(flag_bit(inner.trim())?));
        }
        if let Some(inner) = rest.strip_suffix(" becomes clear") {
            return Ok(Condition::FlagBecomesClear(flag_bit(inner.trim())?));
        }
        // "flag z set" / "flag z clear"
        if let Some(inner) = rest.strip_suffix(" set") {
            return Ok(Condition::FlagSet(flag_bit(inner.trim())?));
        }
        if let Some(inner) = rest.strip_suffix(" clear") {
            return Ok(Condition::FlagClear(flag_bit(inner.trim())?));
        }
        return Err(format!("invalid flag condition: '{s}'. Expected: flag z set, flag c clear, flag z becomes set, flag c becomes clear"));
    }

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

    // Bitwise-AND forms: must be checked BEFORE plain `=` so that
    // `field & mask = value` is parsed as FieldBitMaskEquals, not as
    // FieldEquals on the literal `field & mask` field name.
    if let Some(amp) = s.find('&') {
        let field = s[..amp].trim().to_string();
        let rest = s[amp + 1..].trim();
        if field.is_empty() {
            return Err(format!("invalid bitmask condition '{s}': field must be non-empty"));
        }
        // `field & mask = value`
        if let Some(eq) = rest.find('=') {
            let mask_str = rest[..eq].trim();
            let value_str = rest[eq + 1..].trim();
            let mask = parse_number(mask_str)
                .ok_or_else(|| format!("invalid mask in '{s}': '{mask_str}' is not a number"))?;
            let value = parse_number(value_str)
                .ok_or_else(|| format!("invalid value in '{s}': '{value_str}' is not a number"))?;
            return Ok(Condition::FieldBitMaskEquals { field, mask, value });
        }
        // `field & mask`
        let mask = parse_number(rest)
            .ok_or_else(|| format!("invalid mask in '{s}': '{rest}' is not a number"))?;
        return Ok(Condition::FieldBitMask { field, mask });
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
