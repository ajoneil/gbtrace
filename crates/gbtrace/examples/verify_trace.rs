use gbtrace::TraceReader;
use std::env;

fn main() {
    let path = env::args().nth(1).expect("Usage: verify_trace <file.gbtrace>");
    let reader = TraceReader::open(&path).expect("Failed to open trace file");

    let header = reader.header();
    println!("Emulator:  {}", header.emulator);
    println!("Version:   {}", header.emulator_version);
    println!("Model:     {}", header.model);
    println!("Profile:   {}", header.profile);
    println!("Fields:    {:?}", header.fields);
    println!("ROM hash:  {}", header.rom_sha256);
    println!();

    let mut count = 0u64;
    let mut last_cy = None;
    let mut monotonic = true;

    for result in reader {
        let entry = result.expect("Failed to read entry");
        let cy = entry.cy().expect("Missing cy field");

        if let Some(prev) = last_cy {
            if cy < prev {
                eprintln!("WARNING: non-monotonic cy at entry {count}: {prev} -> {cy}");
                monotonic = false;
            }
        }
        last_cy = Some(cy);
        count += 1;
    }

    println!("Entries:   {count}");
    println!("Cycle range: 0..={}", last_cy.unwrap_or(0));
    println!("Monotonic: {monotonic}");
    if monotonic {
        println!("\nTrace file is valid!");
    }
}
