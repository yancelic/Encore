# Native capture engine

This directory integrates libobs through a small C ABI. OBS supplies native GPU
capture and encoders; Encore retains its existing bounded Rust replay buffer and
save queue. No OBS Studio frontend or WebView is kept running by the recorder.

`encore_capture.h` is the ownership boundary. Packet payloads and codec metadata
are borrowed during callbacks and copied into Encore-owned storage. OBS objects
never cross it. `ec_mux` writes H.264 plus three AAC tracks through libavformat;
it runs on the save worker without stopping or restarting the live encoders.

Integration is under development. Do not advertise native recording until the
engine, platform runtime packaging, and real-device capture tests pass.

The single captured window feeds OBS output channel 0 directly. Do not wrap it
in a scene with relative coordinates: the canvas starts at 1920x1080 before the
source is ready, and resizing it to the window's actual dimensions would also
scale and displace the scene item. Output resolution is applied only by the
video output downscaler. The synthetic `--self-test` draws a 1280x720 source
with four colored corner markers and encodes at 640x360; `verify-self-test.py`
checks that all four corners survive the canvas resize and downscale. A uniform
framebuffer clear alone cannot detect this regression. The fixture stays below
1080p so GitHub's software-rendered virtual Macs can keep up with the test.

On a real Mac, also verify Native and 1080p clips of a Retina game window with
visible content at all four edges, including after resizing the window or
switching between windowed and fullscreen mode. Both settings must preserve the
whole window and its aspect ratio.

References inspected: OBS Studio 32.0.4, commit
`dcdbd2e9048ae66237fdab32dbba72b745b1b9c5`, particularly libobs output/encoder APIs,
mac-capture, linux-pipewire, and linux-capture. OBS sources carry GPL-2.0-or-later
notices. The Encore integration code in this directory is GPL-3.0-only.

Additional macOS design reference: [OpenScreenStudio](https://github.com/Glyph-Software/OpenScreenStudio/tree/9b257aca7c44d7a3da5034fb72658e3b0cf2d35a),
especially its application privacy descriptions and permission-before-capture
flow. That project is AGPL-3.0. Its implementation is not incorporated here;
Encore's permission guards and bundle changes are independently implemented
using the public Apple APIs. The main app and its background helper both declare
their microphone/screen usage because macOS can attribute child-process access
to the responsible application.
