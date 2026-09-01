// Why the research action is unavailable, and how a failed export reads.
//
// Lifted out of `ChatPanel` for size. They are pure functions of the panel's
// state with no rendering of their own, so they read better beside it than
// buried under 700 lines of markup.

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import type { ChatContextOwner } from '../../lib/api/chat';
import { isManagedMlxProvider } from '../providers/useMlxServers';
import type { SelectedModel } from '../model-picker/useSelectedModel';

export function researchUnavailableReason({
  contextOwner,
  selected,
  researchModelSupported,
  researchTextSourceCount,
  researchSourceCount,
  researchActive,
  isStreaming,
  mlxHandlePresent,
}: {
  contextOwner: ChatContextOwner | undefined;
  selected: SelectedModel | null;
  researchModelSupported: boolean;
  researchTextSourceCount: number;
  researchSourceCount: number;
  researchActive: boolean;
  isStreaming: boolean;
  mlxHandlePresent: boolean;
}): string | null {
  if (researchActive || isStreaming) return 'Wait for the current work to finish.';
  if (contextOwner === undefined) return 'Save this chat before creating a research note.';
  if (!researchModelSupported || selected === null) {
    return 'Choose Apple On-Device, Qwen, or Qwen2-VL.';
  }
  if (isManagedMlxProvider(selected.providerId) && !mlxHandlePresent) {
    return 'Start the selected model first.';
  }
  if (researchTextSourceCount === 0) return 'Attach captured page text first.';
  if (researchSourceCount > 10) return 'Remove captured sources until 10 or fewer remain.';
  return null;
}

export function researchProductError(error: unknown): string {
  if (isIpcError(error)) return ipcErrorMessage(error);
  if (error instanceof Error) return error.message;
  return 'The research note could not be exported.';
}
