//! Parquet read/write support for `.gbtrace` trace files.
//!
//! Stores fields as native integer types (UInt8, UInt16, UInt64) instead of
//! hex strings, and preserves the trace header as file-level metadata.

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{
    ArrayBuilder, ArrayRef, BooleanArray, BooleanBuilder, RecordBatch, StringArray,
    StringBuilder, UInt16Array, UInt16Builder, UInt64Array, UInt64Builder, UInt8Array,
    UInt8Builder,
};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;

use crate::entry::TraceEntry;
use crate::error::{Error, Result};
use crate::header::TraceHeader;
use crate::profile::{field_type, FieldType};

const HEADER_METADATA_KEY: &str = "gbtrace_header";
const BATCH_SIZE: usize = 65536;

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

enum ColumnBuffer {
    UInt64(UInt64Builder),
    UInt16(UInt16Builder),
    UInt8(UInt8Builder),
    Bool(BooleanBuilder),
    Str(StringBuilder),
}

impl ColumnBuffer {
    fn new(ft: FieldType) -> Self {
        match ft {
            FieldType::UInt64 => Self::UInt64(UInt64Builder::with_capacity(BATCH_SIZE)),
            FieldType::UInt16 => Self::UInt16(UInt16Builder::with_capacity(BATCH_SIZE)),
            FieldType::UInt8 => Self::UInt8(UInt8Builder::with_capacity(BATCH_SIZE)),
            FieldType::Bool => Self::Bool(BooleanBuilder::with_capacity(BATCH_SIZE)),
            FieldType::Str => Self::Str(StringBuilder::with_capacity(BATCH_SIZE, BATCH_SIZE * 4)),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::UInt64(b) => b.len(),
            Self::UInt16(b) => b.len(),
            Self::UInt8(b) => b.len(),
            Self::Bool(b) => b.len(),
            Self::Str(b) => b.len(),
        }
    }

    fn finish(&mut self) -> ArrayRef {
        match self {
            Self::UInt64(b) => Arc::new(b.finish()),
            Self::UInt16(b) => Arc::new(b.finish()),
            Self::UInt8(b) => Arc::new(b.finish()),
            Self::Bool(b) => Arc::new(b.finish()),
            Self::Str(b) => Arc::new(b.finish()),
        }
    }
}

/// Writes trace entries to a Parquet file with native integer types.
///
/// When the trace includes an `ly` field, row groups are flushed at frame
/// boundaries (when `ly` wraps from 153→0). When the trace includes full-frame
/// `pix` dumps (23040 chars), each dump also starts a new row group.
/// This allows readers to decode individual frames on demand.
pub struct ParquetTraceWriter {
    writer: ArrowWriter<File>,
    schema: Arc<Schema>,
    columns: Vec<ColumnBuffer>,
    field_names: Vec<String>,
    field_types: Vec<FieldType>,
    /// Column index of the `ly` field, if present.
    ly_col: Option<usize>,
    /// Column index of the `pix` field, if present.
    pix_col: Option<usize>,
    /// Previous value of `ly`, for detecting frame boundaries.
    prev_ly: Option<u8>,
}

impl ParquetTraceWriter {
    /// Create a new Parquet writer. The header is stored in file metadata.
    pub fn create(path: impl AsRef<Path>, header: &TraceHeader) -> Result<Self> {
        header.validate()?;

        let field_names = header.fields.clone();
        let field_types: Vec<FieldType> = field_names.iter().map(|n| field_type(n)).collect();

        // Build Arrow schema
        let arrow_fields: Vec<Field> = field_names
            .iter()
            .zip(&field_types)
            .map(|(name, ft)| {
                let dt = match ft {
                    FieldType::UInt64 => DataType::UInt64,
                    FieldType::UInt16 => DataType::UInt16,
                    FieldType::UInt8 => DataType::UInt8,
                    FieldType::Bool => DataType::Boolean,
                    FieldType::Str => DataType::Utf8,
                };
                Field::new(name, dt, false)
            })
            .collect();

        let mut metadata = HashMap::new();
        metadata.insert(
            HEADER_METADATA_KEY.to_string(),
            serde_json::to_string(header)?,
        );

        let schema = Arc::new(Schema::new_with_metadata(arrow_fields, metadata));

        let file = File::create(path.as_ref())?;
        let props = WriterProperties::builder()
            .set_compression(Compression::ZSTD(ZstdLevel::try_new(3).unwrap()))
            .set_max_row_group_size(usize::MAX) // only flush row groups explicitly
            .build();

        let writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;
        let columns: Vec<ColumnBuffer> = field_types.iter().map(|ft| ColumnBuffer::new(*ft)).collect();

        let ly_col = field_names.iter().position(|n| n == "ly");
        let pix_col = field_names.iter().position(|n| n == "pix");

        Ok(Self {
            writer,
            schema,
            columns,
            field_names,
            field_types,
            ly_col,
            pix_col,
            prev_ly: None,
        })
    }

    /// Write a single trace entry. Entries are buffered and flushed in batches.
    ///
    /// When the trace includes `ly`, a new row group is started at each frame
    /// boundary (ly wraps 153→0). Otherwise, row groups are flushed every
    /// BATCH_SIZE entries.
    pub fn write_entry(&mut self, entry: &TraceEntry) -> Result<()> {
        // Detect frame boundary before appending (so the new frame starts
        // a fresh row group).
        let mut boundary = false;
        if let Some(ly_idx) = self.ly_col {
            let cur_ly = parse_u8(entry.get(&self.field_names[ly_idx]));
            if let Some(prev) = self.prev_ly {
                if prev == 153 && cur_ly == 0 {
                    boundary = true;
                }
            }
            self.prev_ly = Some(cur_ly);
        }
        if let Some(pix_idx) = self.pix_col {
            if let Some(val) = entry.get(&self.field_names[pix_idx]) {
                if let Some(s) = val.as_str() {
                    if s.len() == 160 * 144 {
                        boundary = true;
                    }
                }
            }
        }
        if boundary {
            self.flush_batch()?;
            self.writer.flush()?;
        }

        for (i, (name, ft)) in self.field_names.iter().zip(&self.field_types).enumerate() {
            let val = entry.get(name);
            match (&mut self.columns[i], ft) {
                (ColumnBuffer::UInt64(b), FieldType::UInt64) => {
                    b.append_value(val.and_then(|v| v.as_u64()).unwrap_or(0));
                }
                (ColumnBuffer::UInt16(b), FieldType::UInt16) => {
                    b.append_value(parse_u16(val));
                }
                (ColumnBuffer::UInt8(b), FieldType::UInt8) => {
                    b.append_value(parse_u8(val));
                }
                (ColumnBuffer::Bool(b), FieldType::Bool) => {
                    b.append_value(val.and_then(|v| v.as_bool()).unwrap_or(false));
                }
                (ColumnBuffer::Str(b), FieldType::Str) => {
                    b.append_value(val.and_then(|v| v.as_str()).unwrap_or(""));
                }
                _ => unreachable!(),
            }
        }

        if self.columns[0].len() >= BATCH_SIZE {
            self.flush_batch()?;
        }

        Ok(())
    }

    // --- Direct typed access for FFI (bypasses TraceEntry) ---

    /// Signal a frame boundary check with the given ly and pix_len values.
    /// Call this BEFORE appending column values for the entry.
    /// If a boundary is detected, the current batch is flushed.
    pub fn check_boundary(&mut self, ly: Option<u8>, pix_len: usize) -> Result<()> {
        let mut boundary = false;
        if self.ly_col.is_some() {
            if let Some(cur_ly) = ly {
                if let Some(prev) = self.prev_ly {
                    if prev == 153 && cur_ly == 0 {
                        boundary = true;
                    }
                }
                self.prev_ly = Some(cur_ly);
            }
        }
        if self.pix_col.is_some() && pix_len == 160 * 144 {
            boundary = true;
        }
        if boundary {
            self.flush_batch()?;
            self.writer.flush()?;
        }
        Ok(())
    }

    /// Append a u8 value to the given column index.
    pub fn append_u8(&mut self, col: usize, val: u8) {
        if let ColumnBuffer::UInt8(b) = &mut self.columns[col] {
            b.append_value(val);
        }
    }

    /// Append a u16 value to the given column index.
    pub fn append_u16(&mut self, col: usize, val: u16) {
        if let ColumnBuffer::UInt16(b) = &mut self.columns[col] {
            b.append_value(val);
        }
    }

    /// Append a u64 value to the given column index.
    pub fn append_u64(&mut self, col: usize, val: u64) {
        if let ColumnBuffer::UInt64(b) = &mut self.columns[col] {
            b.append_value(val);
        }
    }

    /// Append a bool value to the given column index.
    pub fn append_bool(&mut self, col: usize, val: bool) {
        if let ColumnBuffer::Bool(b) = &mut self.columns[col] {
            b.append_value(val);
        }
    }

    /// Append a string value to the given column index.
    pub fn append_str(&mut self, col: usize, val: &str) {
        if let ColumnBuffer::Str(b) = &mut self.columns[col] {
            b.append_value(val);
        }
    }

    /// Call after appending all columns for one entry. Flushes batch if full.
    pub fn finish_row(&mut self) -> Result<()> {
        if self.columns[0].len() >= BATCH_SIZE {
            self.flush_batch()?;
        }
        Ok(())
    }

    /// Get field types (for FFI to know which setter to call per column).
    pub fn field_types(&self) -> &[FieldType] {
        &self.field_types
    }

    /// Get field names.
    pub fn field_names(&self) -> &[String] {
        &self.field_names
    }

    fn flush_batch(&mut self) -> Result<()> {
        if self.columns.is_empty() || self.columns[0].len() == 0 {
            return Ok(());
        }

        let arrays: Vec<ArrayRef> = self.columns.iter_mut().map(|c| c.finish()).collect();
        let batch = RecordBatch::try_new(self.schema.clone(), arrays)?;
        self.writer.write(&batch)?;

        // Re-create builders
        self.columns = self.field_types.iter().map(|ft| ColumnBuffer::new(*ft)).collect();
        Ok(())
    }

    /// Flush remaining entries and finalize the file.
    pub fn finish(mut self) -> Result<()> {
        self.flush_batch()?;
        self.writer.close()?;
        Ok(())
    }
}

fn parse_u16(val: Option<&serde_json::Value>) -> u16 {
    val.and_then(|v| v.as_u64()).map(|n| n as u16).unwrap_or(0)
}

fn parse_u8(val: Option<&serde_json::Value>) -> u8 {
    val.and_then(|v| v.as_u64()).map(|n| n as u8).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// Reads trace entries from a Parquet file, converting native integers back
/// to the TraceEntry format (hex strings for registers, u64 for cycles).
pub struct ParquetTraceReader {
    header: TraceHeader,
    field_types: Vec<FieldType>,
    /// Flattened rows from the current batch
    current_rows: Vec<TraceEntry>,
    /// Index into current_rows
    row_idx: usize,
    /// Arrow record batch reader
    batch_reader: Box<dyn Iterator<Item = std::result::Result<RecordBatch, arrow::error::ArrowError>>>,
}

impl ParquetTraceReader {
    /// Open a Parquet trace file and read its header from file metadata.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path.as_ref())?;
        Self::from_chunk_reader(file)
    }

    /// Load from in-memory bytes. Useful for WASM where filesystem isn't available.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        Self::from_chunk_reader(bytes::Bytes::from(data))
    }

    fn from_chunk_reader<R: parquet::file::reader::ChunkReader + 'static>(reader: R) -> Result<Self> {
        let builder = ParquetRecordBatchReaderBuilder::try_new(reader)?;

        // Extract header from Arrow schema metadata (preserved in Parquet file)
        let schema = builder.schema();
        let kv = schema
            .metadata()
            .get(HEADER_METADATA_KEY)
            .ok_or_else(|| Error::MissingHeader)?;

        let header: TraceHeader = serde_json::from_str(kv)?;
        header.validate()?;

        let field_types: Vec<FieldType> = header.fields.iter().map(|n| field_type(n)).collect();

        let batch_reader = Box::new(builder.with_batch_size(BATCH_SIZE).build()?);

        Ok(Self {
            header,
            field_types,
            current_rows: Vec::new(),
            row_idx: 0,
            batch_reader,
        })
    }

    pub fn header(&self) -> &TraceHeader {
        &self.header
    }

    fn load_next_batch(&mut self) -> Result<bool> {
        match self.batch_reader.next() {
            Some(Ok(batch)) => {
                self.current_rows = batch_to_entries(&batch, &self.header.fields, &self.field_types)?;
                self.row_idx = 0;
                Ok(true)
            }
            Some(Err(e)) => Err(Error::Arrow(e)),
            None => Ok(false),
        }
    }
}

impl Iterator for ParquetTraceReader {
    type Item = Result<TraceEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.row_idx >= self.current_rows.len() {
            match self.load_next_batch() {
                Ok(true) => {}
                Ok(false) => return None,
                Err(e) => return Some(Err(e)),
            }
        }

        if self.row_idx < self.current_rows.len() {
            let entry = self.current_rows[self.row_idx].clone();
            self.row_idx += 1;
            Some(Ok(entry))
        } else {
            None
        }
    }
}

fn batch_to_entries(
    batch: &RecordBatch,
    field_names: &[String],
    field_types: &[FieldType],
) -> Result<Vec<TraceEntry>> {
    let num_rows = batch.num_rows();
    let mut entries: Vec<TraceEntry> = (0..num_rows).map(|_| TraceEntry::new()).collect();

    for (col_idx, (name, ft)) in field_names.iter().zip(field_types).enumerate() {
        let col = batch.column(col_idx);
        match ft {
            FieldType::UInt64 => {
                let arr = col.as_any().downcast_ref::<UInt64Array>().unwrap();
                for (row, entry) in entries.iter_mut().enumerate() {
                    entry.set_cy(arr.value(row));
                }
            }
            FieldType::UInt16 => {
                let arr = col.as_any().downcast_ref::<UInt16Array>().unwrap();
                for (row, entry) in entries.iter_mut().enumerate() {
                    entry.set_u16(name, arr.value(row));
                }
            }
            FieldType::UInt8 => {
                let arr = col.as_any().downcast_ref::<UInt8Array>().unwrap();
                for (row, entry) in entries.iter_mut().enumerate() {
                    entry.set_u8(name, arr.value(row));
                }
            }
            FieldType::Bool => {
                let arr = col.as_any().downcast_ref::<BooleanArray>().unwrap();
                for (row, entry) in entries.iter_mut().enumerate() {
                    entry.set_bool(name, arr.value(row));
                }
            }
            FieldType::Str => {
                let arr = col.as_any().downcast_ref::<StringArray>().unwrap();
                for (row, entry) in entries.iter_mut().enumerate() {
                    entry.set_str(name, arr.value(row));
                }
            }
        }
    }

    Ok(entries)
}
