# gbtrace-sameboy adapter
#
# Requires SameBoy to be built as a library first:
#   git clone https://github.com/LIJI32/SameBoy.git
#   cd SameBoy && make lib CONF=release
#
# Then build the adapter:
#   make

SAMEBOY_DIR ?= SameBoy

CXX ?= g++
CXXFLAGS = -O2 -std=c++17 -Wall -Wextra
# Include Core/ directly (we define GB_INTERNAL for full struct access)
INCLUDES = -I$(SAMEBOY_DIR)/Core
LDFLAGS = -L$(SAMEBOY_DIR)/build/lib
LDLIBS = -lsameboy -lm -lpthread

TARGET = gbtrace-sameboy

.PHONY: all clean

all: $(TARGET)

$(TARGET): gbtrace-sameboy.cpp
	$(CXX) $(CXXFLAGS) $(INCLUDES) -o $@ $< $(LDFLAGS) $(LDLIBS)

clean:
	rm -f $(TARGET)
