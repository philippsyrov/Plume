// D32: per-column inner-panel toggle strip.
//
// Renders a row of pill-shaped buttons at the top of a column,
// one per panel that lives in that column. Filled pill = panel
// visible; outlined pill = panel hidden. Click toggles. The
// chips are always rendered when the column itself is visible,
// so a user who hides every panel inside a column still has the
// affordance to bring one back without going through the
// column-level show/hide toggle.
//
// `aria-pressed` is the canonical "is this control currently on?"
// signal — pressed means VISIBLE, not pressed means HIDDEN. That
// matches what the eye sees (filled = on) and what screen readers
// announce.

export type InnerToggleItem = {
  id: string;
  label: string;
  visible: boolean;
  onToggle: () => void;
};

export type InnerToggleStripProps = {
  side: 'left' | 'right';
  items: InnerToggleItem[];
};

export function InnerToggleStrip({ side, items }: InnerToggleStripProps) {
  return (
    <div
      className={`plume-inner-toggles plume-inner-toggles-${side}`}
      role="group"
      aria-label={`${side === 'left' ? 'Left' : 'Right'} column panels`}
    >
      {items.map((item) => {
        const action = item.visible ? 'Hide' : 'Show';
        const title = `${action} ${item.label}`;
        return (
          <button
            key={item.id}
            type="button"
            className={`plume-inner-toggle${
              item.visible ? ' plume-inner-toggle-visible' : ' plume-inner-toggle-hidden'
            }`}
            onClick={item.onToggle}
            aria-pressed={item.visible}
            title={title}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
