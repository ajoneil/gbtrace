//! Native `.gbtrace` binary format.
//!
//! File layout:
//! ```text
//! [Magic "GBTR" (4)] [Version (1)] [Header len (4)] [Header JSON zstd]
//! [Chunk 0] [Chunk 1] ... [Chunk N]
//! [Framebuffer blobs (optional)]
//! [Footer]
//! [Footer offset (8)]
//! ```
//!
//! Each chunk contains ~64K entries with field groups compressed independently.
//! Frames (vblank boundaries) are metadata in the footer, not per-entry data.

pub mod write;
pub mod read;

pub const MAGIC: &[u8; 4] = b"GBTR";
pub const VERSION: u8 = 1;

/// Default maximum entries per chunk.
pub const DEFAULT_CHUNK_SIZE: usize = 65536;

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

/// Entry in the frame index (footer).
#[derive(Debug, Clone)]
pub struct FrameIndexEntry {
    /// Global entry index where this frame starts.
    pub entry_index: u64,
    /// Byte offset of the framebuffer blob (0 = no framebuffer).
    pub framebuffer_offset: u64,
    /// Compressed size of the framebuffer blob.
    pub framebuffer_size: u32,
}

/// The footer, read from the end of the file.
#[derive(Debug, Clone)]
pub struct Footer {
    pub chunks: Vec<ChunkIndexEntry>,
    pub frames: Vec<FrameIndexEntry>,
    pub total_entries: u64,
}
