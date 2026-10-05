import { lazy, Suspense, useEffect, useReducer } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import { useDataVersion } from "./api/queries";
import { AppBar } from "./components/AppBar";
import { AppFooter } from "./components/AppFooter";
import { AsyncBoundary } from "./components/AsyncBoundary";
import {
  dateRangeSelectionReducer,
  loadDateRangeSelection,
  saveDateRangeSelection,
} from "./components/DateRangeSelector";
import { useAppMode } from "./components/useAppMode";
import { usePriceRefresh } from "./components/usePriceRefresh";

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
const RebalancePage = lazy(() =>
  import("./components/RebalancePage").then((module) => ({
    default: module.RebalancePage,
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

export function App() {
  const [dateRangeSelection, dispatchDateRangeSelection] = useReducer(
    dateRangeSelectionReducer,
    undefined,
    loadDateRangeSelection,
  );

  useEffect(() => {
    saveDateRangeSelection(dateRangeSelection);
  }, [dateRangeSelection]);

  const appMode = useAppMode();
  const priceRefresh = usePriceRefresh();
  const dataVersion = useDataVersion();

  const dateRangeProps = {
    selectedDatePreset: dateRangeSelection.datePreset,
    customRange: dateRangeSelection.customRange,
    valuationDate: dataVersion.data?.valuation_date ?? null,
    onDatePresetChange: (datePreset: typeof dateRangeSelection.datePreset) =>
      dispatchDateRangeSelection({ type: "datePresetChanged", datePreset }),
    onDateRangeChange: (dateRange: typeof dateRangeSelection.customRange) =>
      dispatchDateRangeSelection({ type: "dateRangeChanged", dateRange }),
  };

  return (
    <div className="app-shell">
      <AppBar appMode={appMode} priceRefresh={priceRefresh} />

      <main className="workspace">
        <Suspense fallback={<RouteFallback />}>
          <Routes>
            <Route
              element={<PortfolioLayout refreshStatus={priceRefresh.status} />}
            >
              <Route path="/" element={<Dashboard {...dateRangeProps} />} />
              <Route path="/holdings" element={<HoldingsPage />} />
              <Route path="/rebalance" element={<RebalancePage />} />
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
                appMode.canMutate ? (
                  <ImportView
                    priceRefreshRunning={priceRefresh.status.running}
                  />
                ) : (
                  <Navigate to="/" replace />
                )
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
