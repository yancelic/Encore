import { usePlatform } from '../platform';
import { ArrowDownToLine, X } from 'lucide-react';
import { version } from '../../package.json';
import type { Updater } from '../updater';
import { Modal } from './Modal';

export function UpdateSettings({ updater }: { updater: Updater }) {
  const platform = usePlatform();
  if (!platform.updates) return <section className="settings-section">
    <h3>App updates</h3>
    <p className="capture-mode-help">Update this preview through the download page or your package manager.</p>
  </section>;
  return (
    <section className="settings-section update-settings" aria-labelledby="updates-heading">
      <div className="update-row">
        <div>
          <h3 id="updates-heading">App updates</h3>
          <p className="capture-mode-help">Encore {version} · Checks when you open the library.</p>
        </div>
        <button type="button" className="btn-secondary" disabled={updater.phase === 'checking' || updater.busy}
          onClick={() => void updater.check()}>
          {updater.phase === 'checking' ? 'Checking…' : 'Check for updates'}
        </button>
      </div>
      <div aria-live="polite">
        {updater.phase === 'upToDate' && <p className="capture-mode-help">You’re up to date.</p>}
        {updater.error && <p className="update-error" role="alert">{updater.error}</p>}
        {updater.update && <button type="button" className="btn-ghost" onClick={updater.review}>
          Review Encore {updater.update.version}
        </button>}
      </div>
    </section>
  );
}

export function UpdateNotice({ updater }: { updater: Updater }) {
  if (!updater.showNotice || !updater.update) return null;
  return (
    <aside className="update-notice" aria-label="App update available">
      <ArrowDownToLine size={16} aria-hidden="true" />
      <p>Encore {updater.update.version} is available.</p>
      <button type="button" className="btn-secondary" onClick={updater.review}>Review update</button>
      <button type="button" className="btn-ghost" onClick={updater.dismiss}>Later</button>
    </aside>
  );
}

export function UpdateDialog({ updater }: { updater: Updater }) {
  if (!updater.dialogOpen || !updater.update) return null;
  const percent = updater.progress.total
    ? Math.min(100, Math.round(updater.progress.downloaded / updater.progress.total * 100)) : undefined;
  return (
    <Modal titleId="update-title" busy={updater.busy} onClose={updater.close}>
      <div className="modal-header">
        <div>
          <h2 id="update-title">Encore {updater.update.version}</h2>
          <p className="modal-subtitle">Ready when you are.</p>
        </div>
        <button type="button" className="btn-icon" aria-label="Close update" disabled={updater.busy} onClick={updater.close}>
          <X size={15} />
        </button>
      </div>
      <div className="modal-body update-body">
        {updater.update.notes && <div className="update-notes">{updater.update.notes}</div>}
        <p>Encore will download the update, finish accepted saves and exports, then restart.
          Recording pauses during installation and resumes if it was running.
          Your saved clips and settings stay in place.</p>
        <p>Save any settings you have changed and any replay you want to keep before continuing.
          The current replay buffer and unsaved edits will be cleared.</p>
        <div aria-live="polite">
          {updater.phase === 'downloading' && <>
            <p>Downloading update{percent === undefined ? '…' : ` · ${percent}%`}</p>
            <progress aria-label="Update download" max={100} value={percent} />
          </>}
          {updater.phase === 'preparing' && <p>Finishing saves and exports. Please keep Encore open…</p>}
          {updater.phase === 'installing' && <p>Starting the installer. Encore will reopen after the update…</p>}
          {updater.error && <p className="update-error" role="alert">{updater.error}</p>}
        </div>
      </div>
      <div className="modal-footer">
        <button type="button" className="btn-ghost" onClick={updater.close} disabled={updater.busy}>Later</button>
        {updater.phase === 'error' ? <button type="button" className="btn-primary" onClick={() => void updater.check()}>Check again</button>
          : <button type="button" className="btn-primary" disabled={updater.busy || updater.phase === 'checking'} onClick={() => void updater.install()}>
            {updater.busy ? 'Updating…' : 'Update and restart'}
          </button>}
      </div>
    </Modal>
  );
}
