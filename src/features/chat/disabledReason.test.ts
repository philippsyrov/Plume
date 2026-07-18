import { expect, it } from 'vitest';

import { computeDisabledReason } from './disabledReason';
import type { SelectedModel } from '../model-picker/useSelectedModel';

const apple: SelectedModel = {
  providerId: 'apple-foundation',
  providerDisplayName: 'Apple On-Device',
  modelId: 'system',
};

it('allows exactly the Apple on-device provider without reachability or MLX-handle gates', () => {
  expect(computeDisabledReason(apple, 'idle', true, true, false)).toBeNull();
  expect(computeDisabledReason({ ...apple, providerId: 'other-provider' }, 'idle', false, false, false))
    .toBe('unsupported-provider');
});
