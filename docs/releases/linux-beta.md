An experimental Ubuntu recording preview for testing and feedback.

**Target: Ubuntu 24.04+ x64 (amd64).** Download the `.deb` asset below. This is a
beta and still needs real-device testing across GPUs, desktops and audio setups.

### Install

In the directory containing the downloaded package:

```sh
sudo apt install ./Encore_*_amd64.deb
```

Launch **Encore** from the applications menu, or run `encore-ui` in a terminal.
APT installs the required runtime dependencies. Rust and Node.js are not needed.

### Included

- Experimental X11 window capture and Wayland window selection through the
  system sharing dialog, with a bounded RAM replay buffer and background saves.
- Separate game, chat and microphone tracks, plus the clip library and editor.
- Wayland shortcut conversion fixes for letters, navigation, punctuation, keypad
  and media keys using the XDG shortcut format.

### Validation and known limits

- Built on Ubuntu 24.04; native encoder/replay tests, relocated-package tests,
  controlled X11 window capture, GTK file clipboard and shared Rust tests must
  pass before this release is published.
- Real Wayland capture, portal permission dialogs, hardware encoders and
  application audio routing still need testing on users' devices.
- The desktop must provide the Global Shortcuts portal for Wayland save
  shortcuts. If it is unavailable or denied, use **Save replay** in the library.
  Wayland cannot follow arbitrary focused windows; select the recording window
  and the game's audio application explicitly.
- This beta provides a `.deb` only. Flatpak validation is incomplete.
- This prerelease uses manual installation and does not ship a Windows updater
  manifest. The stable Windows release remains available separately.

The attached **TEST-NOTU.md** contains a Turkish test checklist and log locations.
When reporting a problem, include Ubuntu version, desktop/session type,
GPU/driver, recording settings, reproduction steps and the recorder log tail.
Logs are in `${XDG_CONFIG_HOME:-$HOME/.config}/Encore/recorder.log`.

Use **Quit Encore** from the application menu to stop the background recorder
after testing. Checksums for the release assets are in **SHA256SUMS.txt**.
