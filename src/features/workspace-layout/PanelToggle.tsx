// D30: show/hide button for one side of the trusted-project shell.
//
// The button glyph follows the convention used by most code IDEs:
// when the panel is visible the chevron points AT the edge (the
// direction the panel will collapse to); when hidden it points
// AWAY from the edge (the direction the panel will appear from).
// Tooltips spell out the action AND the keyboard shortcut so the
// behaviour is discoverable without docs.

export type PanelToggleProps = {
  side: 'left' | 'right';
  visible: boolean;
  onToggle: () => void;
};

export function PanelToggle({ side, visible, onToggle }: PanelToggleProps) {
  const action = visible ? 'Hide' : 'Show';
  const shortcut = side === 'left' ? '⌘⇧[' : '⌘⇧]';
  const label = `${action} ${side} panel (${shortcut})`;
  // Direction logic: the chevron points toward where the panel
  // either is (when visible — "click to collapse it that way")
  // or will appear from (when hidden — "click to expand it from
  // there").
  const glyph =
    side === 'left'
      ? visible
        ? '‹'
        : '›'
      : visible
        ? '›'
        : '‹';
  return (
    <button
      type="button"
      className={`ink-button plume-panel-toggle plume-panel-toggle-${side}${
        visible ? '' : ' plume-panel-toggle-hidden'
      }`}
      onClick={onToggle}
      aria-label={label}
      title={label}
      // `aria-pressed` follows the "is this control currently
      // toggled" semantic — pressed means the panel is hidden.
      aria-pressed={!visible}
    >
      {glyph}
    </button>
  );
}
