// D6: workspace-side display of the currently selected model.
//
// Lives at the top of `AgentWorkspace` so an agent (or a human)
// reading the central zone sees what the picker resolved to. Honest
// scaffolding: the banner is read-only state plus a Clear button —
// it does not start the model, send tokens, or imply chat is wired up.
//
// Two states:
//
//   * empty   — nothing selected. Prompt the user to pick from the
//               provider panel on the left.
//   * ready   — show provider · model id, and the fit verdict badge
//               when one was captured at selection time (only Ollama
//               carries a fit today; LM Studio / llama.cpp picks
//               omit it).
//
// What this banner deliberately does NOT do today:
//
//   * cross-check the latest health snapshot. If the provider goes
//     offline after selection the banner keeps the picked text; the
//     provider panel on the left is the source of truth for current
//     reachability. Hoisting health into a parent so the banner can
//     render a "(offline)" caveat is a follow-up — see
//     `docs/IPC_ROADMAP.md § Session mode and policy`.
//   * load the model, talk to a provider, or imply chat exists.

import { fitLabel, type FitState } from '../../lib/api/providers';
import type { SelectedModel } from './useSelectedModel';

export type SelectedModelBannerProps = {
  selected: SelectedModel | null;
  onClear: () => void;
};

export function SelectedModelBanner({ selected, onClear }: SelectedModelBannerProps) {
  return (
    <section
      className="plume-agent-selection ink-panel"
      aria-label="Selected model"
      aria-live="polite"
    >
      <div className="plume-agent-selection-head">
        <span className="plume-agent-selection-label">Selected model</span>
        {selected !== null ? (
          <button
            type="button"
            className="ink-button plume-agent-selection-clear"
            onClick={onClear}
            aria-label={`Clear selected model ${selected.providerDisplayName} ${selected.modelId}`}
          >
            Clear
          </button>
        ) : null}
      </div>
      {selected === null ? (
        <p className="plume-agent-selection-empty">
          No model selected. Pick one from the provider panel on the left. Plume
          will remember it for this window only — no chat, no loading, no
          downloads happen yet.
        </p>
      ) : (
        <p className="plume-agent-selection-body">
          <span className="plume-agent-selection-provider">{selected.providerDisplayName}</span>
          <span className="plume-agent-selection-sep" aria-hidden>
            ·
          </span>
          <span className="plume-agent-selection-model">{selected.modelId}</span>
          {selected.fit ? <FitChip fit={selected.fit} /> : null}
        </p>
      )}
    </section>
  );
}

function FitChip({ fit }: { fit: FitState }) {
  return (
    <span
      className={`ink-badge plume-fit plume-fit-${fit}`}
      aria-label={`Fit verdict: ${fitLabel(fit)}`}
    >
      {fitLabel(fit)}
    </span>
  );
}
