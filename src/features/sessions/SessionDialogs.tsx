// D63B: Plume-styled session dialogs — rename, delete confirmation,
// and the archived-chats modal. No `window.prompt` / `window.confirm`
// (the D62 placeholders used `window.prompt`; this replaces them per
// the design spec). Markup reuses the D62 settings-modal classes so
// the visual system stays untouched.
//
// `useSessionDialogs` owns which dialog is open so the App shell only
// renders `dialogs.node` and forwards row callbacks — session logic
// stays in this feature folder, not in App.tsx.

import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type JSX,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react';

import type { SessionScope, SessionSummary } from '../../lib/api/sessions';
import type { PersistedChatApi } from './usePersistedChat';
import type { SessionsApi } from './useSessions';

type DialogState =
  | { kind: 'closed' }
  | { kind: 'rename'; scope: SessionScope; session: SessionSummary }
  | { kind: 'delete'; scope: SessionScope; session: SessionSummary }
  | { kind: 'rewind'; scope: SessionScope; session: SessionSummary }
  | { kind: 'archived'; scope: SessionScope };

export type SessionDialogsApi = {
  /** Render this once near the end of the shell. */
  node: JSX.Element | null;
  openRename: (scope: SessionScope, session: SessionSummary) => void;
  openDelete: (scope: SessionScope, session: SessionSummary) => void;
  openRewind: (scope: SessionScope, session: SessionSummary) => void;
  openArchived: (scope: SessionScope) => void;
};

export function useSessionDialogs({
  sessions,
  persisted,
  onChatCreated,
}: {
  sessions: SessionsApi;
  persisted: PersistedChatApi;
  onChatCreated?: (scope: SessionScope) => void;
}): SessionDialogsApi {
  const [state, setState] = useState<DialogState>({ kind: 'closed' });
  const close = () => setState({ kind: 'closed' });

  let node: JSX.Element | null = null;
  if (state.kind === 'rename') {
    node = (
      <RenameSessionDialog
        session={state.session}
        onSubmit={(title) => sessions.rename(state.scope, state.session.id, title)}
        onClose={close}
      />
    );
  } else if (state.kind === 'delete') {
    node = (
      <DeleteSessionDialog
        session={state.session}
        streamingActive={
          persisted.chat.status === 'streaming' &&
          persisted.activeSessionId === state.session.id
        }
        onConfirm={async () => {
          const result = await sessions.remove(state.scope, state.session.id);
          if (result.ok) persisted.handleDeleted(state.scope, state.session.id);
          return result;
        }}
        onClose={close}
      />
    );
  } else if (state.kind === 'rewind') {
    node = (
      <RewindSessionDialog
        session={state.session}
        onSubmit={async (turnCount) => {
          const ok = await persisted.rewindInNewChat(
            state.scope,
            state.session.id,
            turnCount,
          );
          if (ok) onChatCreated?.(state.scope);
          return ok;
        }}
        onClose={close}
      />
    );
  } else if (state.kind === 'archived') {
    node = (
      <ArchivedSessionsModal
        scope={state.scope}
        sessions={sessions}
        persisted={persisted}
        onClose={close}
      />
    );
  }

  return {
    node,
    openRename: (scope, session) => setState({ kind: 'rename', scope, session }),
    openDelete: (scope, session) => setState({ kind: 'delete', scope, session }),
    openRewind: (scope, session) => setState({ kind: 'rewind', scope, session }),
    openArchived: (scope) => setState({ kind: 'archived', scope }),
  };
}

function RewindSessionDialog({
  session,
  onSubmit,
  onClose,
}: {
  session: SessionSummary;
  onSubmit: (turnCount: number) => Promise<boolean>;
  onClose: () => void;
}) {
  const [value, setValue] = useState('1');
  const [busy, setBusy] = useState(false);
  const turnCount = Number(value);
  const valid = Number.isInteger(turnCount) && turnCount >= 1 && turnCount <= 20;

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!valid || busy) return;
    setBusy(true);
    const ok = await onSubmit(turnCount);
    setBusy(false);
    if (ok) onClose();
  };

  return (
    <SessionDialogFrame titleId="plume-session-rewind-title" onClose={onClose}>
      <header className="plume-project-settings-header">
        <div>
          <h3 id="plume-session-rewind-title">Rewind into new chat</h3>
          <p>
            Creates a new chat ending before the selected recent turns. The original
            stays unchanged. Source: “{session.title}”.
          </p>
        </div>
      </header>
      <form className="plume-session-dialog-form" onSubmit={submit}>
        <label className="plume-open-form-label">
          User turns to omit
          <input
            autoFocus
            type="number"
            min={1}
            max={20}
            step={1}
            className="plume-open-form-input"
            value={value}
            onChange={(event) => setValue(event.target.value)}
            aria-invalid={!valid}
          />
        </label>
        <div className="plume-session-dialog-actions">
          <button type="button" className="ink-button" onClick={onClose} disabled={busy}>Cancel</button>
          <button type="submit" className="ink-button" disabled={!valid || busy}>
            {busy ? 'Rewinding…' : 'Rewind'}
          </button>
        </div>
      </form>
    </SessionDialogFrame>
  );
}

type MutationResult = { ok: true } | { ok: false; message: string };

function RenameSessionDialog({
  session,
  onSubmit,
  onClose,
}: {
  session: SessionSummary;
  onSubmit: (title: string) => Promise<MutationResult>;
  onClose: () => void;
}) {
  const [title, setTitle] = useState(session.title);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const trimmed = title.trim();

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (trimmed.length === 0 || busy) return;
    setBusy(true);
    const result = await onSubmit(trimmed);
    setBusy(false);
    if (result.ok) {
      onClose();
    } else {
      // Failed rename: the row keeps its old title (database-first)
      // and the dialog stays open with the reason.
      setError(result.message);
    }
  };

  return (
    <SessionDialogFrame titleId="plume-session-rename-title" onClose={onClose}>
      <header className="plume-project-settings-header">
        <div>
          <h3 id="plume-session-rename-title">Rename chat</h3>
          <p>Titles are trimmed and capped at 120 characters.</p>
        </div>
        <button
          type="button"
          className="ink-button plume-project-settings-close"
          onClick={onClose}
          aria-label="Close rename chat"
        >
          Close
        </button>
      </header>
      <form className="plume-session-dialog-form" onSubmit={submit}>
        <label className="plume-open-form-label">
          Chat title
          <input
            type="text"
            className="plume-open-form-input"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            spellCheck={false}
          />
        </label>
        {error !== null ? (
          <p className="plume-session-dialog-error" role="alert">
            {error}
          </p>
        ) : null}
        <div className="plume-session-dialog-actions">
          <button type="submit" className="ink-button" disabled={trimmed.length === 0 || busy}>
            {busy ? 'Saving…' : 'Save'}
          </button>
        </div>
      </form>
    </SessionDialogFrame>
  );
}

function DeleteSessionDialog({
  session,
  streamingActive,
  onConfirm,
  onClose,
}: {
  session: SessionSummary;
  streamingActive: boolean;
  onConfirm: () => Promise<MutationResult>;
  onClose: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const confirm = async () => {
    if (busy) return;
    setBusy(true);
    const result = await onConfirm();
    setBusy(false);
    if (result.ok) {
      onClose();
    } else {
      setError(result.message);
    }
  };

  return (
    <SessionDialogFrame titleId="plume-session-delete-title" onClose={onClose}>
      <header className="plume-project-settings-header">
        <div>
          <h3 id="plume-session-delete-title">Delete chat?</h3>
          <p>
            “{session.title}” and its transcript will be deleted permanently. This cannot
            be undone.
          </p>
        </div>
        <button
          type="button"
          className="ink-button plume-project-settings-close"
          onClick={onClose}
          aria-label="Close delete chat"
        >
          Close
        </button>
      </header>
      {streamingActive ? (
        <p className="plume-session-dialog-error" role="alert">
          This chat is still streaming a reply. Stop it or let it finish before deleting.
        </p>
      ) : null}
      {error !== null ? (
        <p className="plume-session-dialog-error" role="alert">
          {error}
        </p>
      ) : null}
      <div className="plume-session-dialog-actions">
        <button type="button" className="ink-button" onClick={onClose} disabled={busy}>
          Cancel
        </button>
        <button
          type="button"
          className="ink-button plume-session-dialog-danger"
          onClick={() => void confirm()}
          disabled={busy || streamingActive}
          aria-label={`Delete chat ${session.title} permanently`}
        >
          {busy ? 'Deleting…' : 'Delete permanently'}
        </button>
      </div>
    </SessionDialogFrame>
  );
}

function ArchivedSessionsModal({
  scope,
  sessions,
  persisted,
  onClose,
}: {
  scope: SessionScope;
  sessions: SessionsApi;
  persisted: PersistedChatApi;
  onClose: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  // Two-step inline delete: first click arms the row, second confirms.
  const [armedDeleteId, setArmedDeleteId] = useState<string | null>(null);
  const archived = sessions.archivedOf(scope);
  const scopeLabel = scope === 'local' ? 'Chats' : 'Project chats';
  // Same protection as the normal delete dialog (Codex P2 on #108):
  // an archived chat can still be the one actively streaming (archive
  // never unloads the surface), and deleting it mid-stream would pull
  // the session out from under a live reply.
  const streamingActiveId =
    persisted.chat.status === 'streaming' && persisted.activeScope === scope
      ? persisted.activeSessionId
      : null;

  const run = async (result: Promise<{ ok: true } | { ok: false; message: string }>) => {
    const outcome = await result;
    setError(outcome.ok ? null : outcome.message);
    return outcome.ok;
  };

  const armDelete = (sessionId: string) => {
    if (sessionId === streamingActiveId) {
      setError(
        'This chat is still streaming a reply. Stop it or let it finish before deleting.',
      );
      return;
    }
    setError(null);
    setArmedDeleteId(sessionId);
  };

  const confirmDelete = (sessionId: string) => {
    if (sessionId === streamingActiveId) {
      setError(
        'This chat is still streaming a reply. Stop it or let it finish before deleting.',
      );
      setArmedDeleteId(null);
      return;
    }
    void run(sessions.remove(scope, sessionId)).then((ok) => {
      if (ok) persisted.handleDeleted(scope, sessionId);
      setArmedDeleteId(null);
    });
  };

  return (
    <SessionDialogFrame titleId="plume-session-archived-title" onClose={onClose}>
      <header className="plume-project-settings-header">
        <div>
          <h3 id="plume-session-archived-title">Archived chats — {scopeLabel}</h3>
          <p>Archived chats are hidden from the sidebar but fully kept.</p>
        </div>
        <button
          type="button"
          className="ink-button plume-project-settings-close"
          onClick={onClose}
          aria-label="Close archived chats"
        >
          Close
        </button>
      </header>
      {error !== null ? (
        <p className="plume-session-dialog-error" role="alert">
          {error}
        </p>
      ) : null}
      {archived.length === 0 ? (
        <p className="plume-session-dialog-empty" role="status">
          No archived chats.
        </p>
      ) : (
        <ul className="plume-session-archived-list">
          {archived.map((session) => (
            <li key={session.id} className="plume-session-archived-row">
              <span className="plume-session-archived-title">{session.title}</span>
              {armedDeleteId === session.id ? (
                <>
                  <button
                    type="button"
                    className="ink-button plume-session-dialog-danger"
                    onClick={() => confirmDelete(session.id)}
                    aria-label={`Confirm permanent delete of ${session.title}`}
                  >
                    Confirm delete
                  </button>
                  <button
                    type="button"
                    className="ink-button"
                    onClick={() => setArmedDeleteId(null)}
                    aria-label={`Keep ${session.title}`}
                  >
                    Keep
                  </button>
                </>
              ) : (
                <>
                  <button
                    type="button"
                    className="ink-button"
                    onClick={() => void run(sessions.setArchived(scope, session.id, false))}
                    aria-label={`Unarchive ${session.title}`}
                  >
                    Unarchive
                  </button>
                  <button
                    type="button"
                    className="ink-button plume-session-dialog-danger"
                    onClick={() => armDelete(session.id)}
                    aria-label={`Delete ${session.title}`}
                  >
                    Delete
                  </button>
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </SessionDialogFrame>
  );
}

/** Shared backdrop + compact window, matching the D62 modal system. */
function SessionDialogFrame({
  titleId,
  onClose,
  children,
}: {
  titleId: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const previousFocusRef = useRef(
    document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );
  const dialogRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog !== null && !dialog.contains(document.activeElement)) {
      focusableControls(dialog)[0]?.focus();
    }
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      onCloseRef.current();
    };
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      document.removeEventListener('keydown', closeOnEscape);
      previousFocusRef.current?.focus();
    };
  }, []);

  const containTab = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key !== 'Tab') return;
    const controls = focusableControls(event.currentTarget);
    const first = controls[0];
    const last = controls.at(-1);
    if (first === undefined || last === undefined) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className="plume-project-settings-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        ref={dialogRef}
        className="plume-project-settings-window plume-session-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onKeyDown={containTab}
      >
        {children}
      </section>
    </div>
  );
}

function focusableControls(container: HTMLElement): HTMLElement[] {
  return Array.from(
    container.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
    ),
  );
}
