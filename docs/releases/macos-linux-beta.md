Beta previews for **macOS 15+ Apple Silicon, Ubuntu 24.04+ x64 and Linux x86_64 via Flatpak**.

**Windows stable:** [Encore v0.5.0](https://github.com/yancelic/Encore/releases/tag/v0.5.0).

### Downloads and current status

- **Encore-macOS-Apple-Silicon.dmg — macOS beta.** The app runs, but some features
  have known bugs. Fixes will be published in subsequent beta updates. Detailed
  macOS installation instructions are being prepared and will be added to the
  README when available. This build uses ad-hoc signing; it is not Apple Developer
  ID signed or notarized.
- **Encore-Ubuntu-amd64.deb — Ubuntu beta.** Automated checks have passed, but the
  app has not yet been tested on a real Ubuntu user device.
- **Encore-Linux-x86_64.flatpak — Flatpak beta.** Build, install, runtime-library
  resolution and synthetic sandbox replay checks have passed. Real-device testing
  is still pending. This is a downloadable development bundle, not a Flathub listing.

Linux desktop integration, Wayland permissions, GPU drivers, separate application
audio and GUI playback still need real-device verification. On Wayland, choose a
recording window through the system sharing dialog and select the game's audio
application separately. Global save shortcuts require desktop portal support;
use **Save replay** in the library when the portal is unavailable or denied.

### Linux installation

Ubuntu, in the directory containing the downloaded package:

```sh
sudo apt install ./Encore-Ubuntu-amd64.deb
encore-ui
```

Flatpak, with Flatpak installed:

```sh
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user ./Encore-Linux-x86_64.flatpak
flatpak run com.encore.clipper
```

### Testing and updates

Each successful `dev` push refreshes the rolling **beta** prerelease after native
regression checks, macOS/Ubuntu package checks and Flatpak sandbox checks pass.
Packages receive an increasing beta version. Download and install a newer beta
manually to test fixes. `BUILD.json` records the package version, exact source
commit and CI run. `SHA256SUMS.txt` verifies all attached files.

The attached **LINUX-TESTING.md** contains a Turkish test checklist and log paths.
When reporting a problem, include the beta version, OS/desktop/session type,
GPU/driver, recording settings, reproduction steps and relevant log output.
Use **Quit Encore** to stop the background recorder after testing.

This prerelease does not contain a Windows installer or `latest.json`. The
Windows stable updater continues to use its signed stable release.
