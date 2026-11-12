// SPDX-License-Identifier: GPL-3.0-only
// GlobalShortcuts portal: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html
#include "wayland-shortcuts.hpp"
#include <gio/gio.h>
#include <algorithm>
#include <cctype>
#include <unistd.h>

namespace encore {
namespace {
constexpr const char *bus_name = "org.freedesktop.portal.Desktop";
constexpr const char *bus_path = "/org/freedesktop/portal/desktop";
constexpr const char *interface = "org.freedesktop.portal.GlobalShortcuts";
GDBusConnection *connection = nullptr;
guint response_subscription = 0, activation_subscription = 0;
unsigned generation = 0;
std::string session, request_path, preferred_trigger, warning;
bool pressed = false, creating = false;

void call(const char *method, GVariant *arguments) {
    // Completion is asynchronous; Request::Response is subscribed before the call.
    g_dbus_connection_call(connection, bus_name, bus_path, interface, method, arguments,
        G_VARIANT_TYPE("(o)"), G_DBUS_CALL_FLAGS_NONE, 5000, nullptr,
        +[](GObject *object, GAsyncResult *result, gpointer data) {
            GError *error = nullptr;
            GVariant *reply = g_dbus_connection_call_finish(G_DBUS_CONNECTION(object), result, &error);
            if (GPOINTER_TO_UINT(data) == generation && error)
                warning = "The desktop global-shortcut portal is unavailable. Save from the Encore menu or library, or assign a system shortcut to encore-recorder --save.";
            if (error) g_error_free(error);
            if (reply) g_variant_unref(reply);
        }, GUINT_TO_POINTER(generation));
}

GVariant *options(const std::string &token, bool create) {
    GVariantBuilder builder; g_variant_builder_init(&builder, G_VARIANT_TYPE_VARDICT);
    g_variant_builder_add(&builder, "{sv}", "handle_token", g_variant_new_string(token.c_str()));
    if (create) g_variant_builder_add(&builder, "{sv}", "session_handle_token", g_variant_new_string(token.c_str()));
    return g_variant_builder_end(&builder);
}

void set_request(const std::string &token) {
    std::string sender = g_dbus_connection_get_unique_name(connection);
    if (!sender.empty() && sender.front() == ':') sender.erase(0, 1);
    std::replace(sender.begin(), sender.end(), '.', '_');
    request_path = "/org/freedesktop/portal/desktop/request/" + sender + "/" + token;
}
}

void wayland_shortcuts_shutdown() {
    ++generation;
    if (!connection) return;
    if (!request_path.empty()) g_dbus_connection_call(connection, bus_name, request_path.c_str(),
        "org.freedesktop.portal.Request", "Close", nullptr, nullptr, G_DBUS_CALL_FLAGS_NONE, 1000, nullptr, nullptr, nullptr);
    if (!session.empty()) g_dbus_connection_call(connection, bus_name, session.c_str(),
        "org.freedesktop.portal.Session", "Close", nullptr, nullptr, G_DBUS_CALL_FLAGS_NONE, 1000, nullptr, nullptr, nullptr);
    if (response_subscription) g_dbus_connection_signal_unsubscribe(connection, response_subscription);
    if (activation_subscription) g_dbus_connection_signal_unsubscribe(connection, activation_subscription);
    response_subscription = activation_subscription = 0;
    g_object_unref(connection); connection = nullptr;
    session.clear(); request_path.clear(); pressed = false; warning.clear();
}

void wayland_shortcuts_configure(const std::string &trigger) {
    wayland_shortcuts_shutdown();
    GError *error = nullptr;
    connection = g_bus_get_sync(G_BUS_TYPE_SESSION, nullptr, &error);
    if (!connection) {
        warning = "Cannot connect to the desktop shortcut portal. Save from Encore's menu or library.";
        if (error) g_error_free(error);
        return;
    }
    preferred_trigger = trigger;
    std::transform(preferred_trigger.begin(), preferred_trigger.end(), preferred_trigger.begin(),
        [](unsigned char character) { return std::toupper(character); });
    const auto replace = [&](const std::string &from, const std::string &to) {
        size_t offset = 0;
        while ((offset = preferred_trigger.find(from, offset)) != std::string::npos) {
            preferred_trigger.replace(offset, from.size(), to); offset += to.size();
        }
    };
    replace("CONTROL+", "CTRL+"); replace("SUPER+", "LOGO+"); replace("COMMAND+", "LOGO+");
    replace("META+", "LOGO+"); replace("ARROW", "");
    creating = true;
    warning = "Approve the save shortcut in the desktop permission dialog.";
    response_subscription = g_dbus_connection_signal_subscribe(connection, bus_name,
        "org.freedesktop.portal.Request", "Response", nullptr, nullptr, G_DBUS_SIGNAL_FLAGS_NONE,
        +[](GDBusConnection *, const gchar *, const gchar *path, const gchar *, const gchar *, GVariant *parameters, gpointer) {
            if (request_path != path) return;
            guint response = 0; GVariant *results = nullptr;
            g_variant_get(parameters, "(u@a{sv})", &response, &results);
            if (response != 0) {
                warning = "The save shortcut was not granted. Save from Encore's menu or library, or allow the shortcut in desktop settings.";
            } else if (creating) {
                const char *handle = nullptr;
                if (g_variant_lookup(results, "session_handle", "&s", &handle) && handle) {
                    session = handle; creating = false;
                    const auto token = "encore_bind_" + std::to_string(getpid()) + "_" + std::to_string(generation);
                    set_request(token);
                    GVariantBuilder shortcuts, properties;
                    g_variant_builder_init(&shortcuts, G_VARIANT_TYPE("a(sa{sv})"));
                    g_variant_builder_init(&properties, G_VARIANT_TYPE_VARDICT);
                    g_variant_builder_add(&properties, "{sv}", "description", g_variant_new_string("Save Encore replay"));
                    g_variant_builder_add(&properties, "{sv}", "preferred_trigger", g_variant_new_string(preferred_trigger.c_str()));
                    g_variant_builder_add(&shortcuts, "(s@a{sv})", "save-replay", g_variant_builder_end(&properties));
                    call("BindShortcuts", g_variant_new("(o@a(sa{sv})s@a{sv})", session.c_str(),
                        g_variant_builder_end(&shortcuts), "", options(token, false)));
                } else warning = "The desktop did not return a shortcut session. Save from Encore's menu or library.";
            } else {
                GVariant *shortcuts = g_variant_lookup_value(results, "shortcuts", G_VARIANT_TYPE("a(sa{sv})"));
                warning = shortcuts && g_variant_n_children(shortcuts) ? "" : "No save shortcut was granted by the desktop.";
                if (shortcuts) g_variant_unref(shortcuts);
            }
            g_variant_unref(results);
        }, nullptr, nullptr);
    activation_subscription = g_dbus_connection_signal_subscribe(connection, bus_name,
        interface, "Activated", bus_path, nullptr, G_DBUS_SIGNAL_FLAGS_NONE,
        +[](GDBusConnection *, const gchar *, const gchar *, const gchar *, const gchar *, GVariant *parameters, gpointer) {
            const char *handle = nullptr, *shortcut = nullptr; guint64 timestamp = 0; GVariant *options = nullptr;
            g_variant_get(parameters, "(&o&st@a{sv})", &handle, &shortcut, &timestamp, &options);
            if (session == handle && std::string(shortcut) == "save-replay") pressed = true;
            g_variant_unref(options);
        }, nullptr, nullptr);
    const auto token = "encore_create_" + std::to_string(getpid()) + "_" + std::to_string(generation);
    set_request(token);
    call("CreateSession", g_variant_new("(@a{sv})", options(token, true)));
}
bool wayland_shortcuts_pressed() { bool value = pressed; pressed = false; return value; }
std::string wayland_shortcuts_warning() { return warning; }
}
