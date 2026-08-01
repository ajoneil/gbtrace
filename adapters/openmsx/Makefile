# morepork-openmsx adapter
#
# Rust binary in the workspace (package morepork-openmsx), linking the
# morepork core crate directly. Drives the system `openmsx` binary
# headlessly over its stdio control channel.
#
#   make          # build ./morepork-openmsx
#   make clean

TARGET = morepork-openmsx

.PHONY: all clean

all: $(TARGET)

$(TARGET): $(wildcard src/*.rs) Cargo.toml
	cd ../.. && cargo build --release -p morepork-openmsx
	cp ../../target/release/morepork-openmsx $(TARGET)

clean:
	rm -f $(TARGET)
