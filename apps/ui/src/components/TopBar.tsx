import { usePlatform } from '../platform';
import {
  Camera,
  FolderOpen,
  Gamepad2,
  Keyboard,
  Monitor,
  Play,
  RefreshCw,
  Settings,
  Square,
  Volume2,
} from 'lucide-react';
import encoreIcon from '../assets/encore-mark.svg';
import type { CaptureMode, Config, RecorderStatus } from '../types';

interface TopBarProps {
  status: RecorderStatus | null;
  config: Config | null;
  busyAction: string | null;
  onStartRecorder: () => void;
  onStopRecorder: () => void;
  onSaveClipNow: () => void;
  onChangeMode: (mode: CaptureMode) => void;
  onOpenFolder: () => void;
  onRefreshClips: () => void;
  onOpenSettings: () => void;
}

function prettyTarget(target: string | null): string {
  if (!target) return 'Capturing';
  const cleaned = target.replace(/\.exe$/i, '').replace(/-Win\d+-Shipping$/i, '');
  return cleaned;
}

export function TopBar({
  status,
  config,
  busyAction,
  onStartRecorder,
  onStopRecorder,
  onSaveClipNow,
  onChangeMode,
  onOpenFolder,
  onRefreshClips,
  onOpenSettings,
}: TopBarProps) {
  const platform = usePlatform();
  const mode = status?.mode ?? config?.mode ?? 'GameDetection';
  const fps = status?.fps ?? config?.fps ?? 60;
  const height = status?.height ?? config?.height ?? 1080;
  const bufferSecs = status?.buffer_secs ?? config?.buffer_secs ?? 20;
  const hotkey = status?.hotkey ?? config?.hotkey ?? 'Alt+S';

  let dotClass = 'offline';
  let statusText = platform.recording ? 'Recorder Offline' : 'Library & editor preview';
  if (status) {
    if (status.state === 'Recording') {
      dotClass = 'recording';
      statusText = `Recording · ${prettyTarget(status.target)}`;
    } else if (status.state === 'Saving') {
      dotClass = 'saving';
      statusText = 'Saving clip...';
    } else {
      dotClass = 'ready';
      statusText = mode === 'GameDetection' ? 'Ready to capture' : 'Watching active window';
    }
  }

  return (
    <header className="top-bar">
      <div className="brand" aria-label="Encore">
        <img className="brand-mark-img" src={encoreIcon} alt="" />
        <span className="brand-title">encore<span className="brand-period">.</span></span>
      </div>

      <div className="top-center">
        <div className="status-pill" role="status" title={status?.error ?? statusText}>
          <span className={`status-dot ${dotClass}`} />
          <span className="status-label">{statusText}</span>
        </div>

        {platform.recording && (!status ? (
          <button
            className="btn-primary"
            onClick={onStartRecorder}
            disabled={busyAction === 'start'}
            title="Start background clip recorder"
          >
            <Play size={13} fill="currentColor" />
            {busyAction === 'start' ? 'Starting...' : 'Start Recorder'}
          </button>
        ) : (
          <>
            {status.state === 'Recording' && (
              <button
                className="btn-primary"
                onClick={onSaveClipNow}
                disabled={busyAction === 'save'}
                title={`Save last ${bufferSecs}s clip (${hotkey})`}
              >
                <Camera size={14} />
                Save Clip
              </button>
            )}
            <button
              className="btn-icon"
              onClick={onStopRecorder}
              disabled={busyAction === 'stop'}
              title="Stop background recorder"
              aria-label="Stop background recorder"
            >
              <Square size={12} fill="currentColor" />
            </button>
          </>
        ))}

        {platform.recording && <div className="mode-switch" role="group" aria-label="Capture Mode">
          <button
            className={mode === 'GameDetection' ? 'active' : ''}
            aria-pressed={mode === 'GameDetection'}
            onClick={() => mode !== 'GameDetection' && onChangeMode('GameDetection')}
            disabled={busyAction === 'mode'}
            title="Automatically record supported games"
          >
            <Gamepad2 size={13} />
            Games
          </button>
          <button
            className={mode === 'ActiveWindow' ? 'active' : ''}
            aria-pressed={mode === 'ActiveWindow'}
            onClick={() => mode !== 'ActiveWindow' && onChangeMode('ActiveWindow')}
            disabled={busyAction === 'mode'}
            title="Record whichever window is currently focused"
          >
            <Monitor size={13} />
            Window
          </button>
        </div>}
      </div>

      <div className="top-right">
        {status?.audio_warnings?.some(Boolean) && <button className="btn-icon" onClick={onOpenSettings}
          aria-label="Audio needs attention" title={status.audio_warnings.filter(Boolean).join('\n')}>
          <Volume2 size={16} />
        </button>}
        {platform.recording && <div className="meta-chip mono" title="Clip Buffer · Resolution · Framerate">
          <span>{bufferSecs}s</span>
          <span>·</span>
          <span>{height === 0 ? 'Native' : `${height}p`}</span>
          <span>·</span>
          <span>{fps}fps</span>
        </div>}

        {platform.recording && <button
          className="hotkey-keycap mono"
          onClick={onOpenSettings}
          title="Click to configure save hotkey"
        >
          <Keyboard size={12} />
          <strong>{hotkey}</strong>
        </button>}

        <button className="btn-icon" onClick={onOpenFolder} title="Open clips folder" aria-label="Open clips folder">
          <FolderOpen size={15} />
        </button>

        <button className="btn-icon" onClick={onRefreshClips} title="Rescan clips folder" aria-label="Rescan clips folder">
          <RefreshCw size={15} />
        </button>

        <button className="btn-secondary" onClick={onOpenSettings} title="Settings">
          <Settings size={14} />
          Settings
        </button>
      </div>
    </header>
  );
}
