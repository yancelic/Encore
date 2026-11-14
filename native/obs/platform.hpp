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
void platform_configure_hotkey(const std::string &trigger);
std::string platform_hotkey_warning();
void platform_request_background(bool autostart);
obs_data_t *platform_background_status();
bool platform_is_wayland();
std::vector<Application> platform_applications();
std::vector<std::string> platform_audio_applications();
uint64_t platform_audio_identity(const std::string &application);
const char *platform_graphics_module();
const char *platform_video_source();
const char *platform_video_fallback_source();
bool platform_video_ready(std::string &warning);
void platform_video_settings(obs_data_t *settings, const Application &application);
const char *platform_audio_source(bool microphone, bool application);
void platform_audio_settings(obs_data_t *settings, const std::string &selection, bool application);
}
