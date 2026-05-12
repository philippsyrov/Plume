// Central agent workspace.
//
// D1.5 carved out the shape of the workspace shell — left navigation,
// central agent surface, right inspector — without committing to a
// chat backend. D6 added a "Selected model" banner. D7 adds the first
// real interactive surface in this zone: a read-only chat panel that
// rounds-trips one prompt + one assistant reply against the selected
// local model (Ollama only for now).
//
// The mode cards below name the four safety modes in `docs/SAFETY.md`
// (`chat`, `propose-diff`, `scoped-edit`, `agent-loop`). With D7 the
// "Chat" card flips to "shipped (read-only)" and the rest stay
// labelled "not yet implemented". The card grid is a map of what's
// planned; the chat panel above it is what works today.

import { ChatPanel } from '../chat/ChatPanel';
import { SelectedModelBanner } from '../model-picker/SelectedModelBanner';
import type { SelectedModel } from '../model-picker/useSelectedModel';

type ModeCard = {
  id: string;
  title: string;
  blurb: string;
  status: 'shipped' | 'planned';
};

const MODE_CARDS: ModeCard[] = [
  {
    id: 'chat',
    title: 'Chat',
    blurb:
      'Send a prompt to the selected local model and read the reply. No file access, no commands, no patches. Today via Ollama only.',
    status: 'shipped',
  },
  {
    id: 'propose-diff',
    title: 'Propose diff',
    blurb:
      'Model emits a unified diff; you review and apply. Plume validates the patch before it touches disk.',
    status: 'planned',
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
};

export function AgentWorkspace({ selected, onClearSelection }: AgentWorkspaceProps) {
  return (
    <section
      className="plume-agent-workspace ink-panel"
      aria-label="Agent workspace"
      aria-describedby="plume-agent-workspace-status"
    >
      <header className="plume-agent-header">
        <h2>Agent workspace</h2>
        <p id="plume-agent-workspace-status" className="plume-agent-subtitle">
          Read-only chat is wired today (Ollama only). Model loading, the
          propose-diff path, scoped edits, and the agent loop aren&apos;t
          implemented yet — the mode cards below name what&apos;s coming. Use
          the provider panel on the left to pick a model, then send a prompt
          below.
        </p>
      </header>

      <SelectedModelBanner selected={selected} onClear={onClearSelection} />

      <ChatPanel selected={selected} />

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
                {card.status === 'shipped' ? 'shipped (read-only)' : 'not yet implemented'}
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
