import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

import { forkSession, rollbackSession } from './sessions';

describe('sessions API', () => {
  it('forks with the exact verb and source payload', async () => {
    mocks.invokeIpc.mockResolvedValue({ session: {} });
    await forkSession({ scope: 'project', sessionId: 's123' });
    expect(mocks.invokeIpc).toHaveBeenCalledWith('sessions_fork', {
      scope: 'project',
      sessionId: 's123',
    });
  });

  it('rolls back with the exact verb and bounded turn payload', async () => {
    mocks.invokeIpc.mockResolvedValue({ session: {} });
    await rollbackSession({ scope: 'local', sessionId: 's123', turnCount: 2 });
    expect(mocks.invokeIpc).toHaveBeenCalledWith('sessions_rollback', {
      scope: 'local',
      sessionId: 's123',
      turnCount: 2,
    });
  });
});
