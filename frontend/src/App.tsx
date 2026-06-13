export function App() {
  return (
    <main className="app-shell">
      <section className="workspace">
        <header className="workspace-header">
          <div>
            <p className="eyebrow">Portfolio tracker</p>
            <h1>TickerTapeTallyBoard</h1>
          </div>
          <span className="status-pill">Skeleton</span>
        </header>

        <div className="summary-grid">
          <article>
            <span>Total value</span>
            <strong>SEK 0</strong>
          </article>
          <article>
            <span>Holdings</span>
            <strong>0</strong>
          </article>
          <article>
            <span>Transactions</span>
            <strong>0</strong>
          </article>
        </div>

        <section className="work-list" aria-labelledby="next-work">
          <h2 id="next-work">Next implementation slices</h2>
          <ul>
            <li>Backend health API</li>
            <li>Frontend API status query</li>
            <li>Sharesight import spike</li>
          </ul>
        </section>
      </section>
    </main>
  );
}
