import { Link, NavLink, Route, Routes } from "react-router-dom";
import { BoardView } from "./components/BoardView";
import { ImportView } from "./components/ImportView";

function navClass({ isActive }: { isActive: boolean }) {
  return isActive ? "active" : undefined;
}

export function App() {
  return (
    <div className="app-shell">
      <header className="app-bar">
        <Link className="brand" to="/" aria-label="TickerTapeTallyBoard home">
          <span className="brand-mark" aria-hidden="true" />
          <span>TickerTapeTallyBoard</span>
        </Link>

        <nav className="app-nav" aria-label="Primary">
          <NavLink to="/" end className={navClass}>
            Board
          </NavLink>
          <NavLink to="/import" className={navClass}>
            Import
          </NavLink>
        </nav>
      </header>

      <main className="workspace">
        <Routes>
          <Route path="/" element={<BoardView />} />
          <Route path="/import" element={<ImportView />} />
        </Routes>
      </main>
    </div>
  );
}
