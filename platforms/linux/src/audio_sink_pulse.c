#include <pulse/simple.h>
#include <pulse/error.h>
#include "audio_sink.h"
#include <stdlib.h>
#include <stdio.h>

struct audio_sink {
    pa_simple* s;
    uint32_t sample_rate;
    uint32_t channels;
};

audio_sink_t* audio_sink_create(uint32_t sample_rate, uint32_t channels) {
    audio_sink_t* sink = malloc(sizeof(audio_sink_t));
    if (!sink) return NULL;

    sink->sample_rate = sample_rate;
    sink->channels = channels;

    pa_sample_spec ss;
    ss.format = PA_SAMPLE_FLOAT32NE; // native endian float
    ss.rate = sample_rate;
    ss.channels = channels;

    int error;
    sink->s = pa_simple_new(NULL, "ProsodiaDaemon", PA_STREAM_PLAYBACK, NULL, "Speech Output", &ss, NULL, NULL, &error);
    if (!sink->s) {
        fprintf(stderr, "pa_simple_new() failed: %s\n", pa_strerror(error));
        free(sink);
        return NULL;
    }

    return sink;
}

int audio_sink_write(audio_sink_t* sink, const float* samples, uint32_t count) {
    if (!sink || !sink->s || !samples || count == 0) return -1;
    int error;
    if (pa_simple_write(sink->s, samples, count * sizeof(float), &error) < 0) {
        fprintf(stderr, "pa_simple_write() failed: %s\n", pa_strerror(error));
        return -1;
    }
    return 0;
}

void audio_sink_destroy(audio_sink_t* sink) {
    if (!sink) return;
    if (sink->s) {
        int error;
        pa_simple_drain(sink->s, &error);
        pa_simple_free(sink->s);
    }
    free(sink);
}
