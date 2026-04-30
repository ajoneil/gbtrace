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

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::slice;

use gbtrace::format::write::GbtraceWriter as NativeWriter;
use gbtrace::format::read::derive_groups_pub;
use gbtrace::header::TraceHeader;
use gbtrace::profile::{FieldType, Profile};

// ---------------------------------------------------------------------------
// Profile handle
// ---------------------------------------------------------------------------

/// Opaque profile handle exposed to C.
pub struct GbtraceProfile {
    profile: Profile,
    /// Cached CStrings for field names (kept alive for pointer stability).
    field_cstrings: Vec<CString>,
    /// Cached CStrings for memory field names.
    memory_names: Vec<CString>,
    /// Memory addresses in the same order as memory_names.
    memory_addrs: Vec<u16>,
    /// Cached trigger string.
    trigger_cstring: CString,
    /// Cached name string.
    name_cstring: CString,
    /// Cached description string.
    description_cstring: CString,
}

/// Load a profile from a TOML file.
/// Returns an opaque pointer, or null on error.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_profile_load(path: *const c_char) -> *mut GbtraceProfile {
    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let profile = match Profile::load(path_str) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("gbtrace_profile_load: {e}");
            return std::ptr::null_mut();
        }
    };

    let field_cstrings: Vec<CString> = profile
        .fields
        .iter()
        .map(|f| CString::new(f.as_str()).unwrap())
        .collect();

    let memory_names: Vec<CString> = profile
        .memory
        .keys()
        .map(|k| CString::new(k.as_str()).unwrap())
        .collect();

    let memory_addrs: Vec<u16> = profile.memory.values().copied().collect();

    let trigger_str = match profile.trigger {
        gbtrace::header::Trigger::Instruction => "instruction",
        gbtrace::header::Trigger::Mcycle => "mcycle",
        gbtrace::header::Trigger::Tcycle => "tcycle",
        gbtrace::header::Trigger::Scanline => "scanline",
        gbtrace::header::Trigger::Frame => "frame",
        gbtrace::header::Trigger::Custom => "custom",
    };
    let trigger_cstring = CString::new(trigger_str).unwrap();
    let name_cstring = CString::new(profile.name.as_str()).unwrap();
    let description_cstring = CString::new(profile.description.as_str()).unwrap();

    Box::into_raw(Box::new(GbtraceProfile {
        profile,
        field_cstrings,
        memory_names,
        memory_addrs,
        trigger_cstring,
        name_cstring,
        description_cstring,
    }))
}

/// Get the profile name.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_profile_name(p: *const GbtraceProfile) -> *const c_char {
    (*p).name_cstring.as_ptr()
}

/// Get the profile description.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_profile_description(p: *const GbtraceProfile) -> *const c_char {
    (*p).description_cstring.as_ptr()
}

/// Get the trigger string (e.g. "instruction", "tcycle").
#[no_mangle]
pub unsafe extern "C" fn gbtrace_profile_trigger(p: *const GbtraceProfile) -> *const c_char {
    (*p).trigger_cstring.as_ptr()
}

/// Get the number of fields in the profile.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_profile_num_fields(p: *const GbtraceProfile) -> usize {
    (*p).profile.fields.len()
}

/// Get a field name by index. Returns null if out of bounds.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_profile_field_name(
    p: *const GbtraceProfile,
    index: usize,
) -> *const c_char {
    match (&(*p).field_cstrings).get(index) {
        Some(cs) => cs.as_ptr(),
        None => std::ptr::null(),
    }
}

/// Get the number of memory address fields.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_profile_num_memory(p: *const GbtraceProfile) -> usize {
    (*p).memory_names.len()
}

/// Get a memory field name by index. Returns null if out of bounds.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_profile_memory_name(
    p: *const GbtraceProfile,
    index: usize,
) -> *const c_char {
    match (&(*p).memory_names).get(index) {
        Some(cs) => cs.as_ptr(),
        None => std::ptr::null(),
    }
}

/// Get a memory field address by index. Returns 0 if out of bounds.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_profile_memory_addr(
    p: *const GbtraceProfile,
    index: usize,
) -> u16 {
    (&(*p).memory_addrs).get(index).copied().unwrap_or(0)
}

/// Free a profile handle.
#[no_mangle]
pub unsafe extern "C" fn gbtrace_profile_free(p: *mut GbtraceProfile) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}

// ---------------------------------------------------------------------------
// Writer handle
// ---------------------------------------------------------------------------

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
    let field_types: Vec<FieldType> = field_names.iter()
        .map(|n| header.resolve_field_type(n))
        .collect();

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
