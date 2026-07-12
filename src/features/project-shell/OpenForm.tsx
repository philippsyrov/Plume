// The open-project form from the pre-project shell. Extracted from
// App.tsx verbatim when D132 pushed it over the decomposition amber
// cap (docs/DECOMPOSITION.md § Cadence rule) — a pure move, not a
// rewrite.

import { useEffect, useRef } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { UnlistenFn } from '@tauri-apps/api/event';

type OpenFormProps = {
  path: string;
  busy: boolean;
  onOpen: (path: string) => void;
  onChange: (path: string) => void;
  /** D49: take the user to no-project chat without opening any
   *  folder. The button sits below the Open form so the project
   *  flow stays the primary affordance. */
  onChatOnly: () => void;
};

export function OpenForm({ path, busy, onOpen, onChange, onChatOnly }: OpenFormProps) {
  const trimmed = path.trim();
  const canOpen = trimmed.length > 0 && !busy;

  // Drag-and-drop a folder onto the window to populate the path
  // input. Validation lives on the backend — `project.open` will
  // reject non-directory paths with a typed error, so we don't
  // pre-flight check here. See docs/AGENT_OPERABILITY.md: this is
  // the same surface a visual agent uses (drop a folder, then click
  // Open) — no automation-only IPC bypass.
  //
  // The listener is registered once and reads `busy` through a ref so
  // we don't tear down + re-register on every parent state flip. When
  // an open is in flight, drops are ignored — otherwise dropping
  // folder B while A is opening would move the view back to idle and
  // then jump back to A when its request resolves.
  const busyRef = useRef(busy);
  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (busyRef.current) return;
        if (event.payload.type !== 'drop') return;
        const first = event.payload.paths[0];
        if (!first) return;
        onChange(first);
      })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch((err) => {
        console.error(
          'OpenForm: drag-drop listener registration failed:',
          err instanceof Error ? err.message : String(err),
        );
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [onChange]);

  return (
    <section className="plume-empty ink-panel">
      <p>
        Open a project folder to begin. Type or paste an absolute path,
        or drag a folder onto this window. The file picker dialog plugin
        lands in a later slice.
      </p>
      <form
        className="plume-open-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (canOpen) onOpen(trimmed);
        }}
      >
        <label className="plume-open-form-label">
          Project path
          <input
            type="text"
            className="plume-open-form-input"
            value={path}
            placeholder="/Users/you/code/some-project"
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            onChange={(e) => onChange(e.target.value)}
            disabled={busy}
          />
        </label>
        <button type="submit" className="ink-button" disabled={!canOpen}>
          {busy ? 'Opening…' : 'Open'}
        </button>
      </form>
      {/* D49: secondary affordance — chat with a local model without
          opening a project. File tree / inspector / patch / memory
          stay disabled in that mode; this is for the "I just want
          to talk to my local model" path. */}
      <div className="plume-open-form-secondary">
        <button
          type="button"
          className="ink-button plume-open-form-chat-only"
          onClick={onChatOnly}
          disabled={busy}
          aria-label="Chat with a local model without opening a project"
        >
          Chat without a project
        </button>
        <p className="plume-open-form-hint">
          Talk to a local model right away. No file editing, no
          memory, no agent mode — open a project later when you want
          those.
        </p>
      </div>
    </section>
  );
}
