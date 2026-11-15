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
// Different from the 1920x1080 bootstrap canvas. Keep this small enough for
// software-rendered CI Macs while testing both canvas resize and downscale.
uint32_t video_width(void *) { return 1280; }
uint32_t video_height(void *) { return 720; }
void video_render(void *, gs_effect_t *) {
    vec4 color{};
    const auto phase = (os_gettime_ns() / 100000000) % 3;
    color.x = phase == 0 ? 0.8f : 0.05f;
    color.y = phase == 1 ? 0.8f : 0.05f;
    color.z = phase == 2 ? 0.8f : 0.05f;
    color.w = 1.0f;
    auto *effect = obs_get_base_effect(OBS_EFFECT_SOLID);
    vec4 white{};
    white.x = white.y = white.z = white.w = 1.0f;
    gs_effect_set_vec4(gs_effect_get_param_by_name(effect, "color"), &white);
    auto draw = [&](float x, float y, uint32_t width, uint32_t height, const vec4 &fill) {
        const float vertices[][2] = {{x, y}, {x + width, y}, {x, y + height},
            {x, y + height}, {x + width, y}, {x + width, y + height}};
        for (const auto &vertex : vertices) {
            gs_color(vec4_to_rgba(&fill));
            gs_vertex2f(vertex[0], vertex[1]);
        }
    };
    // Draw geometry, not gs_clear: clearing the framebuffer ignores scene
    // transforms and would let a cropped or displaced source pass the test.
    vec4 corners[4]{};
    for (auto &corner : corners) corner.w = 1.0f;
    corners[0].x = 1.0f; // top left: red
    corners[1].y = 1.0f; // top right: green
    corners[2].z = 1.0f; // bottom left: blue
    corners[3].x = corners[3].y = corners[3].z = 1.0f; // bottom right: white
    const auto width = video_width(nullptr), height = video_height(nullptr);
    // One draw call avoids repeated software OpenGL submissions on CI Macs.
    while (gs_effect_loop(effect, "SolidColored")) {
        gs_render_start(true);
        draw(0, 0, width, height, color);
        for (unsigned i = 0; i < 4; ++i)
            draw(i & 1 ? width * 7 / 8 : 0, i & 2 ? height * 7 / 8 : 0,
                 width / 8, height / 8, corners[i]);
        gs_render_stop(GS_TRIS);
    }
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
    video.output_flags = OBS_SOURCE_VIDEO | OBS_SOURCE_CUSTOM_DRAW;
    video.get_name = video_name; video.create = video_create; video.destroy = video_destroy;
    video.get_width = video_width; video.get_height = video_height; video.video_render = video_render;
    obs_register_source(&video);
    obs_source_info tone{};
    tone.id = "encore_test_tone"; tone.type = OBS_SOURCE_TYPE_INPUT; tone.output_flags = OBS_SOURCE_AUDIO;
    tone.get_name = tone_name; tone.create = tone_create; tone.destroy = tone_destroy;
    obs_register_source(&tone);
}
