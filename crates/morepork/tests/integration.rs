use morepork::*;

#[test]
fn profile_rejects_unknown_layer() {
    let toml = r#"
[profile]
name = "bad"
description = "bad profile"
trigger = "instruction"

[fields]
cpu = "bogus_layer"
"#;
    let result = Profile::parse(toml);
    assert!(result.is_err());
}

#[test]
fn profile_layer_selection_variants() {
    // Bool true = all layers
    let toml = r#"
[profile]
name = "test"
description = "test"
trigger = "instruction"

[fields]
cpu = true
"#;
    let p = Profile::parse(toml).unwrap();
    assert!(p.fields.contains(&"pc".to_string()));
    assert!(p.fields.contains(&"ime".to_string()));
    assert!(p.fields.contains(&"mcycles".to_string()));

    // Single layer string
    let toml = r#"
[profile]
name = "test"
description = "test"
trigger = "instruction"

[fields]
cpu = "registers"
"#;
    let p = Profile::parse(toml).unwrap();
    assert!(p.fields.contains(&"pc".to_string()));
    assert!(!p.fields.contains(&"mcycles".to_string()));

    // Multiple layers
    let toml = r#"
[profile]
name = "test"
description = "test"
trigger = "tcycle"

[fields]
ppu = ["registers", "output"]
"#;
    let p = Profile::parse(toml).unwrap();
    assert!(p.fields.contains(&"lcdc".to_string()));
    assert!(p.fields.contains(&"pix".to_string()));
    assert!(!p.fields.contains(&"oam0_x".to_string()));
    assert!(!p.fields.contains(&"vram_addr".to_string()));
}

#[test]
fn entry_hex_formatting() {
    let mut e = TraceEntry::new();
    e.set_u8("a", 0x0F);
    e.set_u8("f", 0x00);
    e.set_u16("pc", 0x0100);
    e.set_u16("sp", 0xFFFF);
    e.set_bool("ime", true);

    assert_eq!(e.get("a").unwrap().as_u64().unwrap(), 0x0F);
    assert_eq!(e.get("f").unwrap().as_u64().unwrap(), 0x00);
    assert_eq!(e.get("pc").unwrap().as_u64().unwrap(), 0x0100);
    assert_eq!(e.get("sp").unwrap().as_u64().unwrap(), 0xFFFF);
    assert!(e.get("ime").unwrap().as_bool().unwrap());
}

#[test]
fn header_validation() {
    let h = TraceHeader {
        _header: false,
        format_version: "0.1.0".into(),
        emulator: "test".into(),
        emulator_version: "1.0".into(),
        rom_sha256: "0000".into(),
        model: "DMG".into(),
        boot_rom: BootRom::Skip,
        profile: "test".into(),
        fields: vec!["pc".into()],
        trigger: Trigger::Instruction,
        pix_format: PixFormat::default(),
        extension_fields: std::collections::BTreeMap::new(),
        notes: String::new(),
        ..Default::default()
    };
    assert!(h.validate().is_err());

    // Empty `fields` is permitted at validate time — JSONL inputs may infer
    // fields from the first data line, so the construction-time check would
    // be too strict. Field-list emptiness shows up later as a no-op trace.
    let h = TraceHeader {
        _header: true,
        format_version: "0.1.0".into(),
        emulator: "test".into(),
        emulator_version: "1.0".into(),
        rom_sha256: "0000".into(),
        model: "DMG".into(),
        boot_rom: BootRom::Skip,
        profile: "test".into(),
        fields: vec![],
        trigger: Trigger::Instruction,
        pix_format: PixFormat::default(),
        extension_fields: std::collections::BTreeMap::new(),
        notes: String::new(),
        ..Default::default()
    };
    assert!(h.validate().is_ok());
}

#[test]
fn profile_system_defaults_to_dmg() {
    let toml = r#"
[profile]
name = "t"
description = "t"
trigger = "instruction"

[fields]
cpu = "registers"
"#;
    let p = Profile::parse(toml).unwrap();
    assert_eq!(p.system, "dmg");
    assert!(p.fields.contains(&"pc".to_string()));
}

#[test]
fn profile_rejects_unknown_system() {
    let toml = r#"
[profile]
name = "t"
description = "t"
trigger = "instruction"
system = "n64"

[fields]
cpu = "registers"
"#;
    let err = Profile::parse(toml).unwrap_err().to_string();
    assert!(err.contains("unknown system 'n64'"), "{err}");
}

#[test]
fn profile_rejects_unknown_subsystem() {
    let toml = r#"
[profile]
name = "t"
description = "t"
trigger = "instruction"

[fields]
vdp = "registers"
"#;
    let err = Profile::parse(toml).unwrap_err().to_string();
    assert!(err.contains("unknown subsystem 'vdp'"), "{err}");
}

#[test]
fn nes_profile_and_flag_queries() {
    let toml = r#"
[profile]
name = "nes-smoke"
description = "NES CPU + PPU registers"
trigger = "instruction"
system = "nes"

[fields]
cpu = "registers"
ppu = "registers"
"#;
    let p = Profile::parse(toml).unwrap();
    assert_eq!(p.system, "nes");
    assert_eq!(
        p.fields,
        ["pc", "a", "x", "y", "s", "p", "control", "mask", "line", "dot"]
            .map(String::from)
    );

    // Flag vocabulary resolves against P, not the GB F register.
    let nes = morepork::system::system("nes").unwrap();
    let cond = morepork::query::parse_condition("flag n becomes set", nes).unwrap();
    match cond {
        morepork::query::Condition::BitTransition { field, bit, to } => {
            assert_eq!((field.as_str(), bit, to), ("p", 7, true));
        }
        other => panic!("unexpected condition: {other:?}"),
    }
    // GB phrases are not in the NES vocabulary.
    assert!(morepork::query::parse_condition("lcd on", nes).is_err());
    assert!(morepork::query::parse_condition("flag h set", nes).is_err());
}

#[test]
fn sg1000_profile_and_flag_queries() {
    let toml = r#"
[profile]
name = "sg1000-smoke"
description = "SG-1000 CPU + VDP registers"
trigger = "instruction"
system = "sg1000"

[fields]
cpu = "registers"
vdp = "registers"
"#;
    let p = Profile::parse(toml).unwrap();
    assert_eq!(p.system, "sg1000");
    assert_eq!(
        p.fields,
        [
            "pc", "op_addr", "sp", "a", "f", "b", "c", "d", "e", "h", "l",
            "ix", "iy", "wz", "a_", "f_", "b_", "c_", "d_", "e_", "h_", "l_",
            "i", "r",
            "reg0", "reg1", "reg2", "reg3", "reg4", "reg5", "reg6", "reg7",
            "status", "line", "dot",
        ]
        .map(String::from)
    );

    let sg = morepork::system::system("sg1000").unwrap();
    assert_eq!(sg.isa.id, "z80");

    // Flag vocabulary resolves against the Z80 F register, including the
    // undocumented X/Y bits and the P/V aliases.
    let cond = morepork::query::parse_condition("flag s becomes set", sg).unwrap();
    match cond {
        morepork::query::Condition::BitTransition { field, bit, to } => {
            assert_eq!((field.as_str(), bit, to), ("f", 7, true));
        }
        other => panic!("unexpected condition: {other:?}"),
    }
    let cond = morepork::query::parse_condition("flag pv set", sg).unwrap();
    match cond {
        morepork::query::Condition::FieldBitMask { field, mask } => {
            assert_eq!((field.as_str(), mask), ("f", 1 << 2));
        }
        other => panic!("unexpected condition: {other:?}"),
    }

    // "vblank starts" desugars to the line-192 transition.
    let cond = morepork::query::parse_condition("vblank starts", sg).unwrap();
    match cond {
        morepork::query::Condition::FieldChangesTo { field, value } => {
            assert_eq!((field.as_str(), value.as_str()), ("line", "0xc0"));
        }
        other => panic!("unexpected condition: {other:?}"),
    }

    // Shadow-register names parse as ordinary field conditions.
    let cond = morepork::query::parse_condition("a_ changes", sg).unwrap();
    match cond {
        morepork::query::Condition::FieldChanges { field } => assert_eq!(field, "a_"),
        other => panic!("unexpected condition: {other:?}"),
    }

    // GB phrases are not in the SG-1000 vocabulary.
    assert!(morepork::query::parse_condition("lcd on", sg).is_err());
}

#[test]
fn coleco_and_msx1_share_the_ti_vdp_line() {
    for id in ["coleco", "msx1"] {
        let sys = morepork::system::system(id).unwrap();
        assert_eq!(sys.isa.id, "z80", "{id}");
        // Same shared z80 + ti-vdp catalogue as sg1000.
        assert!(sys.lookup_field("wz").is_some(), "{id}");
        assert!(sys.lookup_field("reg7").is_some(), "{id}");
        let cond = morepork::query::parse_condition("vblank starts", sys).unwrap();
        assert!(matches!(cond, morepork::query::Condition::FieldChangesTo { .. }), "{id}");
    }
}

#[test]
fn vcs_profile_and_flag_queries() {
    let toml = r#"
[profile]
name = "vcs-smoke"
description = "6507 + TIA beam + RIOT"
trigger = "instruction"
system = "vcs"

[fields]
cpu = "registers"
tia = "registers"
riot = "registers"
"#;
    let p = Profile::parse(toml).unwrap();
    assert_eq!(p.system, "vcs");
    assert_eq!(
        p.fields,
        ["pc", "a", "x", "y", "s", "p", "line", "clock", "timer", "port_a", "port_b"]
            .map(String::from)
    );

    // The 6502 flag vocabulary is shared with the NES family.
    let vcs = morepork::system::system("vcs").unwrap();
    let cond = morepork::query::parse_condition("flag c set", vcs).unwrap();
    match cond {
        morepork::query::Condition::FieldBitMask { field, mask } => {
            assert_eq!((field.as_str(), mask), ("p", 1));
        }
        other => panic!("unexpected condition: {other:?}"),
    }
    // Phrases from the other families are not in the VCS vocabulary.
    assert!(morepork::query::parse_condition("lcd on", vcs).is_err());
    assert!(morepork::query::parse_condition("vblank starts", vcs).is_err());
}

// --- ISA-driven disassembly through the shared missingno_core vocabulary ---

use missingno_core::isa::{Flow, Instruction, InstructionSet, OperandClass};

/// A minimal SM83 front end over the shared trait, standing in for the real
/// `missingno_gb::Sm83` (whose crate cannot be a morepork dependency without
/// a cycle). Enough opcodes to prove the render path decodes through the trait.
struct ToySm83;

impl InstructionSet for ToySm83 {
    fn max_len(&self) -> usize {
        3
    }
    fn decode(&self, _address: u32, bytes: &[u8]) -> Instruction {
        match bytes.first().copied().unwrap_or(0) {
            0x00 => Instruction { mnemonic: "nop".into(), length: 1, flow: Flow::Sequential },
            0x01 => {
                let word = u16::from_le_bytes([bytes[1], bytes[2]]);
                Instruction {
                    mnemonic: format!("ld bc,${word:04x}"),
                    length: 3,
                    flow: Flow::Sequential,
                }
            }
            other => Instruction {
                mnemonic: format!("${other:02x}"),
                length: 1,
                flow: Flow::Sequential,
            },
        }
    }
    fn classify_operand(&self, _operand: &str) -> OperandClass {
        OperandClass::Plain
    }
}

#[test]
fn disassemble_sm83_through_shared_isa() {
    // nop ; ld bc,$1234 ; nop
    let rom = [0x00u8, 0x01, 0x34, 0x12, 0x00];
    let rows = morepork::disasm::disassemble_rows(&ToySm83, &rom, 0, 3);
    assert_eq!(
        rows,
        vec![
            (0, "nop".to_string()),
            (1, "ld bc,$1234".to_string()),
            (4, "nop".to_string()),
        ]
    );
}

#[test]
fn disassemble_6502_through_real_decoder() {
    // The real missingno-mos-6502 decoder, driven by the same shared trait.
    let isa = missingno_mos_6502::Mos6502;
    // lda #$7f ; sta $02 ; jmp $8000
    let rom = [0xA9u8, 0x7F, 0x85, 0x02, 0x4C, 0x00, 0x80];
    let rows = morepork::disasm::disassemble_rows(&isa, &rom, 0, 3);
    assert_eq!(rows[0], (0, "lda #$7f".to_string()));
    assert_eq!(rows[1], (2, "sta $02".to_string()));
    assert_eq!(rows[2], (4, "jmp $8000".to_string()));
}
