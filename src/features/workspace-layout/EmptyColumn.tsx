// D32: empty-state placeholder rendered inside a column when the
// column itself is visible but every inner panel inside it has
// been toggled off. The pill chip strip is still rendered above
// this — so the recovery affordance ("tap a pill to bring a
// panel back") is right there.

export type EmptyColumnProps = {
  side: 'left' | 'right';
};

export function EmptyColumn({ side }: EmptyColumnProps) {
  return (
    <div className="plume-inner-empty ink-panel" role="status">
      <p>
        No {side === 'left' ? 'navigation' : 'inspector'} panels visible.
      </p>
      <p className="plume-inner-empty-hint">
        Tap a pill above to bring one back, or hide the column
        entirely from the status strip.
      </p>
    </div>
  );
}
