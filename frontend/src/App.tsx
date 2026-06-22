import { lazy, Suspense } from "react";
import { Link, Navigate, NavLink, Route, Routes } from "react-router-dom";
import { AppFooter } from "./components/AppFooter";
import { AsyncBoundary } from "./components/AsyncBoundary";

const Dashboard = lazy(() =>
  import("./components/Dashboard").then((module) => ({
    default: module.Dashboard,
  })),
);
const PortfolioLayout = lazy(() =>
  import("./components/PortfolioLayout").then((module) => ({
    default: module.PortfolioLayout,
  })),
);
const HoldingsPage = lazy(() =>
  import("./components/HoldingsPage").then((module) => ({
    default: module.HoldingsPage,
  })),
);
const GainsPage = lazy(() =>
  import("./components/GainsPage").then((module) => ({
    default: module.GainsPage,
  })),
);
const TransactionsPage = lazy(() =>
  import("./components/TransactionsPage").then((module) => ({
    default: module.TransactionsPage,
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
          <NavLink to="/holdings" className={navClass}>
            Holdings
          </NavLink>
          <NavLink to="/gains" className={navClass}>
            Gains
          </NavLink>
          <NavLink to="/transactions" className={navClass}>
            Transactions
          </NavLink>
          <NavLink to="/import" className={navClass}>
            Import
          </NavLink>
        </nav>
      </header>

      <main className="workspace">
        <Suspense fallback={<RouteFallback />}>
          <Routes>
            <Route element={<PortfolioLayout />}>
              <Route path="/" element={<Dashboard />} />
              <Route path="/holdings" element={<HoldingsPage />} />
              <Route path="/gains" element={<GainsPage />} />
              <Route path="/transactions" element={<TransactionsPage />} />
            </Route>
            <Route
              path="/board"
              element={<Navigate to="/holdings" replace />}
            />
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
