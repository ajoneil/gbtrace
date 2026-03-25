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

#[cfg(feature = "parquet")]
pub mod parquet;

#[cfg(feature = "parquet")]
pub mod partitioned_store;

pub use column_store::{ColumnStore, EntryView};
pub use downsample::DownsampledStore;

#[cfg(feature = "parquet")]
pub use partitioned_store::PartitionedStore;
pub use diff::{AlignmentStrategy, DiffConfig, DiffResult, DivergenceClass, MultiDiffResult, TraceDiffer};
pub use entry::TraceEntry;
pub use error::Error;
pub use header::{BootRom, CycleUnit, TraceHeader, Trigger};
pub use profile::{FieldType, Profile};
pub use query::{Condition, ConditionEvaluator};
pub use reader::TraceReader;
pub use writer::TraceWriter;

#[cfg(feature = "parquet")]
pub use parquet::{ParquetTraceReader, ParquetTraceWriter};

use error::Result;
use std::path::Path;

/// Format-agnostic trace reader. Detects format from file extension.
pub enum AnyTraceReader {
    Jsonl(TraceReader),
    #[cfg(feature = "parquet")]
    Parquet(ParquetTraceReader),
}

impl AnyTraceReader {
    /// Open a trace file, detecting format from extension.
    /// `.parquet` -> Parquet reader, everything else -> JSONL reader.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        #[cfg(feature = "parquet")]
        if path.extension().is_some_and(|ext| ext == "parquet") {
            return Ok(Self::Parquet(ParquetTraceReader::open(path)?));
        }
        Ok(Self::Jsonl(TraceReader::open(path)?))
    }

    pub fn header(&self) -> &TraceHeader {
        match self {
            Self::Jsonl(r) => r.header(),
            #[cfg(feature = "parquet")]
            Self::Parquet(r) => r.header(),
        }
    }
}

impl Iterator for AnyTraceReader {
    type Item = Result<TraceEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Jsonl(r) => r.next(),
            #[cfg(feature = "parquet")]
            Self::Parquet(r) => r.next(),
        }
    }
}
