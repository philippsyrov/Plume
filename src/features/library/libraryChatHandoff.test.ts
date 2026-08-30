// Library hands an object into chat, which first needs a chat to hand it into.
//
// This guards the ownership half of that: which scope a source may reach, and
// how a session is obtained when the surface has none. Local scope must resolve
// the one Home conversation through the persisted-chat API rather than minting
// an ordinary chat, because a memory entry attached to a stray chat is a chat
// the user will not find again.

import { describe, expect, it, vi } from 'vitest';

import type { SessionIdentity, SessionScope } from '../../lib/api/sessions';
import type { PersistedChatApi } from '../sessions/usePersistedChat';
import { createLibraryChatHandoff } from './libraryChatHandoff';

function fakePersisted(overrides: {
  scope: SessionScope;
  sessionId: string | null;
  ensureOwnedSession?: PersistedChatApi['ensureOwnedSession'];
}) {
  const identity = { scope: overrides.scope, sessionId: overrides.sessionId };
  const addContextSource = vi.fn().mockReturnValue('added');
  const startNewSession = vi.fn().mockResolvedValue(true);
  const openScope = vi.fn().mockResolvedValue(true);
  const ensureOwnedSession =
    overrides.ensureOwnedSession ??
    vi.fn(async (scope: SessionScope): Promise<SessionIdentity | null> => {
      identity.scope = scope;
      identity.sessionId = 'home-1';
      return { scope, sessionId: 'home-1' };
    });
  const persisted = {
    surfaceIdentity: () => ({ ...identity }),
    chat: { addContextSource },
    ensureOwnedSession,
    startNewSession,
    openScope,
  } as unknown as PersistedChatApi;
  return { persisted, addContextSource, startNewSession, openScope, ensureOwnedSession };
}

describe('createLibraryChatHandoff', () => {
  it('resolves the owning local session through one shared path, never a new chat', async () => {
    const { persisted, startNewSession, ensureOwnedSession, addContextSource } = fakePersisted({
      scope: 'local',
      sessionId: null,
    });
    const onAccepted = vi.fn();
    const handoff = createLibraryChatHandoff({ persisted, projectAvailable: false, onAccepted });

    const result = await handoff.useSourceInChat({ kind: 'userMemoryEntry', entryId: 'm1' });

    expect(result).toBe('added');
    expect(ensureOwnedSession).toHaveBeenCalledWith('local');
    expect(startNewSession).not.toHaveBeenCalled();
    expect(addContextSource).toHaveBeenCalledTimes(1);
    expect(onAccepted).toHaveBeenCalledWith(
      { scope: 'local', sessionId: 'home-1' },
      { kind: 'userMemoryEntry', entryId: 'm1' },
    );
  });

  it('reports unavailable when the owning session cannot be resolved', async () => {
    // Nothing is attached and nothing is created: a failure here must not
    // quietly become "your memory entry is in some other chat".
    const { persisted, addContextSource, startNewSession } = fakePersisted({
      scope: 'local',
      sessionId: null,
      ensureOwnedSession: vi.fn().mockResolvedValue(null),
    });
    const handoff = createLibraryChatHandoff({
      persisted,
      projectAvailable: false,
      onAccepted: vi.fn(),
    });

    const result = await handoff.useSourceInChat({ kind: 'userMemoryEntry', entryId: 'm1' });

    expect(result).toBe('unavailable');
    expect(addContextSource).not.toHaveBeenCalled();
    expect(startNewSession).not.toHaveBeenCalled();
  });

  it('keeps project-only sources out of a local surface', async () => {
    const { persisted, ensureOwnedSession, addContextSource } = fakePersisted({
      scope: 'local',
      sessionId: 'home-1',
    });
    const handoff = createLibraryChatHandoff({
      persisted,
      projectAvailable: false,
      onAccepted: vi.fn(),
    });

    const result = await handoff.useSourceInChat({ kind: 'memoryEntry', entryId: 'p1' });

    expect(result).toBe('unavailable');
    expect(ensureOwnedSession).not.toHaveBeenCalled();
    expect(addContextSource).not.toHaveBeenCalled();
  });

  it('drops the attachment when the surface moves while the session is resolving', async () => {
    // The resolution is asynchronous. If the user selected another chat
    // meanwhile, attaching would place the source on a chat they did not ask
    // for and cannot see it in.
    const identity = { scope: 'local' as SessionScope, sessionId: null as string | null };
    const addContextSource = vi.fn().mockReturnValue('added');
    const persisted = {
      surfaceIdentity: () => ({ ...identity }),
      chat: { addContextSource },
      ensureOwnedSession: vi.fn(async () => {
        identity.sessionId = 'somewhere-else';
        return { scope: 'local' as SessionScope, sessionId: 'home-1' };
      }),
      startNewSession: vi.fn(),
      openScope: vi.fn(),
    } as unknown as PersistedChatApi;
    const handoff = createLibraryChatHandoff({
      persisted,
      projectAvailable: false,
      onAccepted: vi.fn(),
    });

    const result = await handoff.useSourceInChat({ kind: 'userMemoryEntry', entryId: 'm1' });

    expect(result).toBe('unavailable');
    expect(addContextSource).not.toHaveBeenCalled();
  });
});
