// SPDX-License-Identifier: GPL-3.0-only
#include "platform.hpp"
#include "wayland-shortcuts.hpp"
#include <obs-nix-platform.h>
#include <gtk/gtk.h>
#include <libayatana-appindicator/app-indicator.h>
#include <pipewire/pipewire.h>
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
#include <chrono>

namespace encore {
void platform_background_shutdown();
namespace {
Display *xdisplay = nullptr;
bool wayland = false;
AppIndicator *indicator = nullptr;
GtkWidget *tray_menu = nullptr, *tray_status = nullptr;
std::string tray_action;

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
void platform_shutdown() { platform_background_shutdown(); wayland_shortcuts_shutdown(); platform_set_tray(false, false); xdisplay = nullptr; }
void platform_set_tray(bool visible, bool recording) {
    if (!visible) {
        if (indicator) { app_indicator_set_status(indicator, APP_INDICATOR_STATUS_PASSIVE); g_object_unref(indicator); indicator = nullptr; }
        if (tray_menu) { gtk_widget_destroy(tray_menu); tray_menu = nullptr; tray_status = nullptr; }
        return;
    }
    if (!indicator) {
        indicator = app_indicator_new("com.encore.recorder", "media-record", APP_INDICATOR_CATEGORY_APPLICATION_STATUS);
        tray_menu = gtk_menu_new();
        tray_status = gtk_menu_item_new_with_label("Encore — waiting for a game");
        gtk_widget_set_sensitive(tray_status, false);
        gtk_menu_shell_append(GTK_MENU_SHELL(tray_menu), tray_status);
        const char *labels[] = {"Save replay", "Open library", "Open clips folder", "Quit Encore"};
        const char *actions[] = {"save", "library", "folder", "quit"};
        for (unsigned i = 0; i < 4; ++i) {
            auto *item = gtk_menu_item_new_with_label(labels[i]);
            g_signal_connect(item, "activate", G_CALLBACK(+[](GtkMenuItem *, gpointer action) {
                tray_action = static_cast<const char *>(action);
            }), const_cast<char *>(actions[i]));
            gtk_menu_shell_append(GTK_MENU_SHELL(tray_menu), item);
        }
        gtk_widget_show_all(tray_menu);
        app_indicator_set_menu(indicator, GTK_MENU(tray_menu));
        app_indicator_set_status(indicator, APP_INDICATOR_STATUS_ACTIVE);
    }
    gtk_menu_item_set_label(GTK_MENU_ITEM(tray_status), recording ? "Encore — recording" : "Encore — waiting for a game");
}
std::string platform_tray_action() {
    if (wayland_shortcuts_pressed()) return "save";
    auto action = tray_action; tray_action.clear(); return action;
}
bool platform_microphone_ready(std::string &) { return true; }
void platform_configure_hotkey(const std::string &trigger) { if (wayland) wayland_shortcuts_configure(trigger); }
std::string platform_hotkey_warning() { return wayland ? wayland_shortcuts_warning() : ""; }
bool platform_is_wayland() { return wayland; }
const char *platform_graphics_module() { return "libobs-opengl.so"; }
const char *platform_video_source() { return wayland ? "pipewire-window-capture-source" : "xcomposite_input"; }
bool platform_video_ready(std::string &) { return true; } // The Wayland source obtains its portal grant itself.

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

std::vector<std::string> platform_audio_applications() {
    struct Query { std::vector<std::string> applications; bool done = false; int sequence = 0; } query;
    pw_init(nullptr, nullptr);
    auto *loop = pw_main_loop_new(nullptr);
    if (!loop) return {};
    auto *context = pw_context_new(pw_main_loop_get_loop(loop), nullptr, 0);
    auto *core = context ? pw_context_connect(context, nullptr, 0) : nullptr;
    if (core) {
        spa_hook core_listener{}, registry_listener{};
        pw_core_events core_events{}; core_events.version = PW_VERSION_CORE_EVENTS;
        core_events.done = +[](void *data, uint32_t id, int sequence) {
            auto *query = static_cast<Query *>(data);
            if (id == PW_ID_CORE && sequence == query->sequence) query->done = true;
        };
        pw_core_add_listener(core, &core_listener, &core_events, &query);
        auto *registry = pw_core_get_registry(core, PW_VERSION_REGISTRY, 0);
        pw_registry_events events{}; events.version = PW_VERSION_REGISTRY_EVENTS;
        events.global = +[](void *data, uint32_t, uint32_t, const char *type, uint32_t, const spa_dict *properties) {
            if (std::string(type) != PW_TYPE_INTERFACE_Node || !properties) return;
            const char *media_class = spa_dict_lookup(properties, PW_KEY_MEDIA_CLASS);
            const char *binary = spa_dict_lookup(properties, PW_KEY_APP_PROCESS_BINARY);
            if (media_class && std::string(media_class) == "Stream/Output/Audio" && binary && *binary)
                static_cast<Query *>(data)->applications.emplace_back(binary);
        };
        pw_registry_add_listener(registry, &registry_listener, &events, &query);
        query.sequence = pw_core_sync(core, PW_ID_CORE, 0);
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(500);
        while (!query.done && std::chrono::steady_clock::now() < deadline)
            pw_loop_iterate(pw_main_loop_get_loop(loop), 20);
        spa_hook_remove(&registry_listener); spa_hook_remove(&core_listener);
        pw_proxy_destroy(reinterpret_cast<pw_proxy *>(registry));
        pw_core_disconnect(core);
    }
    if (context) pw_context_destroy(context);
    pw_main_loop_destroy(loop);
    return query.applications;
}
uint64_t platform_audio_identity(const std::string &) { return 1; } // PipeWire reconnects streams through registry events.

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
