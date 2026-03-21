# gbtrace-mgba adapter
#
# Requires mGBA to be built as a static library first:
#   git clone https://github.com/mgba-emu/mgba.git
#   cd mgba && mkdir build && cd build
#   cmake .. -DLIBMGBA_ONLY=ON -DBUILD_STATIC=ON -DCMAKE_BUILD_TYPE=Release -DENABLE_DEBUGGERS=ON
#   make
#
# Then build the adapter:
#   make

MGBA_DIR ?= mgba
MGBA_BUILD ?= $(MGBA_DIR)/build

CC ?= cc
CFLAGS = -O2 -std=c11 -Wall -Wextra

# Use the exact same defines as the mGBA library build to ensure struct layout matches
MGBA_DEFINES = $(shell sed -n 's/^C_DEFINES = //p' $(MGBA_BUILD)/CMakeFiles/mgba.dir/flags.make)
INCLUDES = -I$(MGBA_DIR)/include -I$(MGBA_BUILD)/include -I$(MGBA_DIR)/src
LDFLAGS = -L$(MGBA_BUILD)
LDLIBS = -lmgba -lm -lpthread -lz

TARGET = gbtrace-mgba

.PHONY: all clean

all: $(TARGET)

$(TARGET): gbtrace-mgba.c
	$(CC) $(CFLAGS) $(MGBA_DEFINES) $(INCLUDES) -o $@ $< $(LDFLAGS) $(LDLIBS)

clean:
	rm -f $(TARGET)
