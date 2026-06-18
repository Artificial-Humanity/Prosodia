#include <alsa/asoundlib.h>
#include "audio_sink.h"
#include <stdlib.h>
#include <stdio.h>

struct audio_sink {
    snd_pcm_t* handle;
    uint32_t sample_rate;
    uint32_t channels;
};

audio_sink_t* audio_sink_create(uint32_t sample_rate, uint32_t channels) {
    audio_sink_t* sink = malloc(sizeof(audio_sink_t));
    if (!sink) return NULL;

    sink->sample_rate = sample_rate;
    sink->channels = channels;

    int err;
    if ((err = snd_pcm_open(&sink->handle, "default", SND_PCM_STREAM_PLAYBACK, 0)) < 0) {
        fprintf(stderr, "ALSA open error: %s\n", snd_strerror(err));
        free(sink);
        return NULL;
    }

    // Set hardware parameters
    snd_pcm_hw_params_t* params;
    snd_pcm_hw_params_alloca(&params);
    snd_pcm_hw_params_any(sink->handle, params);

    snd_pcm_hw_params_set_access(sink->handle, params, SND_PCM_ACCESS_RW_INTERLEAVED);
    snd_pcm_hw_params_set_format(sink->handle, params, SND_PCM_FORMAT_FLOAT_LE);
    snd_pcm_hw_params_set_channels(sink->handle, params, channels);
    
    unsigned int rate = sample_rate;
    snd_pcm_hw_params_set_rate_near(sink->handle, params, &rate, 0);

    // Set buffer size and period size for low latency
    unsigned int periods = 4;
    snd_pcm_uframes_t period_size = 1024;
    snd_pcm_hw_params_set_periods(sink->handle, params, periods, 0);
    snd_pcm_hw_params_set_period_size_near(sink->handle, params, &period_size, 0);

    if ((err = snd_pcm_hw_params(sink->handle, params)) < 0) {
        fprintf(stderr, "ALSA hw params error: %s\n", snd_strerror(err));
        snd_pcm_close(sink->handle);
        free(sink);
        return NULL;
    }

    return sink;
}

int audio_sink_write(audio_sink_t* sink, const float* samples, uint32_t count) {
    if (!sink || !sink->handle || !samples || count == 0) return -1;
    
    snd_pcm_uframes_t frames = count / sink->channels;
    snd_pcm_sframes_t written = 0;
    
    while (frames > 0) {
        snd_pcm_sframes_t res = snd_pcm_writei(sink->handle, samples + (written * sink->channels), frames);
        if (res < 0) {
            if (res == -EPIPE) {
                // Underrun
                snd_pcm_prepare(sink->handle);
            } else {
                fprintf(stderr, "ALSA write error: %s\n", snd_strerror(res));
                return -1;
            }
        } else {
            frames -= res;
            written += res;
        }
    }
    return 0;
}

void audio_sink_destroy(audio_sink_t* sink) {
    if (!sink) return;
    if (sink->handle) {
        snd_pcm_drain(sink->handle);
        snd_pcm_close(sink->handle);
    }
    free(sink);
}
