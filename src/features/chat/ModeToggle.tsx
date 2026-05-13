// D15: segmented mode toggle in the chat header. Two visible
// states today (`'chat'` and `'proposeDiff'`); the array shape
// lets future modes (`'scopedEdit'`, `'agentLoop'`) plug in
// without restructuring the component. Disabled while a stream
// is in flight — flipping mode mid-stream would be confusing
// because the in-flight turn keeps the mode it was started with.
//
// D22 extraction: lifted out of `ChatPanel.tsx`.

import type { ChatMode } from '../../lib/api/chat';

type ModeOption = {
  value: ChatMode;
  label: string;
  description: string;
};

const MODE_OPTIONS: readonly ModeOption[] = [
  {
    value: 'chat',
    label: 'Chat',
    description: 'Free-form text reply. The default Plume conversation mode.',
  },
  {
    value: 'proposeDiff',
    label: 'Propose diff',
    description:
      'Ask the model for a unified-diff preview. Plume renders the diff inline; it does NOT apply patches in this slice.',
  },
];

export function ModeToggle({
  mode,
  onChange,
  disabled,
}: {
  mode: ChatMode;
  onChange: (next: ChatMode) => void;
  disabled: boolean;
}) {
  return (
    <div
      className="plume-chat-mode-toggle"
      role="radiogroup"
      aria-label="Response mode for next send"
    >
      {MODE_OPTIONS.map((opt) => {
        const active = opt.value === mode;
        return (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={active}
            className={
              active
                ? 'plume-chat-mode-option plume-chat-mode-option-active'
                : 'plume-chat-mode-option'
            }
            disabled={disabled}
            onClick={() => onChange(opt.value)}
            title={opt.description}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}
