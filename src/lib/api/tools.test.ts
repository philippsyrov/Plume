import { describe, expect, it, vi } from 'vitest';

import { listTools, searchTools } from './tools';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

describe('tools.* IPC wrappers (D92)', () => {
  it('listTools invokes tools_list with an empty payload', async () => {
    mocks.invokeIpc.mockResolvedValue({ tools: [] });
    await listTools();
    expect(mocks.invokeIpc).toHaveBeenCalledWith('tools_list', {});
  });

  it('searchTools forwards query + limit to tools_search', async () => {
    mocks.invokeIpc.mockResolvedValue({ query: 'github', core: [], matched: [] });
    await searchTools('github', 10);
    expect(mocks.invokeIpc).toHaveBeenCalledWith('tools_search', {
      query: 'github',
      limit: 10,
    });
  });
});
