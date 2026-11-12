// SPDX-License-Identifier: GPL-3.0-only
#include "platform.hpp"
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#include <unistd.h>

namespace encore {
bool platform_init(std::string &) {
    @autoreleasepool {
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        [NSApp finishLaunching];
    }
    return true;
}

void platform_pump() {
    @autoreleasepool {
        for (unsigned i = 0; i < 32; ++i) {
            NSEvent *event = [NSApp nextEventMatchingMask:NSEventMaskAny untilDate:[NSDate distantPast]
                                                 inMode:NSDefaultRunLoopMode dequeue:YES];
            if (!event) break;
            [NSApp sendEvent:event];
        }
        [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate date]];
    }
}
void platform_shutdown() {}
bool platform_is_wayland() { return false; }
const char *platform_graphics_module() { return "libobs-opengl.dylib"; }
const char *platform_video_source() { return "screen_capture"; }

std::vector<Application> platform_applications() {
    std::vector<Application> result;
    @autoreleasepool {
        const pid_t focused = NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier;
        CFArrayRef array = CGWindowListCopyWindowInfo(kCGWindowListOptionAll | kCGWindowListExcludeDesktopElements, kCGNullWindowID);
        if (!array) return result;
        for (NSDictionary *info in (__bridge NSArray *)array) {
            if ([info[(id)kCGWindowLayer] intValue] != 0) continue;
            const pid_t pid = [info[(id)kCGWindowOwnerPID] intValue];
            if (pid == getpid()) continue;
            NSRunningApplication *app = [NSRunningApplication runningApplicationWithProcessIdentifier:pid];
            if (!app || app.activationPolicy != NSApplicationActivationPolicyRegular) continue;
            NSString *identifier = app.bundleIdentifier ?: app.executableURL.lastPathComponent;
            if (!identifier || [identifier isEqualToString:@"com.encore.clipper"]) continue;
            CGRect bounds{};
            if (!CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)info[(id)kCGWindowBounds], &bounds)
                || bounds.size.width < 200 || bounds.size.height < 120) continue;
            NSString *title = info[(id)kCGWindowName];
            if (!title.length) title = app.localizedName ?: identifier;
            const bool visible = [info[(id)kCGWindowIsOnscreen] boolValue];
            result.push_back({identifier.UTF8String, title.UTF8String,
                [info[(id)kCGWindowNumber] unsignedLongLongValue], pid == focused, !visible});
        }
        CFRelease(array);
    }
    return result;
}

void platform_video_settings(obs_data_t *settings, const Application &application) {
    if (!CGPreflightScreenCaptureAccess()) CGRequestScreenCaptureAccess();
    obs_data_set_int(settings, "type", 1); // Window stream; never substitutes the monitor.
    obs_data_set_int(settings, "window", application.window);
    obs_data_set_string(settings, "application", application.id.c_str());
    obs_data_set_bool(settings, "show_cursor", true);
}

const char *platform_audio_source(bool microphone, bool) {
    return microphone ? "coreaudio_input_capture" : "sck_audio_capture";
}

void platform_audio_settings(obs_data_t *settings, const std::string &selection, bool application) {
    obs_data_set_string(settings, "device_id", selection.c_str());
    obs_data_set_int(settings, "type", application ? 1 : 0);
    if (application) obs_data_set_string(settings, "application", selection.c_str());
}
}
