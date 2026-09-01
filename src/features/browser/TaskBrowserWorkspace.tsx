import type { ContextSourceRef } from '../../lib/api/chat';
import type { SessionIdentity } from '../../lib/api/sessions';
import { ChatPanel, type ChatPanelProps } from '../chat/ChatPanel';
import type { AddContextSourceResult } from '../chat/contextSources';
import { BrowserPanel, type BrowserNavigationRequest } from './BrowserPanel';

export function TaskBrowserWorkspace({
  identity,
  onUseInChat,
  chatProps,
  suspended = false,
  onOverlaySafeChange,
  navigationRequest,
  onOpenResearchSource,
}: {
  identity: SessionIdentity;
  onUseInChat: (
    owner: SessionIdentity,
    source: ContextSourceRef,
  ) => Promise<AddContextSourceResult>;
  /** `prepareSend` is required, not optional as it is on `ChatPanel` itself.
   * The Browser layout is a persisted chat like any other, and it was the one
   * surface that silently kept the ownerless send path when the gate landed. */
  chatProps: Omit<ChatPanelProps, 'contextOwner'> &
    Required<Pick<ChatPanelProps, 'prepareSend'>>;
  suspended?: boolean;
  onOverlaySafeChange?: ((safe: boolean) => void) | undefined;
  navigationRequest?: BrowserNavigationRequest;
  onOpenResearchSource?: (url: string) => void;
}) {
  return (
    <BrowserPanel
      identity={identity}
      onUseInChat={(source) => onUseInChat(identity, source)}
      chatPane={(
        <ChatPanel
          {...chatProps}
          contextOwner={identity}
          {...(onOpenResearchSource ? { onOpenResearchSource } : {})}
        />
      )}
      suspended={suspended}
      onOverlaySafeChange={onOverlaySafeChange}
      {...(navigationRequest ? { navigationRequest } : {})}
    />
  );
}
