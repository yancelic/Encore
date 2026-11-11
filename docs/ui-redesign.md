# Encore UI redesign

The desktop app now shares the website's visual identity, with consistent light and dark themes across the library, editor, timeline, settings and montage dialogs. System appearance is the default; explicit Light/Dark choices are remembered on this computer.

## Design direction

- Palette: Onyx `#0c120c`, Brick Ember `#c20114`, Blue Slate `#5d737e`, Pale Sky `#bfd5e2`, Ghost White `#ecebf3`.
- Type: locally bundled Manrope for controls and reading; Barlow Condensed for library/dialog headings. Font licenses are included alongside the assets.
- Layout: left-aligned navigation and content, compact recording controls, and a stable coordinate system shared by the timeline ruler and tracks.
- Principles: replays lead; red signals primary actions and recording; flat, quiet controls replace the old metal textures. No remote font requests or decorative motion.

Theme direction: the light palette keeps the site's Ghost White background and slate text. Dark mode uses neutral charcoal `#15171b`, raised surfaces `#20242b`, Ghost White text and pale slate details. The red brand accent and Manrope/Barlow type remain shared. Every screen reads root theme tokens; the editor has no independent dark override. Audio waveforms repaint with contrasting theme colors when the theme changes.

```text
encore.     recorder status / capture mode       shortcut / settings
------------------------------------------------------------------
Your library | Your replays.                       Create montage
All replays  | Search                      Sort / grid / list
Favorites    | ---------------------------------------------------
Games        | Today / game
             | [replay]           [replay]           [replay]
             | title + actions    title + actions    title + actions
```

Design review: the website's oversized marketing headings are reduced for desktop use. The library keeps its recognizable type and palette; editing uses the selected app theme with compact controls. The single display heading provides the personality, while the controls remain practical. No recording, encoding, storage schema, IPC contract or installer version changes.

Theme preference is stored as `encore.theme` in the desktop WebView's local storage. Missing, invalid or unavailable storage falls back to System. OS appearance changes are observed live. Theme selection applies immediately and does not save capture settings or restart the recorder. Native window theme/background are synchronized through two explicitly scoped Tauri window permissions.

## Safe development workflow

- Baseline: `515386a142a40b5f0c467a5ab102b0daba051a17` on `main` (`v0.2.0`).
- Local backup: `.local-backups/encore-ui-before-redesign-2026-11-11.zip` contains all 30 tracked files under `apps/ui` at the baseline. Its adjacent JSON records the commit and archive SHA-256. The backup is ignored by Git.
- Work only on `codex/dev-ui-redesign`; use a draft PR targeting `main` for review. Merge is a separate step after visual and native validation.
- The `UI checks` GitHub Actions workflow installs, builds and runs browser smoke tests on development pushes and PRs. It has read-only repository permissions and does not create installers, tags or releases.
- Browser tests replace the Tauri bridge with isolated in-memory fixtures. They never start the real recorder or read/write the user's clip library or settings.

## Verification

```powershell
npm --prefix apps/ui ci
npm --prefix apps/ui run build
cd apps/ui
npx playwright install chromium
npm run test:ui
```

Tests cover library navigation/search, list/grid actions, montage ordering, editor mixing/trim/export payloads, settings and focus handling, recorder states, and layout at desktop minimum size and narrow windows. Screenshots are saved under the ignored `apps/ui/test-results/` directory.

Local validation on 2026-11-11: TypeScript/Vite production build, 12 Playwright tests against compiled UI assets, and a successful Windows release build with NSIS installer packaging (42.01 MiB). The earlier baseline also passed `cargo check -p encore-ui --offline`. Browser coverage verifies UI behavior and IPC arguments; real recording and media processing still need the native manual pass below before merge.

Theme coverage additionally exercises light/dark system defaults across all screens, live OS changes, manual overrides, persistence after reload, radio keyboard navigation, native theme requests, waveform repainting, invalid preferences and unavailable storage. `npm run test:ui` first builds and then serves the compiled production UI on an isolated preview port, so the browser checks the same assets bundled into the installer.

## Windows installer

Use `npm --prefix apps/ui run build:installer` as the standard distribution build. It first builds the release recorder, then runs the Tauri release build with NSIS bundling. The output is `target/release/bundle/nsis/Encore_0.2.0_x64-setup.exe`, with recorder and FFmpeg bundled. It does not install the app or publish a GitHub release. On this development branch the package still carries the existing 0.2.0 version; choose the release version before publishing. Running the installer updates the installed Encore app, so development review should be completed before public release.

For local visual review with the website's existing replay assets, optionally set `ENCORE_TEST_MEDIA_DIR` to its absolute `assets` directory before running tests. Fixtures are served by the test harness and are never bundled into the app.

Native follow-up before merge: run `npm --prefix apps/ui run tauri dev` on Windows and verify real capture, playback with separate audio tracks, file clipboard sharing, trimming, 9:16 export and montage creation using disposable clips. This shares the installed app's configuration/library; use a separate clips folder for this manual pass. The development URL is aligned with Vite's existing port 1420. Production UI assets are compiled into the desktop app; pushing this branch does not alter installed v0.2.0 clients.

## Restore the UI baseline

To inspect the original safely, make a separate checkout of the baseline commit. Alternatively, extract the local ZIP into a separate empty directory; it includes the `apps/ui` prefix. Avoid extracting over an edited checkout. A future rollback after merge should be a normal reviewed revert commit.
