// Stub implementations for metrolib/audio/Audio.h symbols.
// The real Audio.cpp requires SDL2; we don't need audio for headless tracing.

#include "metrolib/audio/Audio.h"

sample_t spu_ring_buffer[512];
uint16_t spu_ring_cursor = 0;

void audio_init() {}
void audio_stop() {}
void audio_post(int, sample_t, sample_t) {}
int  audio_queue_size() { return 0; }
