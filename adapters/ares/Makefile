# morepork-ares adapter
#
# Embeds ares (static `ares` + `mia` cmake targets, cores cv/sg/msx only)
# behind a thin ares::Platform frontend. No ares source patches: the
# per-instruction tracer routes to Platform::log. Links libmorepork_ffi.a
# to write native .morepork.
#
# ares' CMake tree assumes it is the top-level project, so the build is
# two-phase: build ares standalone, then compile the frontend with the
# same flags its cores used (extract-flags.py reads the compile database).
#
#   make            # clone ares (pinned), cmake build, produce ./morepork-ares
#   make clean

ARES_DIR ?= ares
ARES_URL ?= https://github.com/ares-emulator/ares.git
# Pinned upstream commit; bump deliberately.
ARES_PIN  = b80f67d38312648d197762121c3a27b02c0887db
BUILD     = $(ARES_DIR)/build
FFI_DIR  ?= ../../crates/morepork-ffi
FFI_LIB  ?= ../../target/release/libmorepork_ffi.a
TARGET    = morepork-ares

CXX      ?= g++

.PHONY: all clean

all: $(TARGET)

$(ARES_DIR):
	git clone $(ARES_URL) $(ARES_DIR)
	cd $(ARES_DIR) && git checkout $(ARES_PIN)

$(FFI_LIB):
	cd ../.. && cargo build --release -p morepork-ffi

$(BUILD)/ares/libares.a: | $(ARES_DIR)
	cmake -S $(ARES_DIR) -B $(BUILD) -G Ninja -DCMAKE_BUILD_TYPE=Release \
	  -DARES_CORES="cv;sg;msx" -DCMAKE_EXPORT_COMPILE_COMMANDS=ON
	cmake --build $(BUILD) --target ares --target mia

$(TARGET): morepork-ares.cpp extract-flags.py $(BUILD)/ares/libares.a | $(FFI_LIB)
	$(CXX) -O2 $$(python3 extract-flags.py $(BUILD)/compile_commands.json cv/cpu/cpu.cpp mia/mia.cpp) \
	  -I$(ARES_DIR) -I$(FFI_DIR) -DARES_PIN=\"$(shell echo $(ARES_PIN) | cut -c1-12)\" \
	  morepork-ares.cpp \
	  -Wl,--start-group \
	  $(BUILD)/ares/libares.a $(BUILD)/mia/libmia.a \
	  $(BUILD)/libco/liblibco.a \
	  $(BUILD)/nall/nall/CMakeFiles/nall.dir/nall.cpp.o \
	  $(BUILD)/nall/nall/CMakeFiles/nall.dir/sljitAllocator.cpp.o \
	  $$(find $(BUILD)/thirdparty -name '*.a' 2>/dev/null) \
	  -Wl,--end-group \
	  $(abspath $(FFI_LIB)) -lm -ldl -lpthread -o $(TARGET)
	# mia resolves game databases beside the running binary (Path::program()).
	rm -rf Database && cp -r $(ARES_DIR)/mia/Database Database

clean:
	rm -rf build Database $(TARGET)
