/** Known 16-bit fields — always display as 4 hex digits. */
const FIELDS_16BIT = new Set(['pc', 'sp']);

/** Format the F (flags) register: hex value + flag letters. */
function formatFlags(v) {
  const hex = v.toString(16).padStart(2, '0');
  const z = (v & 0x80) ? 'Z' : '·';
  const n = (v & 0x40) ? 'N' : '·';
  const h = (v & 0x20) ? 'H' : '·';
  const c = (v & 0x10) ? 'C' : '·';
  return `${hex} ${z}${n}${h}${c}`;
}

/** Format a value as zero-padded lowercase hex for display.
 *  If fieldName is provided, uses field-aware width (e.g. pc always 4 digits). */
export function displayVal(v, fieldName) {
  if (v === undefined || v === null) return '';
  if (typeof v === 'number') {
    if (fieldName === 'f') return formatFlags(v);
    if (fieldName && FIELDS_16BIT.has(fieldName)) {
      return v.toString(16).padStart(4, '0');
    }
    if (v <= 0xFF) return v.toString(16).padStart(2, '0');
    if (v <= 0xFFFF) return v.toString(16).padStart(4, '0');
    return v.toString(16);
  }
  const s = String(v);
  if (s.startsWith('0x') || s.startsWith('0X')) return s.slice(2).toLowerCase();
  return s;
}

/** Normalize user hex input for querying.
 *  Strips optional 0x prefix, returns bare lowercase hex string.
 *  The Rust query parser treats all values as hex. */
export function normalizeInput(v) {
  const s = v.trim();
  if (!s) return s;
  const bare = (s.startsWith('0x') || s.startsWith('0X')) ? s.slice(2) : s;
  return bare.toLowerCase();
}
