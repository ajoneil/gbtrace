/* gbtrace FFI — C bindings for the gbtrace trace writer.
 *
 * Link with: -lgbtrace_ffi -lm -ldl -lpthread
 *
 * Usage:
 *   1. Build the header JSON string (same format as .gbtrace header line)
 *   2. Create a writer with gbtrace_writer_new()
 *   3. Look up field indices with gbtrace_writer_find_field()
 *   4. For each trace entry:
 *      a. Call gbtrace_writer_check_boundary() with ly and pix_len
 *      b. Call gbtrace_writer_set_* for each field
 *      c. Call gbtrace_writer_finish_entry()
 *   5. Call gbtrace_writer_close() to finalize
 */
#ifndef GBTRACE_H
#define GBTRACE_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque writer handle */
typedef struct GbtraceWriter GbtraceWriter;

/* Field type constants (returned by gbtrace_writer_field_type) */
#define GBTRACE_TYPE_U8   0
#define GBTRACE_TYPE_U16  1
#define GBTRACE_TYPE_U64  2
#define GBTRACE_TYPE_BOOL 3
#define GBTRACE_TYPE_STR  4

/* Create a new trace writer.
 * path: null-terminated output file path.
 * header_json: pointer to JSON header string.
 * header_len: byte length of header_json (not null-terminated).
 * Returns writer handle, or NULL on error. */
GbtraceWriter *gbtrace_writer_new(const char *path,
                                   const char *header_json,
                                   size_t header_len);

/* Return the number of fields in the trace. */
size_t gbtrace_writer_num_fields(const GbtraceWriter *w);

/* Find the column index of a field by name. Returns -1 if not found. */
int gbtrace_writer_find_field(const GbtraceWriter *w, const char *name);

/* Get the field type for a column index.
 * Returns GBTRACE_TYPE_* constant, or -1 if invalid. */
int gbtrace_writer_field_type(const GbtraceWriter *w, size_t field);

/* Check for frame boundary before writing an entry.
 * ly: current LY value (pass 255 if not applicable).
 * pix_len: length of pix string for this entry (pass 0 if none).
 * Returns 0 on success, -1 on error. */
int gbtrace_writer_check_boundary(GbtraceWriter *w, uint8_t ly, size_t pix_len);

/* Set field values. Call one per field per entry. */
void gbtrace_writer_set_u8(GbtraceWriter *w, size_t field, uint8_t value);
void gbtrace_writer_set_u16(GbtraceWriter *w, size_t field, uint16_t value);
void gbtrace_writer_set_u64(GbtraceWriter *w, size_t field, uint64_t value);
void gbtrace_writer_set_bool(GbtraceWriter *w, size_t field, bool value);
void gbtrace_writer_set_str(GbtraceWriter *w, size_t field,
                             const char *ptr, size_t len);

/* Append a null value for a nullable field (pix, vram_addr, vram_data). */
void gbtrace_writer_set_null(GbtraceWriter *w, size_t field);

/* Mark a frame boundary at the current entry position.
 * Call at vblank. Writes boundary to metadata and flushes row group.
 * Returns 0 on success, -1 on error. */
int gbtrace_writer_mark_frame(GbtraceWriter *w);

/* Finish the current entry (after setting all fields).
 * Returns 0 on success, -1 on error. */
int gbtrace_writer_finish_entry(GbtraceWriter *w);

/* Close the writer and finalize the trace file.
 * Consumes the writer — do not use after this call.
 * Returns 0 on success, -1 on error. */
int gbtrace_writer_close(GbtraceWriter *w);

#ifdef __cplusplus
}
#endif

#endif /* GBTRACE_H */
