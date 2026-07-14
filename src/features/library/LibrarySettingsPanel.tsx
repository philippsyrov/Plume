import { useCallback, useEffect, useRef, useState } from 'react';

import {
  forgetUserMemory,
  getUserMemoryIndex,
  rememberUserMemory,
  updateUserMemory,
  type UserMemoryEntry,
  type UserMemoryIndex,
} from '../../lib/api/memory';
import { MemoryPanel } from '../memory/MemoryPanel';
import { bumpUserMemoryRevision } from './libraryRevision';

type UserSettingsState =
  | { kind: 'loading' }
  | { kind: 'ready'; index: UserMemoryIndex }
  | { kind: 'error'; message: string };

export function LibrarySettingsPanel({ projectAvailable }: { projectAvailable: boolean }) {
  return (
    <section className="plume-library-settings" aria-label="Library settings">
      <UserMemorySettings />
      <section className="plume-library-settings-scope" aria-labelledby="plume-project-library-title">
        <header>
          <h3 id="plume-project-library-title">This project</h3>
          <p>Facts and topic links stored only inside the trusted project.</p>
        </header>
        {projectAvailable
          ? <MemoryPanel />
          : <p>Open and trust a project to manage its memory.</p>}
      </section>
    </section>
  );
}

function UserMemorySettings() {
  const mounted = useRef(true);
  const request = useRef(0);
  const [state, setState] = useState<UserSettingsState>({ kind: 'loading' });
  const [draft, setDraft] = useState('');
  const [editing, setEditing] = useState<{ entry: UserMemoryEntry; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    const generation = ++request.current;
    setState({ kind: 'loading' });
    void getUserMemoryIndex().then(
      (index) => {
        if (mounted.current && generation === request.current) {
          setState({ kind: 'ready', index });
        }
      },
      (reason: unknown) => {
        if (mounted.current && generation === request.current) {
          setState({ kind: 'error', message: errorMessage(reason) });
        }
      },
    );
  }, []);

  useEffect(() => {
    mounted.current = true;
    load();
    return () => {
      mounted.current = false;
      request.current += 1;
    };
  }, [load]);

  const updateReadyIndex = (update: (index: UserMemoryIndex) => UserMemoryIndex) => {
    setState((current) => current.kind === 'ready'
      ? { kind: 'ready', index: update(current.index) }
      : current);
    bumpUserMemoryRevision();
  };

  const remember = async () => {
    const text = draft.trim();
    if (text === '' || busy) return;
    setBusy(true);
    setError(null);
    try {
      const response = await rememberUserMemory(text);
      if (!mounted.current) return;
      if (!response.ok) {
        setError(response.message);
        return;
      }
      setDraft('');
      updateReadyIndex((index) => ({
        ...index,
        entries: [response.entry, ...index.entries],
      }));
    } catch (reason) {
      if (mounted.current) setError(errorMessage(reason));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };

  const save = async () => {
    if (editing === null || editing.text.trim() === '' || busy) return;
    setBusy(true);
    setError(null);
    try {
      const response = await updateUserMemory(editing.entry.id, editing.text.trim());
      if (!mounted.current) return;
      if (!response.ok) {
        setError(response.message);
        return;
      }
      updateReadyIndex((index) => ({
        ...index,
        entries: index.entries.map((entry) => entry.id === response.entry.id
          ? response.entry
          : entry),
      }));
      setEditing(null);
    } catch (reason) {
      if (mounted.current) setError(errorMessage(reason));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };

  const forget = async (entry: UserMemoryEntry) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const response = await forgetUserMemory(entry.id);
      if (!mounted.current) return;
      if (!response.ok) {
        setError(response.message);
        return;
      }
      updateReadyIndex((index) => ({
        ...index,
        entries: index.entries.filter((candidate) => candidate.id !== entry.id),
      }));
    } catch (reason) {
      if (mounted.current) setError(errorMessage(reason));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };

  return (
    <section className="plume-library-settings-scope" aria-labelledby="plume-user-library-title">
      <header>
        <h3 id="plume-user-library-title">About you</h3>
        <p>Stored on this Mac, separate from every project's memory.</p>
      </header>
      <UserMemorySettingsBody
        state={state}
        draft={draft}
        editing={editing}
        busy={busy}
        error={error}
        onDraftChange={setDraft}
        onEditingChange={setEditing}
        onRemember={() => void remember()}
        onSave={() => void save()}
        onForget={(entry) => void forget(entry)}
        onRetry={load}
      />
    </section>
  );
}

function UserMemorySettingsBody({
  state,
  draft,
  editing,
  busy,
  error,
  onDraftChange,
  onEditingChange,
  onRemember,
  onSave,
  onForget,
  onRetry,
}: {
  state: UserSettingsState;
  draft: string;
  editing: { entry: UserMemoryEntry; text: string } | null;
  busy: boolean;
  error: string | null;
  onDraftChange: (text: string) => void;
  onEditingChange: (value: { entry: UserMemoryEntry; text: string } | null) => void;
  onRemember: () => void;
  onSave: () => void;
  onForget: (entry: UserMemoryEntry) => void;
  onRetry: () => void;
}) {
  if (state.kind === 'loading') return <p role="status">Loading About you…</p>;
  if (state.kind === 'error') {
    return (
      <div>
        <p role="alert">{state.message}</p>
        <button type="button" onClick={onRetry}>Retry About you</button>
      </div>
    );
  }
  const atCapacity = state.index.entries.length >= state.index.limits.maxEntries;
  return (
    <>
      <div className="plume-library-settings-form">
        <label>
          Add something about you
          <textarea
            aria-label="Add something about you"
            value={draft}
            maxLength={state.index.limits.maxBytesPerEntry}
            disabled={busy || atCapacity}
            onChange={(event) => onDraftChange(event.currentTarget.value)}
          />
        </label>
        <button
          type="button"
          disabled={busy || atCapacity || draft.trim() === ''}
          onClick={onRemember}
        >
          Remember about you
        </button>
      </div>
      {error ? <p role="alert">{error}</p> : null}
      <p>{state.index.entries.length} of {state.index.limits.maxEntries} saved</p>
      {state.index.entries.length === 0 ? <p>Nothing saved about you yet.</p> : (
        <ul className="plume-library-settings-list">
          {state.index.entries.map((entry) => (
            <li key={entry.id}>
              {editing?.entry.id === entry.id ? (
                <>
                  <textarea
                    aria-label="Edit About you memory"
                    value={editing.text}
                    disabled={busy}
                    onChange={(event) => onEditingChange({
                      entry: editing.entry,
                      text: event.currentTarget.value,
                    })}
                  />
                  <button
                    type="button"
                    disabled={busy || editing.text.trim() === ''}
                    onClick={onSave}
                  >
                    Save About you memory
                  </button>
                  <button type="button" disabled={busy} onClick={() => onEditingChange(null)}>
                    Cancel
                  </button>
                </>
              ) : (
                <>
                  <span>{entry.text}</span>
                  {entry.redactionCount > 0 ? <span>{entry.redactionCount} redacted</span> : null}
                  <button
                    type="button"
                    aria-label={`Edit ${entry.text}`}
                    disabled={busy}
                    onClick={() => onEditingChange({ entry, text: entry.text })}
                  >
                    Edit
                  </button>
                  <button
                    type="button"
                    aria-label={`Remove ${entry.text}`}
                    disabled={busy}
                    onClick={() => onForget(entry)}
                  >
                    Remove
                  </button>
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}
