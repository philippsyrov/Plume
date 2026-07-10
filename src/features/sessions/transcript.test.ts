import { describe, expect, it } from 'vitest';

import type { ChatEntry } from '../chat/useChat';
import { entriesToWire, persistableOf, sameEntries, wireToEntries } from './transcript';

const user: ChatEntry = {
  kind: 'message',
  message: { role: 'user', content: 'change greet()' },
  sentInMode: 'proposeDiff',
  attachmentRelPath: 'src/greet.py',
  attachmentLineRange: { startLine: 3, endLine: 7 },
};

const assistant: ChatEntry = {
  kind: 'message',
  message: { role: 'assistant', content: 'done' },
  modelUsed: 'qwen2.5-coder',
  durationMs: 1200,
  stats: {
    outputTokens: 42,
    evalMs: 900,
    tokensPerSecond: 46.5,
    promptTokens: 101,
    promptMs: null,
  },
};

const streaming: ChatEntry = {
  kind: 'streaming',
  streamId: 'chat-1',
  content: 'half a tok',
  tokenCount: 3,
};

describe('transcript mappers', () => {
  it('filters streaming placeholders out of the persistable slice, keeping refs', () => {
    const entries: ChatEntry[] = [user, assistant, streaming];
    const persistable = persistableOf(entries);
    expect(persistable).toHaveLength(2);
    expect(persistable[0]).toBe(user);
    expect(persistable[1]).toBe(assistant);
  });

  it('sameEntries treats a token frame (same refs, new array) as unchanged', () => {
    const before = persistableOf([user, streaming]);
    const afterToken = persistableOf([
      user,
      { ...streaming, content: streaming.kind === 'streaming' ? `${streaming.content}x` : '' },
    ]);
    expect(sameEntries(before, afterToken)).toBe(true);

    const afterTerminal = persistableOf([user, assistant]);
    expect(sameEntries(before, afterTerminal)).toBe(false);
  });

  it('round-trips message, cancelled, and error entries through the wire shape', () => {
    const cancelled: ChatEntry = {
      kind: 'cancelled',
      partial: 'half a thought',
      modelUsed: 'qwen2.5-coder',
      durationMs: 300,
    };
    const error: ChatEntry = { kind: 'error', message: 'provider went away' };
    const entries = [user, assistant, cancelled, error];

    const wire = entriesToWire(entries);
    expect(wire.map((e) => e.kind)).toEqual(['message', 'message', 'cancelled', 'error']);
    expect(wireToEntries(wire)).toEqual(entries);
  });

  it('never emits streaming entries onto the wire', () => {
    expect(entriesToWire([user, streaming])).toHaveLength(1);
  });
});
