import { useCallback, useEffect, useState } from 'react';

export type AppearancePreference = 'system' | 'light' | 'dark';
export type ResolvedAppearance = 'light' | 'dark';

export const APPEARANCE_STORAGE_KEY = 'plume:appearance-v1';

export function readAppearancePreference(): AppearancePreference {
  try {
    const stored = localStorage.getItem(APPEARANCE_STORAGE_KEY);
    if (stored === 'system' || stored === 'light' || stored === 'dark') return stored;
  } catch {
    // Plume light remains the safe in-memory default when storage is unavailable.
  }
  return 'light';
}

export function useAppearance(): {
  preference: AppearancePreference;
  resolved: ResolvedAppearance;
  setPreference: (next: AppearancePreference) => void;
} {
  const [preference, setPreferenceState] = useState<AppearancePreference>(
    readAppearancePreference,
  );
  const [systemDark, setSystemDark] = useState(() => systemPrefersDark());
  const resolved: ResolvedAppearance =
    preference === 'system' ? (systemDark ? 'dark' : 'light') : preference;

  useEffect(() => {
    const media = window.matchMedia?.('(prefers-color-scheme: dark)');
    if (!media) return;
    const update = () => setSystemDark(media.matches);
    update();
    media.addEventListener?.('change', update);
    return () => media.removeEventListener?.('change', update);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.plumeTheme = resolved;
  }, [resolved]);

  const setPreference = useCallback((next: AppearancePreference) => {
    setPreferenceState(next);
    try {
      localStorage.setItem(APPEARANCE_STORAGE_KEY, next);
    } catch {
      // The window-local preference still works for this run.
    }
  }, []);

  return { preference, resolved, setPreference };
}

function systemPrefersDark(): boolean {
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false;
}
