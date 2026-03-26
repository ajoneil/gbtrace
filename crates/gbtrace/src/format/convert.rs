//! Convert from any supported input format to the native `.gbtrace` format.
//!
//! Supports: JSONL, parquet (legacy), and existing `.gbtrace` files.
//! Produces a `GbtraceStore` either from a file or from in-memory bytes.

use crate::column_store::TraceStore;
use crate::entry::TraceEntry;
use crate::error::Result;
use crate::header::TraceHeader;
use crate::profile::{field_type, field_nullable, FieldType};

use super::read::{derive_groups_pub, GbtraceStore};
use super::write::GbtraceWriter;

/// Convert JSONL bytes to a `GbtraceStore` (in-memory).
/// Writes to a temp file, then loads it back.
pub fn jsonl_to_store(data: &[u8]) -> Result<GbtraceStore> {
    let owned = data.to_vec();
    let reader = crate::reader::TraceReader::from_reader(std::io::Cursor::new(owned))?;
    let header = reader.header().clone();
    reader_to_store(header, reader)
}

/// Convert a JSONL file to a `GbtraceStore`.
pub fn jsonl_file_to_store(path: &std::path::Path) -> Result<GbtraceStore> {
    let reader = crate::reader::TraceReader::open(path)?;
    let header = reader.header().clone();
    reader_to_store(header, reader)
}

/// Convert any `TraceEntry` iterator + header into a `GbtraceStore`.
fn reader_to_store(
    header: TraceHeader,
    entries: impl Iterator<Item = Result<TraceEntry>>,
) -> Result<GbtraceStore> {
    let tmp = tempfile::NamedTempFile::new()?;
    let tmp_path = tmp.path().to_path_buf();

    let groups = derive_groups_pub(&header.fields);

    {
        let mut writer = GbtraceWriter::create(&tmp_path, &header, &groups)?;

        for result in entries {
            let entry = result?;
            write_entry_to_gbtrace(&mut writer, &entry, &header)?;
            writer.finish_entry()?;
        }

        writer.finish()?;
    }

    let data = std::fs::read(&tmp_path)?;
    GbtraceStore::from_bytes(&data)
}

/// Write a single `TraceEntry` to a `GbtraceWriter`, handling types and nullability.
pub fn write_entry_to_gbtrace(
    writer: &mut GbtraceWriter,
    entry: &TraceEntry,
    header: &TraceHeader,
) -> Result<()> {
    for (col, name) in header.fields.iter().enumerate() {
        let val = entry.get(name);
        let ft = field_type(name);
        let nullable = field_nullable(name);

        if nullable && val.is_none() {
            writer.set_null(col);
            continue;
        }

        match ft {
            FieldType::UInt64 => {
                writer.set_u64(col, val.and_then(|v| v.as_u64()).unwrap_or(0));
            }
            FieldType::UInt16 => {
                let v = parse_numeric(val) as u16;
                if nullable && v == 0 { writer.set_null(col); }
                else { writer.set_u16(col, v); }
            }
            FieldType::UInt8 => {
                let v = parse_numeric(val) as u8;
                if nullable && v == 0 { writer.set_null(col); }
                else { writer.set_u8(col, v); }
            }
            FieldType::Bool => {
                writer.set_bool(col, val.and_then(|v| v.as_bool()).unwrap_or(false));
            }
            FieldType::Str => {
                let s = val.and_then(|v| v.as_str()).unwrap_or("");
                if nullable && s.is_empty() { writer.set_null(col); }
                else { writer.set_str(col, s); }
            }
        }
    }
    Ok(())
}

fn parse_numeric(val: Option<&serde_json::Value>) -> u64 {
    val.and_then(|v| {
        v.as_u64().or_else(|| {
            v.as_str().and_then(|s| {
                let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
                u64::from_str_radix(s, 16).ok()
            })
        })
    }).unwrap_or(0)
}
