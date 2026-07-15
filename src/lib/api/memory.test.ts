import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

import {
  forgetUserMemory,
  getUserMemoryIndex,
  rememberUserMemory,
  searchUserMemory,
  setMemoryLinks,
  updateUserMemory,
} from './memory';

describe('setMemoryLinks', () => {
  beforeEach(() => mocks.invokeIpc.mockReset());

  it('sends the strict camelCase payload', async () => {
    mocks.invokeIpc.mockResolvedValue({ ok: true, entry: {} });
    await setMemoryLinks('m_00000000000000000000000000000000', ['topics/testing.md']);
    expect(mocks.invokeIpc).toHaveBeenCalledWith('memory_set_links', {
      id: 'm_00000000000000000000000000000000',
      links: ['topics/testing.md'],
    });
  });
});

describe('user memory API', () => {
  beforeEach(() => mocks.invokeIpc.mockReset().mockResolvedValue({ ok: true }));

  it('uses distinct backend-owned commands with no path or project scope', async () => {
    await getUserMemoryIndex();
    await rememberUserMemory('Prefers concise answers');
    await updateUserMemory('m_00000000000000000000000000000000', 'Prefers examples');
    await forgetUserMemory('m_00000000000000000000000000000000');
    await searchUserMemory('examples', 10);

    expect(mocks.invokeIpc.mock.calls).toEqual([
      ['memory_user_index', {}],
      ['memory_user_remember', { text: 'Prefers concise answers' }],
      [
        'memory_user_update',
        { entryId: 'm_00000000000000000000000000000000', text: 'Prefers examples' },
      ],
      ['memory_user_forget', { entryId: 'm_00000000000000000000000000000000' }],
      ['memory_user_search', { query: 'examples', limit: 10 }],
    ]);
  });
});
