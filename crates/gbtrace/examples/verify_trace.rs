use gbtrace::store::open_trace_store;
use std::env;

fn main() {
    let path = env::args().nth(1).expect("Usage: verify_trace <file.gbtrace>");
    let store = open_trace_store(&path).expect("Failed to open trace file");

    let header = store.header();
    println!("Emulator:  {}", header.emulator);
    println!("Version:   {}", header.emulator_version);
    println!("Model:     {}", header.model);
    println!("Profile:   {}", header.profile);
    println!("Fields:    {:?}", header.fields);
    println!("ROM hash:  {}", header.rom_sha256);
    println!("Entries:   {}", store.entry_count());
    println!("\nTrace file is valid!");
}
