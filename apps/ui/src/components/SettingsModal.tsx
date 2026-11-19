import { usePlatform } from '../platform';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  FolderOpen,
  Keyboard,
  Monitor,
  Moon,
  Sun,
  X,
} from 'lucide-react';
import { listAudioApplications, listAudioDevices, pickClipsFolder, selectCaptureSource } from '../api';
import type { ApplicationAudioOptions, AudioDeviceInfo, AudioMode, CaptureMode, Config } from '../types';
import { Modal } from './Modal';
import type { ThemePreference } from '../theme';
import type { Updater } from '../updater';
import { UpdateSettings } from './Updates';
import { GamePicker } from './GamePicker';
import { PermissionSettings } from './Permissions';
import { SettingChoices } from './SettingChoices';

interface SettingsModalProps {
  updater: Updater;
  themePreference: ThemePreference;
  onThemeChange: (preference: ThemePreference) => void;
  config: Config;
  audioWarnings?: (string | null)[];
  busy: boolean;
  onClose: () => void;
  onSave: (cfg: Config) => void;
}

const PRESET_BUFFER_SECS = [15, 20, 30, 45, 60, 90, 120];
const FPS_OPTIONS = [30, 60, 120, 144, 240];
const HEIGHT_OPTIONS: { label: string; value: number }[] = [
  { label: 'Native', value: 0 },
  { label: '1440p', value: 1440 },
  { label: '1080p', value: 1080 },
  { label: '720p', value: 720 },
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
  if (e.metaKey) parts.push('Super');
  parts.push(key);
  return parts.join('+');
}

export function SettingsModal({
  updater,
  themePreference,
  onThemeChange,
  config,
  audioWarnings,
  busy,
  onClose,
  onSave,
}: SettingsModalProps) {
  const platform = usePlatform();
  const [draft, setDraft] = useState<Config>({
    ...config,
    audio_mode: config.audio_mode ?? 'Devices',
    game_audio_enabled: config.game_audio_enabled ?? true,
    chat_application: config.chat_application ?? null,
    games: [...config.games],
  });
  const [activeTab, setActiveTab] = useState('recording');
  const bodyRef = useRef<HTMLDivElement>(null);
  const tabs = [
    { id: 'recording', label: platform.recording ? 'Recording' : 'Library' },
    ...(platform.recording ? [{ id: 'audio', label: 'Audio' }] : []),
    { id: 'general', label: 'General' },
  ];
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [applications, setApplications] = useState<ApplicationAudioOptions | null>(null);
  const [applicationsError, setApplicationsError] = useState(false);
  const [recordingKey, setRecordingKey] = useState(false);
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);
  const [captureError, setCaptureError] = useState<string | null>(null);

  const changeTab = (tab: string) => {
    setRecordingKey(false);
    setHotkeyError(null);
    setActiveTab(tab);
    if (bodyRef.current) bodyRef.current.scrollTop = 0;
  };

  useEffect(() => {
    if (!platform.recording) return;
    listAudioDevices()
      .then(setDevices)
      .catch(() => setDevices([]));
  }, [platform.recording]);

  const refreshApplications = useCallback(() => {
    if (!platform.recording) return;
    setApplicationsError(false);
    listAudioApplications().then(setApplications).catch(() => setApplicationsError(true));
  }, [platform.recording]);
  useEffect(refreshApplications, [refreshApplications]);

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
  const outputName = (value: string | null) => {
    if (!value || value.toLowerCase() === 'default') return outputs.find((d) => d.is_default)?.name;
    return outputs.find((d) => d.name.toLowerCase().includes(value.toLowerCase()))?.name;
  };
  const sameOutput = draft.audio_mode === 'Devices' && !!draft.chat_device && draft.chat_device !== 'off'
    && draft.audio_device !== 'off' && !!outputName(draft.audio_device)
    && outputName(draft.audio_device) === outputName(draft.chat_device);
  const chatInvalid = draft.audio_mode === 'Applications' && !!draft.chat_application
    && (/[\\/\x00-\x1f]/.test(draft.chat_application) || (platform.os === 'windows'
      && (!/\.exe$/i.test(draft.chat_application) || /[:*?"<>|]/.test(draft.chat_application))));
  const audioNeedsReview = platform.recording
    && (chatInvalid || (draft.audio_mode === 'Applications' && !applications?.supported));

  const handleBrowseFolder = async () => {
    const picked = await pickClipsFolder(draft.clips_dir);
    if (picked) {
      setDraft((d) => ({ ...d, clips_dir: picked }));
    }
  };

  return (
    <Modal titleId="settings-title" busy={busy} onClose={onClose} wide>
      <div className="modal-header">
        <div className="modal-title-group">
          <div>
            <h2 id="settings-title">Settings</h2>
            <p className="modal-subtitle">
              Recording, audio and app preferences.
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

      <div className="settings-tabs" role="tablist" aria-label="Settings categories">
        {tabs.map((tab, index) => (
          <button key={tab.id} type="button" role="tab" id={`settings-tab-${tab.id}`}
            aria-selected={activeTab === tab.id} aria-controls={`settings-panel-${tab.id}`}
            tabIndex={activeTab === tab.id ? 0 : -1} disabled={busy}
            onClick={() => changeTab(tab.id)}
            onKeyDown={(event) => {
              const offset = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
              if (!offset && event.key !== 'Home' && event.key !== 'End') return;
              event.preventDefault();
              const next = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1
                : (index + offset + tabs.length) % tabs.length;
              changeTab(tabs[next].id);
              document.getElementById(`settings-tab-${tabs[next].id}`)?.focus();
            }}>{tab.label}</button>
        ))}
      </div>
      <div className="modal-body settings-body" ref={bodyRef}>
        <fieldset className="modal-fields" disabled={busy}>
          <div id="settings-panel-recording" className="settings-panel" role="tabpanel"
            aria-labelledby="settings-tab-recording" hidden={activeTab !== 'recording'} tabIndex={0}>
            {platform.recording ? <>
              <section className="settings-section">
                <h3>Replay &amp; shortcut</h3>
                <p className="settings-description">Choose how much of the last play to keep and how to save it.</p>
                <SettingChoices id="capture-buffer" label="Replay length" value={draft.buffer_secs}
                  describedBy="capture-buffer-help"
                  options={[...PRESET_BUFFER_SECS.map((value) => ({ value, label: `${value}s` })),
                  ...(!PRESET_BUFFER_SECS.includes(draft.buffer_secs) ? [{ value: draft.buffer_secs, label: `${draft.buffer_secs}s (custom)` }] : [])]}
                  onChange={(buffer_secs) => setDraft((d) => ({ ...d, buffer_secs }))} />
                <p id="capture-buffer-help" className="capture-mode-help">
                  Replay memory limit: 512 MiB. High quality settings or clips being saved may reduce the available replay length.
                </p>
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

              </section>
              <section className="settings-section">
                <h3>Video quality</h3>
                <div className="settings-grid">
                  <SettingChoices id="capture-resolution" label="Output resolution" value={draft.height}
                    options={[...HEIGHT_OPTIONS, ...(!HEIGHT_OPTIONS.some((option) => option.value === draft.height)
                      ? [{ value: draft.height, label: `${draft.height}p` }] : [])]}
                    onChange={(height) => setDraft((d) => ({ ...d, height }))} />
                  <SettingChoices id="capture-fps" label="Frame rate" value={draft.fps}
                    options={[...FPS_OPTIONS.map((value) => ({ value, label: `${value} FPS` })),
                    ...(!FPS_OPTIONS.includes(draft.fps) ? [{ value: draft.fps, label: `${draft.fps} FPS` }] : [])]}
                    onChange={(fps) => setDraft((d) => ({ ...d, fps }))} />
                </div>
              </section>
              <section className="settings-section">
                <h3>Capture source</h3>
                <SettingChoices<CaptureMode> id="capture-mode" label="Capture mode" value={draft.mode}
                  describedBy="capture-mode-help"
                  options={[{ value: 'GameDetection', label: platform.portal_capture ? 'Selected window' : 'Games' },
                  {
                    value: platform.os === 'windows' ? 'ScreenCapture' : 'ActiveWindow',
                    label: platform.os === 'windows' ? 'Screen' : 'Active window', disabled: platform.portal_capture
                  }]}
                  onChange={(mode) => setDraft((d) => ({ ...d, mode }))} />
                <p id="capture-mode-help" className="capture-mode-help">
                  {platform.portal_capture ? 'Choose a window in the system sharing dialog. Encore keeps recording that window when you switch apps. Your desktop controls which windows can be shared.' : draft.mode === 'GameDetection'
                    ? 'Records only the game window and stays on it when you switch apps. Minimizing pauses capture; restoring starts a new replay history.'
                    : platform.os === 'windows'
                      ? 'Records everything visible on your primary display, including other apps and notifications. Switching apps keeps the replay history.'
                      : 'Records the focused window. Switching apps changes the recording target.'}
                </p>
                {platform.portal_capture && <button type="button" className="btn-secondary"
                  onClick={() => { setCaptureError(null); void selectCaptureSource().catch((error: unknown) => setCaptureError(String(error))); }}>
                  Choose recording window…
                </button>}
                {captureError && <p className="audio-routing-note" role="alert">{captureError}</p>}
              </section>
            </> : <p className="capture-mode-help" role="status">
              Recording is not available in this preview yet. Choose a clips folder to browse, edit and export existing MP4 files.
            </p>}
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

            {platform.recording && !platform.portal_capture && <section className="settings-section">
              <h3>Games to capture ({draft.games.length})</h3>
              <div className="games-chips">
                {draft.games.map((g) => (
                  <span key={g} className="game-chip mono">
                    {g}
                    <button
                      type="button"
                      title={`Remove ${g}`}
                      aria-label={`Remove ${g}`}
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
              {!platform.portal_capture && <GamePicker games={draft.games} onAdd={(exe) => setDraft((d) =>
                d.games.some((game) => game.toLowerCase() === exe.toLowerCase())
                  ? d : { ...d, games: [...d.games, exe] })} />}
            </section>}
          </div>
          {platform.recording && <div id="settings-panel-audio" className="settings-panel" role="tabpanel"
            aria-labelledby="settings-tab-audio" hidden={activeTab !== 'audio'} tabIndex={0}>
            <section className="settings-section">
              <h3>Separate audio tracks</h3>
              <p className="settings-description">Keep game, chat and microphone audio on their own tracks.</p>
              <SettingChoices<AudioMode> id="audio-mode" label="Separate audio by" value={draft.audio_mode}
                options={[{ value: 'Devices', label: 'Audio devices' }, { value: 'Applications', label: 'Applications', disabled: !applications?.supported }]}
                onChange={(audio_mode) => setDraft((d) => ({ ...d, audio_mode }))} />
              {!applications?.supported && <p className="audio-routing-note">
                {applicationsError ? 'Could not check application audio support. Reopen Settings to retry.'
                  : applications ? platform.os === 'windows' ? 'Application audio requires Windows build 20348 or later. Use audio devices on this system.' : 'Application audio is unavailable in the current audio session. Use audio devices or check the native capture runtime.'
                    : 'Checking application audio support…'}
              </p>}
              {draft.audio_mode === 'Applications' ? <>
                <p className="audio-routing-note">{platform.os === 'windows'
                  ? draft.mode === 'ScreenCapture'
                    ? 'The Game track records all audio from the default playback device during screen capture. Chat is also captured separately and may be present in both tracks.'
                    : 'Game audio follows the recorded application and its child processes. Chat uses an executable name, so title changes and restarts are handled automatically. No separate playback devices are needed.'
                  : 'Game and chat audio are captured separately by application. Choose chat from the list of running applications.'}</p>
                <div className="settings-grid audio-routing">
                  <div className="field-group">
                    <SettingChoices id="game-application-audio" label="Game audio" value={draft.game_audio_enabled ? 'game' : 'off'}
                      options={[{ value: 'game', label: platform.os === 'windows' && draft.mode === 'ScreenCapture' ? 'System audio (Default Playback Device)' : 'Recorded application' }, { value: 'off', label: 'Off' }]}
                      onChange={(value) => setDraft((d) => ({ ...d, game_audio_enabled: value === 'game' }))} />
                    {platform.portal_capture && draft.game_audio_enabled && <>
                      <label htmlFor="game-audio-application">Game audio application</label>
                      <input id="game-audio-application" list="audio-applications" placeholder="Choose the game's audio application"
                        value={draft.game_application ?? ''}
                        onChange={(event) => setDraft((value) => ({ ...value, game_application: event.target.value || null }))} />
                      <p className="audio-routing-note">Play audio in your game, then refresh applications. The system window dialog does not identify its audio stream.</p>
                    </>}
                  </div>
                  <div className="field-group">
                    <label htmlFor="chat-application">Chat application</label>
                    <input id="chat-application" list="audio-applications" placeholder={platform.os === 'windows' ? 'Off — enter an app.exe' : 'Off — choose an application identifier'}
                      value={draft.chat_application ?? ''} aria-invalid={chatInvalid}
                      onChange={(e) => setDraft((d) => ({ ...d, chat_application: e.target.value || null }))}
                      onBlur={() => setDraft((d) => ({ ...d, chat_application: d.chat_application?.trim() || null }))} />
                    <datalist id="audio-applications">{applications?.executables.map((exe) => <option key={exe} value={exe} />)}</datalist>
                    <button type="button" className="btn-ghost" onClick={refreshApplications}>Refresh applications</button>
                  </div>
                </div>
                {chatInvalid && <p className="audio-routing-note" role="alert">{platform.os === 'windows' ? 'Enter an executable filename such as Chat.exe, without a file path or window title.' : 'Choose an application identifier, without a file path.'}</p>}
                {draft.chat_application && !applications?.executables.some((exe) => exe.toLowerCase() === draft.chat_application?.toLowerCase())
                  && <p className="audio-routing-note">{draft.chat_application} is not running. Chat audio will connect when it opens.</p>}
              </> : <>
                <p className="audio-routing-note">{platform.os === 'macos' ? 'System audio includes all applications. Choose Applications above to separate game and chat audio.' : 'Route the game and chat app to different playback devices in your sound settings. Apps sharing an output cannot be separated in this mode.'}</p>
                {sameOutput && <p className="audio-routing-note" role="status">Game and chat use the same playback device. Both tracks may contain the same audio.</p>}
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

                </div>
              </>}
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
              {audioWarnings?.filter(Boolean).map((message, i) => <p key={i} className="audio-routing-note" role="status">Current recorder: {message}</p>)}
            </section>
          </div>}
          <div id="settings-panel-general" className="settings-panel" role="tabpanel"
            aria-labelledby="settings-tab-general" hidden={activeTab !== 'general'} tabIndex={0}>
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
                System follows your device appearance. Theme changes apply
                immediately.
              </p>
            </section>
            {platform.recording && platform.autostart && <section className="settings-section">
              <h3>Startup</h3>
              <div className="toggle-row">
                <div>
                  <div style={{ fontWeight: 600 }}>
                    {platform.os === 'windows' ? 'Start Recorder with Windows' : 'Start Recorder at login'}
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
                  aria-label={platform.os === 'windows' ? 'Start recorder with Windows' : 'Start recorder at login'}
                >
                  <span className="switch-thumb" />
                </button>
              </div>
            </section>}
            {platform.os === 'macos' && platform.recording && activeTab === 'general' && <PermissionSettings />}
            <UpdateSettings updater={updater} />
          </div>
        </fieldset>
      </div>

      <div className="modal-footer">
        {audioNeedsReview && activeTab !== 'audio' && <button type="button" className="btn-ghost settings-review"
          disabled={busy} onClick={() => {
            changeTab('audio');
            document.getElementById('settings-tab-audio')?.focus();
          }}>Review audio settings</button>}
        <button className="btn-ghost" onClick={onClose} disabled={busy}>
          Cancel
        </button>
        <button
          className="btn-primary"
          disabled={busy || audioNeedsReview}
          onClick={() => onSave(draft)}
        >
          {busy ? 'Applying…' : 'Save settings'}
        </button>
      </div>
    </Modal>
  );
}
