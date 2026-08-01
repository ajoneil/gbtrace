//! Chips shared across systems, mirroring missingno's `crates/hardware/`
//! split: a chip lives here once two systems carry it (the 6502 in the
//! NES's 2A03 and the VCS's 6507); single-system silicon stays with its
//! system (the Game Boy's SM83 lives in `system/gb`, like missingno's
//! `systems/gb/src/isa.rs`). Modules here export only vocabulary — flag
//! tables and subsystem field catalogues — consumed by the [`crate::system`]
//! registry entries; the `Isa`/`System` structs themselves stay there.

pub mod mos6502;
