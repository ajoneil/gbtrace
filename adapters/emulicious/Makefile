# morepork-emulicious adapter
#
# Rust binary in the workspace (package morepork-emulicious), linking the
# morepork core crate directly. Drives Emulicious (Java) over its DAP
# remote-debugging server.
#
#   make          # build ./morepork-emulicious
#   make clean

TARGET = morepork-emulicious

.PHONY: all clean

all: $(TARGET)

$(TARGET): $(wildcard src/*.rs) Cargo.toml
	cd ../.. && cargo build --release -p morepork-emulicious
	cp ../../target/release/morepork-emulicious $(TARGET)

clean:
	rm -f $(TARGET)
