//! C FFI bindings for the gbtrace native format writer.
//!
//! Adapters link against libgbtrace_ffi.a and call these functions to write
//! .gbtrace files directly, bypassing JSONL serialization entirely.
//!
//! Typical usage from C:
//! ```c
//! GbtraceWriter *w = gbtrace_writer_new("out.gbtrace", header_json, len);
//! int ly_col = gbtrace_writer_find_field(w, "ly");
//! int pc_col = gbtrace_writer_find_field(w, "pc");
//! // ...
//! // For each trace entry:
//! gbtrace_writer_set_u16(w, pc_col, pc_val);
//! gbtrace_writer_set_u8(w, ly_col, ly_val);
//! // ... set all fields ...
//! gbtrace_writer_finish_entry(w);
//! // At vblank:
//! gbtrace_writer_mark_frame(w);
//! // When done:
//! gbtrace_writer_close(w);
//! ```

use std::ffi::CStr;
use std::os::raw::c_char;
use std::slice;

use gbtrace::format::write::GbtraceWriter as NativeWriter;
use gbtrace::format::read::derive_groups_pub;
use gbtrace::header::TraceHeader;
use gbtrace::profile::{field_type, FieldType};

/// Opaque writer handle exposed to C.
pub struct GbtraceWriter {
    writer: NativeWriter,
    field_names: Vec<String>,
    field_types: Vec<FieldType>,
}

/// Create a new native format writer.
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

    let groups = derive_groups_pub(&header.fields);
    let field_names = header.fields.clone();
    let field_types: Vec<FieldType> = field_names.iter().map(|n| field_type(n)).collect();

    let writer = match NativeWriter::create(path_str, &header, &groups) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("gbtrace_writer_new: failed to create writer: {e}");
            return std::ptr::null_mut();
        }
    };

    Box::into_raw(Box::new(GbtraceWriter { writer, field_names, field_types }))
}

/// Return the number of fields.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_num_fields(w: *const GbtraceWriter) -> usize {
    (*w).field_names.len()
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
    match (*w).field_names.iter().position(|n| n == name_str) {
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
    if field >= (&(*w).field_types).len() { return -1; }
    match (&(*w).field_types)[field] {
        FieldType::UInt8 => 0,
        FieldType::UInt16 => 1,
        FieldType::UInt64 => 2,
        FieldType::Bool => 3,
        FieldType::Str => 4,
    }
}

/// Legacy boundary check — no-op in native format.
/// Retained for C adapter compatibility.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_check_boundary(
    _w: *mut GbtraceWriter,
    _ly: u8,
    _pix_len: usize,
) -> i32 {
    0
}

/// Set a u8 field value.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_set_u8(
    w: *mut GbtraceWriter,
    field: usize,
    value: u8,
) {
    (*w).writer.set_u8(field, value);
}

/// Set a u16 field value.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_set_u16(
    w: *mut GbtraceWriter,
    field: usize,
    value: u16,
) {
    (*w).writer.set_u16(field, value);
}

/// Set a u64 field value.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_set_u64(
    w: *mut GbtraceWriter,
    field: usize,
    value: u64,
) {
    (*w).writer.set_u64(field, value);
}

/// Set a bool field value.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_set_bool(
    w: *mut GbtraceWriter,
    field: usize,
    value: bool,
) {
    (*w).writer.set_bool(field, value);
}

/// Append a null value for a nullable field.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_set_null(
    w: *mut GbtraceWriter,
    field: usize,
) {
    (*w).writer.set_null(field);
}

/// Set a string field value.
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
    (*w).writer.set_str(field, s);
}

/// Mark a frame boundary at the current entry position.
/// Call at vblank. Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_writer_mark_frame(w: *mut GbtraceWriter) -> i32 {
    match (*w).writer.mark_frame(None) {
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
    match (*w).writer.finish_entry() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gbtrace_writer_finish_entry: {e}");
            -1
        }
    }
}

/// Close the writer and finalize the file.
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
