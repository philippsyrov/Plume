// Disabled-state logic for the chat panel.
//
// D22 extraction: the reason-state machine and its render helpers
// moved out of `ChatPanel.tsx` so the panel can stay focused on
// JSX glue. Pure logic — no React, no IPC.
//
// D46 Codex fix: `SUPPORTED_PROVIDER_IDS` widens to include
// `'mlx-lm'` so Start auto-selecting the MLX provider no longer
// trips `unsupported-provider`. The `providers.health` probe
// stays Ollama-only (the supervisor doesn't register MLX servers
// in that snapshot); the MLX path uses a separate
// `'mlx-not-started'` reason driven by the D40 supervisor's
// handle registry. When `mlxHandlePresent` is false for an
// mlx-lm selection, Send is gated and the placeholder tells the
// user to click Start in the Local models panel.

import type { ProviderReachabilityState } from './useProviderReachability';
import type { SelectedModel } from '../model-picker/useSelectedModel';

/** Provider ids the chat panel knows how to dispatch to today.
 *  Ollama uses the legacy NDJSON path; mlx-lm uses the D45 SSE
 *  adapter against a D40-supervised server; apple-foundation uses
 *  Rust's bundled on-device helper with no daemon or server handle. Other ids
 *  (`lm-studio`, `llama-cpp`, …) still trip `unsupported-provider`
 *  until their adapters land. */
const SUPPORTED_PROVIDER_IDS = ['ollama', 'mlx-lm', 'apple-foundation'] as const;

/** True iff `providerId` is in `SUPPORTED_PROVIDER_IDS`. */
function isSupportedProvider(providerId: string): boolean {
  return (SUPPORTED_PROVIDER_IDS as readonly string[]).includes(providerId);
}

/** True iff the reachability probe is meaningful for this provider.
 *  Ollama runs out-of-band and is what `providers.health` actually
 *  probes; MLX-LM is Plume-managed and the supervisor's handle
 *  registry is the source of truth, not the health snapshot. Apple is
 *  preflighted through its explicit catalog availability action instead. */
function usesReachabilityProbe(providerId: string): boolean {
  return providerId === 'ollama';
}

/// D14: `'provider-unreachable'` joins the older disabled states.
/// It only fires when the user has picked a supported model AND
/// the reachability probe came back as offline / not-configured.
/// The probe is Ollama-specific today; MLX-LM uses the dedicated
/// `'mlx-not-started'` reason.
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
///
/// `'mlx-not-started'` (D46 Codex fix) fires when the selected
/// provider is `mlx-lm` but no live supervisor handle exists for
/// the selected model. Without the gate, Send would fire and the
/// backend would reject with `BadArgument: handleId required` —
/// a worse UX than telling the user up front to click Start.
export type DisabledReason =
  | 'no-selection'
  | 'unsupported-provider'
  | 'streaming'
  | 'provider-checking'
  | 'provider-unreachable'
  | 'mlx-not-started'
  | null;

export function computeDisabledReason(
  selected: SelectedModel | null,
  status: 'idle' | 'streaming' | 'error',
  providerUnreachable: boolean,
  providerChecking: boolean,
  mlxHandlePresent: boolean,
): DisabledReason {
  if (status === 'streaming') return 'streaming';
  if (selected === null) return 'no-selection';
  if (!isSupportedProvider(selected.providerId)) return 'unsupported-provider';
  // D46: for mlx-lm, the handle registry is the gate. The Ollama-
  // shaped reachability probe doesn't probe mlx-lm servers, so we
  // skip its checks for this provider and route through the
  // `'mlx-not-started'` state instead.
  if (selected.providerId === 'mlx-lm') {
    if (!mlxHandlePresent) return 'mlx-not-started';
    return null;
  }
  // Apple availability is an explicit catalog action, not an Ollama-shaped
  // daemon probe. Once selected, its backend generation route needs neither a
  // reachability result nor an MLX supervisor handle.
  if (selected.providerId === 'apple-foundation') return null;
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
///
/// MLX-LM is excluded: the `providers.health` snapshot doesn't
/// cover Plume-managed servers, and surfacing "not reachable" for
/// a provider that lives in our own registry would be a lie.
export function isProviderUnreachable(
  selected: SelectedModel | null,
  reachability: ProviderReachabilityState,
): boolean {
  if (selected === null) return false;
  if (!usesReachabilityProbe(selected.providerId)) return false;
  if (reachability.status !== 'ready') return false;
  return reachability.reachability !== 'available';
}

/// `true` while a reachability probe is in flight for the
/// currently-selected probe-eligible provider. Keeps the UI on
/// the Recheck-aware code path during the brief window between
/// clicking Recheck and the new snapshot landing. `'idle'` and
/// `'error'` deliberately don't qualify — those are "we don't
/// know" states that fall through to the optimistic null branch.
export function isProviderChecking(
  selected: SelectedModel | null,
  reachability: ProviderReachabilityState,
): boolean {
  if (selected === null) return false;
  if (!usesReachabilityProbe(selected.providerId)) return false;
  return reachability.status === 'loading';
}

export function inputPlaceholder(
  selected: SelectedModel | null,
  disabledReason: DisabledReason,
): string {
  switch (disabledReason) {
    case 'no-selection':
      return 'Choose a model to start';
    case 'unsupported-provider':
      return `Chat is wired for Ollama, Apple On-Device, and Plume-managed MLX today (selected: ${selected?.providerDisplayName ?? 'unknown'}).`;
    case 'streaming':
      return 'Streaming reply… click Stop to cancel.';
    case 'provider-checking':
      return `Type your message — checking ${selected?.providerDisplayName ?? 'the daemon'} reachability…`;
    case 'provider-unreachable':
      // Textarea stays ENABLED for this state (see `isInputDisabled`
      // helper) so the user can compose while starting the
      // daemon. The placeholder tells them how to unblock Send.
      return `Type your message — start ${selected?.providerDisplayName ?? 'the daemon'} and click Recheck to send.`;
    case 'mlx-not-started':
      // Same enabled-textarea treatment as
      // `'provider-unreachable'` — the user might still be
      // composing the prompt while they go click Start on the
      // matching Local models row.
      return `Type your message — start ${selected?.modelId ?? 'the MLX model'} from Settings to send.`;
    case null:
      return `Send a message to ${selected?.modelId ?? 'the model'}…`;
  }
}

/// `disabledReason !== null` is too broad for the textarea — the
/// `'provider-unreachable'`, `'provider-checking'`, and
/// `'mlx-not-started'` cases should still let the user type so
/// they can compose a prompt while the daemon comes up, the
/// probe is in flight, or the user crosses over to click Start.
/// Send stays disabled regardless. Pulled into a helper so the
/// next state that wants the same treatment can opt in by name.
export function isInputDisabled(reason: DisabledReason): boolean {
  if (reason === null) return false;
  if (reason === 'provider-unreachable') return false;
  if (reason === 'provider-checking') return false;
  if (reason === 'mlx-not-started') return false;
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
      return '';
    case 'unsupported-provider':
      return 'Selected provider has no chat adapter yet (Ollama, Apple On-Device, and Plume-managed MLX are wired).';
    case 'streaming':
      return 'Streaming reply…';
    case 'provider-checking':
      return `Checking ${selected?.providerDisplayName ?? 'provider'} reachability…`;
    case 'provider-unreachable':
      return `${selected?.providerDisplayName ?? 'Provider'} not reachable — start the daemon and click Recheck.`;
    case 'mlx-not-started':
      return `${selected?.modelId ?? 'MLX model'} has no Plume-managed server — start it from Settings.`;
    case null:
      return selected
        ? `Ready · ${selected.providerDisplayName} · ${selected.modelId}`
        : 'Ready.';
  }
}
