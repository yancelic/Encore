# Desktop interface

Encore uses consistent System, Light and Dark themes across the library, editor,
timeline, settings and montage dialogs. System follows operating-system changes;
an explicit Light or Dark selection is remembered on the device.

## Interface assets

The desktop app and website share a red/slate palette and locally bundled Manrope
and Barlow Condensed fonts. Font licenses are included alongside the assets.
Theme tokens are shared across screens, including audio waveform colors.

The Windows executable, taskbar and tray icons use the Encore mark in
`apps/ui/src/assets/encore-mark.svg`. After installing UI dependencies, run
`apps/ui/scripts/update-windows-icons.ps1` to regenerate the ICO/PNG assets and
the recorder's BGRA tray image. Rebuild the installer to include updated icons.

## Verification

```sh
npm --prefix apps/ui ci
npm --prefix apps/ui run build
npm --prefix apps/ui exec -- playwright install chromium
npm --prefix apps/ui run test:ui
```

Browser tests serve compiled UI assets with an isolated Tauri bridge. They cover
navigation, themes, settings, clip actions, timeline and export requests without
accessing a real clip library. Native capture, media processing and operating-system
integration require the appropriate platform tests as well.

Build and package instructions are in the [README](../README.md) and
[cross-platform guide](cross-platform.md).
