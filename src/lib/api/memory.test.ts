import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

import { setMemoryLinks } from './memory';

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
