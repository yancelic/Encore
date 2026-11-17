import { usePlatform } from './platform';
import { Channel, invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';
import { isTauriEnv } from './api';

export interface AvailableUpdate {
  version: string;
  notes: string | null;
}

export type UpdateProgress =
  | { phase: 'downloading'; downloaded: number; total: number | null }
  | { phase: 'preparing' | 'installing' };

type Phase = 'idle' | 'checking' | 'available' | 'upToDate' | 'error' | UpdateProgress['phase'];

export function useUpdater() {
  const platform = usePlatform();
  const [phase, setPhase] = useState<Phase>('idle');
  const [update, setUpdate] = useState<AvailableUpdate | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ downloaded: number; total: number | null }>({ downloaded: 0, total: null });
  const [dialogOpen, setDialogOpen] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const operation = useRef(false);
  const checkedOnLaunch = useRef(false);

  const check = useCallback(async () => {
    if (!platform.updates || operation.current) return;
    operation.current = true;
    setPhase('checking');
    setError(null);
    try {
      if (!isTauriEnv()) throw new Error('Update checks are available in the installed desktop app.');
      const available = await invoke<AvailableUpdate | null>('check_for_updates');
      setUpdate(available);
      setDismissed(false);
      setPhase(available ? 'available' : 'upToDate');
    } catch (e) {
      setError(String(e));
      setPhase('error');
    } finally {
      operation.current = false;
    }
  }, [platform.updates]);

  useEffect(() => {
    // No interval, background service or duplicate StrictMode launch check.
    if (!platform.updates || checkedOnLaunch.current || !isTauriEnv()) return;
    checkedOnLaunch.current = true;
    void check();
  }, [check, platform.updates]);

  const install = async () => {
    if (!update || operation.current) return;
    operation.current = true;
    setError(null);
    setProgress({ downloaded: 0, total: null });
    setPhase('downloading');
    try {
      const onProgress = new Channel<UpdateProgress>();
      onProgress.onmessage = (event) => {
        setPhase(event.phase);
        if (event.phase === 'downloading') setProgress(event);
      };
      await invoke<void>('install_update', { onProgress });
    } catch (e) {
      setError(String(e));
      setPhase('error');
    } finally {
      operation.current = false;
    }
  };

  return {
    phase, update, error, progress, dialogOpen,
    busy: phase === 'downloading' || phase === 'preparing' || phase === 'installing',
    showNotice: Boolean(update) && !dismissed,
    check,
    install,
    review: () => setDialogOpen(true),
    close: () => setDialogOpen(false),
    dismiss: () => setDismissed(true),
  };
}

export type Updater = ReturnType<typeof useUpdater>;
