# Encore

Low-overhead Windows game clipping and multi-track replay studio built with Rust and Tauri 2.

## Download

- **Windows Installer (x64):** [Download Encore (`Encore_0.5.0_x64-setup.exe`)](https://github.com/yancelic/Encore/releases/latest/download/Encore_0.5.0_x64-setup.exe)
- **All Releases:** [GitHub Releases](https://github.com/yancelic/Encore/releases)

> **Requirements:** Windows 10/11 (64-bit). The installer bundles `encore-ui.exe`, the headless `encore-recorder.exe` service, and `ffmpeg.exe`.

## Features

- **Two-process low-overhead architecture:** A pure-Rust background recorder owns the native tray and global save hotkey. Closing the Tauri library exits its UI/WebView2 process while recording continues. Open Encore from the tray to reopen the library; Quit Encore stops both processes. Accepted exports finish before the library exits.
- **Anti-cheat-conscious capture:** Uses Windows Graphics Capture (WGC), WASAPI loopback/input capture, and Toolhelp process snapshots (`CreateToolhelp32Snapshot`) without DLL injection, in-game overlays, or opening handles to game processes (`OpenProcess`).
- **Capture modes:** Automatic **Game Detection** captures the game's main window and stays attached across Alt+Tab and title changes. **Active Window** explicitly follows focus. A minimized game pauses capture; restoration starts a fresh replay history. Neither mode silently falls back to recording the monitor.
- **GPU capture & configurable quality:** WGC/D3D11 capture and Media Foundation H.264 encoding with hardware transforms enabled (actual encoder selection depends on the system), `720p` / `1080p` / `1440p` / `Native` resolution, `30`–`240` FPS, `15s`–`120s` replay buffer, and a customizable global save hotkey (default `Alt+S`).
- **RAM replay history:** Continuously encoded H.264 and three AAC tracks are held in a keyframe-aligned memory buffer. No temporary video segments are written during steady recording. Compressed payloads, including pending saves, share a 512 MiB budget; high-quality settings may retain less than the requested duration. Saving runs on a bounded background worker without restarting the live encoder.
- **3-track audio capture & mixer:** Separate **Game**, **Chat**, and **Microphone** tracks. Choose output-device routing or Windows application process-tree capture (build 20348+), with any chat executable and automatic reconnect after restart. Existing device settings are preserved. Per-track waveforms, volume controls, and mute toggles are available in the editor.
- **Clip library & export tools:** Search, filter by game, favorite, trim clips, export `9:16` vertical videos, build multi-clip montages, and copy clip files straight to the Windows clipboard (`CF_HDROP`).
- **Share the current edit:** Share Clip uses the selected trim range without requiring Save Trim, and combines Game, Chat and Microphone into one stereo audio stream with the selected mute and volume settings. Library Copy also creates a mixed share file. Source clips keep their separate tracks for editing.
- **Signed in-app updates:** Check for updates at startup or in Settings, then confirm download and restart. Encore verifies the package before stopping recording and waits for accepted saves and exports. Users on 0.4.0 or earlier must install 0.5.0 manually once to enable future in-app updates.

## Workspace Structure

- `apps/recorder` — Headless Windows recorder, system tray menu, global hotkey listener, and named-pipe IPC server.
- `apps/ui` — Tauri 2 + React + TypeScript desktop UI and NSIS bundler configuration.
- `crates/common` — Shared configuration (`%APPDATA%\Encore\config.json`), IPC protocol, autostart helper, and utilities.
- `crates/encode` — Keyframe-aligned replay ring buffer.
- `crates/clipstore` — SQLite clip index, thumbnail generator, and waveform extractor.
- `crates/editor` — Multi-track clip trimming, `9:16` vertical export, montage rendering, and clipboard file sharing.

Capture performance diagnostics, RAM replay limits and the repeatable comparison procedure are described in [docs/capture-performance.md](docs/capture-performance.md).

## Building from Source

### Prerequisites

- Windows 10 or Windows 11 (x64)
- Rust toolchain (MSVC)
- Node.js & npm
- `ffmpeg.exe` placed at `tools/ffmpeg.exe` (used for Tauri installer bundling and clip processing/tests)

### Build Commands

The standard Windows distribution is an NSIS setup executable. With dependencies installed and `tools/ffmpeg.exe` present, build the recorder, UI, and installer together:

```powershell
npm --prefix apps/ui ci
npm --prefix apps/ui run build:installer
```

To run those build steps individually:

```powershell
# 1. Build the headless recorder (bundled by Tauri)
cargo build --release --bin encore-recorder

# 2. Install frontend dependencies
npm --prefix apps/ui install

# 3. Build the desktop UI and NSIS installer
npm --prefix apps/ui run tauri build
```

The NSIS installer is written to:

```text
target/release/bundle/nsis/Encore_0.5.0_x64-setup.exe
```

Building creates the local installer; publishing a GitHub release is a separate step.
The installer build also signs the update package and generates `latest.json`.
Signing key setup, publishing instructions, and the one-time transition from
manual installs are described in [docs/updates.md](docs/updates.md).

### Appearance

Settings → Appearance offers **System**, **Light**, and **Dark**. System is the default and follows changes to Windows appearance while Encore is open. A manual choice is remembered on this computer and applies throughout the library, editor, timeline, and dialogs. Theme changes take effect immediately, independently of capture settings.

### Adding games

Settings → Games to capture → **Choose running application** lists open application
windows by title. Find your game and click **Add**, then **Save settings**. The
executable filename is remembered, so changing window titles and restarting the
game do not require adding it again. For a game that is not running, use **Browse
game .exe** and select its actual executable. Manual filename entry remains under
**Enter executable name manually**. Window discovery runs only when you open or
refresh the picker, with no extra background polling or game process handles.

### Tests

```powershell
cargo test --release --workspace
```

## License

Licensed under the [GNU General Public License v3.0](LICENSE).
