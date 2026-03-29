//! Round-trip test: write a .gbtrace file, read it back, verify correctness.

use gbtrace::store::TraceStore;
use gbtrace::format::read::GbtraceStore;
use gbtrace::format::write::GbtraceWriter;
use gbtrace::format::FieldGroup;
use gbtrace::header::{BootRom, TraceHeader, Trigger};

fn test_header() -> TraceHeader {
    TraceHeader {
        _header: true,
        format_version: "0.1.0".into(),
        emulator: "test".into(),
        emulator_version: "1.0".into(),
        rom_sha256: "0000".into(),
        model: "DMG".into(),
        boot_rom: BootRom::Skip,
        profile: "test".into(),
        fields: vec![
            "pc".into(), "sp".into(),
            "a".into(), "f".into(), "b".into(), "c".into(),
            "d".into(), "e".into(), "h".into(), "l".into(),
            "lcdc".into(), "stat".into(), "ly".into(),
            "pix".into(),
            "vram_addr".into(), "vram_data".into(),
        ],
        trigger: Trigger::Tcycle,

        notes: String::new(),
    }
}

fn test_groups() -> Vec<FieldGroup> {
    vec![
        FieldGroup {
            name: "cpu".into(),
            fields: vec![
                "pc".into(), "sp".into(),
                "a".into(), "f".into(), "b".into(), "c".into(),
                "d".into(), "e".into(), "h".into(), "l".into(),
            ],
        },
        FieldGroup {
            name: "ppu".into(),
            fields: vec!["lcdc".into(), "stat".into(), "ly".into()],
        },
        FieldGroup {
            name: "pixel".into(),
            fields: vec!["pix".into()],
        },
        FieldGroup {
            name: "vram".into(),
            fields: vec!["vram_addr".into(), "vram_data".into()],
        },
    ]
}

#[test]
fn test_basic_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.gbtrace");

    let header = test_header();
    let groups = test_groups();
    let num_entries = 1000;

    // --- Write ---
    {
        let mut w = GbtraceWriter::create(&path, &header, &groups).unwrap();

        // Mark frame at entry 0
        w.mark_frame(None).unwrap();

        for i in 0..num_entries {
            let pc = 0x0150u16 + (i as u16);
            let sp = 0xFFFEu16;
            let a = (i & 0xFF) as u8;
            let f = if a == 0 { 0x80u8 } else { 0x00u8 };
            let ly = ((i / 4) % 154) as u8;

            w.set_u16(0, pc);       // pc
            w.set_u16(1, sp);       // sp
            w.set_u8(2, a);         // a
            w.set_u8(3, f);         // f
            w.set_u8(4, 0);         // b
            w.set_u8(5, 0x13);      // c
            w.set_u8(6, 0);         // d
            w.set_u8(7, 0xD8);      // e
            w.set_u8(8, 0x01);      // h
            w.set_u8(9, 0x4D);      // l
            w.set_u8(10, 0x91);     // lcdc
            w.set_u8(11, 0x80);     // stat (dictionary-encoded)
            w.set_u8(12, ly);       // ly

            // pix: every 4th entry has a pixel
            if i % 4 == 0 {
                let shade = (i % 4) as u8 + b'0';
                w.set_str(13, std::str::from_utf8(&[shade]).unwrap());
            } else {
                w.set_null(13);     // pix null
            }

            // vram: every 50th entry has a write
            if i % 50 == 0 {
                w.set_u16(14, 0x8000 + (i as u16 % 0x1800)); // vram_addr
                w.set_u8(15, (i & 0xFF) as u8);               // vram_data
            } else {
                w.set_null(14);
                w.set_null(15);
            }

            w.finish_entry().unwrap();

            // Mark frame at entry 500
            if i == 499 {
                w.mark_frame(None).unwrap();
            }
        }

        w.finish().unwrap();
    }

    // --- Read ---
    let data = std::fs::read(&path).unwrap();
    let store = GbtraceStore::from_bytes(&data).unwrap();

    // Verify metadata
    assert_eq!(store.entry_count(), num_entries);
    assert_eq!(store.header().emulator, "test");
    assert_eq!(store.header().fields.len(), 16);

    // Verify frame boundaries
    let boundaries = store.frame_boundaries();
    assert_eq!(boundaries.len(), 2, "expected 2 frame boundaries, got {:?}", boundaries);
    assert_eq!(boundaries[0], 0);
    assert_eq!(boundaries[1], 500);

    // Verify entry values
    for i in 0..num_entries {
        let pc = store.get_numeric(0, i);
        assert_eq!(pc, 0x0150 + i as u64, "pc mismatch at entry {i}");

        let sp = store.get_numeric(1, i);
        assert_eq!(sp, 0xFFFE, "sp mismatch at entry {i}");

        let a = store.get_numeric(2, i);
        assert_eq!(a, (i & 0xFF) as u64, "a mismatch at entry {i}");

        let f = store.get_numeric(3, i);
        let expected_f = if (i & 0xFF) == 0 { 0x80 } else { 0x00 };
        assert_eq!(f, expected_f, "f mismatch at entry {i}");

        let ly = store.get_numeric(12, i);
        assert_eq!(ly, ((i / 4) % 154) as u64, "ly mismatch at entry {i}");

        // stat (dictionary-encoded) should always be 0x80
        let stat = store.get_numeric(11, i);
        assert_eq!(stat, 0x80, "stat mismatch at entry {i}");

        // pix
        if i % 4 == 0 {
            assert!(!store.is_null(13, i), "pix should not be null at entry {i}");
            let pix = store.get_str(13, i);
            assert_eq!(pix, "0", "pix value mismatch at entry {i}");
        } else {
            assert!(store.is_null(13, i), "pix should be null at entry {i}");
        }

        // vram
        if i % 50 == 0 {
            assert!(!store.is_null(14, i), "vram_addr should not be null at entry {i}");
            let addr = store.get_numeric(14, i);
            assert_eq!(addr, (0x8000 + (i as u64 % 0x1800)), "vram_addr mismatch at entry {i}");
        } else {
            assert!(store.is_null(14, i), "vram_addr should be null at entry {i}");
        }
    }
}

#[test]
fn test_large_chunk_boundary() {
    // Test that data spanning multiple chunks (>64K entries) works correctly
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.gbtrace");

    let header = TraceHeader {
        _header: true,
        format_version: "0.1.0".into(),
        emulator: "test".into(),
        emulator_version: "1.0".into(),
        rom_sha256: "0000".into(),
        model: "DMG".into(),
        boot_rom: BootRom::Skip,
        profile: "test".into(),
        fields: vec!["pc".into(), "a".into()],
        trigger: Trigger::Instruction,

        notes: String::new(),
    };

    let groups = vec![
        FieldGroup { name: "cpu".into(), fields: vec!["pc".into(), "a".into()] },
    ];

    let num_entries = 150_000; // spans ~2.3 chunks at 64K

    // Write
    {
        let mut w = GbtraceWriter::create(&path, &header, &groups).unwrap();
        w.mark_frame(None).unwrap();

        for i in 0..num_entries {
            w.set_u16(0, (i & 0xFFFF) as u16); // pc
            w.set_u8(1, (i & 0xFF) as u8);      // a
            w.finish_entry().unwrap();
        }

        w.finish().unwrap();
    }

    // Read and verify
    let data = std::fs::read(&path).unwrap();
    let store = GbtraceStore::from_bytes(&data).unwrap();

    assert_eq!(store.entry_count(), num_entries);

    // Check entries near chunk boundaries
    for i in [0, 1, 65535, 65536, 65537, 131071, 131072, 131073, num_entries - 1] {
        let pc = store.get_numeric(0, i);
        assert_eq!(pc, (i & 0xFFFF) as u64, "pc mismatch at entry {i}");
        let a = store.get_numeric(1, i);
        assert_eq!(a, (i & 0xFF) as u64, "a mismatch at entry {i}");
    }
}

#[test]
fn test_framebuffer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fb.gbtrace");

    let header = TraceHeader {
        _header: true,
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

    let groups = vec![
        FieldGroup { name: "cpu".into(), fields: vec!["pc".into()] },
    ];

    // Create a test framebuffer (23040 bytes)
    let mut fb = vec![0u8; 23040];
    for i in 0..23040 {
        fb[i] = (i % 4) as u8;
    }

    // Write
    {
        let mut w = GbtraceWriter::create(&path, &header, &groups).unwrap();

        // Frame 0 with no framebuffer
        w.mark_frame(None).unwrap();
        for i in 0..100 {
            w.set_u16(0, i);
            w.finish_entry().unwrap();
        }

        // Frame 1 with framebuffer
        w.mark_frame(Some(&fb)).unwrap();
        for i in 0..100 {
            w.set_u16(0, 100 + i);
            w.finish_entry().unwrap();
        }

        w.finish().unwrap();
    }

    // Read and verify
    let data = std::fs::read(&path).unwrap();
    let store = GbtraceStore::from_bytes(&data).unwrap();

    assert_eq!(store.entry_count(), 200);

    // Frame 0 has no framebuffer
    let fb0 = store.framebuffer(0);
    assert!(fb0.is_none(), "frame 0 should have no framebuffer");

    // Frame 1 has a framebuffer
    let fb1 = store.framebuffer(1);
    assert!(fb1.is_some(), "frame 1 should have a framebuffer");
    let fb1 = fb1.unwrap();
    assert_eq!(fb1.len(), 23040);
    assert_eq!(fb1, fb);
}

#[test]
fn test_empty_trace() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.gbtrace");

    let header = TraceHeader {
        _header: true,
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

    let groups = vec![
        FieldGroup { name: "cpu".into(), fields: vec!["pc".into()] },
    ];

    // Write empty trace
    {
        let w = GbtraceWriter::create(&path, &header, &groups).unwrap();
        w.finish().unwrap();
    }

    // Read
    let data = std::fs::read(&path).unwrap();
    let store = GbtraceStore::from_bytes(&data).unwrap();

    assert_eq!(store.entry_count(), 0);
    assert_eq!(store.frame_boundaries().len(), 0);
}
