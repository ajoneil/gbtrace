//! SM83 (Game Boy CPU) disassembler.
//!
//! Decodes instructions from ROM bytes given a program counter address.
//! Handles all 256 base opcodes and 256 CB-prefixed opcodes.

/// Instruction length and mnemonic format for each base opcode.
/// Format placeholders: %b = byte, %w = word, %r = signed relative offset
const BASE_OPS: [(&str, u8); 256] = [
    // 0x00
    ("nop", 1), ("ld bc,%w", 3), ("ld (bc),a", 1), ("inc bc", 1),
    ("inc b", 1), ("dec b", 1), ("ld b,%b", 2), ("rlca", 1),
    ("ld (%w),sp", 3), ("add hl,bc", 1), ("ld a,(bc)", 1), ("dec bc", 1),
    ("inc c", 1), ("dec c", 1), ("ld c,%b", 2), ("rrca", 1),
    // 0x10
    ("stop", 2), ("ld de,%w", 3), ("ld (de),a", 1), ("inc de", 1),
    ("inc d", 1), ("dec d", 1), ("ld d,%b", 2), ("rla", 1),
    ("jr %r", 2), ("add hl,de", 1), ("ld a,(de)", 1), ("dec de", 1),
    ("inc e", 1), ("dec e", 1), ("ld e,%b", 2), ("rra", 1),
    // 0x20
    ("jr nz,%r", 2), ("ld hl,%w", 3), ("ld (hl+),a", 1), ("inc hl", 1),
    ("inc h", 1), ("dec h", 1), ("ld h,%b", 2), ("daa", 1),
    ("jr z,%r", 2), ("add hl,hl", 1), ("ld a,(hl+)", 1), ("dec hl", 1),
    ("inc l", 1), ("dec l", 1), ("ld l,%b", 2), ("cpl", 1),
    // 0x30
    ("jr nc,%r", 2), ("ld sp,%w", 3), ("ld (hl-),a", 1), ("inc sp", 1),
    ("inc (hl)", 1), ("dec (hl)", 1), ("ld (hl),%b", 2), ("scf", 1),
    ("jr c,%r", 2), ("add hl,sp", 1), ("ld a,(hl-)", 1), ("dec sp", 1),
    ("inc a", 1), ("dec a", 1), ("ld a,%b", 2), ("ccf", 1),
    // 0x40
    ("ld b,b", 1), ("ld b,c", 1), ("ld b,d", 1), ("ld b,e", 1),
    ("ld b,h", 1), ("ld b,l", 1), ("ld b,(hl)", 1), ("ld b,a", 1),
    ("ld c,b", 1), ("ld c,c", 1), ("ld c,d", 1), ("ld c,e", 1),
    ("ld c,h", 1), ("ld c,l", 1), ("ld c,(hl)", 1), ("ld c,a", 1),
    // 0x50
    ("ld d,b", 1), ("ld d,c", 1), ("ld d,d", 1), ("ld d,e", 1),
    ("ld d,h", 1), ("ld d,l", 1), ("ld d,(hl)", 1), ("ld d,a", 1),
    ("ld e,b", 1), ("ld e,c", 1), ("ld e,d", 1), ("ld e,e", 1),
    ("ld e,h", 1), ("ld e,l", 1), ("ld e,(hl)", 1), ("ld e,a", 1),
    // 0x60
    ("ld h,b", 1), ("ld h,c", 1), ("ld h,d", 1), ("ld h,e", 1),
    ("ld h,h", 1), ("ld h,l", 1), ("ld h,(hl)", 1), ("ld h,a", 1),
    ("ld l,b", 1), ("ld l,c", 1), ("ld l,d", 1), ("ld l,e", 1),
    ("ld l,h", 1), ("ld l,l", 1), ("ld l,(hl)", 1), ("ld l,a", 1),
    // 0x70
    ("ld (hl),b", 1), ("ld (hl),c", 1), ("ld (hl),d", 1), ("ld (hl),e", 1),
    ("ld (hl),h", 1), ("ld (hl),l", 1), ("halt", 1), ("ld (hl),a", 1),
    ("ld a,b", 1), ("ld a,c", 1), ("ld a,d", 1), ("ld a,e", 1),
    ("ld a,h", 1), ("ld a,l", 1), ("ld a,(hl)", 1), ("ld a,a", 1),
    // 0x80
    ("add a,b", 1), ("add a,c", 1), ("add a,d", 1), ("add a,e", 1),
    ("add a,h", 1), ("add a,l", 1), ("add a,(hl)", 1), ("add a,a", 1),
    ("adc a,b", 1), ("adc a,c", 1), ("adc a,d", 1), ("adc a,e", 1),
    ("adc a,h", 1), ("adc a,l", 1), ("adc a,(hl)", 1), ("adc a,a", 1),
    // 0x90
    ("sub b", 1), ("sub c", 1), ("sub d", 1), ("sub e", 1),
    ("sub h", 1), ("sub l", 1), ("sub (hl)", 1), ("sub a", 1),
    ("sbc a,b", 1), ("sbc a,c", 1), ("sbc a,d", 1), ("sbc a,e", 1),
    ("sbc a,h", 1), ("sbc a,l", 1), ("sbc a,(hl)", 1), ("sbc a,a", 1),
    // 0xA0
    ("and b", 1), ("and c", 1), ("and d", 1), ("and e", 1),
    ("and h", 1), ("and l", 1), ("and (hl)", 1), ("and a", 1),
    ("xor b", 1), ("xor c", 1), ("xor d", 1), ("xor e", 1),
    ("xor h", 1), ("xor l", 1), ("xor (hl)", 1), ("xor a", 1),
    // 0xB0
    ("or b", 1), ("or c", 1), ("or d", 1), ("or e", 1),
    ("or h", 1), ("or l", 1), ("or (hl)", 1), ("or a", 1),
    ("cp b", 1), ("cp c", 1), ("cp d", 1), ("cp e", 1),
    ("cp h", 1), ("cp l", 1), ("cp (hl)", 1), ("cp a", 1),
    // 0xC0
    ("ret nz", 1), ("pop bc", 1), ("jp nz,%w", 3), ("jp %w", 3),
    ("call nz,%w", 3), ("push bc", 1), ("add a,%b", 2), ("rst $00", 1),
    ("ret z", 1), ("ret", 1), ("jp z,%w", 3), ("prefix cb", 1),
    ("call z,%w", 3), ("call %w", 3), ("adc a,%b", 2), ("rst $08", 1),
    // 0xD0
    ("ret nc", 1), ("pop de", 1), ("jp nc,%w", 3), ("illegal", 1),
    ("call nc,%w", 3), ("push de", 1), ("sub %b", 2), ("rst $10", 1),
    ("ret c", 1), ("reti", 1), ("jp c,%w", 3), ("illegal", 1),
    ("call c,%w", 3), ("illegal", 1), ("sbc a,%b", 2), ("rst $18", 1),
    // 0xE0
    ("ldh ($ff%b),a", 2), ("pop hl", 1), ("ld ($ff00+c),a", 1), ("illegal", 1),
    ("illegal", 1), ("push hl", 1), ("and %b", 2), ("rst $20", 1),
    ("add sp,%r", 2), ("jp (hl)", 1), ("ld (%w),a", 3), ("illegal", 1),
    ("illegal", 1), ("illegal", 1), ("xor %b", 2), ("rst $28", 1),
    // 0xF0
    ("ldh a,($ff%b)", 2), ("pop af", 1), ("ld a,($ff00+c)", 1), ("di", 1),
    ("illegal", 1), ("push af", 1), ("or %b", 2), ("rst $30", 1),
    ("ld hl,sp+%r", 2), ("ld sp,hl", 1), ("ld a,(%w)", 3), ("ei", 1),
    ("illegal", 1), ("illegal", 1), ("cp %b", 2), ("rst $38", 1),
];

const REGS: [&str; 8] = ["b", "c", "d", "e", "h", "l", "(hl)", "a"];
const CB_OPS: [&str; 8] = ["rlc", "rrc", "rl", "rr", "sla", "sra", "swap", "srl"];

/// Disassemble one instruction at the given PC from ROM data.
/// Returns (mnemonic_string, instruction_length).
pub fn disassemble(rom: &[u8], pc: u16) -> (String, u8) {
    let addr = pc as usize;
    if addr >= rom.len() {
        return ("???".to_string(), 1);
    }

    let op = rom[addr];

    // CB prefix
    if op == 0xCB {
        if addr + 1 >= rom.len() {
            return ("cb ???".to_string(), 2);
        }
        let cb_op = rom[addr + 1];
        let reg = REGS[(cb_op & 0x07) as usize];
        let mnemonic = match cb_op >> 6 {
            0 => {
                let op_name = CB_OPS[((cb_op >> 3) & 0x07) as usize];
                format!("{op_name} {reg}")
            }
            1 => format!("bit {},{}",  (cb_op >> 3) & 0x07, reg),
            2 => format!("res {},{}",  (cb_op >> 3) & 0x07, reg),
            3 => format!("set {},{}",  (cb_op >> 3) & 0x07, reg),
            _ => unreachable!(),
        };
        return (mnemonic, 2);
    }

    let (fmt, len) = BASE_OPS[op as usize];

    if len == 1 {
        return (fmt.to_string(), 1);
    }

    let mut result = fmt.to_string();

    if len >= 2 && addr + 1 < rom.len() {
        let b = rom[addr + 1];
        if result.contains("%r") {
            let target = pc.wrapping_add(2).wrapping_add(b as i8 as u16);
            result = result.replace("%r", &format!("${target:04x}"));
        } else if result.contains("%b") {
            result = result.replace("%b", &format!("{b:02x}"));
        }
    }

    if len == 3 && addr + 2 < rom.len() {
        let lo = rom[addr + 1];
        let hi = rom[addr + 2];
        let word = u16::from_le_bytes([lo, hi]);
        result = result.replace("%w", &format!("${word:04x}"));
    }

    (result, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nop() {
        let rom = [0x00];
        assert_eq!(disassemble(&rom, 0), ("nop".to_string(), 1));
    }

    #[test]
    fn test_ld_bc_imm() {
        let rom = [0x01, 0x34, 0x12];
        assert_eq!(disassemble(&rom, 0), ("ld bc,$1234".to_string(), 3));
    }

    #[test]
    fn test_jr_nz() {
        let rom = [0x20, 0x05];
        assert_eq!(disassemble(&rom, 0), ("jr nz,$0007".to_string(), 2));
    }

    #[test]
    fn test_cb_bit() {
        let rom = [0xCB, 0x47];
        assert_eq!(disassemble(&rom, 0), ("bit 0,a".to_string(), 2));
    }

    #[test]
    fn test_cb_swap() {
        let rom = [0xCB, 0x37];
        assert_eq!(disassemble(&rom, 0), ("swap a".to_string(), 2));
    }

    #[test]
    fn test_ldh() {
        let rom = [0xE0, 0x40];
        assert_eq!(disassemble(&rom, 0), ("ldh ($ff40),a".to_string(), 2));
    }
}
