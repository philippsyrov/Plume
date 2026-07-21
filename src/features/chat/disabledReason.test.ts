import { expect, it } from 'vitest';

import {
  chatStatusText,
  computeDisabledReason,
  inputPlaceholder,
} from './disabledReason';
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

it('keeps the no-model prompt in the composer without repeating it as status copy', () => {
  expect(inputPlaceholder(null, 'no-selection')).toBe('Choose a model to start');
  expect(chatStatusText(null, 'no-selection', false)).toBe('');
});

it('uses a neutral composer placeholder instead of exposing the model id', () => {
  expect(inputPlaceholder(apple, null)).toBe('Message Plume');
});
