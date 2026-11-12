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

References inspected: OBS Studio 32.0.4, commit
`dcdbd2e9048ae66237fdab32dbba72b745b1b9c5`, particularly libobs output/encoder APIs,
mac-capture, linux-pipewire, and linux-capture. OBS sources carry GPL-2.0-or-later
notices. The Encore integration code in this directory is GPL-3.0-only.
