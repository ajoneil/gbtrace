.PHONY: all clean

RELEASE ?= 1
CARGO_FLAGS = $(if $(filter 1,$(RELEASE)),--release,)
TARGET_DIR = $(if $(filter 1,$(RELEASE)),release,debug)

all: gbtrace-missingno

gbtrace-missingno: src/main.rs Cargo.toml
	cargo build $(CARGO_FLAGS) --manifest-path Cargo.toml
	cp target/$(TARGET_DIR)/gbtrace-missingno .

clean:
	cargo clean --manifest-path Cargo.toml
	rm -f gbtrace-missingno
