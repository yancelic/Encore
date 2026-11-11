import { useRef, useState } from 'react';
import { Check, FolderOpen, Gamepad2, Plus, RefreshCw } from 'lucide-react';
import { listCaptureApplications, pickGameExecutable } from '../api';
import type { CaptureApplication } from '../types';

export function GamePicker({ games, onAdd }: { games: string[]; onAdd: (exe: string) => void }) {
  const [open, setOpen] = useState(false);
  const [applications, setApplications] = useState<CaptureApplication[]>([]);
  const [loading, setLoading] = useState(false);
  const [browsing, setBrowsing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [manual, setManual] = useState('');
  const [manualError, setManualError] = useState<string | null>(null);
  const [addedMessage, setAddedMessage] = useState<string | null>(null);
  const refreshing = useRef(false);
  const hasGame = (exe: string) => games.some((game) => game.toLowerCase() === exe.toLowerCase());

  const refresh = async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    setLoading(true);
    setError(null);
    try {
      setApplications(await listCaptureApplications());
    } catch {
      setError('Could not load running applications. Try refreshing or browse for your game.');
    } finally {
      refreshing.current = false;
      setLoading(false);
    }
  };

  const add = (exe: string) => {
    if (hasGame(exe)) {
      setAddedMessage(`${exe} is already in your games.`);
      return;
    }
    onAdd(exe);
    setAddedMessage(`${exe} added. Save settings to remember it.`);
  };

  const browse = async () => {
    if (browsing) return;
    setBrowsing(true);
    setError(null);
    try {
      const exe = await pickGameExecutable();
      if (exe) add(exe);
    } catch {
      setError('Could not select the game. Choose its .exe file and try again.');
    } finally {
      setBrowsing(false);
    }
  };

  const addManually = () => {
    const name = manual.trim();
    const exe = name.toLowerCase().endsWith('.exe') ? name : `${name}.exe`;
    if (!name || exe.length <= 4 || /[\\/:*?"<>|\x00-\x1f]/.test(exe)) {
      setManualError('Enter an executable filename, such as Game.exe.');
      return;
    }
    setManualError(null);
    add(exe);
    setManual('');
  };

  const filtered = applications.filter((app) => `${app.title} ${app.exe}`.toLowerCase().includes(search.trim().toLowerCase()));

  return (
    <div className="game-picker">
      <p className="capture-mode-help">Open your game and choose its window, or browse for the game’s .exe file.</p>
      <div className="game-picker-actions">
        <button type="button" className="btn-secondary" aria-expanded={open} aria-controls="running-game-picker"
          onClick={() => { setOpen(!open); if (!open) void refresh(); }}>
          <Gamepad2 size={14} /> {open ? 'Hide running applications' : 'Choose running application'}
        </button>
        <button type="button" className="btn-secondary" disabled={browsing} onClick={() => void browse()}>
          <FolderOpen size={14} /> {browsing ? 'Choosing…' : 'Browse game .exe'}
        </button>
      </div>
      {open && <div id="running-game-picker" className="running-game-picker" aria-label="Running applications">
        <div className="field-row">
          <input type="search" aria-label="Search running applications" placeholder="Find your game…"
            value={search} onChange={(event) => setSearch(event.target.value)} />
          <button type="button" className="btn-secondary" disabled={loading} onClick={() => void refresh()}>
            <RefreshCw size={14} /> Refresh
          </button>
        </div>
        {loading ? <p className="capture-mode-help" role="status">Loading running applications…</p>
          : !error && filtered.length === 0 ? <p className="capture-mode-help" role="status">
            {applications.length === 0 ? 'No application windows found. Open your game and refresh, or browse for its .exe file.'
              : 'No matching applications. Try another search or browse for your game.'}
          </p>
          : <ul className="running-games">
            {filtered.map((app) => {
              const added = hasGame(app.exe);
              return <li key={app.exe.toLowerCase()}>
                <div><strong>{app.title}</strong><span className="mono">{app.exe}</span></div>
                <button type="button" className="btn-secondary" disabled={added}
                  aria-label={added ? `${app.title} already added` : `Add ${app.title}`} onClick={() => add(app.exe)}>
                  {added ? <Check size={14} /> : <Plus size={14} />} {added ? 'Added' : 'Add'}
                </button>
              </li>;
            })}
          </ul>}
      </div>}
      {error && <p className="update-error" role="alert">{error}</p>}
      <p className="capture-mode-help game-picker-feedback" role="status">{addedMessage}</p>
      <details className="manual-game-entry">
        <summary>Enter executable name manually</summary>
        <div className="field-row">
          <input type="text" placeholder="Game.exe" aria-label="Add game executable" value={manual}
            onChange={(event) => setManual(event.target.value)}
            onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addManually(); } }} />
          <button type="button" className="btn-secondary" onClick={addManually}><Plus size={14} /> Add Game</button>
        </div>
        {manualError && <p className="update-error" role="alert">{manualError}</p>}
      </details>
    </div>
  );
}
