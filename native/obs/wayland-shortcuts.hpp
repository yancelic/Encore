#pragma once
#include <string>
namespace encore {
void wayland_shortcuts_configure(const std::string &trigger);
void wayland_shortcuts_shutdown();
bool wayland_shortcuts_pressed();
std::string wayland_shortcuts_warning();
}
