# morepork-gearsystem adapter
#
# Embeds the Gearsystem core (plain C++ library, GPLv3) in SG-1000 mode
# and drives it one instruction at a time. A small checked-in patch
# (gearsystem-trace-api.patch) ports Gearcoleco's debug accessors to
# Gearsystem's Video. Links libmorepork_ffi.a to write native .morepork.
#
#   make            # clone Gearsystem (pinned), patch, build core, build ./morepork-gearsystem
#   make clean

GS_DIR ?= gearsystem
GS_URL ?= https://github.com/drhelius/Gearsystem.git
# Pinned upstream commit; bump deliberately (re-verify the patch applies).
GS_PIN  = 26024c2ce8b82aa961f5f57d714918d359162ab7
G       = $(GS_DIR)/src
MINIZ   = $(GS_DIR)/platforms/shared/dependencies/miniz
FFI_DIR ?= ../../crates/morepork-ffi
FFI_LIB ?= ../../target/release/libmorepork_ffi.a
TARGET   = morepork-gearsystem

CXX      ?= g++
CC       ?= cc
CXXFLAGS  = -std=c++17 -O2 -I$(G) -I$(MINIZ) -I$(FFI_DIR) -DGEARSYSTEM_PIN=\"$(shell echo $(GS_PIN) | cut -c1-12)\"

.PHONY: all clean link

# The core sources only exist after the clone, so `all` stages sub-makes:
# the wildcard in `link` is evaluated on a fresh parse.
all:
	$(MAKE) $(GS_DIR)/.patched
	$(MAKE) $(FFI_LIB)
	$(MAKE) link

$(GS_DIR):
	git clone $(GS_URL) $(GS_DIR)
	cd $(GS_DIR) && git checkout $(GS_PIN)

$(GS_DIR)/.patched: gearsystem-trace-api.patch | $(GS_DIR)
	cd $(GS_DIR) && git apply ../gearsystem-trace-api.patch
	touch $@

$(FFI_LIB):
	cd ../.. && cargo build --release -p morepork-ffi

CORE_SRCS = $(wildcard $(G)/*.cpp) $(wildcard $(G)/audio/*.cpp)
CORE_OBJS = $(CORE_SRCS:.cpp=.o) $(G)/audio/emu2413/emu2413.o $(MINIZ)/miniz.o

%.o: %.cpp
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(G)/audio/emu2413/emu2413.o: $(G)/audio/emu2413/emu2413.c
	$(CC) -O2 -c $< -o $@

$(MINIZ)/miniz.o: $(MINIZ)/miniz.c
	$(CC) -O2 -I$(MINIZ) -c $< -o $@

link: $(TARGET)

$(TARGET): morepork-gearsystem.cpp $(CORE_OBJS)
	$(CXX) $(CXXFLAGS) morepork-gearsystem.cpp $(CORE_OBJS) \
	  $(abspath $(FFI_LIB)) -lm -ldl -lpthread -o $(TARGET)

clean:
	rm -f $(TARGET) $(G)/*.o $(G)/audio/*.o
