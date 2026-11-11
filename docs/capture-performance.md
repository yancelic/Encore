# Capture performance and RAM replay

Game Detection now chooses a known game's main window using its HWND and PID, without querying game memory or injecting a DLL. It stays with that game across Alt+Tab and title changes, prefers a main window over an owned dialog, and can replace a startup window with a larger window from the same process. Active Window intentionally follows focus. Neither mode falls back to monitor capture.

Minimizing ends the capture session; restoration begins a new replay history. Recreating the game window also restarts capture. Selection is polled every two seconds, and an active recorder checks its target's lifetime/minimized state every 250 ms. Protected or unsupported windows may fail to capture. The application does not bypass capture restrictions, and this design is not an anti-cheat certification.

## Reducing capture work

- Request WGC's `MinUpdateInterval` at the configured recording FPS when supported.
- Apply a software limiter before extracting/copying GPU surfaces on all supported Windows versions. Time buckets tolerate small delivery jitter. Startup may admit one extra copy; there is no cumulative overshoot or catch-up copying.
- Drain queued WGC frames to the newest available frame instead of copying every queued frame.
- Reuse GPU surfaces only after their external COM references have been released. Start with six surfaces and grow only if necessary, up to 24 for the current size. Drop a capture frame if all slots remain busy. Do not overwrite an encoder-owned surface.

The slot estimate below excludes the WGC frame pool, the encoder/converter's own allocations, and old-size surfaces temporarily retained across resize. The existing audio synchronization allowance is retained.

## Audio separation

Settings → Separate audio tracks → Separate audio by offers two independent configurations. Switching modes preserves the other mode's choices. Existing config files continue using Devices; upgrading does not silently change which sound is recorded.

- **Audio devices:** the Game and Chat tracks use the selected output devices; Microphone uses its input device. Route the game and chat app to different Windows playback devices. The UI warns when both tracks resolve to the same output. Two applications on one output remain mixed in this mode.
- **Applications:** Game uses the recorded window's PID and descendants. Chat uses a generic executable filename selected from running processes or entered manually, such as `TeamSpeak.exe`; an empty field disables Chat. Windows application loopback captures each chosen process tree independently of the output device. It requires Windows build 20348 or later; unsupported systems retain the device option.

Only executable filenames and Toolhelp PID/parent metadata identify chat apps. Window titles are never stored or matched. The resolver checks once per second, selects a same-executable root rather than its unrelated launcher, stays on a live root if several independent instances share a filename, and reconnects after restart. It captures one independent root and its descendants, not every unrelated instance sharing that executable name. A browser chosen as Chat includes its descendant tabs; this does not isolate an individual browser tab. Voice chat rendered by the game itself remains in Game.

No game DLL injection, graphics hook or game-memory read is introduced by application audio. The implementation uses Windows' application loopback API through the existing WASAPI dependency; see [Microsoft's sample and requirements](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/). This is not an anti-cheat compatibility guarantee.

Missing apps show a reconnect message. Initialization/capture failures are shown in the recorder's audio warnings, exposed through an audio-attention button and Settings. Application capture never silently falls back to recording the whole output device. Sources share the existing 48 kHz stereo S16, QPC-anchored timeline and three AAC encoder tracks. Raw audio delivery is bounded to 32 packets per track; a stalled encoder drops packets instead of blocking the audio thread or growing an unlimited queue. Silence is padded by the timeline.

These changes do not establish that application mode has zero cost or lower input latency than device mode. The native tests verify separation/reconnect and sound surviving the existing 25 ms recording synchronization allowance; that allowance is not game input latency. Competitive-game comparisons are still required before performance claims.

## Library process lifetime

The recorder owns the only tray icon, whether the library is open or closed. The UI no longer builds a Tauri tray or runs a separate tray-status polling thread. Recorder launch does not use `--no-tray`; an already running older recorder is asked to restore its native icon when the library opens.

Closing the library (X/Alt+F4) hides its window, then exits the UI process and releases WebView2. Recording, the replay buffer and the global hotkey continue in the recorder process. Explicit trim/vertical/montage exports accepted before closing finish and are indexed before UI exit; new exports are rejected during closing. Reopening during an export cancels the pending library exit. Read-only status polling does not hold the UI open.

Open Encore in the native tray wakes the existing library or launches a new process when none is running. The wake protocol acknowledges whether a wake was accepted; the caller waits at most 300 ms for a reply. Duplicate launch retries a briefly exiting singleton rather than losing the open request. Quit Encore in the native tray requests both library and recorder exit, with accepted export/replay-save jobs drained normally. Stopping only the recorder from an open library leaves the library available to start it again.

Recorder/performance development uses a dedicated development branch, currently `codex/dev-recorder-performance`, independently of the UI redesign branch.

Recorder IPC keeps a persistent named-pipe instance across client connections. Closing and recreating an instance could disconnect a new client after its open had already succeeded. A regression test sends 1,000 consecutive requests. Client retries apply only to opening a pipe during startup/teardown; requests already written are never replayed, avoiding duplicate clip saves.

## RAM replay and saving

A custom rateless Media Foundation sink receives encoded H.264/AAC samples directly from Sink Writer. The live encoder stays open across saves; there are no periodic segment rotations or steady-state video file writes. Capture starts without creating a segment/clip directory. Diagnostics write a log only when explicitly enabled.

The buffer retains video decoding timestamps, presentation timestamps, durations, keyframes and all three AAC tracks. A save uses the latest common audio/video watermark and starts at the keyframe preceding the requested cutoff. Video stays in decode order, including future reference pictures needed by reordered frames. This avoids re-encoding, but clip duration can exceed the requested duration by a GOP plus codec reordering; startup or memory pressure can produce a shorter clip.

Compressed packet payloads share a hard **512 MiB** budget, including references held by pending saves and old-size snapshots. Old complete GOPs are removed when needed. If snapshots prevent space from being reclaimed, history is cleared and waits for the next keyframe instead of growing or retaining an undecodable chain. This budget does not cover raw GPU surfaces, codec/OS allocations, transient export buffers or packet metadata (also bounded by packet/job counts). It is not a total process-RAM limit.

One worker handles disk output, with room for one additional pending save. A full queue rejects the request with a recorder error rather than blocking capture or spawning unlimited workers. Stop waits for accepted saves to finish. Resizing drains the old encoder and resets live history; accepted old-size saves remain valid and charged to the same budget.

Saving muxes compressed packets into a temporary MP4, then FFmpeg performs one stream-copy pass for metadata and `faststart`. No video/audio encoding occurs in this export path. Old payload references are released after the first mux. Temporary files are excluded from the library, cleaned up on errors and renamed to the final clip only after completion. Disk activity therefore still occurs during an explicit save. Old `%APPDATA%\Encore\segments` files from prior versions are no longer read or modified by capture.

## Recording diagnostics

Diagnostics are opt-in. Close any existing recorder first so it inherits the environment from this shell. From the repository root:

```powershell
New-Item -ItemType Directory -Path .local-backups -Force | Out-Null
$env:ENCORE_STATS = '1'
$env:ENCORE_STATS_FILE = Join-Path (Get-Location) '.local-backups/capture-stats.jsonl'
cargo run --release -p encore-recorder -- --fps 60 --height 1080
```

Every approximately five seconds of recording, a JSON object is printed after `[stats]` and appended without that prefix to the optional JSONL file. `ENCORE_STATS_FILE` alone also enables diagnostics. Stop the recorder normally before changing environment/settings. Each capture session starts new interval counters; timestamps and dimensions identify transitions.

| Field | Meaning |
| --- | --- |
| `received_frames` | Frames retrieved from WGC, including frames discarded while draining |
| `drained_frames` | Older queued frames discarded in favor of the newest |
| `throttled_frames` | Frames rejected before a GPU copy by the recording-rate limiter |
| `busy_drops` | Frames discarded because no safe GPU slot could be obtained |
| `copied_frames`, `copied_fps` | Successfully published GPU copies |
| `superseded_frames` | Published frames replaced before the recording loop submitted them |
| `submitted_frames`, `submitted_fps` | Video samples accepted by `IMFSinkWriter::WriteSample`, not confirmed output frames |
| `repeated_samples` | Successful video submissions reusing the previous capture sequence |
| `copy_callback_avg_ms`, `copy_callback_max_ms` | CPU wall time for callbacks that publish a copy; does not wait for GPU completion |
| `video_submit_avg_ms`, `video_submit_max_ms` | Time spent submitting a sample to the sink writer, including failed attempts; not total encoding latency |
| `frame_age_avg_ms`, `frame_age_max_ms` | Capture timestamp age just before a successful video submission; includes repeated frames |
| `late_ticks` | Recording iterations at least one frame interval behind their scheduled deadline |
| `video_errors`, `audio_errors` | Failed video/audio sample submissions |
| `gpu_slots`, `gpu_slot_bytes` | Current surface count and estimated bytes for that pool only |
| `replay_bytes`, `replay_limit_bytes` | Live encoded payload bytes, including pending saves, and their hard budget |
| `replay_history_ms` | Currently available common A/V history from the oldest retained keyframe |
| `replay_memory_drops`, `replay_memory_evictions` | Cumulative packet-pressure resets and complete GOP evictions for memory pressure in this capture session |
| `pending_saves` | Active plus queued save jobs (at most two) |

Drained, throttled, busy and superseded frames are different stages; do not add them to repeated samples as if they represented the same output-loss metric. Resize/invalid-size frames may also be discarded. A static/minimized/occluded application can produce fewer new frames than the recording FPS. Decode the saved clip to validate actual changing frames and audio sync.

These metrics do **not** measure mouse-to-display input lag, game frame times, whole-process CPU/GPU use, or confirmed hardware encoder selection. Diagnostic logging itself has a cost; use identical logging settings for Encore comparisons and confirm representative results with logging disabled.

## Comparison protocol

1. Preserve a build of the previous revision before modifying it. Run a warm-up and repeat the same game scene/replay with capture off, the previous Encore build, this build, GG, Medal and OBS individually. Do not run recorders simultaneously.
2. Match game FPS limit, rendering settings, display mode/refresh rate, capture area, recording resolution/FPS, bitrate/codec where possible, audio tracks, replay duration and UI visibility. Note settings that cannot be matched. Keep UI state fixed when isolating recorder changes.
3. Run each condition for the same duration, at least three times, alternating order. Separate steady recording from the interval around saving a clip. Record game frame times/1% lows, process CPU and memory, GPU engine use and disk writes with the same measurement tools.
4. Measure real input-to-display latency with a suitable external setup or an independently validated latency measurement. Neither average FPS nor `frame_age_*` can substitute for it. Report the measurement uncertainty and repeated-run variation.
5. Inspect saved footage for changing frames, duration and audio synchronization. A low-overhead recorder that silently loses motion/audio fails acceptance.
6. Accept performance claims only after these comparisons. The product requirement is no worse input latency than GG/Medal within measurement uncertainty; OBS is the reference target. No such competitive-game latency result has been established by the implementation tests.

For this local development session, the first-stage disk-buffer release executables were preserved before rebuilding in `.local-backups/baselines/wgc-disk-buffer/`, with SHA-256 hashes in `manifest.json`. Run only one build at a time: both builds share the recorder singleton, user settings and clip library. This local baseline is ignored by Git and is not a published release.

## Verification

```powershell
cargo test --release -p encore-recorder -- --test-threads=1
cargo test --release -p encore-encode
```

Default tests exercise target policy, title/minimize/restore behavior with a generated native window, limiter rates/jitter, real D3D/MF buffer ownership, JSONL output, scaling, RAM memory accounting, slow-save queue pressure, MP4 export, and multi-track AV synchronization during concurrent saving. Some restricted environments block renaming exported files in the temporary directory; those integration tests need filesystem permission to publish their generated MP4s.

The actual WGC smoke test requires an interactive Windows desktop and capture permission. It records only its generated animated window, disables all real audio sources, verifies no clip/segment directory exists before the first save, and checks copy rate, changing frames, resizing/saving, and shutdown on close:

```powershell
cargo test --release -p encore-recorder window_capture_limits -- --ignored --nocapture --test-threads=1
```

After `cargo build --release --workspace`, an additional opt-in process test starts only a generated game window, selects its exact test executable, disables real audio, and isolates config/clips/WebView2 data in a temporary directory. It verifies library/WebView2 exit, continued recording and saving, native-tray reopen, duplicate-launch reuse and native Quit:

```powershell
cargo test --release -p encore-recorder native_library_close -- --ignored --nocapture --test-threads=1
```

This test requires no existing Encore library/recorder to be running. It briefly opens generated test/library windows, and cleans up only processes it created. It never captures the user's desktop or microphone.

Application audio has a separate opt-in test. It plays short quiet generated tones from owned fixture processes on the default output, captures only those process trees, and verifies concurrent Game/Chat separation on the same output, inclusion of a child renderer, reconnect after restart, and the recorder's QPC timeline. It does not capture a microphone or a whole output device:

```powershell
cargo test --release -p encore-recorder application_audio_filters -- --ignored --nocapture --test-threads=1
```

Application-root policy/config migration also have default unit tests; Settings has a Playwright test for generic application names, invalid titles, device overlap warnings and mode-switch persistence. A restricted sandbox can deny the unrelated registry-writing autostart test; run only the relevant common tests there, or use `cargo test --release -p encore-common -- --skip autostart::tests`.

The legacy primary-monitor capture test is ignored by default because it records the user's desktop and configured audio. Do not run all ignored tests indiscriminately. None of these tests proves game input latency or anti-cheat approval.
