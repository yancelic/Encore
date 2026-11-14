export type CaptureMode = 'GameDetection' | 'ActiveWindow';
export type AudioMode = 'Devices' | 'Applications';

export interface Config {
  mode: CaptureMode;
  fps: number;
  height: number;
  buffer_secs: number;
  hotkey: string;
  audio_mode: AudioMode;
  game_audio_enabled: boolean;
  game_application?: string | null;
  chat_application: string | null;
  audio_device: string | null;
  chat_device: string | null;
  mic_device: string | null;
  start_with_windows: boolean;
  clips_dir: string;
  games: string[];
}

export type RecorderState = 'Idle' | 'Recording' | 'Saving';

export interface RecorderStatus {
  state: RecorderState;
  target: string | null;
  mode: CaptureMode;
  fps: number;
  height: number;
  buffer_secs: number;
  hotkey: string;
  last_clip: string | null;
  error: string | null;
  audio_warnings?: (string | null)[];
}

export interface ApplicationAudioOptions {
  supported: boolean;
  executables: string[];
}

export interface CaptureApplication {
  exe: string;
  title: string;
}

export interface AudioDeviceInfo {
  name: string;
  kind: 'Output' | 'Input';
  is_default: boolean;
}

export interface ClipRecord {
  id: string;
  path: string;
  title: string;
  game: string;
  tag: string;
  description: string;
  favorite: boolean;
  duration_ms: number;
  width: number;
  height: number;
  fps: number;
  audio_tracks: number;
  size_bytes: number;
  created_at: number;
  thumb_path: string;
}

export type SortOrder = 'Newest' | 'Oldest' | 'Longest' | 'FavoritesFirst';

export interface ClipQuery {
  search: string;
  game: string | null;
  favorites_only: boolean;
  sort: SortOrder | null;
}

export interface ToastMessage {
  id: number;
  kind: 'success' | 'error' | 'info';
  text: string;
}
