// D71: curated memory topic files.
//
// A read-only "Topic files" disclosure for the Memory panel. Surfaces
// the human-authored Markdown layer under `.plume/memory/` — the
// always-loaded core trio (INDEX/USER/SOUL) plus `topics/*.md`. Plume
// does not write these in D71; the user authors them in their own
// editor, and this panel makes the convention visible and inspectable.
//
// Self-contained: fetches `memory.topics` on first expand and manages
// its own load state. Trust is already established by the time the
// Memory panel renders its ready view, but a `NeedsApproval` is handled
// defensively all the same.

import { useCallback, useEffect, useRef, useState } from 'react';

import { getMemoryTopics, type MemoryTopicFile, type MemoryTopics } from '../../lib/api/memory';
import { isIpcError } from '../../lib/api/errors';
import { bumpMemoryRevision } from './memoryRevision';

type TopicsState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ready'; topics: MemoryTopics }
  | { kind: 'error'; message: string };

export function MemoryTopicsDisclosure() {
  const [expanded, setExpanded] = useState(false);
  const [state, setState] = useState<TopicsState>({ kind: 'idle' });

  // D81 (review M1): skip post-await state writes if the disclosure
  // unmounted while a topics read was in flight.
  const mountedRef = useRef(true);
  useEffect(
    () => () => {
      mountedRef.current = false;
    },
    [],
  );

  const fetchTopics = useCallback(async () => {
    setState({ kind: 'loading' });
    try {
      const topics = await getMemoryTopics();
      if (!mountedRef.current) return;
      setState({ kind: 'ready', topics });
      bumpMemoryRevision();
    } catch (err: unknown) {
      if (!mountedRef.current) return;
      const message =
        isIpcError(err) && err.kind === 'NeedsApproval'
          ? 'Trust the project to read topic files.'
          : err instanceof Error
            ? err.message
            : String(err);
      setState({ kind: 'error', message });
    }
  }, []);

  const onToggle = useCallback(() => {
    const next = !expanded;
    setExpanded(next);
    if (next && state.kind === 'idle') {
      void fetchTopics();
    }
  }, [expanded, state.kind, fetchTopics]);

  return (
    <div className="plume-memory-topics">
      <button
        type="button"
        className="plume-memory-distill-toggle"
        onClick={onToggle}
        aria-expanded={expanded}
      >
        <span className="plume-local-models-caret" aria-hidden="true">
          {expanded ? '▾' : '▸'}
        </span>
        Topic files
      </button>
      {expanded ? <MemoryTopicsBody state={state} onRefresh={() => void fetchTopics()} /> : null}
    </div>
  );
}

function MemoryTopicsBody({
  state,
  onRefresh,
}: {
  state: TopicsState;
  onRefresh: () => void;
}) {
  if (state.kind === 'loading' || state.kind === 'idle') {
    return (
      <p className="plume-memory-hint" role="status">
        Reading topic files…
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
  const { topics } = state;
  const anyCore = topics.core.some((file) => file.exists);
  return (
    <div>
      <p className="plume-memory-hint">
        Curated Markdown under <code>.plume/memory/</code>. Authored in your editor; Plume reads
        them.
      </p>
      <ul className="plume-memory-topics-list" role="list">
        {topics.core.map((file) => (
          <TopicFileRow key={file.name} file={file} />
        ))}
      </ul>
      {topics.topics.length > 0 && (
        <>
          <p className="plume-memory-hint">topics/</p>
          <ul className="plume-memory-topics-list" role="list">
            {topics.topics.map((file) => (
              <TopicFileRow key={file.name} file={file} />
            ))}
          </ul>
        </>
      )}
      {topics.topicsTruncated && (
        <p className="plume-memory-hint">
          More than {topics.limits.maxTopics} topic files — only the first are shown.
        </p>
      )}
      {!anyCore && topics.topics.length === 0 && (
        <p className="plume-memory-hint">
          None created yet. Add <code>INDEX.md</code>, <code>USER.md</code>, or{' '}
          <code>SOUL.md</code> under <code>.plume/memory/</code> to give the agent durable,
          inspectable context.
        </p>
      )}
      <button type="button" className="plume-memory-distill-refresh" onClick={onRefresh}>
        Refresh
      </button>
    </div>
  );
}

/** One file row: a header line (name + size / "not created"), and —
 *  when the file exists — a toggle that reveals its capped content. */
function TopicFileRow({ file }: { file: MemoryTopicFile }) {
  const [open, setOpen] = useState(false);
  const label = coreLabel(file);
  if (!file.exists) {
    return (
      <li className="plume-memory-topics-row">
        <span className="plume-memory-topics-name">{label}</span>
        <span className="plume-memory-hint"> — not created</span>
      </li>
    );
  }
  return (
    <li className="plume-memory-topics-row">
      <button
        type="button"
        className="plume-memory-topics-head"
        onClick={() => setOpen((prev) => !prev)}
        aria-expanded={open}
      >
        <span className="plume-local-models-caret" aria-hidden="true">
          {open ? '▾' : '▸'}
        </span>
        <span className="plume-memory-topics-name">{label}</span>
        <span className="plume-memory-hint">
          {' '}
          · {formatBytes(file.bytes)}
          {file.truncated ? ' · preview' : ''}
        </span>
      </button>
      {open && <pre className="plume-memory-topics-content">{file.content}</pre>}
    </li>
  );
}

/** Friendly label for the core trio; topic files show their relative
 *  name (e.g. `topics/architecture.md`). */
function coreLabel(file: MemoryTopicFile): string {
  switch (file.kind) {
    case 'index':
      return 'INDEX.md';
    case 'user':
      return 'USER.md';
    case 'soul':
      return 'SOUL.md';
    case 'topic':
      return file.name;
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KiB`;
}
