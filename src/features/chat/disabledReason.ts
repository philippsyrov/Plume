// Disabled-state logic for the chat panel.
//
// D22 extraction: the reason-state machine and its render helpers
// moved out of `ChatPanel.tsx` so the panel can stay focused on
// JSX glue. Pure logic — no React, no IPC.

import type { ProviderReachabilityState } from './useProviderReachability';
import type { SelectedModel } from '../model-picker/useSelectedModel';

const SUPPORTED_PROVIDER_ID = 'ollama';

/// D14: `'provider-unreachable'` joins the older disabled states.
/// It only fires when the user has picked a supported model AND
/// the reachability probe came back as offline / not-configured.
/// The provider name is generic here so the same code path covers
/// the future LM Studio + llama.cpp adapters without a new variant.
///
/// `'provider-checking'` is the transient state between clicking
/// `Recheck` (or first mount) and the probe resolving. Pre-fix
/// this wasn't a distinct reason: `isProviderUnreachable` only
/// returned `true` on `status === 'ready' && reachability !==
/// 'available'`, so the moment the user clicked Recheck the hook
/// flipped to `loading`, the disabled-reason dropped to `null`,
/// the Recheck button vanished, and Send briefly enabled before
/// the new probe result landed. That contradicted the SMOKE
/// expectation of a stable `Rechecking…` button and was a real
/// flicker for the user. The distinct state keeps the Recheck
/// affordance visible (and disabled) while the probe is in
/// flight, and Send stays gated.
export type DisabledReason =
  | 'no-selection'
  | 'unsupported-provider'
  | 'streaming'
  | 'provider-checking'
  | 'provider-unreachable'
  | null;

export function computeDisabledReason(
  selected: SelectedModel | null,
  status: 'idle' | 'streaming' | 'error',
  providerUnreachable: boolean,
  providerChecking: boolean,
): DisabledReason {
  if (status === 'streaming') return 'streaming';
  if (selected === null) return 'no-selection';
  if (selected.providerId !== SUPPORTED_PROVIDER_ID) return 'unsupported-provider';
  // Order matters: unreachable wins over checking. If the previous
  // probe already returned "not available" we surface that copy
  // immediately and the user can act; the in-flight refresh just
  // updates the Recheck button label.
  if (providerUnreachable) return 'provider-unreachable';
  if (providerChecking) return 'provider-checking';
  return null;
}

/// Treat the probe result as "unreachable" only when we have a
/// definitive answer. `loading`, `idle`, and `error` all collapse
/// to "we don't know" — better to let the user try Send and learn
/// from the actual transport error than to lock them out on a
/// flaky `providers.health` IPC.
export function isProviderUnreachable(
  selected: SelectedModel | null,
  reachability: ProviderReachabilityState,
): boolean {
  if (selected === null) return false;
  if (selected.providerId !== SUPPORTED_PROVIDER_ID) return false;
  if (reachability.status !== 'ready') return false;
  return reachability.reachability !== 'available';
}

/// `true` while a reachability probe is in flight for the
/// currently-selected supported provider. Keeps the UI on the
/// Recheck-aware code path during the brief window between
/// clicking Recheck and the new snapshot landing. `'idle'` and
/// `'error'` deliberately don't qualify — those are "we don't
/// know" states that fall through to the optimistic null branch.
export function isProviderChecking(
  selected: SelectedModel | null,
  reachability: ProviderReachabilityState,
): boolean {
  if (selected === null) return false;
  if (selected.providerId !== SUPPORTED_PROVIDER_ID) return false;
  return reachability.status === 'loading';
}

export function inputPlaceholder(
  selected: SelectedModel | null,
  disabledReason: DisabledReason,
): string {
  switch (disabledReason) {
    case 'no-selection':
      return 'Pick a model on the left to enable chat.';
    case 'unsupported-provider':
      return `Chat is only wired for Ollama today (selected: ${selected?.providerDisplayName ?? 'unknown'}).`;
    case 'streaming':
      return 'Streaming reply… click Stop to cancel.';
    case 'provider-checking':
      return `Type your message — checking ${selected?.providerDisplayName ?? 'the daemon'} reachability…`;
    case 'provider-unreachable':
      // Textarea stays ENABLED for this state (see `isInputDisabled`
      // helper) so the user can compose while starting the
      // daemon. The placeholder tells them how to unblock Send.
      return `Type your message — start ${selected?.providerDisplayName ?? 'the daemon'} and click Recheck to send.`;
    case null:
      return `Send a message to ${selected?.modelId ?? 'the model'}…`;
  }
}

/// `disabledReason !== null` is too broad for the textarea — the
/// `'provider-unreachable'` and `'provider-checking'` cases should
/// still let the user type so they can compose a prompt while the
/// daemon comes up or while the probe is in flight. Send stays
/// disabled regardless. Pulled into a helper so the next state
/// that wants the same treatment can opt in by name.
export function isInputDisabled(reason: DisabledReason): boolean {
  if (reason === null) return false;
  if (reason === 'provider-unreachable') return false;
  if (reason === 'provider-checking') return false;
  return true;
}

export function chatStatusText(
  selected: SelectedModel | null,
  disabledReason: DisabledReason,
  isStreaming: boolean,
): string {
  if (isStreaming) return 'Streaming reply…';
  switch (disabledReason) {
    case 'no-selection':
      return 'No model selected.';
    case 'unsupported-provider':
      return 'Selected provider has no chat adapter yet (Ollama only).';
    case 'streaming':
      return 'Streaming reply…';
    case 'provider-checking':
      return `Checking ${selected?.providerDisplayName ?? 'provider'} reachability…`;
    case 'provider-unreachable':
      return `${selected?.providerDisplayName ?? 'Provider'} not reachable — start the daemon and click Recheck.`;
    case null:
      return selected
        ? `Ready · ${selected.providerDisplayName} · ${selected.modelId}`
        : 'Ready.';
  }
}
