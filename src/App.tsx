export function App() {
  return (
    <main className="plume-shell">
      <header className="plume-header">
        <h1>Plume</h1>
        <p>A quiet local AI coding editor — early scaffold.</p>
      </header>
      <section className="plume-empty ink-panel">
        <p>
          Open a project folder to begin. The UI is not implemented yet — see{' '}
          <code>docs/ARCHITECTURE.md</code> for the planned panes and{' '}
          <code>docs/UI_STYLE.md</code> for the visual system.
        </p>
      </section>
    </main>
  );
}
