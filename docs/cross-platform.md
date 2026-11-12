# Cross-platform development

Work on `dev`. `main` is the production branch. Preview workflows upload development artifacts and do not create releases, update `latest.json`, or publish to Flathub.

## Targets and current status

| Target | Package | Current implementation |
| --- | --- | --- |
| Windows x64 | Existing signed NSIS installer | Full recorder, library, editor, and signed updates |
| Apple Silicon macOS | ARM64 `.app` / `.dmg` | Library/editor preview; native recording pending |
| Ubuntu 24.04+ x64 | `.deb` | Library/editor preview; native recording pending |
| Linux x64 via Flatpak | Development `.flatpak` bundle | Packaging manifest and workflow; native recording pending |

Intel Macs and Linux ARM64 are outside the initial scope. The initial macOS preview targets macOS 15+ to match the ARM64 build environment and bundled Homebrew dependencies; supporting older releases requires separately built and verified dependencies. Preview artifacts must pass real-device checks before being presented as supported releases.

## Platform boundaries

- `crates/common/src/platform/windows`: existing Registry autostart, Toolhelp process metadata, named-pipe IPC, and WASAPI device discovery.
- `crates/common/src/ipc.rs`: shared JSON request/response types, independent of transport.
- `crates/common/src/platform/mod.rs`: configuration/video paths, resource lookup, helper processes, file-manager integration, and capability reporting.
- `apps/recorder/src/windows.rs`: the existing Windows recorder entry point and capture modules. Non-Windows builds explicitly report unavailable recording and exit unsuccessfully; they do not simulate capture.
- `apps/ui/src-tauri/src/platform`: native picker and updater implementations. Portable previews use manual/package-manager updates and never query the production Windows updater feed.
- `crates/editor/src/clipboard`: native file clipboard implementations. Windows uses CF_HDROP, macOS uses AppleScript with the filename passed as an argument. Linux clipboard sharing is disabled in the first preview; use Save Trim and Show in folder.

Windows configuration and clip IDs retain their existing behavior. macOS uses its Application Support directory; Linux follows XDG configuration/video paths, including Flatpak's redirected config directory. Unix clip identifiers preserve case-sensitive filenames.

The React app reads capabilities from Rust. Preview builds expose library, folder selection, themes, audio mixing, trimming, crop/vertical export, and montage. They hide recording controls, application capture settings, login startup, and the Windows updater.

## Build a preview

Common prerequisites: Rust, Node.js, npm, and FFmpeg with H.264/AAC support and the `libx264` encoder. Native packages and GUI playback still need to be built and exercised on their target systems; a cross-target `cargo check` of shared crates is not a complete application test.

### Apple Silicon

On an ARM64 Mac with Xcode Command Line Tools:

```sh
brew install ffmpeg
npm --prefix apps/ui ci
python3 apps/ui/scripts/prepare-macos-ffmpeg.py
npm --prefix apps/ui run build:preview
```

The preparation script copies FFmpeg and its non-system Homebrew dylibs, rewrites load paths, verifies ARM64 support, and applies ad-hoc signatures. The preview bundles these resources so users do not need Homebrew. This is not Apple Developer ID signing or notarization. Resolve and sign all bundled components with the release identity before a notarized public release.

### Ubuntu

```sh
sudo apt-get update
sudo apt-get install -y build-essential libwebkit2gtk-4.1-dev libssl-dev librsvg2-dev libxdo-dev ffmpeg gstreamer1.0-libav gstreamer1.0-plugins-good gstreamer1.0-plugins-bad
npm --prefix apps/ui ci
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

This local development manifest packages the compiled binary against GNOME runtime 49 and builds pinned x264/FFmpeg sources inside the SDK. The manifest grants the Videos directory; other locations require the folder chooser/portal or an explicit permission. FFmpeg cannot access files outside the sandbox merely because the host can. A Flathub submission will require a source-build manifest with vendored Cargo/npm dependencies and store metadata; the local binary manifest is not a Flathub submission.

## CI and publication

`platform-checks.yml` checks the native workspace and runs shared/media/library tests on Windows, Apple Silicon, and Ubuntu. `ui-checks.yml` also runs on pushes to `dev`. To build downloadable development artifacts, manually run **Platform checks** on `dev` with `build_previews` enabled.

Once recording backends and native acceptance checks pass, attach each platform package to one versioned GitHub Release. Keep development previews away from the existing production `latest.json`. Flatpak updates belong to its repository/package manager.

## Remaining recording work, in order

1. Extract platform-neutral codec metadata from `ReplaySnapshot`; it currently contains `IMFMediaType`. Preserve keyframe alignment, PTS/DTS, bounded memory, and pending-save ownership.
2. Add a native macOS recorder: ScreenCaptureKit, VideoToolbox, microphone/system-audio permissions and routing, native background lifecycle, tray/menu integration, and a local IPC transport. Start with explicit source selection and system audio + microphone. Do not advertise separate Game/Chat until routing is verified.
3. Add a Linux recorder: ScreenCast portal + PipeWire for Wayland, native audio routing, encoder probing and hardware paths, GlobalShortcuts portal support with capability detection, and sandbox-compatible IPC/background lifecycle. Treat X11 as a separate configuration. Do not silently substitute monitor capture for a selected window.
4. Validate recording on devices: 1080p60, 60-second bounded RAM replay, repeated saves without encoder restart, audio synchronization, permissions denied/revoked, game restart, minimize/restore, display changes, sleep/wake, and recording while the library is closed.
5. Test native playback, mixed exports, Flatpak folder permissions, clean installs/upgrades, and signing; then publish packages.

## References

- [Tauri platform configuration](https://v2.tauri.app/reference/config/)
- [Tauri Flatpak packaging](https://v2.tauri.app/distribute/flatpak/)
- [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit/)
- [VideoToolbox](https://developer.apple.com/documentation/videotoolbox/)
- [ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
- [GlobalShortcuts portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)
