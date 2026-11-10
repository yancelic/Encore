import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import type {
  AudioDeviceInfo,
  ClipQuery,
  ClipRecord,
  Config,
  RecorderStatus,
} from './types';

export const isTauriEnv = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const mediaUrl = (filePath: string): string =>
  filePath ? (isTauriEnv() ? convertFileSrc(filePath) : filePath) : '';

export const getRecorderStatus = (): Promise<RecorderStatus | null> =>
  invoke<RecorderStatus | null>('get_recorder_status');

export const startRecorder = (): Promise<void> =>
  invoke<void>('start_recorder');

export const stopRecorder = (): Promise<void> =>
  invoke<void>('stop_recorder');

export const triggerSaveClip = (): Promise<void> =>
  invoke<void>('trigger_save_clip');

export const listAudioDevices = (): Promise<AudioDeviceInfo[]> =>
  invoke<AudioDeviceInfo[]>('list_audio_devices');

export const getConfig = (): Promise<Config> =>
  invoke<Config>('get_config');

export const saveConfig = (cfg: Config): Promise<void> =>
  invoke<void>('save_config', { cfg });

export const pickClipsFolder = (current?: string | null): Promise<string | null> =>
  invoke<string | null>('pick_clips_folder', { current: current ?? null });

export const syncAndListClips = (query: ClipQuery): Promise<ClipRecord[]> =>
  invoke<ClipRecord[]>('sync_and_list_clips', { query });

export const toggleFavorite = (id: string, favorite: boolean): Promise<void> =>
  invoke<void>('toggle_favorite', { id, favorite });

export const updateClipDetails = (
  id: string,
  title: string,
  tag: string,
  description: string,
): Promise<void> =>
  invoke<void>('update_clip_details', { id, title, tag, description });

export const deleteClip = (id: string): Promise<void> =>
  invoke<void>('delete_clip', { id });

export const getClipWaveforms = (id: string): Promise<number[][]> =>
  invoke<number[][]>('get_clip_waveforms', { id });

export const getClipAudioTracks = (id: string): Promise<string[]> =>
  invoke<string[]>('get_clip_audio_tracks', { id });

export const trimClip = (
  id: string,
  startSecs: number,
  endSecs: number,
  mutedTracks: boolean[],
  trackVolumes: number[],
): Promise<ClipRecord> =>
  invoke<ClipRecord>('trim_clip_cmd', {
    id,
    startSecs,
    endSecs,
    mutedTracks,
    trackVolumes,
  });

export const exportVertical = (
  id: string,
  blurBg: boolean,
  startSecs: number | null,
  endSecs: number | null,
  mutedTracks: boolean[],
  trackVolumes: number[],
): Promise<ClipRecord> =>
  invoke<ClipRecord>('export_vertical_cmd', {
    id,
    blurBg,
    startSecs,
    endSecs,
    mutedTracks,
    trackVolumes,
  });

export const buildMontage = (ids: string[], title: string): Promise<ClipRecord> =>
  invoke<ClipRecord>('build_montage_cmd', { ids, title });

export const copyClipToClipboard = (path: string): Promise<void> =>
  invoke<void>('copy_clip_to_clipboard', { path });

export const revealInExplorer = (path: string): Promise<void> =>
  invoke<void>('reveal_in_explorer', { path });

export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, '0')}`;
}

export function formatTimecode(secs: number): string {
  const clamped = Math.max(0, secs);
  const m = Math.floor(clamped / 60);
  const s = Math.floor(clamped % 60);
  const tenths = Math.floor((clamped - Math.floor(clamped)) * 10);
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}.${tenths}`;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(2)} GB`;
}

export function formatRelativeAge(unixSecs: number): string {
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - unixSecs);
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  const days = Math.floor(diff / 86400);
  if (days === 1) return 'yesterday';
  if (days < 7) return `${days}d ago`;
  return new Date(unixSecs * 1000).toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
  });
}

export function dateGroupLabel(unixSecs: number): string {
  const d = new Date(unixSecs * 1000);
  const now = new Date();
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const daysDiff = Math.round((startOf(now) - startOf(d)) / 86_400_000);
  if (daysDiff === 0) return 'Today';
  if (daysDiff === 1) return 'Yesterday';
  return d.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
}

export function formatDateTime(unixSecs: number): string {
  return new Date(unixSecs * 1000).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}
