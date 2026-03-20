# gbtrace-gambatte adapter
#
# Requires gambatte-speedrun to be built as a library first:
#   git clone --recursive https://github.com/pokemon-speedrunning/gambatte-speedrun.git
#   cd gambatte-speedrun/gambatte_core/libgambatte && scons
#
# Then point GAMBATTE_DIR at the gambatte_core directory:
#   make GAMBATTE_DIR=/path/to/gambatte-speedrun/gambatte_core

GAMBATTE_DIR ?= ../../../gambatte-speedrun/gambatte_core

CXX ?= g++
CXXFLAGS = -O2 -std=c++17 -Wall -Wextra
INCLUDES = -I$(GAMBATTE_DIR)/libgambatte/include
LDFLAGS = -L$(GAMBATTE_DIR)/libgambatte
LDLIBS = -lgambatte

TARGET = gbtrace-gambatte

.PHONY: all clean

all: $(TARGET)

$(TARGET): gbtrace-gambatte.cpp
	$(CXX) $(CXXFLAGS) $(INCLUDES) -o $@ $< $(LDFLAGS) $(LDLIBS)

clean:
	rm -f $(TARGET)
