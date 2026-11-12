#pragma once
#include <obs.h>
#include <string>
#include <vector>

namespace encore {
struct Application {
    std::string id;
    std::string title;
    uint64_t window = 0;
    bool focused = false;
    bool minimized = false;
};

bool platform_init(std::string &error);
void platform_pump();
void platform_shutdown();
void platform_set_tray(bool visible, bool recording);
std::string platform_tray_action();
bool platform_microphone_ready(std::string &warning);
bool platform_is_wayland();
std::vector<Application> platform_applications();
const char *platform_graphics_module();
const char *platform_video_source();
void platform_video_settings(obs_data_t *settings, const Application &application);
const char *platform_audio_source(bool microphone, bool application);
void platform_audio_settings(obs_data_t *settings, const std::string &selection, bool application);
}
