// SPDX-License-Identifier: GPL-3.0-only
#include "platform.hpp"
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <AVFoundation/AVFoundation.h>
#include <unistd.h>

namespace { std::string tray_action; NSStatusItem *tray = nil; NSMenuItem *tray_status = nil; }
@interface EncoreTrayDelegate : NSObject
- (void)activate:(id)sender;
@end
@implementation EncoreTrayDelegate
- (void)activate:(id)sender { tray_action = [[sender representedObject] UTF8String]; }
@end
namespace { EncoreTrayDelegate *tray_delegate = nil; }

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
void platform_shutdown() { platform_set_tray(false, false); [tray_delegate release]; tray_delegate = nil; }
void platform_set_tray(bool visible, bool recording) {
    @autoreleasepool {
        if (!visible) {
            if (tray) { [[NSStatusBar systemStatusBar] removeStatusItem:tray]; [tray release]; tray = nil; }
            tray_status = nil;
            return;
        }
        if (!tray) {
            tray = [[[NSStatusBar systemStatusBar] statusItemWithLength:NSSquareStatusItemLength] retain];
            tray.button.image = [NSImage imageWithSystemSymbolName:@"record.circle" accessibilityDescription:@"Encore"];
            if (!tray.button.image) tray.button.title = @"E";
            if (!tray_delegate) tray_delegate = [[EncoreTrayDelegate alloc] init];
            NSMenu *menu = [[NSMenu alloc] initWithTitle:@"Encore"];
            tray_status = [menu addItemWithTitle:@"Encore — waiting for a game" action:nil keyEquivalent:@""];
            [menu addItem:[NSMenuItem separatorItem]];
            for (NSArray *entry in @[@[@"Save replay", @"save"], @[@"Open library", @"library"],
                                    @[@"Open clips folder", @"folder"], @[@"Quit Encore", @"quit"]]) {
                NSMenuItem *item = [menu addItemWithTitle:entry[0] action:@selector(activate:) keyEquivalent:@""];
                item.target = tray_delegate; item.representedObject = entry[1];
            }
            tray.menu = menu; [menu release];
        }
        tray_status.title = recording ? @"Encore — recording" : @"Encore — waiting for a game";
    }
}
std::string platform_tray_action() { auto action = tray_action; tray_action.clear(); return action; }
bool platform_microphone_ready(std::string &warning) {
    const auto permission = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
    if (permission == AVAuthorizationStatusAuthorized) return true;
    if (permission == AVAuthorizationStatusNotDetermined) {
        static bool requested = false;
        if (!requested) {
            requested = true;
            [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL) {}];
        }
        warning = "Allow Encore Recorder to use the microphone in the system permission dialog.";
    } else warning = "Microphone permission is disabled. Enable Encore Recorder in System Settings > Privacy & Security > Microphone.";
    return false;
}
void platform_configure_hotkey(const std::string &) {
    static bool requested = false;
    if (!CGPreflightListenEventAccess() && !requested) { requested = true; CGRequestListenEventAccess(); }
}
std::string platform_hotkey_warning() {
    return CGPreflightListenEventAccess() ? "" : "Allow Encore Recorder in System Settings > Privacy & Security > Input Monitoring to save replays while another app is focused.";
}
void platform_request_background(bool) {}
obs_data_t *platform_background_status() {
    auto *result = obs_data_create();
    obs_data_set_string(result, "error", "Use the macOS login startup setting");
    return result;
}
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

std::vector<std::string> platform_audio_applications() {
    std::vector<std::string> result;
    @autoreleasepool {
        for (NSRunningApplication *application in NSWorkspace.sharedWorkspace.runningApplications) {
            NSString *identifier = application.bundleIdentifier;
            if (!identifier.length || [identifier hasPrefix:@"com.encore."]
                || application.activationPolicy == NSApplicationActivationPolicyProhibited) continue;
            result.emplace_back(identifier.UTF8String);
        }
    }
    return result;
}
uint64_t platform_audio_identity(const std::string &identifier) {
    @autoreleasepool {
        NSString *bundle = [NSString stringWithUTF8String:identifier.c_str()];
        NSArray *applications = [NSRunningApplication runningApplicationsWithBundleIdentifier:bundle];
        for (NSRunningApplication *application in applications) if (!application.terminated) return application.processIdentifier;
    }
    return 0;
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
