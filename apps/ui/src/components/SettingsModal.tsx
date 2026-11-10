import { useEffect, useState } from 'react';
import { FolderOpen, Keyboard, Plus, Volume2, X } from 'lucide-react';
import { listAudioDevices, pickClipsFolder } from '../api';
import type { AudioDeviceInfo, CaptureMode, Config } from '../types';

interface SettingsModalProps {
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
  const ignoreKeys = ['Control', 'Alt', 'Shift', 'Meta', 'OS', 'CapsLock', 'NumLock', 'ScrollLock'];
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

export function SettingsModal({ config, busy, onClose, onSave }: SettingsModalProps) {
  const [draft, setDraft] = useState<Config>({ ...config, games: [...config.games] });
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [recordingKey, setRecordingKey] = useState(false);
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);
  const [newGame, setNewGame] = useState('');

  useEffect(() => {
    listAudioDevices().then(setDevices).catch(() => setDevices([]));
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
      if (ignoreCodes.includes(e.code) || ['Control', 'Alt', 'Shift', 'Meta', 'OS'].includes(e.key)) {
        return;
      }
      const combo = formatKeyCombo(e);
      if (combo) {
        setDraft((d) => ({ ...d, hotkey: combo }));
        setHotkeyError(null);
        setRecordingKey(false);
      } else {
        setHotkeyError('Unsupported key — try A-Z, 0-9, F1-F12, or standard punctuation keys.');
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
    const normalized = trimmed.toLowerCase().endsWith('.exe') ? trimmed : `${trimmed}.exe`;
    if (!draft.games.some((g) => g.toLowerCase() === normalized.toLowerCase())) {
      setDraft((d) => ({ ...d, games: [...d.games, normalized] }));
    }
    setNewGame('');
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal-card"
        style={{ maxWidth: 740 }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="rack-screw" aria-hidden="true" />
            <h2>Encore Settings</h2>
          </div>
          <div className="modal-title-group">
            <button className="btn-icon" onClick={onClose} disabled={busy}>
              <X size={15} />
            </button>
            <span className="rack-screw" aria-hidden="true" />
          </div>
        </div>

        <div className="modal-body">
          <div className="settings-grid">
            <section className="settings-section">
              <h3>Video Capture</h3>
              <div className="field-group">
                <label>Capture Mode</label>
                <div className="field-row">
                  <select
                    value={draft.mode}
                    onChange={(e) =>
                      setDraft((d) => ({ ...d, mode: e.target.value as CaptureMode }))
                    }
                  >
                    <option value="GameDetection">Game Detection (Recommended)</option>
                    <option value="ActiveWindow">Active Window / Display</option>
                  </select>
                </div>
              </div>

              <div className="field-group">
                <label>Output Resolution</label>
                <div className="field-row">
                  <select
                    value={draft.height}
                    onChange={(e) =>
                      setDraft((d) => ({ ...d, height: Number(e.target.value) }))
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
                <label>Frame Rate</label>
                <div className="field-row">
                  <select
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
              <h3>Clip Buffer &amp; Hotkey</h3>
              <div className="field-group">
                <label>Instant Replay Length ({draft.buffer_secs} seconds)</label>
                <div className="field-row">
                  <select
                    value={PRESET_BUFFER_SECS.includes(draft.buffer_secs) ? draft.buffer_secs : 'custom'}
                    onChange={(e) => {
                      const v = Number(e.target.value);
                      if (!Number.isNaN(v)) setDraft((d) => ({ ...d, buffer_secs: v }));
                    }}
                  >
                    {PRESET_BUFFER_SECS.map((s) => (
                      <option key={s} value={s}>
                        Last {s} seconds
                      </option>
                    ))}
                    {!PRESET_BUFFER_SECS.includes(draft.buffer_secs) && (
                      <option value="custom">Custom ({draft.buffer_secs}s)</option>
                    )}
                  </select>
                </div>
              </div>

              <div className="field-group">
                <label>Global Save Clip Hotkey</label>
                <button
                  type="button"
                  className={`hotkey-recorder mono ${recordingKey ? 'listening' : ''}`}
                  onClick={() => {
                    setHotkeyError(null);
                    setRecordingKey(true);
                  }}
                >
                  <span style={{ display: 'inline-flex', alignItems: 'center', gap: 7 }}>
                    <Keyboard size={14} />
                    {recordingKey ? 'Press key combination... (Esc cancels)' : draft.hotkey}
                  </span>
                  <span style={{ fontSize: 11, color: 'var(--ink-engraved)' }}>
                    {recordingKey ? 'Listening' : 'Change'}
                  </span>
                </button>
                {hotkeyError && (
                  <div style={{ fontSize: 11, color: 'var(--danger, #f87171)', marginTop: 4 }}>
                    {hotkeyError}
                  </div>
                )}
              </div>

              <div className="toggle-row" style={{ marginTop: 4 }}>
                <div>
                  <div style={{ fontWeight: 600 }}>Start Recorder with Windows</div>
                  <div style={{ fontSize: 11, color: 'var(--ink-engraved)' }}>
                    Runs quietly in system tray on login
                  </div>
                </div>
                <button
                  type="button"
                  className={`switch-btn ${draft.start_with_windows ? 'on' : ''}`}
                  onClick={() =>
                    setDraft((d) => ({ ...d, start_with_windows: !d.start_with_windows }))
                  }
                  aria-pressed={draft.start_with_windows}
                >
                  <span className="switch-thumb" />
                </button>
              </div>
            </section>
          </div>

          <section className="settings-section">
            <h3 style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
              <Volume2 size={13} />
              Multi-Track Audio Routing
            </h3>
            <div className="settings-grid" style={{ gridTemplateColumns: 'repeat(3, minmax(0, 1fr))' }}>
              <div className="field-group">
                <label style={{ color: '#16C9B3' }}>Track 1 · Game Audio</label>
                <select
                  value={draft.audio_device ?? ''}
                  onChange={(e) =>
                    setDraft((d) => ({ ...d, audio_device: e.target.value || null }))
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
                <label style={{ color: '#3B82F6' }}>Track 2 · Chat Audio</label>
                <select
                  value={draft.chat_device ?? 'off'}
                  onChange={(e) =>
                    setDraft((d) => ({ ...d, chat_device: e.target.value || 'off' }))
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
                <label style={{ color: '#F59E0B' }}>Track 3 · Microphone</label>
                <select
                  value={draft.mic_device ?? 'off'}
                  onChange={(e) =>
                    setDraft((d) => ({ ...d, mic_device: e.target.value || 'off' }))
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
            <h3>Clips Storage Folder</h3>
            <div className="field-row">
              <input
                type="text"
                className="mono"
                value={draft.clips_dir}
                onChange={(e) => setDraft((d) => ({ ...d, clips_dir: e.target.value }))}
              />
              <button className="btn-secondary" onClick={handleBrowseFolder}>
                <FolderOpen size={14} />
                Browse...
              </button>
            </div>
          </section>

          <section className="settings-section">
            <h3>Detected Game Executables ({draft.games.length})</h3>
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
            {busy ? 'Applying...' : 'Save & Apply Live'}
          </button>
        </div>
      </div>
    </div>
  );
}
