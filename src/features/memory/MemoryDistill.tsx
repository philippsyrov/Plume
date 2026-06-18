// D54 / D64 / D66: distillation UI for the Memory panel.
//
// The "Find duplicates" disclosure that hangs below the memory list.
// Split out of MemoryPanel.tsx (D67) to keep both files under the
// decomposition cap. The fetch/apply state lives in MemoryPanel and is
// passed down as props; this file is purely presentational.
//
//   * D54 — read-only preview of exact-duplicate groups.
//   * D64 — Compact button that rewrites the JSONL store.
//   * D66 — per-group checkboxes + Select/Clear-all so the user
//     compacts a chosen subset.

import { useCallback, useEffect, useState } from 'react';

import type {
  MemoryDistillApplyFailure,
  MemoryDistillPreview,
  MemoryDuplicateGroup,
} from '../../lib/api/memory';

/** D54: distill-preview affordance. `idle` = button hidden body;
 *  `loading` = waiting on `memory.distillPreview`; `ready` = result
 *  displayed inline; `error` = surface the failure under the toggle. */
export type DistillState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ready'; preview: MemoryDistillPreview }
  | { kind: 'error'; message: string };

export function distillApplyFailureLabel(reason: MemoryDistillApplyFailure): string {
  switch (reason) {
    case 'storeFailed':
      return 'Storage error';
  }
}

/**
 * D54: "Find duplicates" affordance. The toggle is always available
 * when the memory store has ≥1 entry; clicking opens a disclosure that
 * fetches `memory.distillPreview` and renders the candidate groups
 * inline. D64/D66 added the affirmative Compact action (per-group
 * selection).
 *
 * The disclosure is a peer of the search results, not a child of an
 * individual row, because duplication is a property of the whole
 * store. Refresh re-runs the verb against the current on-disk state
 * so the user can preview after remembering / forgetting without
 * collapsing and re-expanding.
 */
export function DistillPreviewDisclosure({
  expanded,
  state,
  applyBusy,
  notice,
  onToggle,
  onRefresh,
  onApply,
}: {
  expanded: boolean;
  state: DistillState;
  applyBusy: boolean;
  notice: string | null;
  onToggle: () => void;
  onRefresh: () => void;
  onApply: (groupIds: string[]) => void;
}) {
  return (
    <div className="plume-memory-distill">
      <button
        type="button"
        className="plume-memory-distill-toggle"
        onClick={onToggle}
        aria-expanded={expanded}
      >
        <span className="plume-local-models-caret" aria-hidden="true">
          {expanded ? '▾' : '▸'}
        </span>
        Find duplicates
      </button>
      {expanded ? (
        <DistillPreviewBody
          state={state}
          applyBusy={applyBusy}
          notice={notice}
          onRefresh={onRefresh}
          onApply={onApply}
        />
      ) : null}
    </div>
  );
}

function DistillPreviewBody({
  state,
  applyBusy,
  notice,
  onRefresh,
  onApply,
}: {
  state: DistillState;
  applyBusy: boolean;
  notice: string | null;
  onRefresh: () => void;
  onApply: (groupIds: string[]) => void;
}) {
  if (state.kind === 'loading' || state.kind === 'idle') {
    return (
      <p className="plume-memory-hint" role="status">
        Scanning entries…
      </p>
    );
  }
  if (state.kind === 'error') {
    return (
      <div>
        <p className="plume-memory-error" role="alert">
          {state.message}
        </p>
        <button type="button" className="plume-memory-distill-refresh" onClick={onRefresh}>
          Retry
        </button>
      </div>
    );
  }
  const { preview } = state;
  if (preview.duplicateGroups.length === 0) {
    return (
      <div>
        <p className="plume-memory-hint">
          No duplicates found among {preview.totalEntries}{' '}
          {preview.totalEntries === 1 ? 'entry' : 'entries'}.
        </p>
        {notice !== null && (
          <p className="plume-memory-hint" role="status">
            {notice}
          </p>
        )}
        <button type="button" className="plume-memory-distill-refresh" onClick={onRefresh}>
          Refresh
        </button>
      </div>
    );
  }
  // D66: per-group selection. The preview list is the confirmation
  // surface — each row shows the surviving (newest) text and a checkbox
  // — and the Compact button applies only the checked groups. Hard
  // delete; no undo in v1 (the JSONL is hand-editable).
  return (
    <div>
      <p className="plume-memory-hint">
        {preview.duplicateGroups.length}{' '}
        {preview.duplicateGroups.length === 1 ? 'duplicate group' : 'duplicate groups'} ·{' '}
        {preview.wouldRemove} {preview.wouldRemove === 1 ? 'duplicate' : 'duplicates'} removable
      </p>
      {notice !== null && (
        <p className="plume-memory-hint" role="status">
          {notice}
        </p>
      )}
      <DistillGroupSelector
        groups={preview.duplicateGroups}
        applyBusy={applyBusy}
        onApply={onApply}
        onRefresh={onRefresh}
      />
    </div>
  );
}

/**
 * D66: selectable duplicate-group list. Each group defaults to checked;
 * the Compact button passes only the checked group ids to the backend
 * (which already compacts a subset — D64). Selection re-initialises to
 * "all checked" whenever the underlying group set changes (a Refresh,
 * or the reshaped groups after a prior apply), keyed on the joined
 * group-id signature.
 */
function DistillGroupSelector({
  groups,
  applyBusy,
  onApply,
  onRefresh,
}: {
  groups: MemoryDuplicateGroup[];
  applyBusy: boolean;
  onApply: (groupIds: string[]) => void;
  onRefresh: () => void;
}) {
  const idSignature = groups.map((group) => group.id).join('\n');
  const [selected, setSelected] = useState<Set<string>>(() => new Set(idSignature.split('\n')));
  useEffect(() => {
    setSelected(new Set(idSignature.split('\n')));
  }, [idSignature]);

  const toggle = useCallback((id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const selectedIds = groups.map((group) => group.id).filter((id) => selected.has(id));
  const selectedRemovable = groups
    .filter((group) => selected.has(group.id))
    .reduce((sum, group) => sum + group.removableCount, 0);
  const allSelected = selectedIds.length === groups.length;

  return (
    <>
      <div className="plume-memory-distill-select-all">
        <button
          type="button"
          className="plume-memory-distill-refresh"
          onClick={() =>
            setSelected(allSelected ? new Set() : new Set(groups.map((group) => group.id)))
          }
          disabled={applyBusy}
        >
          {allSelected ? 'Clear all' : 'Select all'}
        </button>
      </div>
      <ul className="plume-memory-distill-groups" role="list">
        {groups.map((group) => (
          <DistillGroupRow
            key={group.id}
            group={group}
            checked={selected.has(group.id)}
            disabled={applyBusy}
            onToggle={() => toggle(group.id)}
          />
        ))}
      </ul>
      <div className="plume-memory-distill-actions">
        <button
          type="button"
          className="plume-memory-distill-apply"
          onClick={() => onApply(selectedIds)}
          disabled={applyBusy || selectedRemovable === 0}
          title="Remove the duplicates in the checked groups, keeping the newest of each"
        >
          {applyBusy
            ? 'Compacting…'
            : selectedRemovable === 0
              ? 'Select groups to compact'
              : `Compact ${selectedRemovable} duplicate${selectedRemovable === 1 ? '' : 's'}`}
        </button>
        <button
          type="button"
          className="plume-memory-distill-refresh"
          onClick={onRefresh}
          disabled={applyBusy}
        >
          Refresh
        </button>
      </div>
    </>
  );
}

function DistillGroupRow({
  group,
  checked,
  disabled,
  onToggle,
}: {
  group: MemoryDuplicateGroup;
  checked: boolean;
  disabled: boolean;
  onToggle: () => void;
}) {
  const survivor = group.entries[0];
  return (
    <li className="plume-memory-distill-group">
      <label className="plume-memory-distill-group-head">
        <input
          type="checkbox"
          checked={checked}
          disabled={disabled}
          onChange={onToggle}
          aria-label="Include this duplicate group when compacting"
        />
        <span
          className="plume-memory-distill-text"
          title="Newest entry — kept when this group is compacted"
        >
          {survivor?.text ?? '(empty group)'}
        </span>
      </label>
      <p className="plume-memory-hint">
        {group.entries.length} {group.entries.length === 1 ? 'entry' : 'entries'} ·{' '}
        {group.removableCount} {group.removableCount === 1 ? 'duplicate' : 'duplicates'} would be
        removed
      </p>
    </li>
  );
}
