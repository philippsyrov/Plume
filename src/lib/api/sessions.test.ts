import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

import { forkSession } from './sessions';

describe('sessions API', () => {
  it('forks with the exact verb and source payload', async () => {
    mocks.invokeIpc.mockResolvedValue({ session: {} });
    await forkSession({ scope: 'project', sessionId: 's123' });
    expect(mocks.invokeIpc).toHaveBeenCalledWith('sessions_fork', {
      scope: 'project',
      sessionId: 's123',
    });
  });
});
