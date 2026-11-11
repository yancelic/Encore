import { useEffect, useState } from 'react';
import {
  FolderOpen,
  Keyboard,
  Monitor,
  Moon,
  Plus,
  Sun,
  Volume2,
  X,
} from 'lucide-react';
import { listAudioDevices, pickClipsFolder } from '../api';
import type { AudioDeviceInfo, CaptureMode, Config } from '../types';
import { Modal } from './Modal';
import type { ThemePreference } from '../theme';

interface SettingsModalProps {
  themePreference: ThemePreference;
  onThemeChange: (preference: ThemePreference) => void;
  config: Config;
  busy: boolean;
  onClose: () => void;
  onSave: (cfg: Config) => void;
}

const PRESET_BUFFER_SECS = [15, 20, 30, 45, 60, 90, 120];
const FPS_OPTIONS = [30, 60, 120, 144, 240];
const HEIGHT_OPTIONS: { label: string; value: number }[] = [
  { label: 'Native (Display Resolution)', value: 0 },
  { label: '1440p (QHD)', value: 1440 },
  { label: '1080p (Full HD)', value: 1080 },
  { label: '720p (HD)', value: 720 },
];

const SUPPORTED_KEY_CODES = new Set([
  'Backquote',
  'Minus',
  'Equal',
  'BracketLeft',
  'BracketRight',
  'Backslash',
  'Semicolon',
  'Quote',
  'Comma',
  'Period',
  'Slash',
  'IntlBackslash',
  'Space',
  'Tab',
  'Enter',
  'Backspace',
  'Delete',
  'Insert',
  'Home',
  'End',
  'PageUp',
  'PageDown',
  'PrintScreen',
  'Pause',
  'ScrollLock',
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'Numpad0',
  'Numpad1',
  'Numpad2',
  'Numpad3',
  'Numpad4',
  'Numpad5',
  'Numpad6',
  'Numpad7',
  'Numpad8',
  'Numpad9',
  'NumpadAdd',
  'NumpadSubtract',
  'NumpadMultiply',
  'NumpadDivide',
  'NumpadDecimal',
  'NumpadEnter',
  'NumpadEqual',
  'NumpadComma',
  'AudioVolumeDown',
  'AudioVolumeUp',
  'AudioVolumeMute',
  'MediaPlayPause',
  'MediaStop',
  'MediaTrackNext',
  'MediaTrackPrevious',
]);

function mapCodeToHotkeyToken(e: KeyboardEvent): string | null {
  const code = e.code;
  if (code) {
    const letter = /^Key([A-Z])$/.exec(code);
    if (letter) return letter[1];
    const digit = /^Digit(\d)$/.exec(code);
    if (digit) return digit[1];
    if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return code;
    if (SUPPORTED_KEY_CODES.has(code)) return code;
    return null;
  }
  let key = e.key;
  if (key === ' ') return 'Space';
  if (/^[a-zA-Z0-9]$/.test(key)) return key.toUpperCase();
  if (/^F([1-9]|1\d|2[0-4])$/i.test(key)) return key.toUpperCase();
  return null;
}

function formatKeyCombo(e: KeyboardEvent): string | null {
  const ignoreKeys = [
    'Control',
    'Alt',
    'Shift',
    'Meta',
    'OS',
    'CapsLock',
    'NumLock',
    'ScrollLock',
  ];
  const ignoreCodes = [
    'ControlLeft',
    'ControlRight',
    'AltLeft',
    'AltRight',
    'ShiftLeft',
    'ShiftRight',
    'MetaLeft',
    'MetaRight',
    'CapsLock',
    'NumLock',
    'ScrollLock',
    'OSLeft',
    'OSRight',
  ];
  if (ignoreKeys.includes(e.key) || ignoreCodes.includes(e.code)) return null;

  const key = mapCodeToHotkeyToken(e);
  if (!key) return null;

  const parts: string[] = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');
  parts.push(key);
  return parts.join('+');
}

export function SettingsModal({
  themePreference,
  onThemeChange,
  config,
  busy,
  onClose,
  onSave,
}: SettingsModalProps) {
  const [draft, setDraft] = useState<Config>({
    ...config,
    games: [...config.games],
  });
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [recordingKey, setRecordingKey] = useState(false);
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);
  const [newGame, setNewGame] = useState('');

  useEffect(() => {
    listAudioDevices()
      .then(setDevices)
      .catch(() => setDevices([]));
  }, []);

  useEffect(() => {
    if (!recordingKey) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.code === 'Escape' || e.key === 'Escape') {
        setRecordingKey(false);
        setHotkeyError(null);
        return;
      }
      const ignoreCodes = [
        'ControlLeft',
        'ControlRight',
        'AltLeft',
        'AltRight',
        'ShiftLeft',
        'ShiftRight',
        'MetaLeft',
        'MetaRight',
      ];
      if (
        ignoreCodes.includes(e.code) ||
        ['Control', 'Alt', 'Shift', 'Meta', 'OS'].includes(e.key)
      ) {
        return;
      }
      const combo = formatKeyCombo(e);
      if (combo) {
        setDraft((d) => ({ ...d, hotkey: combo }));
        setHotkeyError(null);
        setRecordingKey(false);
      } else {
        setHotkeyError(
          'Unsupported key — try A-Z, 0-9, F1-F12, or standard punctuation keys.',
        );
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [recordingKey]);

  const outputs = devices.filter((d) => d.kind === 'Output');
  const inputs = devices.filter((d) => d.kind === 'Input');

  const handleBrowseFolder = async () => {
    const picked = await pickClipsFolder(draft.clips_dir);
    if (picked) {
      setDraft((d) => ({ ...d, clips_dir: picked }));
    }
  };

  const addGame = () => {
    const trimmed = newGame.trim();
    if (!trimmed) return;
    const normalized = trimmed.toLowerCase().endsWith('.exe')
      ? trimmed
      : `${trimmed}.exe`;
    if (
      !draft.games.some((g) => g.toLowerCase() === normalized.toLowerCase())
    ) {
      setDraft((d) => ({ ...d, games: [...d.games, normalized] }));
    }
    setNewGame('');
  };

  return (
    <Modal titleId="settings-title" busy={busy} onClose={onClose} wide>
      <div className="modal-header">
        <div className="modal-title-group">
          <div>
            <h2 id="settings-title">Make it your setup.</h2>
            <p className="modal-subtitle">
              Capture, audio and the way you save.
            </p>
          </div>
        </div>
        <div className="modal-title-group">
          <button
            className="btn-icon"
            onClick={onClose}
            disabled={busy}
            aria-label="Close settings"
          >
            <X size={15} />
          </button>
        </div>
      </div>

      <div className="modal-body">
        <fieldset className="modal-fields" disabled={busy}>
          <section className="settings-section appearance-section">
            <h3>Appearance</h3>
            <fieldset className="theme-picker">
              <legend>Theme</legend>
              <div className="theme-options">
                {(
                  [
                    { value: 'system', label: 'System', icon: Monitor },
                    { value: 'light', label: 'Light', icon: Sun },
                    { value: 'dark', label: 'Dark', icon: Moon },
                  ] as const
                ).map(({ value, label, icon: Icon }) => (
                  <label
                    className={`theme-option ${themePreference === value ? 'selected' : ''}`}
                    key={value}
                  >
                    <input
                      type="radio"
                      name="theme"
                      value={value}
                      checked={themePreference === value}
                      onChange={() => onThemeChange(value)}
                    />
                    <Icon size={17} aria-hidden="true" />
                    <span>{label}</span>
                  </label>
                ))}
              </div>
            </fieldset>
            <p className="appearance-help">
              System follows your Windows appearance. Theme changes apply
              immediately.
            </p>
          </section>
          <div className="settings-grid">
            <section className="settings-section">
              <h3>Video capture</h3>
              <div className="field-group">
                <label htmlFor="capture-mode">Capture mode</label>
                <div className="field-row">
                  <select
                    id="capture-mode"
                    value={draft.mode}
                    onChange={(e) =>
                      setDraft((d) => ({
                        ...d,
                        mode: e.target.value as CaptureMode,
                      }))
                    }
                  >
                    <option value="GameDetection">
                      Game Detection (Recommended)
                    </option>
                    <option value="ActiveWindow">
                      Active Window / Display
                    </option>
                  </select>
                </div>
              </div>

              <div className="field-group">
                <label htmlFor="capture-resolution">Output resolution</label>
                <div className="field-row">
                  <select
                    id="capture-resolution"
                    value={draft.height}
                    onChange={(e) =>
                      setDraft((d) => ({
                        ...d,
                        height: Number(e.target.value),
                      }))
                    }
                  >
                    {HEIGHT_OPTIONS.map((opt) => (
                      <option key={opt.value} value={opt.value}>
                        {opt.label}
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              <div className="field-group">
                <label htmlFor="capture-fps">Frame rate</label>
                <div className="field-row">
                  <select
                    id="capture-fps"
                    value={draft.fps}
                    onChange={(e) =>
                      setDraft((d) => ({ ...d, fps: Number(e.target.value) }))
                    }
                  >
                    {FPS_OPTIONS.map((f) => (
                      <option key={f} value={f}>
                        {f} FPS
                      </option>
                    ))}
                  </select>
                </div>
              </div>
            </section>

            <section className="settings-section">
              <h3>Replay buffer &amp; shortcut</h3>
              <div className="field-group">
                <label htmlFor="capture-buffer">
                  Replay length ({draft.buffer_secs} seconds)
                </label>
                <div className="field-row">
                  <select
                    id="capture-buffer"
                    value={
                      PRESET_BUFFER_SECS.includes(draft.buffer_secs)
                        ? draft.buffer_secs
                        : 'custom'
                    }
                    onChange={(e) => {
                      const v = Number(e.target.value);
                      if (!Number.isNaN(v))
                        setDraft((d) => ({ ...d, buffer_secs: v }));
                    }}
                  >
                    {PRESET_BUFFER_SECS.map((s) => (
                      <option key={s} value={s}>
                        Last {s} seconds
                      </option>
                    ))}
                    {!PRESET_BUFFER_SECS.includes(draft.buffer_secs) && (
                      <option value="custom">
                        Custom ({draft.buffer_secs}s)
                      </option>
                    )}
                  </select>
                </div>
              </div>

              <div className="field-group">
                <label htmlFor="capture-hotkey">Save shortcut</label>
                <button
                  type="button"
                  id="capture-hotkey"
                  className={`hotkey-recorder mono ${recordingKey ? 'listening' : ''}`}
                  onClick={() => {
                    setHotkeyError(null);
                    setRecordingKey(true);
                  }}
                >
                  <span
                    style={{
                      display: 'inline-flex',
                      alignItems: 'center',
                      gap: 7,
                    }}
                  >
                    <Keyboard size={14} />
                    {recordingKey
                      ? 'Press key combination... (Esc cancels)'
                      : draft.hotkey}
                  </span>
                  <span style={{ fontSize: 11, color: 'var(--ink-engraved)' }}>
                    {recordingKey ? 'Listening' : 'Change'}
                  </span>
                </button>
                {hotkeyError && (
                  <div
                    style={{
                      fontSize: 11,
                      color: 'var(--danger, #f87171)',
                      marginTop: 4,
                    }}
                  >
                    {hotkeyError}
                  </div>
                )}
              </div>

              <div className="toggle-row" style={{ marginTop: 4 }}>
                <div>
                  <div style={{ fontWeight: 600 }}>
                    Start Recorder with Windows
                  </div>
                  <div style={{ fontSize: 11, color: 'var(--ink-engraved)' }}>
                    Runs quietly in system tray on login
                  </div>
                </div>
                <button
                  type="button"
                  className={`switch-btn ${draft.start_with_windows ? 'on' : ''}`}
                  onClick={() =>
                    setDraft((d) => ({
                      ...d,
                      start_with_windows: !d.start_with_windows,
                    }))
                  }
                  aria-pressed={draft.start_with_windows}
                  aria-label="Start recorder with Windows"
                >
                  <span className="switch-thumb" />
                </button>
              </div>
            </section>
          </div>

          <section className="settings-section">
            <h3 style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
              <Volume2 size={13} />
              Separate audio tracks
            </h3>
            <div className="settings-grid audio-routing">
              <div className="field-group">
                <label htmlFor="game-audio">Game audio</label>
                <select
                  id="game-audio"
                  value={draft.audio_device ?? ''}
                  onChange={(e) =>
                    setDraft((d) => ({
                      ...d,
                      audio_device: e.target.value || null,
                    }))
                  }
                >
                  <option value="">Default Playback Device</option>
                  <option value="off">Off (Disabled)</option>
                  {outputs.map((dev) => (
                    <option key={dev.name} value={dev.name}>
                      {dev.name}
                      {dev.is_default ? ' (Default)' : ''}
                    </option>
                  ))}
                </select>
              </div>

              <div className="field-group">
                <label htmlFor="chat-audio">Chat audio</label>
                <select
                  id="chat-audio"
                  value={draft.chat_device ?? 'off'}
                  onChange={(e) =>
                    setDraft((d) => ({
                      ...d,
                      chat_device: e.target.value || 'off',
                    }))
                  }
                >
                  <option value="off">Off (Disabled)</option>
                  <option value="default">Default Playback Device</option>
                  {outputs.map((dev) => (
                    <option key={dev.name} value={dev.name}>
                      {dev.name}
                      {dev.is_default ? ' (Default)' : ''}
                    </option>
                  ))}
                </select>
              </div>

              <div className="field-group">
                <label htmlFor="mic-audio">Microphone</label>
                <select
                  id="mic-audio"
                  value={draft.mic_device ?? 'off'}
                  onChange={(e) =>
                    setDraft((d) => ({
                      ...d,
                      mic_device: e.target.value || 'off',
                    }))
                  }
                >
                  <option value="off">Off (Disabled)</option>
                  <option value="default">Default Microphone</option>
                  {inputs.map((dev) => (
                    <option key={dev.name} value={dev.name}>
                      {dev.name}
                      {dev.is_default ? ' (Default)' : ''}
                    </option>
                  ))}
                </select>
              </div>
            </div>
          </section>

          <section className="settings-section">
            <h3>
              <label htmlFor="clips-folder">Clips folder</label>
            </h3>
            <div className="field-row">
              <input
                type="text"
                id="clips-folder"
                className="mono"
                value={draft.clips_dir}
                onChange={(e) =>
                  setDraft((d) => ({ ...d, clips_dir: e.target.value }))
                }
              />
              <button className="btn-secondary" onClick={handleBrowseFolder}>
                <FolderOpen size={14} />
                Browse...
              </button>
            </div>
          </section>

          <section className="settings-section">
            <h3>Games to capture ({draft.games.length})</h3>
            <div className="games-chips">
              {draft.games.map((g) => (
                <span key={g} className="game-chip mono">
                  {g}
                  <button
                    type="button"
                    title={`Remove ${g}`}
                    onClick={() =>
                      setDraft((d) => ({
                        ...d,
                        games: d.games.filter((x) => x !== g),
                      }))
                    }
                  >
                    <X size={12} />
                  </button>
                </span>
              ))}
            </div>
            <div className="field-row">
              <input
                type="text"
                placeholder="Add game executable (e.g. cs2.exe)..."
                aria-label="Add game executable"
                value={newGame}
                onChange={(e) => setNewGame(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && addGame()}
              />
              <button className="btn-secondary" onClick={addGame}>
                <Plus size={14} />
                Add Game
              </button>
            </div>
          </section>
        </fieldset>
      </div>

      <div className="modal-footer">
        <button className="btn-ghost" onClick={onClose} disabled={busy}>
          Cancel
        </button>
        <button
          className="btn-primary"
          disabled={busy}
          onClick={() => onSave(draft)}
        >
          {busy ? 'Applying…' : 'Save settings'}
        </button>
      </div>
    </Modal>
  );
}
