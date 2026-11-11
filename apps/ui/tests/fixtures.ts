import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { Page } from '@playwright/test';
import type { ClipRecord, Config, RecorderStatus } from '../src/types';

export const config: Config = {
  mode: 'GameDetection',
  fps: 60,
  height: 1080,
  buffer_secs: 30,
  hotkey: 'Alt+S',
  audio_mode: 'Devices',
  game_audio_enabled: true,
  chat_application: null,
  audio_device: null,
  chat_device: 'default',
  mic_device: 'default',
  start_with_windows: false,
  clips_dir: 'C:\\Videos\\Encore',
  games: ['RocketLeague.exe', 'cs2.exe'],
};
const now = Math.floor(Date.now() / 1000);
export const clips: ClipRecord[] = [
  'Overtime winner',
  'Off the backboard',
  'Last second save',
  'The perfect line',
  'One more lap',
  'Clean entry',
].map((title, index) => ({
  id: String(index + 1),
  title,
  game: index < 3 ? 'Rocket League' : 'Assetto Corsa',
  tag: index === 0 ? 'Overtime' : '',
  description: '',
  favorite: index === 0,
  path: '/fixtures/replay.mp4',
  thumb_path: `/fixtures/replay-${index}.jpg`,
  duration_ms: 30_000,
  width: 1920,
  height: 1080,
  fps: 60,
  audio_tracks: 3,
  size_bytes: 26_000_000 + index * 2_000_000,
  created_at: now - index * 60,
}));

declare global {
  interface Window {
    __uiCalls: { command: string; args: Record<string, any> }[];
    __TAURI_INTERNALS__: {
      metadata: { currentWindow: { label: string } };
      convertFileSrc: (path: string) => string;
      invoke: (command: string, args?: Record<string, any>) => Promise<any>;
    };
  }
}

/** The bridge only exists inside the test browser. Nothing is added to app code. */
export async function mockDesktop(
  page: Page,
  options: { empty?: boolean; offline?: boolean } = {},
) {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.route('**/fixtures/**', async (route) => {
    const isVideo = route.request().url().endsWith('.mp4');
    if (process.env.ENCORE_TEST_MEDIA_DIR) {
      const filename = isVideo
        ? 'replay.mp4'
        : `replay-frame-${/replay-(\d)/.exec(route.request().url())?.[1] ?? 0}.jpg`;
      await route.fulfill({
        body: readFileSync(join(process.env.ENCORE_TEST_MEDIA_DIR, filename)),
        contentType: isVideo ? 'video/mp4' : 'image/jpeg',
      });
    } else if (isVideo) await route.fulfill({ status: 204 });
    else
      await route.fulfill({
        contentType: 'image/svg+xml',
        body: '<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><rect width="640" height="360" fill="#26373f"/><path d="M0 310 270 170 640 230" fill="none" stroke="#bfd5e2" stroke-width="12"/><circle cx="360" cy="190" r="22" fill="#c20114"/></svg>',
      });
  });
  await page.addInitScript(
    ({ initialClips, initialConfig, options }) => {
      let records = options.empty ? [] : initialClips;
      let cfg = initialConfig;
      let status: RecorderStatus | null = options.offline
        ? null
        : {
            state: 'Recording',
            target: 'RocketLeague.exe',
            mode: cfg.mode,
            fps: cfg.fps,
            height: cfg.height,
            buffer_secs: cfg.buffer_secs,
            hotkey: cfg.hotkey,
            last_clip: null,
            error: null,
          };
      window.__uiCalls = [];
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' } },
        convertFileSrc: (path) => path,
        invoke: async (command, args = {}) => {
          window.__uiCalls.push({ command, args });
          switch (command) {
            case 'plugin:window|set_theme':
            case 'plugin:window|set_background_color':
              return;
            case 'get_config':
              return cfg;
            case 'get_recorder_status':
              return status;
            case 'sync_and_list_clips': {
              const q = args.query;
              const result = records.filter(
                (c) =>
                  (!q.game || c.game === q.game) &&
                  (!q.favorites_only || c.favorite) &&
                  `${c.title} ${c.game} ${c.tag}`
                    .toLowerCase()
                    .includes(q.search.toLowerCase()),
              );
              return result.sort((a, b) =>
                q.sort === 'Oldest'
                  ? a.created_at - b.created_at
                  : q.sort === 'Longest'
                    ? b.duration_ms - a.duration_ms
                    : q.sort === 'FavoritesFirst'
                      ? Number(b.favorite) - Number(a.favorite)
                      : b.created_at - a.created_at,
              );
            }
            case 'list_audio_devices':
              return [
                { name: 'Headphones', kind: 'Output', is_default: true },
                { name: 'USB microphone', kind: 'Input', is_default: true },
              ];
            case 'list_audio_applications':
              return { supported: true, executables: ['Chat.exe', 'TeamSpeak.exe', 'RocketLeague.exe'] };
            case 'save_config':
              await new Promise((resolve) => setTimeout(resolve, 200));
              cfg = args.cfg;
              if (status)
                status = {
                  ...status,
                  mode: cfg.mode,
                  fps: cfg.fps,
                  height: cfg.height,
                  hotkey: cfg.hotkey,
                  buffer_secs: cfg.buffer_secs,
                };
              return;
            case 'toggle_favorite':
              records = records.map((c) =>
                c.id === args.id ? { ...c, favorite: args.favorite } : c,
              );
              return;
            case 'update_clip_details':
              records = records.map((c) =>
                c.id === args.id
                  ? {
                      ...c,
                      title: args.title,
                      tag: args.tag,
                      description: args.description,
                    }
                  : c,
              );
              return;
            case 'delete_clip':
              records = records.filter((c) => c.id !== args.id);
              return;
            case 'get_clip_waveforms':
              return [0, 1, 2].map((track) =>
                Array.from(
                  { length: 300 },
                  (_, i) =>
                    0.1 +
                    Math.abs(Math.sin(i * 0.27 + track) * Math.cos(i * 0.07)) *
                      0.75,
                ),
              );
            case 'get_clip_audio_tracks':
              return [];
            case 'trim_clip_cmd':
            case 'export_vertical_cmd':
            case 'build_montage_cmd': {
              const clip = {
                ...records[0],
                id: `new-${records.length}`,
                title: args.title ?? 'Edited replay',
              };
              records = [clip, ...records];
              return clip;
            }
            case 'start_recorder':
              status = {
                state: 'Idle',
                target: null,
                ...cfg,
                last_clip: null,
                error: null,
              };
              return;
            case 'stop_recorder':
              status = null;
              return;
            case 'trigger_save_clip':
            case 'copy_clip_to_clipboard':
            case 'reveal_in_explorer':
              return;
            case 'pick_clips_folder':
              return 'C:\\Videos\\New clips';
            default:
              throw new Error(`Unexpected test command: ${command}`);
          }
        },
      };
    },
    { initialClips: clips, initialConfig: config, options },
  );
  await page.goto('/');
  return errors;
}
