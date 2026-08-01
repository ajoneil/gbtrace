# morepork-gearcoleco adapter
#
# Embeds the Gearcoleco core (plain C++ library, GPLv3) and drives it one
# instruction at a time — no patches needed, the core exposes the full Z80
# and VDP state publicly. Links libmorepork_ffi.a to write native
# .morepork.
#
#   make            # clone Gearcoleco (pinned), build core, build ./morepork-gearcoleco
#   make clean

GC_DIR ?= gearcoleco
GC_URL ?= https://github.com/drhelius/Gearcoleco.git
# Pinned upstream commit; bump deliberately.
GC_PIN  = e8cf314e441ac7d507add9ee4eba03593b572910
G       = $(GC_DIR)/src
FFI_DIR ?= ../../crates/morepork-ffi
FFI_LIB ?= ../../target/release/libmorepork_ffi.a
TARGET   = morepork-gearcoleco

MINIZ    = $(GC_DIR)/platforms/shared/dependencies/miniz
CXX      ?= g++
CC       ?= cc
CXXFLAGS  = -std=c++17 -O2 -I$(G) -I$(MINIZ) -I$(FFI_DIR) -DGEARCOLECO_PIN=\"$(shell echo $(GC_PIN) | cut -c1-12)\"

.PHONY: all clean link

# The core sources only exist after the clone, so `all` stages sub-makes:
# the wildcard in `link` is evaluated on a fresh parse.
all:
	$(MAKE) $(GC_DIR)
	$(MAKE) $(FFI_LIB)
	$(MAKE) link

$(GC_DIR):
	git clone $(GC_URL) $(GC_DIR)
	cd $(GC_DIR) && git checkout $(GC_PIN)

$(FFI_LIB):
	cd ../.. && cargo build --release -p morepork-ffi

CORE_SRCS = $(wildcard $(G)/*.cpp) $(wildcard $(G)/audio/*.cpp)
CORE_OBJS = $(CORE_SRCS:.cpp=.o) $(MINIZ)/miniz.o

%.o: %.cpp
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(MINIZ)/miniz.o: $(MINIZ)/miniz.c
	$(CC) -O2 -I$(MINIZ) -c $< -o $@

link: $(TARGET)

$(TARGET): morepork-gearcoleco.cpp $(CORE_OBJS)
	$(CXX) $(CXXFLAGS) morepork-gearcoleco.cpp $(CORE_OBJS) \
	  $(abspath $(FFI_LIB)) -lm -ldl -lpthread -o $(TARGET)

clean:
	rm -f $(TARGET) $(G)/*.o $(G)/audio/*.o
