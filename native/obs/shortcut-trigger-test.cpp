// SPDX-License-Identifier: GPL-3.0-only
#include "shortcut-trigger.hpp"
#include <iostream>
#include <utility>
#ifdef ENCORE_TEST_X11
#include <X11/Xlib.h>
#endif

namespace {
unsigned checks = 0;
bool check(const std::string &input, const std::string &expected) {
    ++checks;
    const auto actual = encore::portal_shortcut_trigger(input);
    if (actual != expected) {
        std::cerr << input << ": expected '" << expected << "', got '" << actual << "'\n";
        return false;
    }
#ifdef ENCORE_TEST_X11
    // Check real X11/XKB names, including case. No display connection is needed.
    if (!actual.empty() && XStringToKeysym(actual.substr(actual.rfind('+') + 1).c_str()) == NoSymbol) {
        std::cerr << input << ": unknown XKB keysym in " << actual << '\n';
        return false;
    }
#endif
    return true;
}
}

int main() {
    bool ok = true;
    for (char key = 'A'; key <= 'Z'; ++key) {
        const auto letter = std::string(1, key);
        const auto expected = "ALT+" + std::string(1, static_cast<char>(key + ('a' - 'A')));
        ok &= check("Alt+" + letter, expected);
        ok &= check("Alt+Key" + letter, expected);
    }
    for (unsigned key = 0; key <= 9; ++key) {
        const auto digit = std::to_string(key);
        ok &= check("Ctrl+" + digit, "CTRL+" + digit);
        ok &= check("Ctrl+Digit" + digit, "CTRL+" + digit);
        ok &= check("Alt+Numpad" + digit, "ALT+KP_" + digit);
    }
    for (unsigned key = 1; key <= 24; ++key)
        ok &= check("Shift+F" + std::to_string(key), "SHIFT+F" + std::to_string(key));
    const std::pair<const char *, const char *> cases[] = {
        {"Alt+S", "ALT+s"}, {"Shift+Super+S", "SHIFT+LOGO+s"},
        {"Control+Option+Command+KeyK", "CTRL+ALT+LOGO+k"},
        {"cmd+ctrl+shift+alt+Enter", "CTRL+ALT+SHIFT+LOGO+Return"},
        {"Meta+F9", "LOGO+F9"}, {"F9", "F9"}, {"ctrl+control+s", "CTRL+s"},
        {"Alt+Backquote", "ALT+grave"}, {"Alt+Minus", "ALT+minus"},
        {"Alt+Equal", "ALT+equal"}, {"Alt+BracketLeft", "ALT+bracketleft"},
        {"Alt+BracketRight", "ALT+bracketright"}, {"Alt+Backslash", "ALT+backslash"},
        {"Alt+Semicolon", "ALT+semicolon"}, {"Alt+Quote", "ALT+apostrophe"},
        {"Alt+Comma", "ALT+comma"}, {"Alt+Period", "ALT+period"}, {"Alt+Slash", "ALT+slash"},
        {"Alt+IntlBackslash", "ALT+less"}, {"Alt+IntlRo", "ALT+backslash"}, {"Alt+IntlYen", "ALT+yen"},
        {"Alt+Space", "ALT+space"}, {"Alt+Tab", "ALT+Tab"}, {"Alt+Enter", "ALT+Return"},
        {"Alt+Backspace", "ALT+BackSpace"}, {"Alt+Delete", "ALT+Delete"}, {"Alt+Insert", "ALT+Insert"},
        {"Alt+Home", "ALT+Home"}, {"Alt+End", "ALT+End"}, {"Alt+PageUp", "ALT+Page_Up"},
        {"Alt+PageDown", "ALT+Page_Down"}, {"Alt+PrintScreen", "ALT+Print"},
        {"Alt+Pause", "ALT+Pause"}, {"Alt+ScrollLock", "ALT+Scroll_Lock"},
        {"Alt+ArrowUp", "ALT+Up"}, {"Alt+ArrowDown", "ALT+Down"},
        {"Alt+ArrowLeft", "ALT+Left"}, {"Alt+ArrowRight", "ALT+Right"},
        {"Alt+Escape", "ALT+Escape"}, {"Alt+CapsLock", "ALT+Caps_Lock"},
        {"Alt+NumLock", "ALT+Num_Lock"}, {"Alt+ContextMenu", "ALT+Menu"},
        {"Alt+Help", "ALT+Help"}, {"Alt+Convert", "ALT+Henkan"},
        {"Alt+NonConvert", "ALT+Muhenkan"}, {"Alt+KanaMode", "ALT+Hiragana_Katakana"},
        {"Alt+NumpadAdd", "ALT+KP_Add"}, {"Alt+NumpadSubtract", "ALT+KP_Subtract"},
        {"Alt+NumpadMultiply", "ALT+KP_Multiply"}, {"Alt+NumpadDivide", "ALT+KP_Divide"},
        {"Alt+NumpadDecimal", "ALT+KP_Decimal"}, {"Alt+NumpadEnter", "ALT+KP_Enter"},
        {"Alt+NumpadEqual", "ALT+KP_Equal"}, {"Alt+NumpadComma", "ALT+KP_Separator"},
        {"Alt+NumpadClear", "ALT+Clear"},
        {"Ctrl+AudioVolumeDown", "CTRL+XF86AudioLowerVolume"},
        {"Ctrl+AudioVolumeUp", "CTRL+XF86AudioRaiseVolume"}, {"Ctrl+AudioVolumeMute", "CTRL+XF86AudioMute"},
        {"Ctrl+MediaPlayPause", "CTRL+XF86AudioPlay"}, {"Ctrl+MediaPlay", "CTRL+XF86AudioPlay"},
        {"Ctrl+MediaPause", "CTRL+XF86AudioPause"}, {"Ctrl+MediaStop", "CTRL+XF86AudioStop"},
        {"Ctrl+MediaTrackNext", "CTRL+XF86AudioNext"}, {"Ctrl+MediaTrackPrevious", "CTRL+XF86AudioPrev"},
        {"Ctrl+MediaSelect", "CTRL+XF86AudioMedia"}, {"Ctrl+BrowserBack", "CTRL+XF86Back"},
        {"Ctrl+BrowserForward", "CTRL+XF86Forward"}, {"Ctrl+BrowserFavorites", "CTRL+XF86Favorites"},
        {"Ctrl+BrowserHome", "CTRL+XF86HomePage"}, {"Ctrl+BrowserRefresh", "CTRL+XF86Refresh"},
        {"Ctrl+BrowserSearch", "CTRL+XF86Search"}, {"Ctrl+BrowserStop", "CTRL+XF86Stop"},
        {"Ctrl+LaunchApp1", "CTRL+XF86Launch0"}, {"Ctrl+LaunchApp2", "CTRL+XF86Launch1"},
        {"Ctrl+LaunchMail", "CTRL+XF86Mail"}, {"Ctrl+Sleep", "CTRL+XF86Sleep"},
    };
    for (const auto &test : cases) ok &= check(test.first, test.second);
    for (const auto *invalid : {"", "Alt", "Ctrl+", "+S", "Ctrl++S", "S+Alt", "S+K", "Ctrl+Unknown",
                               "Alt+F0", "Alt+F25", "Alt+F01", "Alt+Fxx", "Alt+Numpad10", "Ctrl+S\n",
                               "Alt+Key0", "Alt+DigitA"})
        ok &= check(invalid, "");
    if (!ok) return 1;
    std::cout << checks << " portal shortcut checks passed\n";
}
