// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <string>

namespace encore {
// Convert Encore/keyboard-types tokens to the case-sensitive XDG shortcut format.
// An empty result means the shortcut cannot be represented.
std::string portal_shortcut_trigger(const std::string &shortcut);
}
