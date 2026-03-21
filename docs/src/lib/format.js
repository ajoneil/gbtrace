/** Strip 0x prefix and lowercase hex values for display. Non-hex values pass through. */
export function displayVal(v) {
  if (v === undefined || v === null) return '';
  const s = String(v);
  if (s.startsWith('0x') || s.startsWith('0X')) {
    return s.slice(2).toLowerCase();
  }
  return s;
}

/** Normalize a user-entered value to match the stored format (0x-prefixed, uppercase). */
export function normalizeInput(v) {
  const s = v.trim();
  if (!s) return s;
  // If it already has 0x prefix, uppercase the hex part
  if (s.startsWith('0x') || s.startsWith('0X')) {
    return '0x' + s.slice(2).toUpperCase();
  }
  // If it looks like hex (all hex chars), add 0x prefix and uppercase
  if (/^[0-9a-fA-F]+$/.test(s)) {
    return '0x' + s.toUpperCase();
  }
  // Otherwise return as-is (booleans, etc.)
  return s;
}
