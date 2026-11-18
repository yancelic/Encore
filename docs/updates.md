# Encore in-app updates

The UI checks once per launch, and Settings → App updates offers a manual check.
There is no updater timer, service, or dependency in `encore-recorder`. No package
is downloaded until the user confirms **Update and restart**. Offline checks leave
capture and the library usable; Settings shows the error and lets the user retry.

Windows and stable Apple Silicon macOS releases use the same signed feed. macOS
betas, Linux and Flatpak are updated manually or by their package manager.

The updater downloads and verifies the signed package (the NSIS installer on Windows,
`Encore_<version>_aarch64.app.tar.gz` on macOS) before interrupting capture. The UI rejects new exports and keeps itself open during the operation,
waits for accepted exports, sends recorder shutdown, and waits for the recorder's
singleton object to disappear. The recorder retains that object while draining
accepted clip saves. A nonresponsive recorder or shutdown timeout aborts installation;
it is never force killed. On macOS the recorder keeps its socket lock until it has
drained, and the updater waits for that lock instead of the singleton object. A marker in the UI's application config directory resumes
a previously running recorder when the app next opens, including after installation.
An installer launch failure attempts to restore recording immediately. On macOS the
updater replaces `Encore.app` in place (asking for an administrator password only when
the user cannot write to its folder), then reopens Encore after this process exits.

Saved clips, the clip database and `%APPDATA%\Encore\config.json` are outside the
application installation and are not removed by this update flow. The in-memory
replay history and unsaved UI edits do not survive a restart; the confirmation says so.

## Signing key

Official updates must be signed with the key trusted by installed Encore clients.
Release maintainers provide it through `TAURI_SIGNING_PRIVATE_KEY` and, when
required, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Keep signing credentials in secure
storage and CI secrets, outside published source and release assets. Losing or
replacing the key prevents updates for clients that trust the original public key.

The updater public key is committed in `tauri.windows.conf.json` and
`tauri.macos.conf.json`; both platforms trust the same key. This update signature is
separate from Windows Authenticode signing and from macOS code signing.

## macOS code signing

macOS remembers Screen Recording, Microphone and Input Monitoring by the app's code
signature. Stable macOS builds are therefore signed with one long-lived, self-signed
`Encore Release` certificate, so permissions survive in-app updates. An ad-hoc or
different certificate makes every update look like a new app to macOS. The
`stable-packages.yml` workflow imports it from the `MACOS_CERT_P12` (base64) and
`MACOS_CERT_PASSWORD` secrets and fails if the built bundles do not carry it. Keep
the certificate as carefully as the updater key. It is not an Apple Developer ID:
Gatekeeper still asks users to approve the first manual install, but updates
downloaded by Encore are not quarantined.

The same workflow signs the update archive with `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets and verifies it against the public key
shipped in the app. Users of builds signed differently (0.6.1 and earlier are
ad-hoc) install the first certificate-signed release manually and grant permissions
once more.

## Publish each release

1. Increment the version together in root `Cargo.toml`, `apps/ui/package.json`,
   `apps/ui/package-lock.json` and `apps/ui/src-tauri/tauri.conf.json`. Refresh
   `Cargo.lock` with Cargo. Use a new version for every published build.
2. Optionally set `ENCORE_RELEASE_NOTES` to plain-text release notes.
3. Run `npm --prefix apps/ui run build:installer`. It builds the recorder and UI,
   produces an installer and `.sig`, and writes `latest.json` in
   `target/release/bundle/nsis/`. Metadata generation rejects mismatched versions
   or missing signatures. The build then verifies the actual installer with the
   public key shipped in the app and checks that a tampered package cannot install.
   A mismatched signing key fails the build. To regenerate only metadata, use
   `npm --prefix apps/ui run prepare:update`. To include macOS, download the
   artifact of the successful Stable packages run for the same tag and pass its
   folder: `npm --prefix apps/ui run prepare:update -- --macos <folder>` (or set
   `ENCORE_MACOS_UPDATE_DIR` for `build:installer`). This adds `darwin-aarch64`.
4. Create a **draft** GitHub release in `yancelic/Encore` with the exact tag
   `v<version>`. Attach the matching `Encore_<version>_x64-setup.exe`, its `.sig`,
   and `latest.json`. For releases supporting macOS, also attach the matching
   `Encore_<version>_aarch64.dmg`, `Encore_<version>_aarch64.app.tar.gz` and its
   `.sig` from the same Stable packages run used for `latest.json`.
   Add `SHA256SUMS.txt` covering all assets. Never attach a private key. Verify
   versions, source commit and checksums before publishing.
5. Publish the stable release and make it the latest release. The app checks
   `https://github.com/yancelic/Encore/releases/latest/download/latest.json`.
   The manifest references the versioned installer URL, not a moving download URL.
   This simple flow has one stable channel; keep prereleases off the latest stable release.

Version 0.4.0 and earlier have no updater. Their users must install 0.5.0
manually once to enable signed in-app updates for future releases.

## Verification before publishing

- Run `npm --prefix apps/ui run test:ui` and `cargo test --release -p encore-ui`.
- Test an installed, signed older build against a newer signed test release on a
  Windows test account/VM. Exercise active recording, a pending clip save and an
  export; confirm the new version opens and recording resumes when appropriate.
- Check the clips directory, library entries, settings and autostart after upgrade.
- Test an offline check, an interrupted download, an invalid signature, and an
  unavailable recorder. No unverified installer should run, and a failed download
  must leave capture running. Test cancellation/failure in NSIS separately; if the
  installer does not complete, reopen Encore to resume recording.

## Memory

Only the UI loads updater code. An update check fetches a small JSON manifest.
The standard Tauri updater buffers the complete installer in memory while downloading
and verifying it, so expect a temporary peak at least as large as the installer,
plus allocation/network overhead. The installer runs only after confirmation.
Measure idle and download peaks in a release build; no numeric RAM claim is assumed.
