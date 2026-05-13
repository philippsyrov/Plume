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

  useEffect(() => {
    let cancelled = false;
    setState((prev) => ({ ...prev, status: 'loading', error: null }));

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
    // the answer would now differ.
  }, [relPath, startLine, endLine, projectHasInstructions]);

  return state;
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Context preview failed for an unknown reason.';
}
