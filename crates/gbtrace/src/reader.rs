use crate::entry::TraceEntry;
use crate::error::{Error, Result};
use crate::header::TraceHeader;
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// Streaming reader for `.gbtrace` and `.gbtrace.gz` files.
///
/// Reads entries one at a time — never loads the full file into memory.
pub struct TraceReader {
    lines: Box<dyn BufRead>,
    header: TraceHeader,
}

impl TraceReader {
    /// Open a trace file and read its header.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path)?;

        let reader: Box<dyn Read> = if path.extension().is_some_and(|ext| ext == "gz") {
            Box::new(GzDecoder::new(file))
        } else {
            Box::new(file)
        };

        let mut lines = BufReader::with_capacity(64 * 1024, reader);

        // Read header from first line
        let mut header_line = String::new();
        lines.read_line(&mut header_line)?;
        if header_line.is_empty() {
            return Err(Error::MissingHeader);
        }

        let header: TraceHeader = serde_json::from_str(&header_line)?;
        header.validate()?;

        Ok(Self {
            lines: Box::new(lines),
            header,
        })
    }

    /// Get a reference to the parsed header.
    pub fn header(&self) -> &TraceHeader {
        &self.header
    }

    /// Read the next trace entry, or `None` at end of file.
    pub fn next_entry(&mut self) -> Result<Option<TraceEntry>> {
        let mut line = String::new();
        let bytes_read = self.lines.read_line(&mut line)?;
        if bytes_read == 0 {
            return Ok(None);
        }

        let line = line.trim();
        if line.is_empty() {
            return Ok(None);
        }

        let value: serde_json::Value = serde_json::from_str(line)?;
        TraceEntry::from_json_value(&value).ok_or_else(|| {
            Error::InvalidHeader("entry is not a JSON object".into())
        })
        .map(Some)
    }
}

/// Iterator adapter over trace entries.
impl Iterator for TraceReader {
    type Item = Result<TraceEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_entry() {
            Ok(Some(entry)) => Some(Ok(entry)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}
