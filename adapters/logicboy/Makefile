# gbtrace-logicboy adapter
#
# Uses the LogicBoy simulation from metroboy to produce .gbtrace files.
# LogicBoy is a fast, logic-level Game Boy simulation (as opposed to the
# gate-level GateBoy).
#
# Dependencies are cloned automatically on first build:
#   make
#
# Or point at existing checkouts:
#   make METROBOY_DIR=/path/to/metroboy METROLIB_DIR=/path/to/metrolib

METROBOY_DIR ?= metroboy
METROLIB_DIR ?= metrolib

CXX      ?= g++
CXXFLAGS  = -O2 -std=c++20 -Wall -Wextra -Wno-unused-parameter \
            -DCONFIG_RELEASE
INCLUDES  = -I$(METROBOY_DIR)/src -I$(METROBOY_DIR)/symlinks \
            -I$(METROBOY_DIR)/symlinks/metrolib

TARGET = gbtrace-logicboy

# ---------------------------------------------------------------------------
# Source lists
# ---------------------------------------------------------------------------

# All GateBoyLib .cpp files, minus the ones we don't need (thread, pair,
# dumper require SDL / threading / display infrastructure).
GATEBOYLIB_EXCLUDE = GateBoyThread.cpp GateBoyPair.cpp GateBoyDumper.cpp
GATEBOYLIB_ALL     = $(wildcard $(METROBOY_DIR)/src/GateBoyLib/*.cpp)
GATEBOYLIB_SRCS    = $(filter-out $(addprefix $(METROBOY_DIR)/src/GateBoyLib/,$(GATEBOYLIB_EXCLUDE)),$(GATEBOYLIB_ALL))

# Metrolib sources we need (skip Audio.cpp – requires SDL2, and GBBlitter –
# requires OpenGL).
METROLIB_CORE_SRCS = $(wildcard $(METROLIB_DIR)/metrolib/core/*.cpp)
METROLIB_GB_SRCS   = $(METROLIB_DIR)/metrolib/gameboy/MetroBoyCPU.cpp \
                     $(METROLIB_DIR)/metrolib/gameboy/Assembler.cpp \
                     $(METROLIB_DIR)/metrolib/gameboy/GBConstants.cpp

ALL_LIB_SRCS = $(GATEBOYLIB_SRCS) $(METROLIB_CORE_SRCS) $(METROLIB_GB_SRCS)
ALL_LIB_OBJS = $(patsubst %.cpp,build/%.o,$(ALL_LIB_SRCS))

ADAPTER_OBJ = build/gbtrace-logicboy.o
AUDIO_STUB_OBJ = build/audio_stub.o

# ---------------------------------------------------------------------------
# Targets
# ---------------------------------------------------------------------------

.PHONY: all clean deps

all: $(TARGET)

# Clone repos and set up symlinks if needed
deps:
	@if [ ! -d "$(METROBOY_DIR)" ]; then \
		echo "Cloning metroboy..."; \
		git clone --depth 1 https://github.com/aappleby/metroboy.git $(METROBOY_DIR); \
	fi
	@if [ ! -d "$(METROLIB_DIR)" ]; then \
		echo "Cloning metrolib..."; \
		git clone --depth 1 https://github.com/aappleby/metrolib.git $(METROLIB_DIR); \
	fi
	@mkdir -p $(METROBOY_DIR)/symlinks
	@if [ ! -e "$(METROBOY_DIR)/symlinks/metrolib" ]; then \
		ln -sf $(realpath $(METROLIB_DIR)) $(METROBOY_DIR)/symlinks/metrolib; \
	fi

$(TARGET): deps $(ALL_LIB_OBJS) $(ADAPTER_OBJ) $(AUDIO_STUB_OBJ)
	$(CXX) $(CXXFLAGS) -o $@ $(ALL_LIB_OBJS) $(ADAPTER_OBJ) $(AUDIO_STUB_OBJ) -lpthread

# Adapter source
build/gbtrace-logicboy.o: gbtrace-logicboy.cpp | deps
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -c -o $@ $<

# Audio stub (provides symbols that Audio.h declares but we don't link Audio.cpp)
build/audio_stub.o: audio_stub.cpp | deps
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -c -o $@ $<

# GateBoyLib objects
build/$(METROBOY_DIR)/src/GateBoyLib/%.o: $(METROBOY_DIR)/src/GateBoyLib/%.cpp | deps
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -c -o $@ $<

# Metrolib core objects
build/$(METROLIB_DIR)/metrolib/core/%.o: $(METROLIB_DIR)/metrolib/core/%.cpp | deps
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -c -o $@ $<

# Metrolib gameboy objects
build/$(METROLIB_DIR)/metrolib/gameboy/%.o: $(METROLIB_DIR)/metrolib/gameboy/%.cpp | deps
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -c -o $@ $<

clean:
	rm -rf build $(TARGET)
