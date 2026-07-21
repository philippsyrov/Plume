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
  chatProps: Omit<ChatPanelProps, 'contextOwner'>;
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
