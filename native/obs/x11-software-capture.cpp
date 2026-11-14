// SPDX-License-Identifier: GPL-3.0-only
// Read the selected window's own Composite pixmap when EGL pixmap import is unavailable.
#include <obs-module.h>
#include <util/platform.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/Xcomposite.h>
#include <atomic>
#include <chrono>
#include <thread>

namespace {
struct Capture {
    obs_source_t *source;
    Window window;
    unsigned fps;
    std::atomic<bool> stop{false};
    std::thread worker;

    void run() {
        Display *display = XOpenDisplay(nullptr);
        if (!display) return;
        int event = 0, error = 0;
        if (!XCompositeQueryExtension(display, &event, &error)) { XCloseDisplay(display); return; }
        XCompositeRedirectWindow(display, window, CompositeRedirectAutomatic);
        XSync(display, False);
        Pixmap pixmap = None;
        int width = 0, height = 0;
        const auto interval = std::chrono::nanoseconds(1000000000ULL / fps);
        while (!stop.load(std::memory_order_acquire)) {
            const auto start = std::chrono::steady_clock::now();
            XWindowAttributes attributes{};
            if (!XGetWindowAttributes(display, window, &attributes) || attributes.map_state != IsViewable) {
                std::this_thread::sleep_for(std::chrono::milliseconds(100));
                continue;
            }
            if (attributes.width < 2 || attributes.height < 2
                || static_cast<uint64_t>(attributes.width) * attributes.height > 33554432) break;
            if (!pixmap || attributes.width != width || attributes.height != height) {
                if (pixmap) XFreePixmap(display, pixmap);
                pixmap = XCompositeNameWindowPixmap(display, window);
                width = attributes.width; height = attributes.height;
            }
            XImage *image = pixmap ? XGetImage(display, pixmap, 0, 0, width, height, AllPlanes, ZPixmap) : nullptr;
            if (image) {
                if (image->bits_per_pixel == 32 && image->byte_order == LSBFirst
                    && attributes.visual->red_mask == 0xff0000 && attributes.visual->green_mask == 0xff00 && attributes.visual->blue_mask == 0xff) {
                    obs_source_frame frame{};
                    frame.data[0] = reinterpret_cast<uint8_t *>(image->data);
                    frame.linesize[0] = static_cast<uint32_t>(image->bytes_per_line);
                    frame.width = static_cast<uint32_t>(width); frame.height = static_cast<uint32_t>(height);
                    frame.format = VIDEO_FORMAT_BGRX;
                    frame.timestamp = os_gettime_ns();
                    obs_source_output_video(source, &frame); // OBS copies the bytes before this returns.
                }
                XDestroyImage(image);
            } else if (pixmap) { XFreePixmap(display, pixmap); pixmap = None; }
            std::this_thread::sleep_until(start + interval);
        }
        if (pixmap) XFreePixmap(display, pixmap);
        XCompositeUnredirectWindow(display, window, CompositeRedirectAutomatic);
        XCloseDisplay(display);
    }
};
const char *name(void *) { return "Encore selected window (CPU transfer)"; }
void *create(obs_data_t *settings, obs_source_t *source) {
    auto *capture = new Capture{};
    capture->source = source;
    capture->window = static_cast<Window>(obs_data_get_int(settings, "window"));
    capture->fps = static_cast<unsigned>(obs_data_get_int(settings, "fps"));
    if (!capture->window || capture->fps < 1 || capture->fps > 240) { delete capture; return nullptr; }
    obs_source_set_async_unbuffered(source, true);
    capture->worker = std::thread([capture] { capture->run(); });
    return capture;
}
void destroy(void *data) {
    auto *capture = static_cast<Capture *>(data);
    capture->stop.store(true, std::memory_order_release);
    if (capture->worker.joinable()) capture->worker.join();
    delete capture;
}
}
void encore_register_x11_software_capture() {
    obs_source_info source{};
    source.id = "encore_x11_window";
    source.type = OBS_SOURCE_TYPE_INPUT;
    source.output_flags = OBS_SOURCE_ASYNC_VIDEO | OBS_SOURCE_DO_NOT_DUPLICATE;
    source.get_name = name; source.create = create; source.destroy = destroy;
    obs_register_source(&source);
}
