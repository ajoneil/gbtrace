use crate::entry::TraceEntry;
use crate::error::Result;
use crate::header::TraceHeader;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

enum Output {
    Plain(BufWriter<File>),
    Gzip(BufWriter<GzEncoder<File>>),
}

impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Output::Plain(w) => w.write(buf),
            Output::Gzip(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Output::Plain(w) => w.flush(),
            Output::Gzip(w) => w.flush(),
        }
    }
}

/// Buffered writer for `.gbtrace` and `.gbtrace.gz` files.
pub struct TraceWriter {
    output: Output,
}

impl TraceWriter {
    /// Create a new writer. Uses gzip if the path ends in `.gz`.
    pub fn create(path: impl AsRef<Path>, header: &TraceHeader) -> Result<Self> {
        header.validate()?;
        let path = path.as_ref();
        let file = File::create(path)?;

        let mut output = if path.extension().is_some_and(|ext| ext == "gz") {
            Output::Gzip(BufWriter::with_capacity(
                64 * 1024,
                GzEncoder::new(file, Compression::default()),
            ))
        } else {
            Output::Plain(BufWriter::with_capacity(64 * 1024, file))
        };

        // Write header as first line
        serde_json::to_writer(&mut output, header)?;
        writeln!(output)?;

        Ok(Self { output })
    }

    /// Write a single trace entry.
    pub fn write_entry(&mut self, entry: &TraceEntry) -> Result<()> {
        serde_json::to_writer(&mut self.output, &entry.to_json_value())?;
        writeln!(self.output)?;
        Ok(())
    }

    /// Flush and finalize the output.
    pub fn finish(mut self) -> Result<()> {
        self.output.flush()?;
        // For gzip, we need to call finish() on the encoder
        if let Output::Gzip(buf) = self.output {
            let encoder = buf.into_inner().map_err(|e| e.into_error())?;
            encoder.finish()?;
        }
        Ok(())
    }
}
