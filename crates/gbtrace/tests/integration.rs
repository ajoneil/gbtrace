use gbtrace::*;
use std::path::PathBuf;

#[test]
fn parse_profiles() {
    let suites_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test-suites");

    let gbmicrotest = Profile::load(suites_dir.join("gbmicrotest/profile.toml")).unwrap();
    assert_eq!(gbmicrotest.name, "gbmicrotest");
    assert!(gbmicrotest.fields.contains(&"pc".to_string()));
    assert!(gbmicrotest.fields.contains(&"lcdc".to_string()));
    assert!(gbmicrotest.fields.contains(&"vram_addr".to_string()));
    assert!(gbmicrotest.memory.contains_key("test_result"));

    let blargg = Profile::load(suites_dir.join("blargg/profile.toml")).unwrap();
    assert!(blargg.fields.contains(&"pix".to_string()));
    assert!(blargg.fields.contains(&"div".to_string()));
    // blargg has ppu registers + output but not internal/writes
    assert!(!blargg.fields.contains(&"vram_addr".to_string()));
}

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
    assert_eq!(e.get("ime").unwrap().as_bool().unwrap(), true);
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
        notes: String::new(),
    };
    assert!(h.validate().is_err());

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
        notes: String::new(),
    };
    assert!(h.validate().is_err());
}
