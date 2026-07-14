import type { ChangeEvent } from 'react';

import type { ChatMode } from '../../lib/api/chat';
import { Icon } from '../project-shell/Icon';

export type TaskAction = 'answer' | 'proposeDiff';

const ACTION_COPY: Record<TaskAction, { label: string; description: string }> = {
  answer: {
    label: 'Answer',
    description: 'Get a direct answer from the selected model.',
  },
  proposeDiff: {
    label: 'Propose a change',
    description: 'Draft a code change for you to review before anything is applied.',
  },
};

export function ModeToggle({
  mode,
  onChange,
  disabled,
}: {
  mode: ChatMode;
  onChange: (next: ChatMode) => void;
  disabled: boolean;
}) {
  const action = actionFromMode(mode);
  const onSelect = (event: ChangeEvent<HTMLSelectElement>) => {
    onChange(modeFromAction(event.target.value as TaskAction));
  };

  return (
    <div className="plume-chat-action-selector">
      <label className="plume-chat-action-control">
        <Icon name="chat" size={14} />
        <span>Action</span>
        <select
          value={action}
          onChange={onSelect}
          disabled={disabled}
          aria-label="Action for this message"
        >
          <option value="answer">{ACTION_COPY.answer.label}</option>
          <option value="proposeDiff">{ACTION_COPY.proposeDiff.label}</option>
        </select>
      </label>
      <p className="plume-chat-action-description">{ACTION_COPY[action].description}</p>
    </div>
  );
}

function actionFromMode(mode: ChatMode): TaskAction {
  return mode === 'proposeDiff' ? 'proposeDiff' : 'answer';
}

function modeFromAction(action: TaskAction): ChatMode {
  return action === 'proposeDiff' ? 'proposeDiff' : 'chat';
}
