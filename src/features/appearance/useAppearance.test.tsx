import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { APPEARANCE_STORAGE_KEY, useAppearance } from './useAppearance';

describe('useAppearance', () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute('data-plume-theme');
    vi.stubGlobal('matchMedia', vi.fn().mockReturnValue({
      matches: true,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
  });

  it('starts in Plume light and persists explicit choices', () => {
    const { result } = renderHook(() => useAppearance());

    expect(result.current.preference).toBe('light');
    expect(result.current.resolved).toBe('light');
    expect(document.documentElement).toHaveAttribute('data-plume-theme', 'light');

    act(() => result.current.setPreference('dark'));
    expect(localStorage.getItem(APPEARANCE_STORAGE_KEY)).toBe('dark');
    expect(document.documentElement).toHaveAttribute('data-plume-theme', 'dark');
  });

  it('lets System follow the current macOS appearance', () => {
    localStorage.setItem(APPEARANCE_STORAGE_KEY, 'system');
    const { result } = renderHook(() => useAppearance());

    expect(result.current.preference).toBe('system');
    expect(result.current.resolved).toBe('dark');
    expect(document.documentElement).toHaveAttribute('data-plume-theme', 'dark');
  });
});
