/* Device-only bridge. File decoding and seeking belong to the Rust worker. */
#define MINIAUDIO_IMPLEMENTATION
#define MA_NO_DECODING
#define MA_NO_ENCODING
#define MA_NO_NULL
#include "miniaudio.h"
#include <stdlib.h>
#include <string.h>

typedef void (*desktop_render)(void *, float *, unsigned int);
typedef struct {
    ma_device device;
    desktop_render render;
    void *user;
} desktop_audio;

static void render_audio(ma_device *device, void *output, const void *input, ma_uint32 frames) {
    desktop_audio *audio = (desktop_audio *)device->pUserData;
    (void)input;
    audio->render(audio->user, (float *)output, frames);
}

void *desktop_audio_open(unsigned int rate, unsigned int channels, const unsigned char *channel_map, desktop_render render, void *user) {
    if (!channel_map || channels == 0 || channels > MA_MAX_CHANNELS) return NULL;
    desktop_audio *audio = (desktop_audio *)calloc(1, sizeof(*audio));
    if (!audio) return NULL;
    audio->render = render;
    audio->user = user;
    ma_device_config config = ma_device_config_init(ma_device_type_playback);
    config.playback.format = ma_format_f32;
    config.playback.channels = channels;
    memcpy(config.playback.channelMap, channel_map, channels * sizeof(ma_channel));
    config.sampleRate = rate;
    config.dataCallback = render_audio;
    config.pUserData = audio;
    if (ma_device_init(NULL, &config, &audio->device) != MA_SUCCESS) {
        free(audio);
        return NULL;
    }
    if (ma_device_start(&audio->device) != MA_SUCCESS) {
        ma_device_uninit(&audio->device);
        free(audio);
        return NULL;
    }
    return audio;
}

void desktop_audio_close(void *handle) {
    desktop_audio *audio = (desktop_audio *)handle;
    if (!audio) return;
    /* Uninitialization joins the callback thread before Rust frees user data. */
    ma_device_uninit(&audio->device);
    free(audio);
}
