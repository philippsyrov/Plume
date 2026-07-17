import type { ChatMode } from '../../lib/api/chat';
import { Icon } from '../project-shell/Icon';

export function ModeToggle({
  mode,
  onChange,
  disabled,
}: {
  mode: ChatMode;
  onChange: (next: ChatMode) => void;
  disabled: boolean;
}) {
  const makingChanges = mode === 'proposeDiff';

  return (
    <div className="plume-chat-action-selector">
      <button
        type="button"
        className="ink-button plume-chat-change-mode"
        onClick={() => onChange(makingChanges ? 'chat' : 'proposeDiff')}
        disabled={disabled}
        aria-pressed={makingChanges}
        aria-label="Make changes"
      >
        <Icon name="files" size={14} />
        {makingChanges ? 'Making changes' : 'Make changes'}
      </button>
      {makingChanges ? (
        <span className="plume-chat-action-description" role="status">
          Plume will draft a file change. You still choose whether to apply it.
        </span>
      ) : null}
    </div>
  );
}
