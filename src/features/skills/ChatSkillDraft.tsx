import { useRef, useState } from 'react';

import { loadSkillPromotionContext, previewSkillPromotion } from '../../lib/api/skills';
import {
  listSessions,
  type SessionSummary,
} from '../../lib/api/sessions';

const MAX_SELECTED_ENTRIES = 20;

type Promotion = Awaited<ReturnType<typeof previewSkillPromotion>>;
type PromotionContext = Awaited<ReturnType<typeof loadSkillPromotionContext>>;

type Props = {
  disabled: boolean;
  onDraft: (promotion: Promotion) => void;
  onBusyChange: (busy: boolean) => void;
  onPromotionStart: () => void;
};

export function ChatSkillDraft({ disabled, onDraft, onBusyChange, onPromotionStart }: Props) {
  const [open, setOpen] = useState(false);
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [context, setContext] = useState<PromotionContext | null>(null);
  const [sessionId, setSessionId] = useState('');
  const [selected, setSelected] = useState<number[]>([]);
  const [busy, setBusy] = useState<'list' | 'load' | 'promote' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const listGeneration = useRef(0);
  const loadGeneration = useRef(0);
  const promotionGeneration = useRef(0);

  const toggle = async () => {
    if (open) {
      setOpen(false);
      setContext(null);
      setSessionId('');
      setSelected([]);
      invalidateRequests();
      return;
    }
    setOpen(true);
    if (sessions) return;
    const request = ++listGeneration.current;
    setBusy('list');
    setError(null);
    try {
      const response = await listSessions({ scope: 'project', includeArchived: false });
      if (listGeneration.current === request) setSessions(response.sessions);
    } catch (cause) {
      if (listGeneration.current === request) setError(errorMessage(cause));
    } finally {
      if (listGeneration.current === request) setBusy(null);
    }
  };

  const selectSession = async (nextId: string) => {
    const request = ++loadGeneration.current;
    promotionGeneration.current += 1;
    setSessionId(nextId);
    setContext(null);
    setSelected([]);
    setError(null);
    if (!nextId) {
      setBusy(null);
      return;
    }
    setBusy('load');
    try {
      const response = await loadSkillPromotionContext(nextId);
      if (loadGeneration.current === request) setContext(response);
    } catch (cause) {
      if (loadGeneration.current === request) setError(errorMessage(cause));
    } finally {
      if (loadGeneration.current === request) setBusy(null);
    }
  };

  const toggleEntry = (index: number) => {
    setSelected((current) =>
      current.includes(index)
        ? current.filter((candidate) => candidate !== index)
        : [...current, index].sort((left, right) => left - right),
    );
    setError(null);
  };

  const createDraft = async () => {
    if (!sessionId || !context || selected.length < 1 || selected.length > MAX_SELECTED_ENTRIES) return;
    const request = ++promotionGeneration.current;
    onPromotionStart();
    onBusyChange(true);
    setBusy('promote');
    setError(null);
    try {
      const promotion = await previewSkillPromotion({
        sessionId,
        snapshotToken: context.snapshotToken,
        entryIndexes: selected,
      });
      if (promotionGeneration.current !== request) return;
      onDraft(promotion);
      setOpen(false);
      setContext(null);
      setSessionId('');
      setSelected([]);
    } catch (cause) {
      if (promotionGeneration.current === request) setError(errorMessage(cause));
    } finally {
      if (promotionGeneration.current === request) {
        setBusy(null);
        onBusyChange(false);
      }
    }
  };

  const invalidateRequests = () => {
    listGeneration.current += 1;
    loadGeneration.current += 1;
    promotionGeneration.current += 1;
    onBusyChange(false);
    setBusy(null);
  };

  const controlsDisabled = disabled || busy === 'promote';

  return (
    <section className="plume-skills-chat-draft" aria-label="Start skill from project chat">
      <button type="button" className="ink-button" onClick={() => void toggle()} disabled={disabled} aria-expanded={open}>
        Start from project chat
      </button>
      {open ? (
        <div className="plume-skills-chat-picker">
          {busy === 'list' ? <p role="status">Loading project chats…</p> : null}
          {sessions ? (
            <label>
              Source project chat
              <select value={sessionId} onChange={(event) => void selectSession(event.target.value)} disabled={controlsDisabled}>
                <option value="">Choose a chat…</option>
                {sessions.map((session) => <option key={session.id} value={session.id}>{session.title}</option>)}
              </select>
            </label>
          ) : null}
          {sessions?.length === 0 ? <p className="plume-skills-muted">No active project chats.</p> : null}
          {busy === 'load' ? <p role="status">Loading transcript…</p> : null}
          {context ? (
            <fieldset>
              <legend>Choose 1–20 messages</legend>
              <div className="plume-skills-chat-entries">
                {context.entries.map((entry) => {
                  const checked = selected.includes(entry.index);
                  return (
                    <label key={entry.index} className="plume-skills-chat-entry">
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={controlsDisabled || (!checked && selected.length >= MAX_SELECTED_ENTRIES)}
                        onChange={() => toggleEntry(entry.index)}
                      />
                      <strong>{entry.role === 'user' ? 'User' : 'Assistant'}</strong>
                      <span>{shortPreview(entry.content)}</span>
                    </label>
                  );
                })}
              </div>
              {context.excludedCount > 0 ? <p>{context.excludedCount} cancelled or error {context.excludedCount === 1 ? 'entry is' : 'entries are'} excluded.</p> : null}
              <p>{selected.length} of {MAX_SELECTED_ENTRIES} selected</p>
              <button type="button" className="ink-button" disabled={selected.length === 0 || controlsDisabled} onClick={() => void createDraft()}>
                {busy === 'promote' ? 'Creating draft…' : 'Create editable draft'}
              </button>
            </fieldset>
          ) : null}
          {error ? <p role="alert">{error}</p> : null}
        </div>
      ) : null}
    </section>
  );
}

function shortPreview(content: string): string {
  const compact = content.replace(/\s+/g, ' ').trim();
  return compact.length > 140 ? `${compact.slice(0, 137)}…` : compact;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
