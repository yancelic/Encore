import { usePlatform } from './platform';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  buildMontage,
  copyClipToClipboard,
  deleteClip,
  exportVertical,
  getConfig,
  getRecorderStatus,
  revealInExplorer,
  saveConfig,
  startRecorder,
  stopRecorder,
  syncAndListClips,
  toggleFavorite,
  triggerSaveClip,
  trimClip,
  updateClipDetails,
} from './api';
import { EditorView } from './components/EditorView';
import { LibraryView } from './components/LibraryView';
import { MontageModal } from './components/MontageModal';
import { SettingsModal } from './components/SettingsModal';
import { TopBar } from './components/TopBar';
import { useTheme } from './theme';
import { useUpdater } from './updater';
import { UpdateDialog, UpdateNotice } from './components/Updates';
import type { CropRect } from './components/CropControls';
import type {
  CaptureMode,
  ClipRecord,
  Config,
  RecorderStatus,
  SortOrder,
  ToastMessage,
} from './types';

export function App() {
  const platform = usePlatform();
  const theme = useTheme();
  const updater = useUpdater();
  const [status, setStatus] = useState<RecorderStatus | null>(null);
  const [config, setConfig] = useState<Config | null>(null);
  const [clips, setClips] = useState<ClipRecord[]>([]);
  const [allGames, setAllGames] = useState<string[]>([]);
  const [loadingClips, setLoadingClips] = useState<boolean>(true);
  const [activeClipId, setActiveClipId] = useState<string | null>(null);

  const [search, setSearch] = useState<string>('');
  const [gameFilter, setGameFilter] = useState<string | null>(null);
  const [favoritesOnly, setFavoritesOnly] = useState<boolean>(false);
  const [sortOrder, setSortOrder] = useState<SortOrder>('Newest');

  const [settingsOpen, setSettingsOpen] = useState<boolean>(false);
  const [montageClips, setMontageClips] = useState<ClipRecord[] | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const toastSeq = useRef(1);
  const lastSavedClipRef = useRef<string | null>(null);
  const lastStatusErrorRef = useRef<string | null>(null);

  const pushToast = useCallback((kind: ToastMessage['kind'], text: string) => {
    const id = toastSeq.current++;
    setToasts((prev) => [...prev.slice(-3), { id, kind, text }]);
    window.setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 3200);
  }, []);

  const refreshClips = useCallback(async () => {
    try {
      const list = await syncAndListClips({
        search,
        game: gameFilter,
        favorites_only: favoritesOnly,
        sort: sortOrder,
      });
      setClips(list);
      // Keep navigation stable while a search or collection narrows the results.
      setAllGames(previous => {
        const games = list.map(clip => clip.game);
        return Array.from(new Set(search || gameFilter || favoritesOnly ? [...previous, ...games] : games)).sort((a, b) => a.localeCompare(b));
      });
    } catch (e) {
      pushToast('error', `Failed loading clips: ${String(e)}`);
    } finally {
      setLoadingClips(false);
    }
  }, [search, gameFilter, favoritesOnly, sortOrder, pushToast]);

  useEffect(() => {
    getConfig().then(setConfig).catch(() => {});
  }, []);

  useEffect(() => {
    refreshClips();
  }, [refreshClips]);

  // Poll recorder IPC status cleanly so UI reflects live capture, saved hotkeys, and state changes.
  useEffect(() => {
    if (!platform.recording) return;
    let active = true;
    const poll = async () => {
      try {
        const st = await getRecorderStatus();
        if (!active) return;
        setStatus(st);
        const err = st?.error ?? null;
        if (err && err !== lastStatusErrorRef.current) {
          pushToast('error', err);
        }
        lastStatusErrorRef.current = err;
        if (st?.last_clip && st.last_clip !== lastSavedClipRef.current) {
          const prev = lastSavedClipRef.current;
          lastSavedClipRef.current = st.last_clip;
          if (prev !== null) {
            pushToast('success', 'New clip captured!');
            refreshClips();
          }
        }
      } catch {
        if (active) setStatus(null);
      }
    };
    poll();
    const timer = window.setInterval(poll, 1600);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [pushToast, refreshClips, platform.recording]);

  const activeClip = useMemo(
    () => clips.find((c) => c.id === activeClipId) ?? null,
    [clips, activeClipId],
  );

  const handleStartRecorder = async () => {
    setBusyAction('start');
    try {
      await startRecorder();
      const st = await getRecorderStatus();
      setStatus(st);
      if (st?.error) {
        lastStatusErrorRef.current = st.error;
        pushToast('error', st.error);
      } else {
        lastStatusErrorRef.current = null;
        pushToast('success', 'Background clip recorder started');
      }
    } catch (e) {
      pushToast('error', String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleStopRecorder = async () => {
    setBusyAction('stop');
    try {
      await stopRecorder();
      setStatus(null);
      pushToast('info', 'Background recorder stopped');
    } catch (e) {
      pushToast('error', String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleSaveClipNow = async () => {
    setBusyAction('save');
    try {
      await triggerSaveClip();
      pushToast('info', 'Saving replay clip...');
      window.setTimeout(refreshClips, 900);
    } catch (e) {
      pushToast('error', String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleChangeMode = async (mode: CaptureMode) => {
    const base = config ?? (await getConfig());
    const next: Config = { ...base, mode };
    setBusyAction('mode');
    try {
      await saveConfig(next);
      setConfig(next);
      setStatus(await getRecorderStatus());
      pushToast('success', `Capture mode: ${mode === 'GameDetection' ? 'Games' : 'Active Window'}`);
    } catch (e) {
      pushToast('error', String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleSaveConfig = async (next: Config) => {
    setBusyAction('settings');
    try {
      await saveConfig(next);
      setConfig(next);
      setStatus(await getRecorderStatus());
      setSettingsOpen(false);
      pushToast('success', 'Settings saved & applied');
      refreshClips();
    } catch (e) {
      pushToast('error', String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleToggleFavorite = async (clip: ClipRecord) => {
    const next = !clip.favorite;
    setClips((prev) => prev.map((c) => (c.id === clip.id ? { ...c, favorite: next } : c)));
    try {
      await toggleFavorite(clip.id, next);
    } catch (e) {
      pushToast('error', String(e));
      refreshClips();
    }
  };

  const handleCopyClip = async (
    id: string,
    startSecs?: number,
    endSecs?: number,
    mutedTracks?: boolean[],
    trackVolumes?: number[],
  ): Promise<boolean> => {
    if (busyAction) return false;
    setBusyAction('share');
    try {
      await copyClipToClipboard(id, startSecs, endSecs, mutedTracks, trackVolumes);
      pushToast('success', 'Clip file copied to clipboard. Paste it into another app to share.');
      return true;
    } catch (e) {
      pushToast('error', String(e));
      return false;
    } finally {
      setBusyAction(null);
    }
  };

  const handleReveal = async (path: string) => {
    try {
      await revealInExplorer(path);
    } catch (e) {
      pushToast('error', String(e));
    }
  };

  const handleDeleteClip = async (clip: ClipRecord) => {
    try {
      await deleteClip(clip.id);
      if (activeClipId === clip.id) setActiveClipId(null);
      setClips((prev) => prev.filter((c) => c.id !== clip.id));
      pushToast('info', `Deleted "${clip.title}"`);
    } catch (e) {
      pushToast('error', String(e));
    }
  };

  const handleSaveDetails = async (
    id: string,
    title: string,
    tag: string,
    description: string,
  ) => {
    try {
      await updateClipDetails(id, title, tag, description);
      setClips((prev) =>
        prev.map((c) => (c.id === id ? { ...c, title, tag, description } : c)),
      );
    } catch (e) {
      pushToast('error', String(e));
    }
  };

  const handleTrimClip = async (
    id: string,
    startSecs: number,
    endSecs: number,
    mutedTracks: boolean[],
    trackVolumes: number[],
  ) => {
    setBusyAction('trim');
    try {
      const created = await trimClip(id, startSecs, endSecs, mutedTracks, trackVolumes);
      await refreshClips();
      setActiveClipId(created.id);
      pushToast('success', `Saved trim "${created.title}"`);
    } catch (e) {
      pushToast('error', String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleExportVertical = async (
    id: string,
    blurBg: boolean,
    startSecs: number | null,
    endSecs: number | null,
    mutedTracks: boolean[],
    trackVolumes: number[],
    crop?: CropRect,
  ) => {
    setBusyAction('export916');
    try {
      const created = await exportVertical(
        id,
        blurBg,
        startSecs,
        endSecs,
        mutedTracks,
        trackVolumes,
        crop,
      );
      await refreshClips();
      setActiveClipId(created.id);
      pushToast('success', `Exported ${crop ? 'cropped' : '9:16 vertical'} clip "${created.title}"`);
    } catch (e) {
      pushToast('error', String(e));
    } finally {
      setBusyAction(null);
    }
  };

  const handleBuildMontage = async (ids: string[], title: string) => {
    setBusyAction('montage');
    try {
      const created = await buildMontage(ids, title);
      setMontageClips(null);
      await refreshClips();
      setActiveClipId(created.id);
      pushToast('success', `Montage "${created.title}" ready!`);
    } catch (e) {
      pushToast('error', String(e));
    } finally {
      setBusyAction(null);
    }
  };

  return (
    <div className="app-shell">
      <TopBar
        status={status}
        config={config}
        busyAction={busyAction}
        onStartRecorder={handleStartRecorder}
        onStopRecorder={handleStopRecorder}
        onSaveClipNow={handleSaveClipNow}
        onChangeMode={handleChangeMode}
        onOpenFolder={() => config?.clips_dir && handleReveal(config.clips_dir)}
        onRefreshClips={refreshClips}
        onOpenSettings={() => setSettingsOpen(true)}
      />

      <UpdateNotice updater={updater} />

      {activeClip ? (
        <EditorView
          theme={theme.resolved}
          clip={activeClip}
          busyAction={busyAction}
          onBack={() => setActiveClipId(null)}
          onSaveDetails={handleSaveDetails}
          onTrimClip={handleTrimClip}
          onExportVertical={handleExportVertical}
          onCopyClip={handleCopyClip}
          onRevealClip={handleReveal}
          onDeleteClip={handleDeleteClip}
        />
      ) : (
        <LibraryView
          clips={clips}
          allGames={allGames}
          loading={loadingClips}
          search={search}
          gameFilter={gameFilter}
          favoritesOnly={favoritesOnly}
          sortOrder={sortOrder}
          hotkey={status?.hotkey ?? config?.hotkey ?? 'Alt+S'}
          clipsDir={config?.clips_dir ?? ''}
          onSearchChange={setSearch}
          onGameFilterChange={setGameFilter}
          onFavoritesOnlyChange={setFavoritesOnly}
          onSortOrderChange={setSortOrder}
          onOpenClip={(c) => setActiveClipId(c.id)}
          onToggleFavorite={handleToggleFavorite}
          onCopyClip={(c) => handleCopyClip(c.id)}
          onRevealClip={(c) => handleReveal(c.path)}
          onDeleteClip={handleDeleteClip}
          onCreateMontage={(picked) => setMontageClips(picked)}
        />
      )}

      {settingsOpen && config && (
        <SettingsModal
          updater={updater}
          themePreference={theme.preference}
          onThemeChange={theme.changePreference}
          config={config}
          audioWarnings={status?.audio_warnings}
          busy={busyAction === 'settings'}
          onClose={() => setSettingsOpen(false)}
          onSave={handleSaveConfig}
        />
      )}

      {montageClips && (
        <MontageModal
          initialClips={montageClips}
          busy={busyAction === 'montage'}
          onClose={() => setMontageClips(null)}
          onBuild={handleBuildMontage}
        />
      )}

      <div className="toast-stack" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`}>
            {t.text}
          </div>
        ))}
      </div>
      <UpdateDialog updater={updater} />
    </div>
  );
}
export default App;
