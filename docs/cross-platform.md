# Cross-platform development

Work on `dev`. `main` is the production branch. Preview workflows upload development artifacts and do not create releases, update `latest.json`, or publish to Flathub.

## Targets and current status

| Target | Package | Current implementation |
| --- | --- | --- |
| Windows x64 | Existing signed NSIS installer | Full recorder, library, editor, and signed updates |
| Apple Silicon macOS | ARM64 `.app` / `.dmg` | Experimental OBS recorder; packaged encoder/replay tests pass; real-device startup failure under investigation |
| Ubuntu 24.04 x64 | `.deb` | Experimental OBS recorder, X11/Wayland capture, PipeWire application audio; native acceptance tests in progress |
| Linux x64 via Flatpak | Development `.flatpak` bundle | Source-built OBS runtime and portal integration; sandbox build/test still pending |

Intel Macs and Linux ARM64 are outside the initial scope. The initial macOS preview targets macOS 15+ to match the ARM64 build environment and bundled Homebrew dependencies; supporting older releases requires separately built and verified dependencies. Preview artifacts must pass real-device checks before being presented as supported releases.

## Platform boundaries

- `crates/common/src/platform/windows`: existing Registry autostart, Toolhelp process metadata, named-pipe IPC, and WASAPI device discovery.
- `crates/common/src/ipc.rs`: shared JSON request/response types, independent of transport.
- `crates/common/src/platform/mod.rs`: configuration/video paths, resource lookup, helper processes, file-manager integration, and capability reporting.
- `apps/recorder/src/windows.rs`: the existing Windows recorder entry point and capture modules.
- `apps/recorder/src/native`: Unix supervisor, bounded save queue, native FFI ownership, IPC, and graceful shutdown.
- `native/obs`: OBS window/audio sources and encoders, the MP4 mux bridge, native menu lifecycle, and Linux desktop portals. The OBS GUI is not launched.
- `apps/ui/src-tauri/src/platform`: native picker and updater implementations. Portable previews use manual/package-manager updates and never query the production Windows updater feed.
- `crates/editor/src/clipboard`: Windows CF_HDROP and macOS AppleScript file sharing. Linux sharing runs through `apps/ui/src-tauri/src/platform/linux_clipboard.rs` on the GTK main thread, with file MIME targets and the File Transfer portal for sandbox access.

Windows configuration and clip IDs retain their existing behavior. macOS uses its Application Support directory; Linux follows XDG configuration/video paths, including Flatpak's redirected config directory. Unix clip identifiers preserve case-sensitive filenames.

The React app reads capabilities from Rust. Builds with the native runtime expose experimental recording, application audio, and login startup. Builds without it retain the library/editor preview. Native builds use manual/package-manager updates and never query the Windows updater feed. Wayland uses the system window-sharing dialog and Global Shortcuts portal; it cannot follow arbitrary focused windows. Select the game's audio application explicitly on Wayland because the window portal does not identify its owner.

## Build a preview

Common prerequisites: Rust, Node.js, npm, and FFmpeg with H.264/AAC support and the `libx264` encoder. Native packages and GUI playback still need to be built and exercised on their target systems; a cross-target `cargo check` of shared crates is not a complete application test.

### Apple Silicon

On an ARM64 Mac with Xcode Command Line Tools:

```sh
brew install ffmpeg cmake pkg-config
npm --prefix apps/ui ci
python3 apps/ui/scripts/prepare-macos-ffmpeg.py
python3 native/obs/prepare-macos.py
npm --prefix apps/ui run build:preview
```

The preparation script copies FFmpeg and its non-system Homebrew dylibs, rewrites load paths, verifies ARM64 support, and applies ad-hoc signatures. The preview bundles these resources so users do not need Homebrew. This is not Apple Developer ID signing or notarization. Resolve and sign all bundled components with the release identity before a notarized public release.

### Ubuntu

```sh
sudo apt-get update
sudo apt-get install -y build-essential cmake pkg-config libwebkit2gtk-4.1-dev libssl-dev librsvg2-dev libxdo-dev ffmpeg gstreamer1.0-libav gstreamer1.0-plugins-good gstreamer1.0-plugins-bad libobs-dev obs-studio libavformat-dev libavcodec-dev libavutil-dev libgtk-3-dev libwayland-dev libayatana-appindicator3-dev libpipewire-0.3-dev
npm --prefix apps/ui ci
python3 native/obs/prepare-linux.py
npm --prefix apps/ui run build:preview
```

The `.deb` declares FFmpeg and playback codec dependencies. Build on Ubuntu 24.04 to retain that glibc baseline. Linux playback uses WebKitGTK/GStreamer and needs native verification, beyond Chromium-based frontend tests.

### Flatpak

First build the Ubuntu preview, then install `flatpak` and `flatpak-builder`:

```sh
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
bash packaging/flatpak/build.sh
flatpak install --user target/Encore-linux-x86_64-dev.flatpak
flatpak run com.encore.clipper
```

This development manifest packages the compiled Rust binaries against GNOME runtime 51 and builds pinned x264, FFmpeg, OBS, application-audio and native-bridge sources inside the SDK. The manifest grants the Videos directory; other locations require the folder chooser/portal or an explicit permission. FFmpeg cannot access files outside the sandbox merely because the host can. A Flathub submission will require vendored Cargo/npm source builds and store metadata; the development binary manifest is not a Flathub submission.

## CI and publication

`platform-checks.yml` checks the native workspace and runs shared/media/library tests on Windows, Apple Silicon, and Ubuntu. Pushes to `dev` also build native development packages and verify relocated packages. The Mac OBS job uses macOS 26; baseline library tests also cover macOS 15. `ui-checks.yml` runs on pushes to `dev`.

Once recording backends and native acceptance checks pass, attach each platform package to one versioned GitHub Release. Keep development previews away from the existing production `latest.json`. Flatpak updates belong to its repository/package manager.

## Remaining recording work, in order

1. Resolve the reported Mac recorder shutdown and retest on the user's Tahoe 26.6.1 device. Both the main app and its helper declare capture usage and microphone entitlements; denied screen permission leaves capture idle. The package test checks the declarations and exercises the normal menu lifecycle as well as synthetic encoding.
2. Finish native Linux acceptance checks: actual controlled X11 window capture, GTK file clipboard formats, and the Flatpak sandbox build/replay test.
3. Validate recording on devices: 1080p60, 60-second bounded RAM replay, repeated saves without encoder restart, separate game/chat/microphone routing, audio synchronization, permissions denied/revoked, application restart, minimize/restore, display changes, sleep/wake, and recording while the library is closed. Ubuntu version, GPU and desktop session details are still pending from the user.
4. Test native playback, mixed exports, Flatpak folder/file-transfer permissions, clean installs/upgrades, and signing before publishing supported packages.

## Mac recorder diagnostics

Install development packages in Applications before testing. The recorder log is
`~/Library/Application Support/Encore/recorder.log`; launches append to it and
the library records the helper's exit status while it remains running. The log
includes startup phases and the helper's bundle identity/privacy declarations.
For a crash after startup, share the end of this log and, if necessary, the
matching macOS DiagnosticReports crash report. A successful synthetic test does
not establish screen/microphone permission or real capture correctness.

## References

- [OpenScreenStudio design reference, pinned revision](https://github.com/Glyph-Software/OpenScreenStudio/tree/9b257aca7c44d7a3da5034fb72658e3b0cf2d35a) — AGPL-3.0; consulted for application permission/lifecycle design, without incorporating its implementation.
- [Apple capture authorization requirements](https://developer.apple.com/documentation/avfoundation/requesting-authorization-to-capture-and-save-media)

- [Tauri platform configuration](https://v2.tauri.app/reference/config/)
- [Tauri Flatpak packaging](https://v2.tauri.app/distribute/flatpak/)
- [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit/)
- [VideoToolbox](https://developer.apple.com/documentation/videotoolbox/)
- [ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
- [GlobalShortcuts portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)
