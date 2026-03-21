/** Format a value as zero-padded lowercase hex for display. */
export function displayVal(v) {
  if (v === undefined || v === null) return '';
  if (typeof v === 'number') {
    if (v <= 0xFF) return v.toString(16).padStart(2, '0');
    if (v <= 0xFFFF) return v.toString(16).padStart(4, '0');
    return v.toString(16);
  }
  const s = String(v);
  // Handle legacy 0x-prefixed string values
  if (s.startsWith('0x') || s.startsWith('0X')) return s.slice(2).toLowerCase();
  return s;
}

/** Normalize user hex input to a numeric value for querying.
 *  Strips optional 0x prefix, parses as hex, returns the decimal number
 *  (which is what the JSONL format stores). */
export function normalizeInput(v) {
  const s = v.trim();
  if (!s) return s;
  const bare = (s.startsWith('0x') || s.startsWith('0X')) ? s.slice(2) : s;
  const n = parseInt(bare, 16);
  if (!isNaN(n)) return String(n);
  // Not hex — return as-is (booleans, etc.)
  return s;
}
