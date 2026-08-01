# morepork-mame adapter
#
# Rust binary in the workspace (package morepork-mame), linking the morepork
# core crate directly — no FFI. Drives the system `mame` binary headlessly
# over its gdbstub.
#
#   make          # build ./morepork-mame
#   make clean

TARGET = morepork-mame

.PHONY: all clean

all: $(TARGET)

$(TARGET): $(wildcard src/*.rs) Cargo.toml
	cd ../.. && cargo build --release -p morepork-mame
	cp ../../target/release/morepork-mame $(TARGET)

clean:
	rm -f $(TARGET)
