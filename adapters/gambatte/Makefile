# gbtrace-gambatte adapter

GAMBATTE_DIR ?= gambatte-speedrun/gambatte_core

CXX ?= g++
CXXFLAGS = -O2 -std=c++17 -Wall -Wextra -DHAVE_CSTDINT
INCLUDES = -I$(GAMBATTE_DIR)/libgambatte/include
LDLIBS = -lz

TARGET = gbtrace-gambatte
LIB = $(GAMBATTE_DIR)/libgambatte/libgambatte.a

.PHONY: all clean deps

all: $(TARGET)

# Clone gambatte-speedrun if not present
gambatte-speedrun:
	git clone --recursive https://github.com/pokemon-speedrunning/gambatte-speedrun.git

# Apply gbtrace patches and build libgambatte via scons
$(LIB): gambatte-speedrun
	bash patches/apply.sh
	cd $(GAMBATTE_DIR)/libgambatte && scons -j$$(nproc)

$(TARGET): gbtrace-gambatte.cpp $(LIB)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -o $@ $< $(LIB) $(LDLIBS)

clean:
	rm -f $(TARGET)
