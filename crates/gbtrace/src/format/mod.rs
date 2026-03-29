//! Native `.gbtrace` binary format.
//!
//! File layout (v2):
//! ```text
//! [Magic "GBTR" (4)] [Version (1)] [Header len (4)] [Header JSON zstd]
//! [Snapshot record]*  -- initial snapshots (optional, before any chunks)
//! [Chunk | Snapshot record]*  -- interleaved chunks and inline snapshots
//! [Footer]
//! [Footer offset (8)]
//! ```
//!
//! Each chunk contains ~64K entries with field groups compressed independently.
//!
//! Snapshot records are typed, self-describing blobs that carry bulk state
//! at specific points in the trace stream. They serve two purposes:
//! - Frame boundaries (with optional screen data)
//! - Initial state (memory, APU state, etc.)
//!
//! A snapshot record on disk:
//! ```text
//! [Tag "SNAP" (4)] [Type (1)] [Entry index (8)] [Payload len (4)] [Payload zstd]
//! ```

pub mod write;
pub mod read;
pub mod convert;

pub const MAGIC: &[u8; 4] = b"GBTR";
pub const VERSION: u8 = 2;
pub const SNAPSHOT_TAG: &[u8; 4] = b"SNAP";

/// Default maximum entries per chunk.
pub const DEFAULT_CHUNK_SIZE: usize = 65536;

/// Snapshot type tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SnapshotType {
    /// Frame boundary. Payload is optional screen data (160x144 shade values).
    Frame = 0,
    /// Bulk memory contents (WRAM, VRAM, OAM, HRAM, wave RAM, cartridge RAM).
    Memory = 1,
    /// CPU state beyond trace row fields (halt state, EI delay, etc.).
    CpuState = 2,
    /// PPU timing state (dot position, window line counter, etc.).
    PpuTiming = 3,
    /// APU internal state not derivable from registers.
    ApuState = 4,
    /// Timer internals (full 16-bit counter, overflow state).
    TimerState = 5,
    /// DMA transfer state.
    DmaState = 6,
    /// Serial transfer state.
    SerialState = 7,
    /// Cartridge mapper state (MBC type, bank selection, etc.).
    MbcState = 8,
}

impl SnapshotType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Frame),
            1 => Some(Self::Memory),
            2 => Some(Self::CpuState),
            3 => Some(Self::PpuTiming),
            4 => Some(Self::ApuState),
            5 => Some(Self::TimerState),
            6 => Some(Self::DmaState),
            7 => Some(Self::SerialState),
            8 => Some(Self::MbcState),
            _ => None,
        }
    }
}

/// A field group definition — maps a group name to its column indices.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FieldGroup {
    pub name: String,
    pub fields: Vec<String>,
}

/// Per-chunk statistics for a single numeric field.
#[derive(Debug, Clone, Default)]
pub struct FieldStats {
    pub min: u64,
    pub max: u64,
}

/// Entry in the chunk index (footer).
#[derive(Debug, Clone)]
pub struct ChunkIndexEntry {
    /// Byte offset of the chunk from file start.
    pub offset: u64,
    /// Number of entries in this chunk.
    pub entry_count: u32,
}

/// Entry in the snapshot index (footer).
#[derive(Debug, Clone)]
pub struct SnapshotIndexEntry {
    /// Snapshot type.
    pub snapshot_type: u8,
    /// Global entry index at which this snapshot occurs.
    pub entry_index: u64,
    /// Byte offset of the snapshot record from file start.
    pub offset: u64,
    /// Size of the compressed payload (0 = no payload).
    pub payload_size: u32,
}

/// The footer, read from the end of the file.
#[derive(Debug, Clone)]
pub struct Footer {
    pub chunks: Vec<ChunkIndexEntry>,
    pub snapshots: Vec<SnapshotIndexEntry>,
    pub total_entries: u64,
}

