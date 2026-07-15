import type { ContextSourceRef } from '../../lib/api/chat';
import type { SessionIdentity, SessionScope } from '../../lib/api/sessions';
import type { PersistedChatApi } from '../sessions/usePersistedChat';
import type {
  LibraryChatItem,
  LibraryUseInChatResult,
} from './libraryTypes';

export function createLibraryChatHandoff({
  persisted,
  projectAvailable,
  onAccepted,
}: {
  persisted: PersistedChatApi;
  projectAvailable: boolean;
  onAccepted: (owner: SessionIdentity, source: ContextSourceRef) => void;
}) {
  const useSourceInChat = async (
    source: ContextSourceRef,
  ): Promise<LibraryUseInChatResult> => {
    const scope = sourceScope(source, persisted.surfaceIdentity().scope, projectAvailable);
    if (scope === null) return 'unavailable';
    const owner = await ensureOwner(persisted, scope);
    if (owner === null || !sameIdentity(persisted.surfaceIdentity(), owner)) {
      return 'unavailable';
    }
    const result = persisted.chat.addContextSource(source);
    if (!sameIdentity(persisted.surfaceIdentity(), owner)) return 'unavailable';
    if (result === 'added' || result === 'duplicate') onAccepted(owner, source);
    return result;
  };

  return {
    useItemInChat: (item: LibraryChatItem) => useSourceInChat(libraryContextSource(item)),
    useSourceInChat,
  };
}

function libraryContextSource(item: LibraryChatItem): ContextSourceRef {
  if (item.kind === 'userMemory') {
    return { kind: 'userMemoryEntry', entryId: item.entryId };
  }
  if (item.kind === 'projectMemory') {
    return { kind: 'memoryEntry', entryId: item.entryId };
  }
  return { kind: 'topicFile', name: item.name };
}

function sourceScope(
  source: ContextSourceRef,
  activeScope: SessionScope,
  projectAvailable: boolean,
): SessionScope | null {
  if (source.kind === 'userMemoryEntry') return activeScope;
  if (source.kind === 'memoryEntry' || source.kind === 'topicFile') {
    return projectAvailable ? 'project' : null;
  }
  return null;
}

async function ensureOwner(
  persisted: PersistedChatApi,
  scope: SessionScope,
): Promise<SessionIdentity | null> {
  let identity = persisted.surfaceIdentity();
  if (identity.scope !== scope) {
    const opened = await persisted.openScope(scope);
    if (!opened) return null;
    identity = persisted.surfaceIdentity();
  }
  if (identity.scope !== scope) return null;
  if (identity.sessionId === null) {
    const created = await persisted.startNewSession(scope);
    if (!created) return null;
    identity = persisted.surfaceIdentity();
  }
  return identity.scope === scope && identity.sessionId !== null
    ? { scope, sessionId: identity.sessionId }
    : null;
}

function sameIdentity(
  current: { scope: SessionScope; sessionId: string | null },
  expected: SessionIdentity,
): boolean {
  return current.scope === expected.scope && current.sessionId === expected.sessionId;
}
