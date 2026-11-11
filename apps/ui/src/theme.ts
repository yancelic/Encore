import { useCallback, useEffect, useLayoutEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauriEnv } from './api';

export type ThemePreference = 'system' | 'light' | 'dark';
export type ResolvedTheme = 'light' | 'dark';
export const THEME_STORAGE_KEY = 'encore.theme';
const systemQuery = '(prefers-color-scheme: dark)';

function readPreference(): ThemePreference {
  try {
    const value = localStorage.getItem(THEME_STORAGE_KEY);
    return value === 'light' || value === 'dark' ? value : 'system';
  } catch {
    return 'system';
  }
}

function resolveTheme(
  preference: ThemePreference,
  systemDark: boolean,
): ResolvedTheme {
  return preference === 'system' ? (systemDark ? 'dark' : 'light') : preference;
}

function applyDocumentTheme(
  preference: ThemePreference,
  resolved: ResolvedTheme,
) {
  document.documentElement.dataset.theme = resolved;
  document.documentElement.dataset.themePreference = preference;
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute('content', resolved === 'dark' ? '#15171b' : '#ecebf3');
}

/** Apply the saved choice before React mounts. */
export function initializeTheme() {
  const preference = readPreference();
  applyDocumentTheme(
    preference,
    resolveTheme(preference, window.matchMedia(systemQuery).matches),
  );
}

export function useTheme() {
  const [preference, setPreference] = useState<ThemePreference>(readPreference);
  const [systemDark, setSystemDark] = useState(
    () => window.matchMedia(systemQuery).matches,
  );
  const resolved = resolveTheme(preference, systemDark);

  useEffect(() => {
    const query = window.matchMedia(systemQuery);
    const onChange = () => setSystemDark(query.matches);
    const onStorage = (event: StorageEvent) => {
      if (event.key === THEME_STORAGE_KEY || event.key === null)
        setPreference(readPreference());
    };
    onChange();
    query.addEventListener('change', onChange);
    window.addEventListener('storage', onStorage);
    return () => {
      query.removeEventListener('change', onChange);
      window.removeEventListener('storage', onStorage);
    };
  }, []);

  useLayoutEffect(() => {
    applyDocumentTheme(preference, resolved);
  }, [preference, resolved]);

  useEffect(() => {
    if (isTauriEnv()) {
      void getCurrentWindow()
        .setTheme(preference === 'system' ? null : preference)
        .catch((error) => console.warn('Could not apply native theme', error));
    }
  }, [preference]);

  useEffect(() => {
    if (isTauriEnv()) {
      void getCurrentWindow()
        .setBackgroundColor(resolved === 'dark' ? '#15171b' : '#ecebf3')
        .catch((error) =>
          console.warn('Could not apply native background', error),
        );
    }
  }, [resolved]);

  const changePreference = useCallback((next: ThemePreference) => {
    setPreference(next);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      /* Keep themes usable when persistent storage is unavailable. */
    }
  }, []);

  return { preference, resolved, changePreference };
}
