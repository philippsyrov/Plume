import type { ContextSourceRef } from '../../lib/api/chat';
import { ContextDropSurface } from '../chat/ContextDropSurface';
import { LibraryPanel } from './LibraryPanel';
import type {
  LibraryChatItem,
  LibraryUseInChatResult,
} from './libraryTypes';

export function LibraryWorkspace({
  projectIdentity,
  disabled,
  onUseInChat,
  onDropSource,
}: {
  projectIdentity: string | null;
  disabled: boolean;
  onUseInChat: (item: LibraryChatItem) => Promise<LibraryUseInChatResult>;
  onDropSource: (source: ContextSourceRef) => Promise<LibraryUseInChatResult>;
}) {
  return (
    <ContextDropSurface onDropSource={onDropSource} disabled={disabled}>
      {({ onDragActiveChange }) => (
        <div className="plume-project-knowledge-view">
          <LibraryPanel
            projectIdentity={projectIdentity}
            onUseInChat={onUseInChat}
            onContextDragActiveChange={onDragActiveChange}
          />
        </div>
      )}
    </ContextDropSurface>
  );
}
