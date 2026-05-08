// Central placeholder for the agent workspace.
//
// D1.5 carves out the shape of the workspace shell — left navigation,
// central agent surface, right inspector — without committing to a
// chat backend. The four mode cards below mirror the safety modes in
// `docs/SAFETY.md` (`chat`, `propose-diff`, `scoped-edit`,
// `agent-loop`); they are deliberately disabled and labeled "not yet
// implemented" rather than rendered as functional buttons. The aim
// is honest scaffolding: an agent reading the DOM should learn that
// Plume is a project browser today, not a working agent.
//
// When chat lands, this component grows real controls — prompt input,
// mode selector, message list — without changing where it sits in the
// shell.

const MODE_CARDS: Array<{ id: string; title: string; blurb: string }> = [
  {
    id: 'chat',
    title: 'Chat',
    blurb:
      'Ask the local model about files in this project. Read-only context, no file writes.',
  },
  {
    id: 'propose-diff',
    title: 'Propose diff',
    blurb:
      'Model emits a unified diff; you review and apply. Plume validates the patch before it touches disk.',
  },
  {
    id: 'scoped-edit',
    title: 'Scoped edit',
    blurb:
      'Plume applies edits inside an explicit file allowlist after each step is approved.',
  },
  {
    id: 'agent-loop',
    title: 'Agent loop',
    blurb:
      'Read, edit, run the verifier, fix. Bounded iterations. Strongest models only; off by default.',
  },
];

export function AgentWorkspace() {
  return (
    <section
      className="plume-agent-workspace ink-panel"
      aria-label="Agent workspace"
      aria-describedby="plume-agent-workspace-status"
    >
      <header className="plume-agent-header">
        <h2>Agent workspace</h2>
        <p id="plume-agent-workspace-status" className="plume-agent-subtitle">
          Chat, model loading, and the agent loop aren&apos;t wired up yet. Today
          this surface is a placeholder; Plume currently ships project open +
          trust, a read-only file inspector, and provider reachability. Use
          the navigator to browse files and the inspector to view them.
        </p>
      </header>

      <div className="plume-agent-modes" role="list" aria-label="Planned agent modes">
        {MODE_CARDS.map((card) => (
          <article key={card.id} className="plume-agent-mode" role="listitem">
            <header className="plume-agent-mode-header">
              <h3>{card.title}</h3>
              <span className="ink-badge plume-agent-mode-pending">not yet implemented</span>
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
