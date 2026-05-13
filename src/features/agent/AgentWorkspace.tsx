// Central agent workspace.
//
// D1.5 carved out the shape of the workspace shell — left navigation,
// central agent surface, right inspector — without committing to a
// chat backend. D6 added a "Selected model" banner. D7 added the
// first interactive surface in this zone: a read-only chat panel
// that round-trips a prompt + assistant reply against the selected
// local model (Ollama only for now). D7.1 turned that into a
// streaming surface with a Stop button. D8 layered an explicit
// "Attach current file" control onto the chat panel, fed by the
// file-inspector selection state hoisted in `App.tsx`.
//
// The mode cards below name the four safety modes in `docs/SAFETY.md`
// (`chat`, `propose-diff`, `scoped-edit`, `agent-loop`). With D7 the
// "Chat" card flipped to "shipped (read-only)". D15 added the
// propose-diff preview path: the model can emit a unified diff and
// the chat panel renders it. D16 layered a read-only patch.validate
// IPC that shows a "valid diff · N files · M hunks" / "invalid
// diff: <reason>" pill under the rendered diff. Applying that diff
// to disk is still roadmap — the propose-diff card carries a
// "preview only — apply not yet" badge to mark that split. Scoped
// edit and agent loop stay labelled "not yet implemented". The
// card grid is a map of what's planned; the chat panel above it
// is what works today.

import { ChatPanel } from '../chat/ChatPanel';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import { SelectedModelBanner } from '../model-picker/SelectedModelBanner';
import type { SelectedModel } from '../model-picker/useSelectedModel';

type ModeCard = {
  id: string;
  title: string;
  blurb: string;
  status: 'shipped' | 'preview' | 'planned';
};

const MODE_CARDS: ModeCard[] = [
  {
    id: 'chat',
    title: 'Chat',
    blurb:
      'Send a prompt to the selected local model and read the reply. Optionally attach one project file as read-only context — Plume redacts known secret patterns before sending. No file writes, no commands, no patches. Today via Ollama only.',
    status: 'shipped',
  },
  {
    id: 'propose-diff',
    title: 'Propose diff',
    blurb:
      'Model emits a unified diff and the chat panel renders it with per-line coloring. D16 added a read-only validator that surfaces "valid diff · N files · M hunks" or "invalid diff: <reason>" below each rendered diff. Apply still disabled today — the user copies and applies by hand. On-disk apply / checkpoint / revert land in a later slice.',
    status: 'preview',
  },
  {
    id: 'scoped-edit',
    title: 'Scoped edit',
    blurb:
      'Plume applies edits inside an explicit file allowlist after each step is approved.',
    status: 'planned',
  },
  {
    id: 'agent-loop',
    title: 'Agent loop',
    blurb:
      'Read, edit, run the verifier, fix. Bounded iterations. Strongest models only; off by default.',
    status: 'planned',
  },
];

export type AgentWorkspaceProps = {
  selected: SelectedModel | null;
  onClearSelection: () => void;
  /**
   * Selection state from the file inspector. ChatPanel uses it to
   * decide whether the "Attach current file" control is eligible.
   * `null` is allowed for tests/scaffolds that mount this surface
   * without a navigator.
   */
  inspectorSelection: SelectionState | null;
  /**
   * D10: current non-empty text selection inside the inspector's
   * editor, expressed as 1-based line numbers. ChatPanel uses
   * this to flip its attach control between "Attach selection"
   * and "Attach current file". `null` when nothing is selected.
   */
  inspectorLineRange: EditorLineRange | null;
  /**
   * D11: `true` when the trusted project has a root `AGENTS.md`.
   * ChatPanel renders a small "Project instructions" indicator
   * when this is set. Driven by `ProjectMeta.hasAgentsMd` —
   * the backend re-reads on every send, so this is a
   * forward-looking promise rather than a per-message confirmation.
   */
  projectHasInstructions: boolean;
};

export function AgentWorkspace({
  selected,
  onClearSelection,
  inspectorSelection,
  inspectorLineRange,
  projectHasInstructions,
}: AgentWorkspaceProps) {
  return (
    <section
      className="plume-agent-workspace ink-panel"
      aria-label="Agent workspace"
      aria-describedby="plume-agent-workspace-status"
    >
      <header className="plume-agent-header">
        <h2>Agent workspace</h2>
        <p id="plume-agent-workspace-status" className="plume-agent-subtitle">
          Read-only chat is wired today (Ollama only), and the propose-diff
          mode renders model-emitted diffs in the chat panel — preview only,
          Apply stays disabled. Model loading, scoped edits, and the agent
          loop aren&apos;t implemented yet — the mode cards below name
          what&apos;s coming. Use the provider panel on the left to pick a
          model, then send a prompt below. Optionally attach one project file
          as read-only context.
        </p>
      </header>

      <SelectedModelBanner selected={selected} onClear={onClearSelection} />

      <ChatPanel
        selected={selected}
        inspectorSelection={inspectorSelection}
        inspectorLineRange={inspectorLineRange}
        projectHasInstructions={projectHasInstructions}
      />

      <div className="plume-agent-modes" role="list" aria-label="Agent modes">
        {MODE_CARDS.map((card) => (
          <article key={card.id} className="plume-agent-mode" role="listitem">
            <header className="plume-agent-mode-header">
              <h3>{card.title}</h3>
              <span
                className={`ink-badge plume-agent-mode-${
                  card.status === 'shipped' ? 'shipped' : 'pending'
                }`}
              >
                {card.status === 'shipped'
                  ? 'shipped (read-only)'
                  : card.status === 'preview'
                    ? 'preview only — apply not yet'
                    : 'not yet implemented'}
              </span>
            </header>
            <p>{card.blurb}</p>
          </article>
        ))}
      </div>

      <footer className="plume-agent-footnote">
        <p>
          Every future mode will still flow through the same safety gates Plume
          uses today: project trust, path sandbox, command approval, and patch
          validation. See <code>docs/SAFETY.md</code> and{' '}
          <code>docs/MODEL_PROVIDERS.md</code>.
        </p>
      </footer>
    </section>
  );
}
