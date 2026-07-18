import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { useSelectedModel } from './useSelectedModel';

describe('useSelectedModel', () => {
  it('advances its synchronous revision for direct select and clear intents', () => {
    const { result } = renderHook(() => useSelectedModel());

    expect(result.current.revision()).toBe(0);
    act(() => {
      result.current.select({
        providerId: 'ollama',
        providerDisplayName: 'Ollama',
        modelId: 'qwen2.5-coder',
      });
    });
    expect(result.current.revision()).toBe(1);
    act(() => {
      result.current.clear();
    });
    expect(result.current.revision()).toBe(2);
  });
});
