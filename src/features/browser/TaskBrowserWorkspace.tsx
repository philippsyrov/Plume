import type { ContextSourceRef } from '../../lib/api/chat';
import type { SessionIdentity } from '../../lib/api/sessions';
import { ChatPanel, type ChatPanelProps } from '../chat/ChatPanel';
import type { AddContextSourceResult } from '../chat/contextSources';
import { BrowserPanel } from './BrowserPanel';

export function TaskBrowserWorkspace({
  identity,
  onUseInChat,
  chatProps,
}: {
  identity: SessionIdentity;
  onUseInChat: (source: ContextSourceRef) => Promise<AddContextSourceResult>;
  chatProps: Omit<ChatPanelProps, 'contextOwner'>;
}) {
  return (
    <BrowserPanel
      identity={identity}
      onUseInChat={onUseInChat}
      chatPane={<ChatPanel {...chatProps} contextOwner={identity} />}
    />
  );
}
