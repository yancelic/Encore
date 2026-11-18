# Cross-platform development

Work on `dev`. `main` is the production branch. Pushes and pull requests run checks but never publish a beta. Development prereleases require an explicit manual Actions run from `dev` and are separate from the Windows and Apple Silicon macOS stable releases. Ubuntu/Flatpak have not yet been tested on real user devices. Preview publication does not publish to Flathub or update `latest.json`.

## Targets and current status

| Target | Package | Current implementation |
| --- | --- | --- |
| Windows x64 | Existing signed NSIS installer | Full recorder, library, editor, and signed updates |
| Apple Silicon macOS | ARM64 `.app` / `.dmg` | Stable since 0.6.0; OBS recorder, separate audio tracks; signed in-app updates from the first release signed with the stable `Encore Release` certificate, without Developer ID or notarization |
| Ubuntu 24.04 x64 | `.deb` | Experimental OBS recorder, X11/Wayland capture, PipeWire application audio; native acceptance tests in progress |
| Linux x64 via Flatpak | Development `.flatpak` bundle | Source-built OBS runtime; CI build, install and synthetic sandbox replay verified; real-device portal/playback checks pending |

Intel Macs and Linux ARM64 are outside the initial scope. The initial macOS preview targets macOS 15+ to match the ARM64 build environment and bundled Homebrew dependencies; supporting older releases requires separately built and verified dependencies. Preview artifacts must pass real-device checks before being presented as supported releases.

## Platform boundaries

- `crates/common/src/platform/windows`: existing Registry autostart, Toolhelp process metadata, named-pipe IPC, and WASAPI device discovery.
- `crates/common/src/ipc.rs`: shared JSON request/response types, independent of transport.
- `crates/common/src/platform/mod.rs`: configuration/video paths, resource lookup, helper processes, file-manager integration, and capability reporting.
- `apps/recorder/src/windows.rs`: the existing Windows recorder entry point and capture modules.
- `apps/recorder/src/native`: Unix supervisor, bounded save queue, native FFI ownership, IPC, and graceful shutdown.
- `native/obs`: OBS window/audio sources and encoders, the MP4 mux bridge, native menu lifecycle, and Linux desktop portals. The OBS GUI is not launched.
- `apps/ui/src-tauri/src/platform`: native picker and updater implementations. Windows and stable macOS share the signed updater; Linux and beta previews use manual/package-manager updates and never query the production feed.
- `crates/editor/src/clipboard`: Windows CF_HDROP and macOS AppleScript file sharing. Linux sharing runs through `apps/ui/src-tauri/src/platform/linux_clipboard.rs` on the GTK main thread, with file MIME targets and the File Transfer portal for sandbox access.

Windows configuration and clip IDs retain their existing behavior. macOS uses its Application Support directory; Linux follows XDG configuration/video paths, including Flatpak's redirected config directory. Unix clip identifiers preserve case-sensitive filenames.

The React app reads capabilities from Rust. Builds with the native runtime expose experimental recording, application audio, and login startup. Builds without it retain the library/editor preview. Stable Apple Silicon builds use the signed in-app updater; Linux and beta native builds use manual/package-manager updates and never query the production feed. Wayland uses the system window-sharing dialog and Global Shortcuts portal; it cannot follow arbitrary focused windows. Select the game's audio application explicitly on Wayland because the window portal does not identify its owner.

### Login startup

The setting starts the **recorder** in the tray/menu bar after the user signs in;
it does not open the library or run a system service before login. Save Settings
after changing the switch.

Windows stores a quoted, standard drive/UNC path in the per-user `Run` value
`EncoreRecorder`. The library refreshes enabled registrations on launch so old
extended-length (`\\?\`) paths and paths from a previous install are repaired.
Windows Startup Apps can separately disable the entry; the repair does not
override that approval. The recorder retries a failed tray registration every two
seconds until Explorer is ready, then stops the timer. A hidden top-level tool
window receives `TaskbarCreated` and restores the icon after Explorer restarts.
An explicitly hidden tray stays hidden during recovery.

macOS writes `~/Library/LaunchAgents/com.encore.recorder.plist` with `RunAtLoad`.
launchd runs the executable inside the inner **Encore Recorder.app** directly, so the
recorder is its own responsible process and keeps the Encore Recorder privacy identity,
as with a manual launch. Because the executable lives inside `Encore.app`, Login Items
lists the entry as **Encore** even without a Developer ID Team ID (launching through
`/usr/bin/open` would list it as "open"). Disabling
the setting removes the plist for future logins; it does not stop an active recorder.
Keep Encore installed at the registered location (normally `/Applications`) and
allow its background item in System Settings if macOS requests approval. Screen,
audio and microphone permissions still need to be granted. Login/menu-bar behavior
requires a real Mac login test; Windows tests only verify the generated plist content.

## Build a preview

Common prerequisites: Rust, Node.js, npm, and FFmpeg with H.264/AAC support and the `libx264` encoder. Native packages and GUI playback still need to be built and exercised on their target systems; a cross-target `cargo check` of shared crates is not a complete application test.

macOS recording uses OBS's VideoToolbox H.264 encoder with software fallback.
Share, vertical/crop exports and montage transcodes prefer FFmpeg's
`h264_videotoolbox` on macOS, then fall back to `libx264` if unavailable.
NVENC is NVIDIA's encoder implementation, not the H.264 video format itself;
macOS does not attempt NVENC or pass NVIDIA-specific encoder settings.

### Apple Silicon

On an ARM64 Mac with Xcode Command Line Tools:

```sh
brew install ffmpeg cmake pkg-config
npm --prefix apps/ui ci
python3 apps/ui/scripts/prepare-macos-ffmpeg.py
python3 native/obs/prepare-macos.py
npm --prefix apps/ui run setup:macos-signing   # once per Mac
npm --prefix apps/ui run build:preview
```

Before the first build on a Mac, create a local signing identity once:

```sh
npm --prefix apps/ui run setup:macos-signing
```

macOS remembers privacy permissions by code signature. Ad-hoc builds get a new
signature on every rebuild, so Screen Recording, Microphone and Input Monitoring
would have to be granted again after each build, and old entries remain in System
Settings without applying to the new build. The packaging scripts sign with the
"Encore Local Development" certificate when it exists (or `ENCORE_CODESIGN_IDENTITY`
/ `APPLE_SIGNING_IDENTITY` when set), so permissions granted once survive rebuilds.
Without it they fall back to ad-hoc signing and print a warning.

### macOS permissions

All capture permissions belong to **Encore Recorder** (`com.encore.recorder`). The
library launches the recorder through LaunchServices, the same way the login agent
does, so macOS never files a request under the library app (`com.encore.clipper`).
Settings → Permissions (macOS only) shows Screen & System Audio Recording,
Microphone and Input Monitoring, requests each one through the recorder, opens the
matching System Settings pane once a decision exists, and restarts the recorder
after a screen-recording grant. **Reset permissions** runs `tccutil reset All` for
both bundle identifiers, which also removes stale entries left by earlier ad-hoc
builds (for example a Microphone switch that cannot be turned on).

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

Wayland save shortcuts use the Global Shortcuts portal when the desktop provides
it. Encore translates letter, navigation, punctuation, keypad and media keys to
the case-sensitive XDG trigger format; the desktop may let the user choose a
different binding. If the portal is unavailable or permission is denied, use
Save replay in the library/menu, or assign a desktop shortcut to
`encore-recorder --save`. The `.deb` recorder helper is bundled in the app's
resource directory, so use its installed absolute path when setting that command.
X11 uses OBS's global hotkey backend.

The native Linux build also provides a shortcut regression test, including X11
keysym validation without connecting to a display:

```sh
ctest --test-dir target/native-obs-linux/bridge-build --output-on-failure
```

This verifies trigger conversion; compositor permission dialogs and shortcut
activation still require a real Wayland session. The format follows the
[XDG Shortcuts specification](https://specifications.freedesktop.org/shortcuts/latest/)
used by the [Global Shortcuts portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html).

### Flatpak

First build the Ubuntu preview, then install `flatpak` and `flatpak-builder`:

```sh
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
bash packaging/flatpak/build.sh
flatpak install --user target/Encore-linux-x86_64-dev.flatpak
flatpak run com.encore.clipper
```

This development manifest packages the compiled Rust binaries against GNOME runtime 51 and builds pinned x264, FFmpeg, OBS, application-audio and native-bridge sources inside the SDK. The manifest grants the Videos directory; other locations require the folder chooser/portal or an explicit permission. FFmpeg cannot access files outside the sandbox merely because the host can. A Flathub submission will require vendored Cargo/npm source builds and store metadata; the development binary manifest is not a Flathub submission.

The GNOME SDK build includes pinned KDE Extra CMake Modules for OBS's build-time
dependency. The bridge uses OBS's installed CMake target for its public headers
and dependencies; OBS's generated pkg-config file can leak C-only compiler flags
into C++ builds.

After installing the bundle, CI runs `bash packaging/flatpak/test.sh` under Xvfb.
It checks that the UI, recorder and native bridge can resolve their dynamic
libraries in the runtime, then records a synthetic source inside the sandbox and
verifies moving H.264 frames, geometry, A/V timing and three isolated AAC tones.
The test needs host `ffmpeg`, `xvfb` and `dbus-x11`. It does not establish real
Wayland capture, portal permissions, hardware encoder or GUI playback correctness.
Ubuntu and Flatpak real-device testing remains pending.

For packaging-only iteration, `platform-checks.yml` accepts the optional manual
input `linux_artifact_run_id`. It reuses `encore-obs-bridge-linux-dev` from a
successful Linux OBS job in that run, verifies the current Rust/UI source matches
the artifact commit, and runs only Flatpak. This is a check-only shortcut and
cannot be combined with `publish_beta`. Leave it empty for full platform checks.
For example:

```sh
gh workflow run platform-checks.yml --ref dev -f linux_artifact_run_id=RUN_ID
```

## CI and publication

`platform-checks.yml` checks the native workspace and runs shared/media/library tests on Windows, Apple Silicon, and Ubuntu. Pushes to `dev` also build native development packages and verify relocated packages. The Mac OBS job uses macOS 26; baseline library tests also cover macOS 15. `ui-checks.yml` runs on pushes to `dev`.

Only pushes to `dev` and `main` save build caches. Pull request caches are private to
that pull request and would evict shared entries from the repository's 10 GB cache
quota. Pull requests restore from their base branch or `main`, the default branch; stable
tag builds restore the Apple Silicon bundle cache from `main`. Package installs are
noninteractive and time out after 10 minutes instead of holding a job for an hour.

Pushes, PRs, numbered beta tag pushes and ordinary manual checks keep the source
package version and do not publish. `build_previews` still adds development build
artifacts to manual checks; it is not permission to release them.

### Publish a development preview manually

In **Actions → Platform checks → Run workflow**:

1. Select the **dev** branch.
2. Enable **publish_beta**.
3. Set **beta_target** to a numeric `X.Y.Z` strictly newer than the latest stable
   release, without a `-beta.N` suffix. For stable `0.6.1`, a patch preview can use
   `0.6.2` and a feature preview can use `0.7.0`.
4. Leave **linux_artifact_run_id** empty. **build_previews** is optional; the
   publication path already builds and verifies all required packages.

Equivalent command, only when a publication is intended:

```sh
gh workflow run platform-checks.yml --ref dev -f publish_beta=true -f beta_target=0.6.2
```

The plan rejects non-dev publication, artifact reuse, invalid versions and targets
at or below the latest stable (including `0.6.1-beta.N` after stable `0.6.1`). It
also rejects a target below the source version. The generated package version is
`<beta_target>-beta.<workflow-run-number>`. Version overrides apply only inside
the build jobs; they do not commit a version change to the repository.

Publication requires successful Windows/macOS/Ubuntu regression checks, macOS and
Ubuntu package verification, controlled X11 capture and Flatpak sandbox replay.
Failed, cancelled or skipped required jobs cannot publish. Artifacts must come
from that same run; staged version, source/run metadata and checksums are checked
before release writes. The latest stable version is checked again immediately
before publishing, so a stable promotion during the build can block the preview.

The authorized run creates or updates only the rolling `beta` prerelease, marked
as a development preview. Numbered prereleases and stable releases are not
modified. `BUILD.json` identifies the source/run, and SHA256 checksums cover its
assets. macOS support remains stable; its preview package is for testing a future
version. The stable Windows installer and `latest.json` feed are separate.

To run checks without publishing, keep **publish_beta** off. A successful check
does not create a release, and a missing rolling `beta` release is a valid state.

Once recording backends and native acceptance checks pass, attach each platform package to one versioned GitHub Release. Keep development previews away from the existing production `latest.json`. Flatpak updates belong to its repository/package manager.

## Validation limits

Automated checks cover controlled X11 capture, native replay, GTK clipboard
formats, and synthetic Flatpak sandbox recording. Ubuntu and Flatpak remain beta:
real-device capture, Wayland portals, hardware encoders, application audio,
playback, and sandbox file transfer still require verification. Consult the
[Linux beta test guide](linux-beta-testing.md) when reporting a problem.

macOS is stable on supported Apple Silicon Macs. Capture permissions and real
login behavior require validation on the target device; generated permission or
LaunchAgent data alone does not establish that those operating-system flows work.

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

## Stable packages

Stable releases use a matching `vX.Y.Z` tag and package versions.
`stable-packages.yml` builds the Apple Silicon DMG and the signed update archive
with the stable `Encore Release` certificate, verifies native replay, checks the
bundle and update signatures and exercises the relocated recorder before uploading
a CI artifact. This does not publish a release automatically. Stage that verified
DMG with the signed Windows installer, `.sig`, `latest.json` and checksums in a
draft versioned release, verify the assets, then publish it as latest. See
[the updater release procedure](updates.md) and [0.6.1 notes](releases/v0.6.1.md).
