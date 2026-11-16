import { useCallback, useEffect, useRef, useState } from 'react';
import { Check, RotateCcw, X } from 'lucide-react';
import {
  getMacosPermissions,
  requestMacosPermission,
  resetMacosPermissions,
  restartRecorder,
  startRecorder,
} from '../api';
import type { MacosPermissions, PermissionKind, PermissionState } from '../types';

// macOS files Screen Recording and Microphone under the outer Encore app that
// contains the recorder, but Input Monitoring under the recorder bundle itself.
// Naming the right entry matters: switching on the other one changes nothing.
const ITEMS: { kind: PermissionKind; label: string; detail: string; listedAs: string }[] = [
  { kind: 'screen', label: 'Screen & System Audio Recording', detail: 'Captures the game window, plus game and chat audio.', listedAs: 'Encore' },
  { kind: 'microphone', label: 'Microphone', detail: 'Records your voice as its own track.', listedAs: 'Encore' },
  { kind: 'input_monitoring', label: 'Input Monitoring', detail: 'Lets the save shortcut work while a game is focused.', listedAs: 'Encore Recorder' },
];

const STATE_LABEL: Record<PermissionState, string> = {
  granted: 'Allowed',
  denied: 'Not allowed',
  restricted: 'Blocked by policy',
  not_determined: 'Not allowed',
};

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * macOS only: every capture permission belongs to the Encore Recorder bundle, so
 * this panel reads and requests them through the running recorder.
 */
export function PermissionSettings() {
  const [permissions, setPermissions] = useState<MacosPermissions | null>(null);
  const [working, setWorking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);
  const mounted = useRef(true);

  const refresh = useCallback(() => {
    getMacosPermissions()
      .then((value) => { if (mounted.current) setPermissions(value); })
      .catch((reason) => { if (mounted.current) setError(errorText(reason)); });
  }, []);

  useEffect(() => {
    mounted.current = true;
    refresh();
    // Microphone and Input Monitoring changes are visible immediately; poll so the
    // panel follows toggles made in System Settings without a manual refresh.
    const timer = window.setInterval(refresh, 2000);
    window.addEventListener('focus', refresh);
    return () => {
      mounted.current = false;
      window.clearInterval(timer);
      window.removeEventListener('focus', refresh);
    };
  }, [refresh]);

  const run = async (label: string, action: () => Promise<MacosPermissions | void>) => {
    setWorking(label);
    setError(null);
    try {
      const value = await action();
      if (value && mounted.current) setPermissions(value);
      else refresh();
    } catch (reason) {
      if (mounted.current) setError(errorText(reason));
    } finally {
      if (mounted.current) setWorking(null);
    }
  };

  // The recorder asks macOS first (that adds Encore Recorder to the list), then
  // opens the matching System Settings pane wherever the switch has to be turned on.
  const allow = (kind: PermissionKind) => run(kind, () => {
    return requestMacosPermission(kind);
  });

  if (!permissions) {
    return (
      <section className="settings-section" aria-labelledby="permissions-heading">
        <h3 id="permissions-heading">Permissions</h3>
        <p className="capture-mode-help">{error ?? 'Checking permissions…'}</p>
      </section>
    );
  }

  return (
    <section className="settings-section permission-settings" aria-labelledby="permissions-heading">
      <div>
        <h3 id="permissions-heading">Permissions</h3>
        <p className="capture-mode-help">
          In System Settings › Privacy &amp; Security, switch on the entry named under each permission.
        </p>
      </div>

      {!permissions.running ? (
        <div className="permission-row">
          <div className="permission-text">
            <span className="permission-label">The recorder is not running</span>
            <span className="capture-mode-help">Start it to check and grant permissions.</span>
          </div>
          <button type="button" className="btn-secondary" disabled={!!working}
            onClick={() => void run('start', startRecorder)}>
            {working === 'start' ? 'Starting…' : 'Start recorder'}
          </button>
        </div>
      ) : (
        <ul className="permission-list">
          {ITEMS.map(({ kind, label, detail, listedAs }) => {
            const state = permissions[kind] ?? 'not_determined';
            const granted = state === 'granted';
            const Icon = granted ? Check : X;
            return (
              <li className="permission-row" key={kind}>
                <span className={`permission-state ${granted ? 'granted' : 'missing'}`} aria-hidden="true">
                  <Icon size={14} strokeWidth={3} />
                </span>
                <div className="permission-text">
                  <span className="permission-label">{label}</span>
                  <span className="capture-mode-help">{detail}</span>
                  {!granted && <span className="capture-mode-help">Switch on <strong>{listedAs}</strong> in System Settings.</span>}
                </div>
                <span className={`permission-badge ${granted ? 'granted' : 'missing'}`}>{STATE_LABEL[state]}</span>
                {!granted && (
                  <button type="button" className="btn-primary permission-allow" disabled={!!working || state === 'restricted'}
                    aria-label={`Allow ${label}`} onClick={() => void allow(kind)}>
                    {working === kind ? 'Opening…' : 'Allow'}
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      )}

      {permissions.running && permissions.screen !== 'granted' && (
        <div className="permission-notice" role="status">
          <span>macOS applies a new Screen Recording permission only to a recorder that starts afterwards. Already switched it on? Restart the recorder.</span>
          <button type="button" className="btn-secondary" disabled={!!working}
            onClick={() => void run('restart', restartRecorder)}>
            {working === 'restart' ? 'Restarting…' : 'Restart recorder'}
          </button>
        </div>
      )}

      {permissions.running && permissions.independent === false && (
        <div className="permission-notice" role="status">
          <span>This recorder was started by an older Encore, so macOS may file permissions under “Encore”. Restart it to keep them in one place.</span>
          <button type="button" className="btn-secondary" disabled={!!working}
            onClick={() => void run('restart', restartRecorder)}>
            {working === 'restart' ? 'Restarting…' : 'Restart recorder'}
          </button>
        </div>
      )}

      {permissions.adhoc_signature && (
        <p className="capture-mode-help permission-warning" role="note">
          This build has an ad-hoc signature, so macOS treats every new build as a different app.
          Developers: run <code>npm --prefix apps/ui run setup:macos-signing</code> once before building to keep permissions.
        </p>
      )}

      <div className="permission-reset">
        <span className="capture-mode-help">
          Permission switch missing or stuck? Reset clears every Encore entry, including ones left by older builds,
          and restarts the recorder so macOS asks again.
        </span>
        {confirmReset ? (
          <span className="permission-reset-actions">
            <button type="button" className="btn-ghost" disabled={!!working} onClick={() => setConfirmReset(false)}>Cancel</button>
            <button type="button" className="btn-secondary" disabled={!!working}
              onClick={() => void run('reset', resetMacosPermissions).then(() => {
                setConfirmReset(false);
              })}>
              {working === 'reset' ? 'Resetting…' : 'Reset now'}
            </button>
          </span>
        ) : (
          <button type="button" className="btn-ghost" disabled={!!working} onClick={() => setConfirmReset(true)}>
            <RotateCcw size={13} /> Reset permissions
          </button>
        )}
      </div>

      {error && <p className="update-error" role="alert">{error}</p>}
    </section>
  );
}
