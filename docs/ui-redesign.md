# Encore UI redesign

The desktop app now shares the website's visual identity. The replay library uses a light surface, a persistent collection/game sidebar, and media-first cards. The editor uses a dark version of that same palette for video, audio mixing and trimming.

## Design direction

- Palette: Onyx `#0c120c`, Brick Ember `#c20114`, Blue Slate `#5d737e`, Pale Sky `#bfd5e2`, Ghost White `#ecebf3`.
- Type: locally bundled Manrope for controls and reading; Barlow Condensed for library/dialog headings. Font licenses are included alongside the assets.
- Layout: left-aligned navigation and content, compact recording controls, and a stable coordinate system shared by the timeline ruler and tracks.
- Principles: replays lead; red signals primary actions and recording; flat, quiet controls replace the old metal textures. No remote font requests or decorative motion.

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

Design review: the website's oversized marketing headings are reduced for desktop use. The library keeps its recognizable type and palette; editing keeps a dark stage and compact controls. The single display heading provides the personality, while the controls remain practical. No recording, encoding, storage schema, IPC contract, installer version or release publishing logic changes.

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

Local validation on 2026-11-11: TypeScript/Vite production build, seven Playwright smoke tests, and `cargo check -p encore-ui --offline` on Windows. Browser coverage verifies UI behavior and IPC arguments; real recording and media processing still need the native manual pass below before merge.

For local visual review with the website's existing replay assets, optionally set `ENCORE_TEST_MEDIA_DIR` to its absolute `assets` directory before running tests. Fixtures are served by the test harness and are never bundled into the app.

Native follow-up before merge: run `npm --prefix apps/ui run tauri dev` on Windows and verify real capture, playback with separate audio tracks, file clipboard sharing, trimming, 9:16 export and montage creation using disposable clips. This shares the installed app's configuration/library; use a separate clips folder for this manual pass. The development URL is aligned with Vite's existing port 1420. Production UI assets are compiled into the desktop app; pushing this branch does not alter installed v0.2.0 clients.

## Restore the UI baseline

To inspect the original safely, make a separate checkout of the baseline commit. Alternatively, extract the local ZIP into a separate empty directory; it includes the `apps/ui` prefix. Avoid extracting over an edited checkout. A future rollback after merge should be a normal reviewed revert commit.
