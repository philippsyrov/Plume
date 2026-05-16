// D12: hook wrapper around the `chat.context` IPC.
//
// The chat panel needs to render a small "what will ride along on
// the next send" preview area. The numbers (AGENTS.md size, chip
// size, redaction count, blocked/ready status) come from the
// backend — the same prompt-read path `chat.send` would run. Pulling
// the fetch into a hook keeps the panel component focused on UI
// concerns and makes the request lifecycle (cancellation on rapid
// chip changes, cleanup on unmount) reusable for any future surface
// that wants the same preview.
//
// Refetch policy:
//   * On mount.
//   * When the attachment chip changes (relPath / line range).
//   * When `projectHasInstructions` flips — the preview's
//     `instructions` field follows the same flag, so a mid-session
//     refresh that creates / removes AGENTS.md should re-probe.
//
// What this hook deliberately does NOT do:
//   * Poll on a timer. The preview is a snapshot, not a live view —
//     stale information is preferable to a chatty IPC loop.
//   * Cache responses across project roots. The hook lives inside
//     `ChatPanel`; closing the project unmounts it.
//   * Retry on error. A transient IPC failure surfaces as the
//     `error` status and the panel renders a one-line hint. The
//     next chip change re-fires; the user can also just send and
//     let the real send report whatever surfaced.
//
// Cancellation: each effect run captures a local `cancelled` flag.
// If a new render fires before the previous request resolves, the
// stale one is discarded — same `useEffect` cleanup pattern used
// elsewhere in the codebase.

import { useEffect, useState } from 'react';

import {
  previewChatContext,
  type ChatAttachment,
  type ChatContextResponse,
} from '../../lib/api/chat';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { useMemoryRevision } from '../memory/memoryRevision';

/// Input shape mirrors the chat panel's chip state. `relPath`
/// `null` means "no attachment to preview"; the hook still fires
/// the IPC so the AGENTS.md side comes back.
export type ChatContextPreviewInput = {
  relPath: string | null;
  startLine: number | null;
  endLine: number | null;
  /** Whether the project metadata reports an AGENTS.md at root.
   * Used as a refetch dependency: the preview's `instructions`
   * field flips between non-null and null when this changes, and
   * the hook re-probes so the UI surface stays honest. */
  projectHasInstructions: boolean;
};

export type ChatContextPreviewStatus = 'idle' | 'loading' | 'ready' | 'error';

export type ChatContextPreviewState = {
  status: ChatContextPreviewStatus;
  /** Last successful response. Held across `loading` transitions
   * so the preview area doesn't flicker between renders. */
  data: ChatContextResponse | null;
  /** Set when `status === 'error'`; cleared on the next successful
   * load. */
  error: string | null;
};

const INITIAL_STATE: ChatContextPreviewState = {
  status: 'idle',
  data: null,
  error: null,
};

/// Fetch the `chat.context` preview whenever the input changes.
/// Returns the latest state; the consumer reads `state.data` to
/// render. The hook never throws — errors surface as
/// `state.status === 'error'`.
export function useChatContextPreview(
  input: ChatContextPreviewInput,
): ChatContextPreviewState {
  const [state, setState] = useState<ChatContextPreviewState>(INITIAL_STATE);
  const { relPath, startLine, endLine, projectHasInstructions } = input;
  // D42 Codex fix: a remember / forget in the Memory panel bumps
  // the revision counter; the chat-context preview reads it as a
  // refetch trigger. Without this dep the chat header's
  // MemoryBadge would keep showing the entry counts from before
  // the user clicked Remember / Forget, even though the next
  // `chat.send` would honestly reflect the new state.
  const memoryRevision = useMemoryRevision();

  useEffect(() => {
    let cancelled = false;
    // Clear the attachment side of `data` immediately when the
    // input changes. Keeping the old attachment preview around
    // during the loading flicker would be a lie — the stale
    // "would ride along" answer no longer matches the chip the
    // user actually has set right now. Instructions side stays
    // cached because it doesn't depend on the attachment input;
    // a project-level flip (`projectHasInstructions`) refetches
    // and replaces it on the next render.
    setState((prev) => ({
      status: 'loading',
      error: null,
      data: prev.data ? { ...prev.data, attachment: null } : null,
    }));

    const attachment: ChatAttachment | undefined =
      relPath !== null
        ? {
            kind: 'projectFile',
            relPath,
            ...(startLine !== null && endLine !== null
              ? { startLine, endLine }
              : {}),
          }
        : undefined;

    previewChatContext(attachment ? { attachment } : {})
      .then((response) => {
        if (cancelled) return;
        setState({ status: 'ready', data: response, error: null });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        const message = formatError(err);
        setState((prev) => ({ status: 'error', data: prev.data, error: message }));
      });

    return () => {
      cancelled = true;
    };
    // `projectHasInstructions` is a refetch dependency even though
    // the IPC payload doesn't carry it — the BACKEND looks at the
    // project state when answering, and a flip in this flag means
    // the answer would now differ. Same shape for `memoryRevision`:
    // the IPC payload doesn't carry it; the BACKEND's preview reads
    // the memory store on every call. We only need it as a "trigger
    // a fresh fetch" signal.
  }, [relPath, startLine, endLine, projectHasInstructions, memoryRevision]);

  return state;
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Context preview failed for an unknown reason.';
}
