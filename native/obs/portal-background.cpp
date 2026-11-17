// SPDX-License-Identifier: GPL-3.0-only
// Flatpak login startup is owned by the Background portal, not host files.
#include "platform.hpp"
#include <gio/gio.h>
#include <algorithm>
#include <unistd.h>

namespace encore {
namespace {
GDBusConnection *connection = nullptr;
guint subscription = 0;
unsigned generation = 0;
std::string request_path, error;
bool pending = false, background = false, autostart = false;
constexpr const char *portal = "org.freedesktop.portal.Desktop";
}

void platform_background_shutdown() {
    ++generation;
    if (connection) {
        if (pending && !request_path.empty()) g_dbus_connection_call(connection, portal, request_path.c_str(),
            "org.freedesktop.portal.Request", "Close", nullptr, nullptr, G_DBUS_CALL_FLAGS_NONE, 1000, nullptr, nullptr, nullptr);
        if (subscription) g_dbus_connection_signal_unsubscribe(connection, subscription);
        g_object_unref(connection); connection = nullptr;
    }
    subscription = 0; pending = false; request_path.clear();
}

void platform_request_background(bool enable_autostart) {
    platform_background_shutdown();
    error.clear(); background = autostart = false;
    if (!g_getenv("FLATPAK_ID")) { error = "Use the native login startup setting outside Flatpak"; return; }
    GError *failure = nullptr;
    connection = g_bus_get_sync(G_BUS_TYPE_SESSION, nullptr, &failure);
    if (!connection) {
        error = "Cannot connect to the desktop background portal";
        if (failure) g_error_free(failure);
        return;
    }
    const auto token = "encore_background_" + std::to_string(getpid()) + "_" + std::to_string(generation);
    std::string sender = g_dbus_connection_get_unique_name(connection);
    if (!sender.empty() && sender.front() == ':') sender.erase(0, 1);
    std::replace(sender.begin(), sender.end(), '.', '_');
    request_path = "/org/freedesktop/portal/desktop/request/" + sender + "/" + token;
    pending = true;
    subscription = g_dbus_connection_signal_subscribe(connection, portal, "org.freedesktop.portal.Request", "Response",
        request_path.c_str(), nullptr, G_DBUS_SIGNAL_FLAGS_NONE,
        +[](GDBusConnection *, const gchar *, const gchar *, const gchar *, const gchar *, GVariant *parameters, gpointer) {
            guint response = 0; GVariant *results = nullptr;
            g_variant_get(parameters, "(u@a{sv})", &response, &results);
            gboolean allowed_background = false, allowed_autostart = false;
            if (response == 0) {
                g_variant_lookup(results, "background", "b", &allowed_background);
                g_variant_lookup(results, "autostart", "b", &allowed_autostart);
                background = allowed_background; autostart = allowed_autostart;
            } else error = "Login startup permission was cancelled or denied by the desktop";
            pending = false;
            g_variant_unref(results);
        }, nullptr, nullptr);
    GVariantBuilder options; g_variant_builder_init(&options, G_VARIANT_TYPE_VARDICT);
    g_variant_builder_add(&options, "{sv}", "handle_token", g_variant_new_string(token.c_str()));
    g_variant_builder_add(&options, "{sv}", "reason", g_variant_new_string("Keep the game replay buffer running while the library is closed"));
    g_variant_builder_add(&options, "{sv}", "autostart", g_variant_new_boolean(enable_autostart));
    g_variant_builder_add(&options, "{sv}", "dbus-activatable", g_variant_new_boolean(false));
    const char *command[] = {"encore-recorder", nullptr};
    g_variant_builder_add(&options, "{sv}", "commandline", g_variant_new_strv(command, -1));
    g_dbus_connection_call(connection, portal, "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Background",
        "RequestBackground", g_variant_new("(s@a{sv})", "", g_variant_builder_end(&options)), G_VARIANT_TYPE("(o)"),
        G_DBUS_CALL_FLAGS_NONE, 5000, nullptr,
        +[](GObject *object, GAsyncResult *result, gpointer data) {
            GError *failure = nullptr;
            GVariant *reply = g_dbus_connection_call_finish(G_DBUS_CONNECTION(object), result, &failure);
            if (GPOINTER_TO_UINT(data) == generation && failure) {
                error = "The desktop background portal is unavailable; login startup could not be changed";
                pending = false;
            }
            if (failure) g_error_free(failure);
            if (reply) g_variant_unref(reply);
        }, GUINT_TO_POINTER(generation));
}

obs_data_t *platform_background_status() {
    auto *result = obs_data_create();
    obs_data_set_bool(result, "pending", pending);
    obs_data_set_bool(result, "background", background);
    obs_data_set_bool(result, "autostart", autostart);
    obs_data_set_string(result, "error", error.c_str());
    return result;
}
}
