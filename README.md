# Encore

Low-overhead Windows game clipping and multi-track replay studio built with Rust and Tauri 2.

## Download

- **Windows Installer (x64):** [Download Encore (`Encore_0.3.0_x64-setup.exe`)](https://github.com/yancelic/Encore/releases/latest/download/Encore_0.3.0_x64-setup.exe)
- **All Releases:** [GitHub Releases](https://github.com/yancelic/Encore/releases)

> **Requirements:** Windows 10/11 (64-bit). The installer bundles `encore-ui.exe`, the headless `encore-recorder.exe` service, and `ffmpeg.exe`.

## Features

- **Two-process low-overhead architecture:** A pure-Rust headless background recorder (`encore-recorder`) runs without a WebView; the Tauri 2 desktop UI (`encore-ui`) opens only when you browse or edit clips.
- **Anti-cheat-conscious capture:** Uses Windows Graphics Capture (WGC), WASAPI loopback/input capture, and Toolhelp process snapshots (`CreateToolhelp32Snapshot`) without DLL injection, in-game overlays, or opening handles to game processes (`OpenProcess`).
- **Capture modes:** Automatic **Game Detection** (with an editable list of game executables) or **Active Window** capture.
- **Hardware-accelerated encoding & configurable quality:** Media Foundation hardware H.264 encoding, `720p` / `1080p` / `1440p` / `Native` resolution, `30`–`240` FPS, `15s`–`120s` replay buffer, and a customizable global save hotkey (default `Alt+S`).
- **3-track audio capture & mixer:** Separate **Game**, **Chat**, and **Microphone** audio tracks with per-track waveforms, volume controls, and mute toggles in the clip editor.
- **Clip library & export tools:** Search, filter by game, favorite, trim clips, export `9:16` vertical videos, build multi-clip montages, and copy clip files straight to the Windows clipboard (`CF_HDROP`).

## Workspace Structure

- `apps/recorder` — Headless Windows recorder, system tray menu, global hotkey listener, and named-pipe IPC server.
- `apps/ui` — Tauri 2 + React + TypeScript desktop UI and NSIS bundler configuration.
- `crates/common` — Shared configuration (`%APPDATA%\Encore\config.json`), IPC protocol, autostart helper, and utilities.
- `crates/encode` — Keyframe-aligned replay ring buffer.
- `crates/clipstore` — SQLite clip index, thumbnail generator, and waveform extractor.
- `crates/editor` — Multi-track clip trimming, `9:16` vertical export, montage rendering, and clipboard file sharing.

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
target/release/bundle/nsis/Encore_0.3.0_x64-setup.exe
```

Building creates the local installer; publishing a GitHub release is a separate step.

### Appearance

Settings → Appearance offers **System**, **Light**, and **Dark**. System is the default and follows changes to Windows appearance while Encore is open. A manual choice is remembered on this computer and applies throughout the library, editor, timeline, and dialogs. Theme changes take effect immediately, independently of capture settings.

### Tests

```powershell
cargo test --release --workspace
```

## License

Licensed under the [GNU General Public License v3.0](LICENSE).
