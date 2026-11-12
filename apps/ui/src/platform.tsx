import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { isTauriEnv } from './api';

export interface PlatformCapabilities {
  os: string;
  recording: boolean;
  application_audio: boolean;
  autostart: boolean;
  updates: boolean;
  portal_capture?: boolean;
}

const unavailable: PlatformCapabilities = {
  os: 'unknown', recording: false, application_audio: false, autostart: false, updates: false,
};
const PlatformContext = createContext(unavailable);

export function PlatformProvider({ children }: { children: ReactNode }) {
  const [platform, setPlatform] = useState(unavailable);
  useEffect(() => {
    if (!isTauriEnv()) return;
    let active = true;
    invoke<PlatformCapabilities>('get_platform_capabilities')
      .then(value => { if (active) setPlatform(value); })
      .catch(() => { /* Keep unavailable actions disabled if the bridge fails. */ });
    return () => { active = false; };
  }, []);
  return <PlatformContext.Provider value={platform}>{children}</PlatformContext.Provider>;
}

export const usePlatform = () => useContext(PlatformContext);
