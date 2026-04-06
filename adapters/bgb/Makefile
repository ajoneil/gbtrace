# gbtrace-bgb adapter
#
# BGB is a closed-source Windows emulator.  This adapter runs it under
# Wine in headless mode, capturing per-instruction state via BGB's
# debug-message breakpoint feature and writing .gbtrace via the FFI.
#
# Runtime dependencies: wine, xvfb-run, curl, unzip
# BGB is downloaded automatically on first run.

CC ?= cc
CFLAGS = -O2 -std=c11 -Wall -Wextra
FFI_DIR  ?= ../../crates/gbtrace-ffi
FFI_LIB  ?= ../../target/release/libgbtrace_ffi.a

INCLUDES = -I$(FFI_DIR)
LDLIBS = -lm -ldl -lpthread

TARGET = gbtrace-bgb

.PHONY: all clean

all: $(TARGET)

$(TARGET): gbtrace-bgb.c $(FFI_LIB)
	$(CC) $(CFLAGS) $(INCLUDES) -o $@ $< $(FFI_LIB) $(LDLIBS)

clean:
	rm -f $(TARGET)
