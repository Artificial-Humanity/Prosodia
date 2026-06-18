#ifndef AUDIO_SINK_H
#define AUDIO_SINK_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct audio_sink audio_sink_t;

/**
 * Creates a new audio sink using the specified sample rate and channel count.
 * Returns NULL on failure.
 */
audio_sink_t* audio_sink_create(uint32_t sample_rate, uint32_t channels);

/**
 * Writes PCM samples (Float format) to the audio hardware queue.
 * Returns 0 on success, or a negative value on failure.
 */
int audio_sink_write(audio_sink_t* sink, const float* samples, uint32_t count);

/**
 * Drains all remaining samples in the queue and destroys the audio sink.
 */
void audio_sink_destroy(audio_sink_t* sink);

#ifdef __cplusplus
}
#endif

#endif /* AUDIO_SINK_H */
