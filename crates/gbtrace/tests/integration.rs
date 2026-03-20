use gbtrace::*;
use std::path::PathBuf;
use tempfile::tempdir;

fn test_header() -> TraceHeader {
    TraceHeader {
        _header: true,
        format_version: "0.1.0".into(),
        emulator: "test-emu".into(),
        emulator_version: "1.0.0".into(),
        rom_sha256: "abcd1234".into(),
        model: "DMG-B".into(),
        boot_rom: BootRom::Skip,
        profile: "cpu_basic".into(),
        fields: vec![
            "cy".into(),
            "pc".into(),
            "sp".into(),
            "a".into(),
            "f".into(),
        ],
        trigger: Trigger::Instruction,
        notes: String::new(),
    }
}

fn test_entry(cy: u64, pc: u16, a: u8) -> TraceEntry {
    let mut e = TraceEntry::new();
    e.set_cy(cy);
    e.set_u16("pc", pc);
    e.set_u16("sp", 0xFFFE);
    e.set_u8("a", a);
    e.set_u8("f", 0xB0);
    e
}

#[test]
fn roundtrip_plain() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.gbtrace");

    let header = test_header();

    // Write
    {
        let mut writer = TraceWriter::create(&path, &header).unwrap();
        writer.write_entry(&test_entry(0, 0x0100, 0x01)).unwrap();
        writer.write_entry(&test_entry(4, 0x0101, 0x02)).unwrap();
        writer.write_entry(&test_entry(8, 0x0103, 0xFF)).unwrap();
        writer.finish().unwrap();
    }

    // Read back
    let reader = TraceReader::open(&path).unwrap();
    assert_eq!(reader.header().emulator, "test-emu");
    assert_eq!(reader.header().fields.len(), 5);

    let entries: Vec<TraceEntry> = reader.collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].cy(), Some(0));
    assert_eq!(entries[1].cy(), Some(4));
    assert_eq!(entries[2].cy(), Some(8));

    // Check hex formatting
    assert_eq!(
        entries[2].get("a").unwrap().as_str().unwrap(),
        "0xFF"
    );
    assert_eq!(
        entries[0].get("pc").unwrap().as_str().unwrap(),
        "0x0100"
    );
}

#[test]
fn roundtrip_gzip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.gbtrace.gz");

    let header = test_header();

    {
        let mut writer = TraceWriter::create(&path, &header).unwrap();
        for i in 0..100 {
            writer
                .write_entry(&test_entry(i * 4, 0x0100 + i as u16, i as u8))
                .unwrap();
        }
        writer.finish().unwrap();
    }

    let reader = TraceReader::open(&path).unwrap();
    assert_eq!(reader.header().emulator, "test-emu");

    let entries: Vec<TraceEntry> = reader.collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(entries.len(), 100);
    assert_eq!(entries[99].cy(), Some(396));
}

#[test]
fn parse_profiles() {
    let profiles_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("profiles");

    let cpu = Profile::load(profiles_dir.join("cpu_basic.toml")).unwrap();
    assert_eq!(cpu.name, "cpu_basic");
    assert_eq!(cpu.trigger, Trigger::Instruction);
    assert!(cpu.fields.contains(&"pc".to_string()));
    assert!(cpu.fields.contains(&"cy".to_string()));
    assert_eq!(cpu.fields[0], "cy"); // cy is always first

    let ppu = Profile::load(profiles_dir.join("ppu_timing.toml")).unwrap();
    assert!(ppu.fields.contains(&"lcdc".to_string()));
    assert!(ppu.fields.contains(&"if_".to_string()));

    let timer = Profile::load(profiles_dir.join("timer_edge.toml")).unwrap();
    assert!(timer.fields.contains(&"div".to_string()));
    assert!(timer.fields.contains(&"tima".to_string()));
}

#[test]
fn header_validation() {
    let mut h = test_header();
    h._header = false;
    assert!(h.validate().is_err());

    let mut h = test_header();
    h.fields.clear();
    assert!(h.validate().is_err());

    let mut h = test_header();
    h.fields = vec!["pc".into()]; // missing cy
    assert!(h.validate().is_err());
}

#[test]
fn profile_rejects_unknown_field() {
    let toml = r#"
[profile]
name = "bad"
description = "bad profile"
trigger = "instruction"

[fields]
cpu = ["pc", "bogus_field"]
"#;
    let result = Profile::parse(toml);
    assert!(result.is_err());
}

#[test]
fn profile_rejects_duplicate_field() {
    let toml = r#"
[profile]
name = "bad"
description = "bad profile"
trigger = "instruction"

[fields]
cpu = ["pc", "a"]
interrupt = ["a"]
"#;
    let result = Profile::parse(toml);
    assert!(result.is_err());
}

#[test]
fn entry_hex_formatting() {
    let mut e = TraceEntry::new();
    e.set_u8("a", 0x0F);
    e.set_u8("f", 0x00);
    e.set_u16("pc", 0x0100);
    e.set_u16("sp", 0xFFFF);
    e.set_bool("ime", true);

    assert_eq!(e.get("a").unwrap().as_str().unwrap(), "0x0F");
    assert_eq!(e.get("f").unwrap().as_str().unwrap(), "0x00");
    assert_eq!(e.get("pc").unwrap().as_str().unwrap(), "0x0100");
    assert_eq!(e.get("sp").unwrap().as_str().unwrap(), "0xFFFF");
    assert_eq!(e.get("ime").unwrap().as_bool().unwrap(), true);
}
