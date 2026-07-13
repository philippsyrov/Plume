import { beforeEach, expect, test, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

import { loadSkillPromotionContext, previewSkillPromotion } from './skills';

beforeEach(() => mocks.invokeIpc.mockReset());

test('requests a promotion preview with only the persisted source selection', async () => {
  mocks.invokeIpc.mockResolvedValue({
    draft: { slug: 'chat', name: 'Chat', description: 'Draft', body: '# Draft' },
    source: { sessionId: 's_123', title: 'Chat', entryIndexes: [0, 2] },
    redactionCount: 0,
  });

  await previewSkillPromotion({
    sessionId: 's_123',
    entryIndexes: [2, 0],
    snapshotToken: 'sha256:abc',
  });

  expect(mocks.invokeIpc).toHaveBeenCalledWith('skills_promote_preview', {
    sessionId: 's_123',
    entryIndexes: [2, 0],
    snapshotToken: 'sha256:abc',
  });
});

test('loads promotion selection context from the trusted project command', async () => {
  mocks.invokeIpc.mockResolvedValue({ entries: [] });
  await loadSkillPromotionContext('s_123');
  expect(mocks.invokeIpc).toHaveBeenCalledWith('skills_promotion_context', {
    sessionId: 's_123',
  });
});
