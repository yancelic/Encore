# Encore in-app updates

The UI checks once per launch, and Settings â†’ App updates offers a manual check.
There is no updater timer, service, or dependency in `encore-recorder`. No package
is downloaded until the user confirms **Update and restart**. Offline checks leave
capture and the library usable; Settings shows the error and lets the user retry.

The updater downloads and verifies the signed NSIS package before interrupting
capture. The UI rejects new exports and keeps itself open during the operation,
waits for accepted exports, sends recorder shutdown, and waits for the recorder's
singleton object to disappear. The recorder retains that object while draining
accepted clip saves. A nonresponsive recorder or shutdown timeout aborts installation;
it is never force killed. A marker in the UI's application config directory resumes
a previously running recorder when the app next opens, including after installation.
An installer launch failure attempts to restore recording immediately.

Saved clips, the clip database and `%APPDATA%\Encore\config.json` are outside the
application installation and are not removed by this update flow. The in-memory
replay history and unsaved UI edits do not survive a restart; the confirmation says so.

## Signing key

This checkout's original updater key is stored at
`.local-backups/updater/encore.key` (private) and `encore.key.pub` (public).
The directory is ignored by Git. **Back up the private key securely outside this
checkout before distributing the first updater-enabled release.** Losing the key
prevents updates for installations that trust its public key. Do not regenerate
or commit it, put it in the frontend, or include it in release assets.

`build:installer` uses `TAURI_SIGNING_PRIVATE_KEY` when set, otherwise the local key.
Set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` for a password-protected key. The initial
local key has no password and relies on the security of this Windows account and
the checkout. For CI, store these values in encrypted secrets and provision FFmpeg
as with a normal build. The Windows updater public key is committed in `tauri.windows.conf.json`.
This update signature is separate from optional Windows Authenticode signing.

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
   `npm --prefix apps/ui run prepare:update`.
4. Create a **draft** GitHub release in `yancelic/Encore` with the exact tag
   `v<version>`. Attach the matching `Encore_<version>_x64-setup.exe`, its `.sig`,
   and `latest.json`. Never attach a private key. Check all three before publishing.
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
