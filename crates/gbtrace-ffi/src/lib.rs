//! C FFI bindings for the gbtrace parquet writer.
//!
//! Adapters link against libgbtrace_ffi.a and call these functions to write
//! parquet files directly, bypassing JSONL serialization entirely.
//!
//! Typical usage from C:
//! ```c
//! GbtraceWriter *w = gbtrace_writer_new("out.parquet", header_json, len);
//! int ly_col = gbtrace_writer_find_field(w, "ly");
//! int pc_col = gbtrace_writer_find_field(w, "pc");
//! // ...
//! // For each trace entry:
//! gbtrace_writer_check_boundary(w, ly_val, pix_len);
//! gbtrace_writer_set_u16(w, pc_col, pc_val);
//! gbtrace_writer_set_u8(w, ly_col, ly_val);
//! // ... set all fields ...
//! gbtrace_writer_finish_entry(w);
//! // When done:
//! gbtrace_writer_close(w);
//! ```

use std::ffi::CStr;
use std::os::raw::c_char;
use std::slice;

use gbtrace::header::TraceHeader;
use gbtrace::parquet::ParquetTraceWriter;
use gbtrace::profile::FieldType;

/// Opaque writer handle exposed to C.
pub struct GbtraceWriter {
    writer: ParquetTraceWriter,
}

/// Create a new parquet writer.
///
/// `path` is a null-terminated C string for the output file path.
/// `header_json` + `header_len` describe the header JSON (not null-terminated).
/// Returns an opaque pointer, or null on error.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_new(
    path: *const c_char,
    header_json: *const c_char,
    header_len: usize,
) -> *mut GbtraceWriter {
    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let json_bytes = slice::from_raw_parts(header_json as *const u8, header_len);
    let json_str = match std::str::from_utf8(json_bytes) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let header: TraceHeader = match serde_json::from_str(json_str) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("gbtrace_writer_new: failed to parse header: {e}");
            return std::ptr::null_mut();
        }
    };

    let writer = match ParquetTraceWriter::create(path_str, &header) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("gbtrace_writer_new: failed to create writer: {e}");
            return std::ptr::null_mut();
        }
    };

    Box::into_raw(Box::new(GbtraceWriter { writer }))
}

/// Return the number of fields.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_num_fields(w: *const GbtraceWriter) -> usize {
    (*w).writer.field_names().len()
}

/// Find the column index of a field by name. Returns -1 if not found.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_find_field(
    w: *const GbtraceWriter,
    name: *const c_char,
) -> i32 {
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match (*w).writer.field_names().iter().position(|n| n == name_str) {
        Some(i) => i as i32,
        None => -1,
    }
}

/// Get the field type for a column index.
/// Returns: 0=u8, 1=u16, 2=u64, 3=bool, 4=str, -1=invalid
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_field_type(
    w: *const GbtraceWriter,
    field: usize,
) -> i32 {
    let types = (*w).writer.field_types();
    if field >= types.len() {
        return -1;
    }
    match types[field] {
        FieldType::UInt8 => 0,
        FieldType::UInt16 => 1,
        FieldType::UInt64 => 2,
        FieldType::Bool => 3,
        FieldType::Str => 4,
    }
}

/// Check for frame boundary (call BEFORE setting field values for each entry).
/// `ly` is the current LY value (pass 255 if LY is not in this entry).
/// `pix_len` is the length of the pix string (pass 0 if no pix data).
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_check_boundary(
    w: *mut GbtraceWriter,
    ly: u8,
    pix_len: usize,
) -> i32 {
    let ly_opt = if ly == 255 { None } else { Some(ly) };
    match (*w).writer.check_boundary(ly_opt, pix_len) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gbtrace_writer_check_boundary: {e}");
            -1
        }
    }
}

/// Set a u8 field value and append it to the column buffer.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_set_u8(
    w: *mut GbtraceWriter,
    field: usize,
    value: u8,
) {
    (*w).writer.append_u8(field, value);
}

/// Set a u16 field value and append it to the column buffer.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_set_u16(
    w: *mut GbtraceWriter,
    field: usize,
    value: u16,
) {
    (*w).writer.append_u16(field, value);
}

/// Set a u64 field value and append it to the column buffer.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_set_u64(
    w: *mut GbtraceWriter,
    field: usize,
    value: u64,
) {
    (*w).writer.append_u64(field, value);
}

/// Set a bool field value and append it to the column buffer.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_set_bool(
    w: *mut GbtraceWriter,
    field: usize,
    value: bool,
) {
    (*w).writer.append_bool(field, value);
}

/// Set a string field value and append it to the column buffer.
/// `ptr` and `len` describe the UTF-8 string (not null-terminated).
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_set_str(
    w: *mut GbtraceWriter,
    field: usize,
    ptr: *const c_char,
    len: usize,
) {
    let bytes = slice::from_raw_parts(ptr as *const u8, len);
    let s = std::str::from_utf8_unchecked(bytes);
    (*w).writer.append_str(field, s);
}

/// Mark a frame boundary at the current entry position.
/// Call at vblank — writes the boundary to parquet metadata and flushes.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_mark_frame(w: *mut GbtraceWriter) -> i32 {
    match (*w).writer.mark_frame() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gbtrace_writer_mark_frame: {e}");
            -1
        }
    }
}

/// Finish the current entry (call after setting all fields).
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_finish_entry(w: *mut GbtraceWriter) -> i32 {
    match (*w).writer.finish_row() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gbtrace_writer_finish_entry: {e}");
            -1
        }
    }
}

/// Close the writer and finalize the parquet file.
/// Consumes the writer — do not use it after this call.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_close(w: *mut GbtraceWriter) -> i32 {
    let w = Box::from_raw(w);
    match w.writer.finish() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gbtrace_writer_close: {e}");
            -1
        }
    }
}
