import { describe, expect, it } from 'vitest';

import type { ChatEntry } from '../chat/useChat';
import {
  DEFAULT_SESSION_TITLE,
  SESSION_TITLE_MAX_CHARS,
  deriveSessionTitle,
  normalizeSessionTitle,
} from './sessionTitle';

describe('normalizeSessionTitle', () => {
  it('trims and collapses whitespace runs, including newlines and tabs', () => {
    expect(normalizeSessionTitle('  fix the \n\n  login\tbug  ')).toBe(
      'fix the login bug',
    );
  });

  it('returns null when nothing displayable is left', () => {
    expect(normalizeSessionTitle('')).toBeNull();
    expect(normalizeSessionTitle('   \n\t  ')).toBeNull();
  });

  it('keeps a short message untouched', () => {
    expect(normalizeSessionTitle('why is the build slow?')).toBe(
      'why is the build slow?',
    );
  });

  it('caps a long message at a nearby word boundary with an ellipsis', () => {
    const raw =
      'please review the session persistence spine and explain how the queue orders lazy saves';
    const title = normalizeSessionTitle(raw);
    expect(title).toBe('please review the session persistence spine and explain how…');
    expect(Array.from(title ?? '').length).toBeLessThanOrEqual(
      SESSION_TITLE_MAX_CHARS + 1, // stem + ellipsis
    );
  });

  it('hard-cuts an unbroken run instead of wasting the cap', () => {
    const raw = 'x'.repeat(200);
    const title = normalizeSessionTitle(raw);
    expect(title).toBe(`${'x'.repeat(SESSION_TITLE_MAX_CHARS)}…`);
  });

  it('never splits a surrogate pair at the cap', () => {
    const raw = '🦆'.repeat(SESSION_TITLE_MAX_CHARS + 10);
    const title = normalizeSessionTitle(raw);
    expect(title).toBe(`${'🦆'.repeat(SESSION_TITLE_MAX_CHARS)}…`);
  });
});

describe('deriveSessionTitle', () => {
  const user = (content: string): ChatEntry => ({
    kind: 'message',
    message: { role: 'user', content },
  });
  const assistant: ChatEntry = {
    kind: 'message',
    message: { role: 'assistant', content: 'an answer' },
  };
  const error: ChatEntry = { kind: 'error', message: 'provider went away' };

  it('uses the FIRST user message, not later ones', () => {
    expect(
      deriveSessionTitle([user('first question'), assistant, user('second question')]),
    ).toBe('first question');
  });

  it('skips non-user entries when looking for the source message', () => {
    expect(deriveSessionTitle([error, assistant, user('after a rough start')])).toBe(
      'after a rough start',
    );
  });

  it('returns null when the snapshot has no user message', () => {
    expect(deriveSessionTitle([])).toBeNull();
    expect(deriveSessionTitle([error, assistant])).toBeNull();
  });

  it('the default title constant matches the backend default', () => {
    // Mirrors src-tauri/src/sessions/validation.rs::DEFAULT_TITLE. If
    // this drifts, auto-titling silently never fires.
    expect(DEFAULT_SESSION_TITLE).toBe('New chat');
  });
});
