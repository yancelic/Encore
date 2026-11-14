// SPDX-License-Identifier: GPL-3.0-only
#include "shortcut-trigger.hpp"
#include <algorithm>
#include <cctype>
#include <utility>

namespace encore {
namespace {
std::string keysym(std::string key) {
    if (key.size() == 4 && key.compare(0, 3, "KEY") == 0 && key[3] >= 'A' && key[3] <= 'Z') key.erase(0, 3);
    if (key.size() == 6 && key.compare(0, 5, "DIGIT") == 0 && key[5] >= '0' && key[5] <= '9') key.erase(0, 5);
    if (key.size() == 1 && key[0] >= 'A' && key[0] <= 'Z')
        return std::string(1, static_cast<char>(std::tolower(static_cast<unsigned char>(key[0]))));
    if (key.size() == 1 && key[0] >= '0' && key[0] <= '9') return key;
    if (key.size() >= 2 && key.size() <= 3 && key[0] == 'F') {
        const auto number = key.substr(1);
        if (std::all_of(number.begin(), number.end(), [](char c) { return c >= '0' && c <= '9'; })) {
            const int value = std::stoi(number);
            if (value >= 1 && value <= 24 && number == std::to_string(value)) return key;
        }
    }
    if (key.size() == 7 && key.compare(0, 6, "NUMPAD") == 0 && key[6] >= '0' && key[6] <= '9')
        return "KP_" + key.substr(6);
    const std::pair<const char *, const char *> names[] = {
        {"BACKQUOTE", "grave"}, {"MINUS", "minus"}, {"EQUAL", "equal"},
        {"BRACKETLEFT", "bracketleft"}, {"BRACKETRIGHT", "bracketright"},
        {"BACKSLASH", "backslash"}, {"SEMICOLON", "semicolon"}, {"QUOTE", "apostrophe"},
        {"COMMA", "comma"}, {"PERIOD", "period"}, {"SLASH", "slash"},
        {"INTLBACKSLASH", "less"}, {"INTLRO", "backslash"}, {"INTLYEN", "yen"},
        {"SPACE", "space"}, {"TAB", "Tab"}, {"ENTER", "Return"}, {"RETURN", "Return"},
        {"BACKSPACE", "BackSpace"}, {"DELETE", "Delete"}, {"INSERT", "Insert"},
        {"HOME", "Home"}, {"END", "End"}, {"PAGEUP", "Page_Up"}, {"PAGEDOWN", "Page_Down"},
        {"PRINTSCREEN", "Print"}, {"PAUSE", "Pause"}, {"SCROLLLOCK", "Scroll_Lock"},
        {"ARROWUP", "Up"}, {"ARROWDOWN", "Down"}, {"ARROWLEFT", "Left"}, {"ARROWRIGHT", "Right"},
        {"ESCAPE", "Escape"}, {"CAPSLOCK", "Caps_Lock"}, {"NUMLOCK", "Num_Lock"},
        {"CONTEXTMENU", "Menu"}, {"HELP", "Help"}, {"CONVERT", "Henkan"},
        {"NONCONVERT", "Muhenkan"}, {"KANAMODE", "Hiragana_Katakana"},
        {"NUMPADADD", "KP_Add"}, {"NUMPADSUBTRACT", "KP_Subtract"},
        {"NUMPADMULTIPLY", "KP_Multiply"}, {"NUMPADDIVIDE", "KP_Divide"},
        {"NUMPADDECIMAL", "KP_Decimal"}, {"NUMPADENTER", "KP_Enter"},
        {"NUMPADEQUAL", "KP_Equal"}, {"NUMPADCOMMA", "KP_Separator"}, {"NUMPADCLEAR", "Clear"},
        {"AUDIOVOLUMEDOWN", "XF86AudioLowerVolume"}, {"AUDIOVOLUMEUP", "XF86AudioRaiseVolume"},
        {"AUDIOVOLUMEMUTE", "XF86AudioMute"}, {"MEDIAPLAYPAUSE", "XF86AudioPlay"},
        {"MEDIAPLAY", "XF86AudioPlay"}, {"MEDIAPAUSE", "XF86AudioPause"},
        {"MEDIASTOP", "XF86AudioStop"}, {"MEDIATRACKNEXT", "XF86AudioNext"},
        {"MEDIATRACKPREVIOUS", "XF86AudioPrev"}, {"MEDIASELECT", "XF86AudioMedia"},
        {"BROWSERBACK", "XF86Back"}, {"BROWSERFORWARD", "XF86Forward"},
        {"BROWSERFAVORITES", "XF86Favorites"}, {"BROWSERHOME", "XF86HomePage"},
        {"BROWSERREFRESH", "XF86Refresh"}, {"BROWSERSEARCH", "XF86Search"}, {"BROWSERSTOP", "XF86Stop"},
        {"LAUNCHAPP1", "XF86Launch0"}, {"LAUNCHAPP2", "XF86Launch1"},
        {"LAUNCHMAIL", "XF86Mail"}, {"SLEEP", "XF86Sleep"},
    };
    for (const auto &name : names) if (key == name.first) return name.second;
    return {};
}
}

std::string portal_shortcut_trigger(const std::string &shortcut) {
    bool control = false, alt = false, shift = false, logo = false;
    std::string key;
    size_t offset = 0;
    while (true) {
        const auto end = shortcut.find('+', offset);
        auto token = shortcut.substr(offset, end == std::string::npos ? end : end - offset);
        std::transform(token.begin(), token.end(), token.begin(), [](unsigned char c) { return std::toupper(c); });
        if (token.empty() || !key.empty()) return {};
        if (token == "CTRL" || token == "CONTROL") control = true;
        else if (token == "ALT" || token == "OPTION") alt = true;
        else if (token == "SHIFT") shift = true;
        else if (token == "SUPER" || token == "COMMAND" || token == "CMD" || token == "META") logo = true;
        else {
            key = keysym(token);
            if (key.empty()) return {};
        }
        if (end == std::string::npos) break;
        offset = end + 1;
    }
    if (key.empty()) return {};
    return std::string(control ? "CTRL+" : "") + (alt ? "ALT+" : "")
        + (shift ? "SHIFT+" : "") + (logo ? "LOGO+" : "") + key;
}
}
