// SPDX-License-Identifier: GPL-3.0-only
#include "platform.hpp"
#include <obs-nix-platform.h>
#include <gtk/gtk.h>
#include <gdk/gdkx.h>
#include <gdk/gdkwayland.h>
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <X11/Xutil.h>
#include <unistd.h>
#include <algorithm>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

namespace encore {
namespace {
Display *xdisplay = nullptr;
bool wayland = false;

std::vector<unsigned long> property(Window window, const char *name) {
    Atom actual{}; int format = 0; unsigned long count = 0, remaining = 0; unsigned char *data = nullptr;
    std::vector<unsigned long> values;
    if (XGetWindowProperty(xdisplay, window, XInternAtom(xdisplay, name, False), 0, 65536, False,
                           AnyPropertyType, &actual, &format, &count, &remaining, &data) == Success && data && format == 32) {
        auto *items = reinterpret_cast<unsigned long *>(data);
        values.assign(items, items + count);
    }
    if (data) XFree(data);
    return values;
}

std::string window_title(Window window) {
    Atom actual{}; int format = 0; unsigned long count = 0, remaining = 0; unsigned char *data = nullptr;
    std::string title;
    if (XGetWindowProperty(xdisplay, window, XInternAtom(xdisplay, "_NET_WM_NAME", False), 0, 4096, False,
                           AnyPropertyType, &actual, &format, &count, &remaining, &data) == Success && data && format == 8)
        title.assign(reinterpret_cast<char *>(data), count);
    if (data) XFree(data);
    return title;
}

std::string window_class(Window window) {
    XClassHint hint{};
    std::string result;
    if (XGetClassHint(xdisplay, window, &hint)) {
        if (hint.res_class) result = hint.res_class;
        if (hint.res_name) XFree(hint.res_name);
        if (hint.res_class) XFree(hint.res_class);
    }
    return result;
}
}

bool platform_init(std::string &error) {
    if (!gtk_init_check(nullptr, nullptr)) { error = "A running X11 or Wayland desktop session is required"; return false; }
    auto *display = gdk_display_get_default();
    wayland = GDK_IS_WAYLAND_DISPLAY(display);
    if (wayland) {
        obs_set_nix_platform(OBS_NIX_PLATFORM_WAYLAND);
        obs_set_nix_platform_display(gdk_wayland_display_get_wl_display(display));
    } else if (GDK_IS_X11_DISPLAY(display)) {
        xdisplay = gdk_x11_display_get_xdisplay(display);
        XSetErrorHandler([](Display *, XErrorEvent *) { return 0; });
        obs_set_nix_platform(OBS_NIX_PLATFORM_X11_EGL);
        obs_set_nix_platform_display(xdisplay);
    } else { error = "Unsupported Linux display server"; return false; }
    return true;
}

void platform_pump() {
    for (unsigned i = 0; i < 32 && g_main_context_pending(nullptr); ++i)
        g_main_context_iteration(nullptr, false);
}
void platform_shutdown() { xdisplay = nullptr; }
bool platform_is_wayland() { return wayland; }
const char *platform_graphics_module() { return "libobs-opengl.so"; }
const char *platform_video_source() { return wayland ? "pipewire-window-capture-source" : "xcomposite_input"; }

std::vector<Application> platform_applications() {
    std::vector<Application> result;
    if (!xdisplay) return result;
    const auto focused = property(DefaultRootWindow(xdisplay), "_NET_ACTIVE_WINDOW");
    auto windows = property(DefaultRootWindow(xdisplay), "_NET_CLIENT_LIST_STACKING");
    if (windows.empty()) windows = property(DefaultRootWindow(xdisplay), "_NET_CLIENT_LIST");
    for (auto window : windows) {
        XWindowAttributes attributes{};
        if (!XGetWindowAttributes(xdisplay, window, &attributes) || attributes.width < 200 || attributes.height < 120) continue;
        const auto pid = property(window, "_NET_WM_PID");
        if (!pid.empty() && pid[0] == static_cast<unsigned long>(getpid())) continue;
        std::string executable;
        if (!pid.empty()) {
            std::error_code error;
            auto path = std::filesystem::read_symlink("/proc/" + std::to_string(pid[0]) + "/exe", error);
            if (!error) executable = path.filename().string();
            if (executable.find("wine") != std::string::npos) {
                std::ifstream comm("/proc/" + std::to_string(pid[0]) + "/comm");
                std::string name; std::getline(comm, name);
                if (!name.empty()) executable = name;
            }
        }
        if (executable.empty()) executable = window_class(window);
        if (executable.empty() || executable == "encore-ui" || executable == "encore-recorder") continue;
        auto title = window_title(window);
        result.push_back({executable, title.empty() ? executable : title, window,
                          !focused.empty() && focused[0] == window, attributes.map_state != IsViewable});
    }
    return result;
}

void platform_video_settings(obs_data_t *settings, const Application &application) {
    if (wayland) { obs_data_set_bool(settings, "ShowCursor", true); return; }
    const auto key = std::to_string(application.window) + "\r\n" + application.title + "\r\n" + window_class(application.window);
    obs_data_set_string(settings, "capture_window", key.c_str());
    obs_data_set_bool(settings, "show_cursor", true);
    obs_data_set_bool(settings, "include_border", false);
}

const char *platform_audio_source(bool microphone, bool application) {
    return microphone ? "pulse_input_capture" : application ? "pipewire_audio_application_capture" : "pulse_output_capture";
}

void platform_audio_settings(obs_data_t *settings, const std::string &selection, bool application) {
    if (application) {
        obs_data_set_int(settings, "CaptureMode", 0);
        obs_data_set_int(settings, "MatchPriorty", 0); // Upstream setting spelling is part of its API.
        obs_data_set_bool(settings, "ExceptApp", false);
        obs_data_set_string(settings, "TargetName", selection.c_str());
    } else obs_data_set_string(settings, "device_id", selection.c_str());
}
}
