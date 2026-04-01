//! Reader for the native `.gbtrace` binary format.
//!
//! `GbtraceStore` implements `TraceStore` with chunk-based lazy loading
//! and per-group decompression.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use arrow::array::*;
use arrow::array::types::UInt8Type;
use arrow::ipc::reader::StreamReader;
use arrow::record_batch::RecordBatch;

use crate::store::TraceStore;
use crate::error::{Error, Result};
use crate::header::TraceHeader;

use super::*;

/// LRU cache for decoded chunks. Each entry is a partially decoded chunk —
/// only the groups that have been accessed are decompressed.
struct ChunkCache {
    entries: Vec<(usize, DecodedChunk)>,
    capacity: usize,
}

/// A decoded chunk. Groups are decompressed on demand.
struct DecodedChunk {
    /// Decoded groups: group_id → Arrow RecordBatch for that group's fields.
    groups: HashMap<u8, RecordBatch>,
    /// Raw compressed group blobs for groups not yet decoded.
    raw_groups: HashMap<u8, Vec<u8>>,
    _entry_count: usize,
}

impl ChunkCache {
    fn new(capacity: usize) -> Self {
        Self { entries: Vec::with_capacity(capacity), capacity }
    }

    fn get(&mut self, chunk_idx: usize) -> Option<&mut DecodedChunk> {
        if let Some(pos) = self.entries.iter().position(|(k, _)| *k == chunk_idx) {
            if pos > 0 {
                let entry = self.entries.remove(pos);
                self.entries.insert(0, entry);
            }
            Some(&mut self.entries[0].1)
        } else {
            None
        }
    }

    fn insert(&mut self, chunk_idx: usize, chunk: DecodedChunk) {
        self.entries.retain(|(k, _)| *k != chunk_idx);
        if self.entries.len() >= self.capacity {
            self.entries.pop();
        }
        self.entries.insert(0, (chunk_idx, chunk));
    }
}

/// The native `.gbtrace` format store.
///
/// Loads chunks on demand with an LRU cache. Within each chunk, field
/// groups are decompressed only when accessed.
pub struct GbtraceStore {
    header: TraceHeader,
    field_index: HashMap<String, usize>,
    /// Maps field name → (group_id, index within group's RecordBatch)
    field_to_group: HashMap<String, (u8, usize)>,

    /// The full file data (for seeking to chunks/snapshots).
    data: Vec<u8>,

    /// Chunk index from footer.
    chunk_index: Vec<ChunkIndexEntry>,
    /// Cumulative entry counts for mapping global row → chunk.
    cumulative: Vec<usize>,
    /// Snapshot index from footer.
    snapshot_index: Vec<SnapshotIndexEntry>,
    total_entries: usize,

    /// LRU cache of decoded chunks.
    cache: RefCell<ChunkCache>,
}

impl GbtraceStore {
    /// Load from in-memory bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 17 { // magic(4) + ver(1) + hdr_len(4) + footer_offset(8)
            return Err(Error::InvalidHeader("file too small".into()));
        }

        // Check magic
        if &data[..4] != MAGIC {
            return Err(Error::InvalidHeader("not a .gbtrace file".into()));
        }
        let version = data[4];
        if version != VERSION {
            return Err(Error::InvalidHeader(format!("unsupported version {version}")));
        }

        // Read header
        let header_len = u32::from_le_bytes([data[5], data[6], data[7], data[8]]) as usize;
        let header_compressed = &data[9..9 + header_len];
        let header_json = zstd::decode_all(Cursor::new(header_compressed))
            .map_err(|e| Error::InvalidHeader(format!("header decompress: {e}")))?;
        let header: TraceHeader = serde_json::from_slice(&header_json)?;
        header.validate()?;

        // Read footer offset (last 8 bytes)
        let footer_offset = u64::from_le_bytes([
            data[data.len()-8], data[data.len()-7], data[data.len()-6], data[data.len()-5],
            data[data.len()-4], data[data.len()-3], data[data.len()-2], data[data.len()-1],
        ]) as usize;

        // Parse footer
        let mut pos = footer_offset;

        // Chunk index
        let num_chunks = read_u32(&data, &mut pos) as usize;
        let mut chunk_index = Vec::with_capacity(num_chunks);
        let mut cumulative = Vec::with_capacity(num_chunks);
        let mut total = 0usize;
        for _ in 0..num_chunks {
            let offset = read_u64(&data, &mut pos);
            let entry_count = read_u32(&data, &mut pos);
            chunk_index.push(ChunkIndexEntry { offset, entry_count });
            total += entry_count as usize;
            cumulative.push(total);
        }

        // Snapshot index
        let num_snapshots = read_u32(&data, &mut pos) as usize;
        let mut snapshot_index = Vec::with_capacity(num_snapshots);
        for _ in 0..num_snapshots {
            let snapshot_type = data[pos]; pos += 1;
            let entry_index = read_u64(&data, &mut pos);
            let offset = read_u64(&data, &mut pos);
            let payload_size = read_u32(&data, &mut pos);
            snapshot_index.push(SnapshotIndexEntry {
                snapshot_type,
                entry_index,
                offset,
                payload_size,
            });
        }

        let total_entries = read_u64(&data, &mut pos) as usize;

        // Build field index
        let field_index: HashMap<String, usize> = header.fields.iter()
            .enumerate()
            .map(|(i, f)| (f.clone(), i))
            .collect();

        // TODO: read group definitions from header JSON.
        // For now, derive from field names using the standard grouping.
        let groups = derive_groups(&header.fields);
        let field_to_group = build_field_to_group(&groups);

        Ok(Self {
            header,
            field_index,
            field_to_group,
            data: data.to_vec(),
            chunk_index,
            cumulative,
            snapshot_index,
            total_entries,
            cache: RefCell::new(ChunkCache::new(8)),
        })
    }

    /// Map a global row index to (chunk_index, local_offset).
    fn locate(&self, global: usize) -> (usize, usize) {
        let ci = self.cumulative.partition_point(|&end| end <= global);
        let start = if ci == 0 { 0 } else { self.cumulative[ci - 1] };
        (ci, global - start)
    }

    /// Ensure a chunk is loaded (at least the raw group blobs).
    fn ensure_chunk_loaded(&self, chunk_idx: usize) {
        let mut cache = self.cache.borrow_mut();
        if cache.get(chunk_idx).is_some() { return; }

        let chunk = self.load_chunk(chunk_idx).expect("failed to load chunk");
        cache.insert(chunk_idx, chunk);
    }

    /// Load a chunk's raw group blobs from the file data.
    fn load_chunk(&self, chunk_idx: usize) -> Result<DecodedChunk> {
        let ci = &self.chunk_index[chunk_idx];
        let mut pos = ci.offset as usize;

        let entry_count = read_u32(&self.data, &mut pos) as usize;
        let num_groups = self.data[pos] as usize;
        pos += 1;

        let mut raw_groups = HashMap::new();

        // Read group table
        struct GroupTableEntry { group_id: u8, offset: u32, compressed_size: u32 }
        let mut table = Vec::with_capacity(num_groups);
        for _ in 0..num_groups {
            let group_id = self.data[pos]; pos += 1;
            let offset = read_u32_at(&self.data, &mut pos);
            let compressed_size = read_u32_at(&self.data, &mut pos);
            let _uncompressed_size = read_u32_at(&self.data, &mut pos);
            table.push(GroupTableEntry { group_id, offset, compressed_size });
        }

        // Read group blobs
        let chunk_start = ci.offset as usize;
        for entry in &table {
            let blob_start = chunk_start + entry.offset as usize;
            let blob_end = blob_start + entry.compressed_size as usize;
            if blob_end <= self.data.len() {
                raw_groups.insert(entry.group_id, self.data[blob_start..blob_end].to_vec());
            }
        }

        Ok(DecodedChunk {
            groups: HashMap::new(),
            raw_groups,
            _entry_count: entry_count,
        })
    }

    /// Decode a specific group within a chunk if not already decoded.
    fn ensure_group_decoded(&self, chunk_idx: usize, group_id: u8) {
        self.ensure_chunk_loaded(chunk_idx);
        let mut cache = self.cache.borrow_mut();
        let chunk = cache.get(chunk_idx).unwrap();

        if chunk.groups.contains_key(&group_id) { return; }

        if let Some(compressed) = chunk.raw_groups.remove(&group_id) {
            let decompressed = zstd::decode_all(Cursor::new(&compressed))
                .expect("zstd decompress failed");

            let mut reader = StreamReader::try_new(Cursor::new(&decompressed), None)
                .expect("Arrow IPC read failed");
            if let Some(Ok(batch)) = reader.next() {
                chunk.groups.insert(group_id, batch);
            }
        }
    }

    /// Read a value from a specific field at a global row index.
    fn read_group_value(&self, field_name: &str, global_row: usize) -> Option<GroupValue> {
        let (group_id, col_in_group) = self.field_to_group.get(field_name)?;
        let (chunk_idx, local_row) = self.locate(global_row);

        self.ensure_group_decoded(chunk_idx, *group_id);

        let cache = self.cache.borrow();
        let chunk = cache.entries.iter().find(|(k, _)| *k == chunk_idx)?.1
            .groups.get(group_id)?;

        let col = chunk.column(*col_in_group);
        Some(read_arrow_value(col, local_row))
    }

    /// Get a framebuffer for a specific frame (by frame index, not snapshot index).
    pub fn framebuffer(&self, frame_idx: usize) -> Option<Vec<u8>> {
        let frame_snapshots: Vec<&SnapshotIndexEntry> = self.snapshot_index.iter()
            .filter(|s| s.snapshot_type == SnapshotType::Frame as u8)
            .collect();
        let snap = frame_snapshots.get(frame_idx)?;
        if snap.payload_size == 0 { return None; }
        self.read_snapshot_payload(snap)
    }

    /// Read and decompress a snapshot's payload.
    pub fn read_snapshot_payload(&self, snap: &SnapshotIndexEntry) -> Option<Vec<u8>> {
        if snap.payload_size == 0 { return None; }
        // In v2, snapshot payload is at offset + 17 (tag:4 + type:1 + entry_index:8 + payload_len:4)
        let payload_start = snap.offset as usize + 17;
        let payload_end = payload_start + snap.payload_size as usize;
        if payload_end > self.data.len() { return None; }
        zstd::decode_all(Cursor::new(&self.data[payload_start..payload_end])).ok()
    }

    /// Get all snapshots of a given type.
    pub fn snapshots_of_type(&self, snapshot_type: SnapshotType) -> Vec<&SnapshotIndexEntry> {
        self.snapshot_index.iter()
            .filter(|s| s.snapshot_type == snapshot_type as u8)
            .collect()
    }
}

// --- TraceStore implementation ---

impl TraceStore for GbtraceStore {
    fn header(&self) -> &TraceHeader { &self.header }
    fn entry_count(&self) -> usize { self.total_entries }

    fn field_col(&self, name: &str) -> Option<usize> {
        self.field_index.get(name).copied()
    }

    fn frame_boundaries(&self) -> Vec<u32> {
        self.snapshot_index.iter()
            .filter(|s| s.snapshot_type == SnapshotType::Frame as u8)
            .map(|s| s.entry_index as u32)
            .collect()
    }

    fn get_str(&self, col: usize, row: usize) -> String {
        let name = &self.header.fields[col];
        match self.read_group_value(name, row) {
            Some(GroupValue::Str(s)) => s,
            _ => String::new(),
        }
    }

    fn get_numeric(&self, col: usize, row: usize) -> u64 {
        let name = &self.header.fields[col];
        match self.read_group_value(name, row) {
            Some(GroupValue::Num(v)) => v,
            _ => 0,
        }
    }

    fn get_bool(&self, col: usize, row: usize) -> bool {
        let name = &self.header.fields[col];
        match self.read_group_value(name, row) {
            Some(GroupValue::Bool(v)) => v,
            _ => false,
        }
    }

    fn is_null(&self, col: usize, row: usize) -> bool {
        let name = &self.header.fields[col];
        match self.read_group_value(name, row) {
            Some(GroupValue::Null) => true,
            None => true,
            _ => false,
        }
    }

    fn get_column_segments(&self, field: &str, start: usize, end: usize) -> Option<Vec<crate::store::ColumnSegment>> {
        let (group_id, col_in_group) = self.field_to_group.get(field)?;
        let end = end.min(self.total_entries);
        if start >= end { return Some(vec![]); }

        let (first_chunk, first_offset) = self.locate(start);
        let (last_chunk, _) = self.locate(end - 1);

        let mut segments = Vec::with_capacity(last_chunk - first_chunk + 1);
        let mut remaining = end - start;

        for ci in first_chunk..=last_chunk {
            let chunk_start_global = if ci == 0 { 0 } else { self.cumulative[ci - 1] };
            let chunk_end_global = self.cumulative[ci];
            let chunk_rows = chunk_end_global - chunk_start_global;

            let local_offset = if ci == first_chunk { first_offset } else { 0 };
            let seg_len = (chunk_rows - local_offset).min(remaining);

            // Ensure group is decoded and extract the ArrayRef
            self.ensure_group_decoded(ci, *group_id);
            let cache = self.cache.borrow();
            let chunk = cache.entries.iter().find(|(k, _)| *k == ci)?.1
                .groups.get(group_id)?;
            let array = chunk.column(*col_in_group).clone(); // Arc clone, cheap

            segments.push(crate::store::ColumnSegment {
                array,
                offset: local_offset,
                len: seg_len,
            });

            remaining -= seg_len;
        }

        Some(segments)
    }
}

// --- Helpers ---

enum GroupValue {
    Num(u64),
    Bool(bool),
    Str(String),
    Null,
}

fn read_arrow_value(col: &ArrayRef, row: usize) -> GroupValue {
    if col.is_null(row) { return GroupValue::Null; }

    if let Some(arr) = col.as_any().downcast_ref::<UInt8Array>() {
        GroupValue::Num(arr.value(row) as u64)
    } else if let Some(arr) = col.as_any().downcast_ref::<UInt16Array>() {
        GroupValue::Num(arr.value(row) as u64)
    } else if let Some(arr) = col.as_any().downcast_ref::<UInt32Array>() {
        GroupValue::Num(arr.value(row) as u64)
    } else if let Some(arr) = col.as_any().downcast_ref::<UInt64Array>() {
        GroupValue::Num(arr.value(row))
    } else if let Some(arr) = col.as_any().downcast_ref::<BooleanArray>() {
        GroupValue::Bool(arr.value(row))
    } else if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
        GroupValue::Str(arr.value(row).to_string())
    } else if let Some(dict) = col.as_any().downcast_ref::<DictionaryArray<UInt8Type>>() {
        // Dictionary-encoded u8
        let values = dict.values().as_any().downcast_ref::<UInt8Array>().unwrap();
        let key = dict.keys().value(row) as usize;
        GroupValue::Num(values.value(key) as u64)
    } else {
        GroupValue::Null
    }
}

fn read_u32(data: &[u8], pos: &mut usize) -> u32 {
    let v = u32::from_le_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]);
    *pos += 4;
    v
}

fn read_u32_at(data: &[u8], pos: &mut usize) -> u32 {
    read_u32(data, pos)
}

fn read_u64(data: &[u8], pos: &mut usize) -> u64 {
    let v = u64::from_le_bytes([
        data[*pos], data[*pos+1], data[*pos+2], data[*pos+3],
        data[*pos+4], data[*pos+5], data[*pos+6], data[*pos+7],
    ]);
    *pos += 8;
    v
}

/// Derive field groups from field names using standard grouping conventions.
pub fn derive_groups_pub(fields: &[String]) -> Vec<FieldGroup> {
    derive_groups(fields)
}

fn derive_groups(fields: &[String]) -> Vec<FieldGroup> {
    let cpu_fields: Vec<String> = fields.iter()
        .filter(|f| matches!(f.as_str(), "pc"|"sp"|"a"|"f"|"b"|"c"|"d"|"e"|"h"|"l"|"op"|"ime"|"op_state"|"mcycle_phase"|"halted"|"bus_addr"))
        .cloned().collect();
    let ppu_fields: Vec<String> = fields.iter()
        .filter(|f| matches!(f.as_str(), "lcdc"|"stat"|"ly"|"lyc"|"scy"|"scx"|"wy"|"wx"|"bgp"|"obp0"|"obp1"|"dma"))
        .cloned().collect();
    let ppu_int_fields: Vec<String> = fields.iter()
        .filter(|f| f.starts_with("oam") || matches!(f.as_str(),
            "bgw_fifo_a"|"bgw_fifo_b"|"spr_fifo_a"|"spr_fifo_b"|
            "mask_pipe"|"pal_pipe"|"tfetch_state"|"sfetch_state"|
            "tile_temp_a"|"tile_temp_b"|"pix_count"|"sprite_count"|
            "scan_count"|"rendering"|"win_mode"))
        .cloned().collect();
    let pixel_fields: Vec<String> = fields.iter()
        .filter(|f| f.as_str() == "pix")
        .cloned().collect();
    let vram_fields: Vec<String> = fields.iter()
        .filter(|f| f.starts_with("vram_"))
        .cloned().collect();
    let interrupt_fields: Vec<String> = fields.iter()
        .filter(|f| matches!(f.as_str(), "if_"|"ie"))
        .cloned().collect();
    let timer_fields: Vec<String> = fields.iter()
        .filter(|f| matches!(f.as_str(), "div"|"tima"|"tma"|"tac"))
        .cloned().collect();
    let serial_fields: Vec<String> = fields.iter()
        .filter(|f| matches!(f.as_str(), "sb"|"sc"))
        .cloned().collect();

    // Collect all grouped fields to find ungrouped ones
    let mut grouped: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for v in [&cpu_fields, &ppu_fields, &ppu_int_fields, &pixel_fields,
              &vram_fields, &interrupt_fields, &timer_fields, &serial_fields] {
        for f in v { grouped.insert(f); }
    }
    let other_fields: Vec<String> = fields.iter()
        .filter(|f| !grouped.contains(f.as_str()))
        .cloned().collect();

    let mut groups = Vec::new();
    let mut add = |name: &str, fields: Vec<String>| {
        if !fields.is_empty() {
            groups.push(FieldGroup { name: name.to_string(), fields });
        }
    };

    add("cpu", cpu_fields);
    add("ppu", ppu_fields);
    add("ppu_internal", ppu_int_fields);
    add("pixel", pixel_fields);
    add("vram", vram_fields);
    add("interrupt", interrupt_fields);
    add("timer", timer_fields);
    add("serial", serial_fields);
    add("other", other_fields);

    groups
}

/// Build a mapping from field name → (group_id, column index within group).
fn build_field_to_group(groups: &[FieldGroup]) -> HashMap<String, (u8, usize)> {
    let mut map = HashMap::new();
    for (gi, group) in groups.iter().enumerate() {
        for (fi, field) in group.fields.iter().enumerate() {
            map.insert(field.clone(), (gi as u8, fi));
        }
    }
    map
}
