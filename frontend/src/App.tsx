import { lazy, Suspense } from "react";
import { Link, NavLink, Route, Routes } from "react-router-dom";
import { AppFooter } from "./components/AppFooter";
import { AsyncBoundary } from "./components/AsyncBoundary";

const Dashboard = lazy(() =>
  import("./components/Dashboard").then((module) => ({
    default: module.Dashboard,
  })),
);
const BoardView = lazy(() =>
  import("./components/BoardView").then((module) => ({
    default: module.BoardView,
  })),
);
const ImportView = lazy(() =>
  import("./components/ImportView").then((module) => ({
    default: module.ImportView,
  })),
);
const AssetView = lazy(() =>
  import("./components/AssetView").then((module) => ({
    default: module.AssetView,
  })),
);

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
            Dashboard
          </NavLink>
          <NavLink to="/board" className={navClass}>
            Board
          </NavLink>
          <NavLink to="/import" className={navClass}>
            Import
          </NavLink>
        </nav>
      </header>

      <main className="workspace">
        <Suspense fallback={<RouteFallback />}>
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/board" element={<BoardView />} />
            <Route path="/import" element={<ImportView />} />
            <Route path="/asset/:id" element={<AssetView />} />
          </Routes>
        </Suspense>
      </main>
      <AppFooter />
    </div>
  );
}

function RouteFallback() {
  return <AsyncBoundary isPending />;
}
