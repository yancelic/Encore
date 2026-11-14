// SPDX-License-Identifier: GPL-3.0-only
// Synthetic inputs are created only by the explicit --self-test command. They do
// not request screen/microphone permission or masquerade as real capture.
#include <obs-module.h>
#include <util/platform.h>
#include <atomic>
#include <chrono>
#include <cmath>
#include <thread>

namespace {
struct Tone {
    obs_source_t *source;
    unsigned frequency;
    std::atomic<bool> stop{false};
    std::thread thread;
    Tone(obs_source_t *input, unsigned hz) : source(input), frequency(hz), thread([this] { run(); }) {}
    ~Tone() { stop.store(true); thread.join(); }
    void run() {
        uint64_t position = 0;
        const uint64_t start = os_gettime_ns();
        auto deadline = std::chrono::steady_clock::now();
        while (!stop.load()) {
            float samples[480];
            for (unsigned i = 0; i < 480; ++i)
                samples[i] = 0.1f * std::sin(6.283185307179586 * frequency * (position + i) / 48000.0);
            obs_source_audio audio{};
            audio.data[0] = reinterpret_cast<const uint8_t *>(samples);
            audio.data[1] = reinterpret_cast<const uint8_t *>(samples);
            audio.frames = 480;
            audio.speakers = SPEAKERS_STEREO;
            audio.format = AUDIO_FORMAT_FLOAT_PLANAR;
            audio.samples_per_sec = 48000;
            audio.timestamp = start + position * 1000000000ULL / 48000;
            obs_source_output_audio(source, &audio);
            position += 480;
            deadline += std::chrono::milliseconds(10);
            std::this_thread::sleep_until(deadline);
        }
    }
};
const char *video_name(void *) { return "Encore diagnostic video"; }
void *video_create(obs_data_t *, obs_source_t *source) { return source; }
void video_destroy(void *) {}
uint32_t video_width(void *) { return 640; }
uint32_t video_height(void *) { return 360; }
void video_render(void *, gs_effect_t *) {
    vec4 color{};
    const auto phase = (os_gettime_ns() / 100000000) % 3;
    color.x = phase == 0 ? 0.8f : 0.05f;
    color.y = phase == 1 ? 0.8f : 0.05f;
    color.z = phase == 2 ? 0.8f : 0.05f;
    color.w = 1.0f;
    gs_clear(GS_CLEAR_COLOR, &color, 0.0f, 0);
}
const char *tone_name(void *) { return "Encore diagnostic tone"; }
void *tone_create(obs_data_t *settings, obs_source_t *source) {
    return new Tone(source, static_cast<unsigned>(obs_data_get_int(settings, "frequency")));
}
void tone_destroy(void *data) { delete static_cast<Tone *>(data); }
}

void encore_register_diagnostic_sources() {
    obs_source_info video{};
    video.id = "encore_test_video"; video.type = OBS_SOURCE_TYPE_INPUT;
    video.output_flags = OBS_SOURCE_VIDEO;
    video.get_name = video_name; video.create = video_create; video.destroy = video_destroy;
    video.get_width = video_width; video.get_height = video_height; video.video_render = video_render;
    obs_register_source(&video);
    obs_source_info tone{};
    tone.id = "encore_test_tone"; tone.type = OBS_SOURCE_TYPE_INPUT; tone.output_flags = OBS_SOURCE_AUDIO;
    tone.get_name = tone_name; tone.create = tone_create; tone.destroy = tone_destroy;
    obs_register_source(&tone);
}
