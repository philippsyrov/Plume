import { useState } from 'react';

import type { MemoryEntry, MemoryTopics } from '../../lib/api/memory';

export function MemoryEntryRow({
  entry,
  busy,
  onForget,
  onUpdate,
  onOpenLinks,
  linksExpanded,
}: {
  entry: MemoryEntry;
  busy: boolean;
  onForget: () => void;
  onUpdate: (text: string) => Promise<boolean>;
  onOpenLinks: () => void;
  linksExpanded: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(entry.text);
  const [saving, setSaving] = useState(false);
  const save = async () => {
    if (saving || draft.trim().length === 0) return;
    setSaving(true);
    const ok = await onUpdate(draft);
    setSaving(false);
    if (ok) setEditing(false);
  };
  if (editing) {
    return (
      <li className="plume-memory-row plume-memory-row-editing">
        <textarea
          className="plume-memory-input"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          rows={2}
          disabled={saving}
          aria-label="Edit memory entry"
        />
        <div className="plume-memory-row-actions">
          <button
            type="button"
            onClick={() => void save()}
            disabled={saving || draft.trim().length === 0}
          >
            Save
          </button>
          <button
            type="button"
            onClick={() => {
              setEditing(false);
              setDraft(entry.text);
            }}
            disabled={saving}
          >
            Cancel
          </button>
        </div>
      </li>
    );
  }
  return (
    <li className="plume-memory-row">
      <div className="plume-memory-row-body">
        <span className="plume-memory-text">{entry.text}</span>
        {entry.redactionCount > 0 && (
          <span
            className="plume-memory-badge"
            title={`${entry.redactionCount} secret value${entry.redactionCount === 1 ? '' : 's'} redacted before this was stored`}
          >
            {entry.redactionCount} redacted
          </span>
        )}
      </div>
      <div className="plume-memory-row-actions">
        <button
          id={`memory-links-button-${entry.id}`}
          type="button"
          className="plume-memory-edit"
          onClick={onOpenLinks}
          disabled={busy}
          title="Organize this memory with topic links"
          aria-expanded={linksExpanded}
          aria-controls={`memory-links-editor-${entry.id}`}
        >
          Links {entry.links.length}
        </button>
        <button
          type="button"
          className="plume-memory-edit"
          onClick={() => {
            setDraft(entry.text);
            setEditing(true);
          }}
          disabled={busy}
          title="Edit this memory entry"
        >
          Edit
        </button>
        <button
          type="button"
          className="plume-memory-forget"
          onClick={onForget}
          disabled={busy}
          title="Remove this memory entry"
        >
          Forget
        </button>
      </div>
    </li>
  );
}

const MAX_MEMORY_LINKS = 5;

export type MemoryLinksEditorState = {
  entry: MemoryEntry;
  topics: MemoryTopics | null;
  selected: string[];
  loading: boolean;
  saving: boolean;
  error: string | null;
};

export function MemoryLinksEditor({
  state,
  onSelectionChange,
  onCancel,
  onSave,
}: {
  state: MemoryLinksEditorState;
  onSelectionChange: (selected: string[]) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const availableNames = new Set(state.topics?.topics.map((topic) => topic.name) ?? []);
  const missing = state.selected.filter((name) => !availableNames.has(name));
  const toggle = (name: string) => {
    if (state.selected.includes(name)) {
      onSelectionChange(state.selected.filter((selected) => selected !== name));
    } else if (state.selected.length < MAX_MEMORY_LINKS) {
      onSelectionChange([...state.selected, name]);
    }
  };
  return (
    <div
      id={`memory-links-editor-${state.entry.id}`}
      className="plume-memory-links-editor"
      role="region"
      aria-labelledby={`memory-links-button-${state.entry.id}`}
    >
      <p className="plume-memory-hint">
        Links organize memory only. Linked topic notes are not loaded into chat yet.
      </p>
      {state.loading && (
        <p className="plume-memory-hint" role="status">
          Reading topics…
        </p>
      )}
      {state.topics !== null && (
        <div className="plume-memory-links-options">
          {state.topics.topics.length === 0 && missing.length === 0 && (
            <p className="plume-memory-empty">No topic files yet.</p>
          )}
          {state.topics.topics.map((topic) => {
            const checked = state.selected.includes(topic.name);
            return (
              <label key={topic.name} className="plume-memory-links-option">
                <input
                  type="checkbox"
                  checked={checked}
                  disabled={state.saving || (!checked && state.selected.length >= MAX_MEMORY_LINKS)}
                  onChange={() => toggle(topic.name)}
                />
                <span>{topic.name}</span>
              </label>
            );
          })}
          {missing.map((name) => (
            <label key={name} className="plume-memory-links-option plume-memory-links-missing">
              <input
                type="checkbox"
                checked
                disabled={state.saving}
                onChange={() => toggle(name)}
              />
              <span>Missing topic: {name}</span>
            </label>
          ))}
        </div>
      )}
      <p className="plume-memory-hint">{state.selected.length} of {MAX_MEMORY_LINKS} links</p>
      {state.error !== null && (
        <p className="plume-memory-error" role="alert">
          {state.error}
        </p>
      )}
      <div className="plume-memory-links-actions">
        <button type="button" onClick={onCancel} disabled={state.saving}>
          Cancel
        </button>
        <button type="button" onClick={onSave} disabled={state.loading || state.saving}>
          {state.saving ? 'Saving…' : 'Save links'}
        </button>
      </div>
    </div>
  );
}
