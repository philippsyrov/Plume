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
    // Session ownership is resolved in exactly one place. Reproducing the
    // scope-switch-then-create sequence here is what let Library mint an
    // ordinary local chat while the Home lookup was still in flight — the
    // memory entry then sat in a chat the user had no way back to.
    const owner = await persisted.ensureOwnedSession(scope);
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

function sameIdentity(
  current: { scope: SessionScope; sessionId: string | null },
  expected: SessionIdentity,
): boolean {
  return current.scope === expected.scope && current.sessionId === expected.sessionId;
}
