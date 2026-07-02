import { lazy, Suspense, useEffect, useReducer } from "react";
import { Link, Navigate, NavLink, Route, Routes } from "react-router-dom";
import { useHealth } from "./api/queries";
import { AppFooter } from "./components/AppFooter";
import { AsyncBoundary } from "./components/AsyncBoundary";
import { appModeViewModel } from "./components/appModeViewModel";
import {
  dateRangeSelectionReducer,
  loadDateRangeSelection,
  saveDateRangeSelection,
} from "./components/DateRangeSelector";

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
  const [dateRangeSelection, dispatchDateRangeSelection] = useReducer(
    dateRangeSelectionReducer,
    undefined,
    loadDateRangeSelection,
  );

  useEffect(() => {
    saveDateRangeSelection(dateRangeSelection);
  }, [dateRangeSelection]);

  const healthQuery = useHealth();
  const appMode = appModeViewModel(healthQuery.data?.demo === true);

  const dateRangeProps = {
    dateRange: dateRangeSelection.dateRange,
    selectedDatePreset: dateRangeSelection.datePreset,
    onDatePresetChange: (datePreset: typeof dateRangeSelection.datePreset) =>
      dispatchDateRangeSelection({ type: "datePresetChanged", datePreset }),
    onDateRangeChange: (dateRange: typeof dateRangeSelection.dateRange) =>
      dispatchDateRangeSelection({ type: "dateRangeChanged", dateRange }),
  };

  return (
    <div className="app-shell">
      <header className="app-bar">
        <Link className="brand" to="/" aria-label="TickerTapeTallyBoard home">
          <span className="brand-mark" aria-hidden="true" />
          <span>TickerTapeTallyBoard</span>
        </Link>
        {appMode.showDemoBadge ? (
          <span className="demo-badge">DEMO</span>
        ) : null}

        <nav className="app-nav" aria-label="Primary">
          {appMode.navItems.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              className={navClass}
            >
              {item.label}
            </NavLink>
          ))}
        </nav>
      </header>

      <main className="workspace">
        <Suspense fallback={<RouteFallback />}>
          <Routes>
            <Route element={<PortfolioLayout />}>
              <Route path="/" element={<Dashboard {...dateRangeProps} />} />
              <Route path="/holdings" element={<HoldingsPage />} />
              <Route
                path="/gains"
                element={<GainsPage {...dateRangeProps} />}
              />
              <Route path="/transactions" element={<TransactionsPage />} />
            </Route>
            <Route
              path="/board"
              element={<Navigate to="/holdings" replace />}
            />
            <Route
              path="/import"
              element={
                appMode.canMutate ? <ImportView /> : <Navigate to="/" replace />
              }
            />
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
