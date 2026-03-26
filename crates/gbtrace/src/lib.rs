pub mod column_store;
pub mod diff;
pub mod disasm;
pub mod downsample;
pub mod entry;
pub mod framebuffer;
pub mod error;
pub mod header;
pub mod profile;
pub mod query;
pub mod reader;
pub mod writer;
pub mod vram;
pub mod format;
pub mod diff_store;

pub use column_store::TraceStore;
pub use downsample::DownsampledStore;
pub use diff::{AlignmentStrategy, DiffConfig, DiffResult, DivergenceClass, MultiDiffResult, TraceDiffer};
pub use entry::TraceEntry;
pub use error::Error;
pub use header::{BootRom, CycleUnit, TraceHeader, Trigger};
pub use profile::{FieldType, Profile};
pub use query::{Condition, ConditionEvaluator};
pub use reader::TraceReader;
pub use writer::TraceWriter;

use error::Result;
use std::path::Path;

/// Format-agnostic trace reader for JSONL files.
/// For native .gbtrace files, use `column_store::open_trace_store` instead.
pub enum AnyTraceReader {
    Jsonl(TraceReader),
}

impl AnyTraceReader {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::Jsonl(TraceReader::open(path)?))
    }

    pub fn header(&self) -> &TraceHeader {
        match self {
            Self::Jsonl(r) => r.header(),
        }
    }
}

impl Iterator for AnyTraceReader {
    type Item = Result<TraceEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Jsonl(r) => r.next(),
        }
    }
}
