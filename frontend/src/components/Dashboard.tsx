import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { useGains, usePortfolioValueHistory } from "../api/queries";
import type { GainsRow } from "../api/types";
import {
  type AllocationDimension,
  allocationBreakdown,
  type MoverRow,
  topMovers,
} from "./dashboardSelectors";
import { TimeSeriesChart } from "./TimeSeriesChart";
import { formatGroupedNumber } from "./valuationDisplay";

export function Dashboard() {
  const gainsQuery = useGains();
  const valueHistory = usePortfolioValueHistory();

  return (
    <section className="dashboard" aria-label="Portfolio dashboard">
      <DashboardValueChart query={valueHistory} />
      <TopMoversPanel rows={gainsQuery.data?.rows ?? []} />
      <AllocationPanel rows={gainsQuery.data?.rows ?? []} />
    </section>
  );
}

function DashboardValueChart({
  query,
}: {
  query: ReturnType<typeof usePortfolioValueHistory>;
}) {
  const history = query.data?.points;
  const points = useMemo(
    () =>
      (history ?? []).map((point) => ({
        time: point.date,
        value: Number(point.value_base),
      })),
    [history],
  );
  const incompleteDays = useMemo(
    () => (history ?? []).filter((point) => point.incomplete).length,
    [history],
  );

  if (query.isPending) {
    return (
      <section className="chart-band" aria-label="Portfolio value">
        <div className="skeleton-bar" />
      </section>
    );
  }

  if (query.isError) {
    return (
      <section className="chart-band error" aria-label="Portfolio value">
        <p className="down">Could not load portfolio value.</p>
        <button
          type="button"
          className="button outline"
          onClick={() => void query.refetch()}
        >
          Retry
        </button>
      </section>
    );
  }

  if (points.length === 0) {
    return (
      <section className="chart-band muted" aria-label="Portfolio value">
        <span className="chart-band-label">
          No portfolio history yet — add a Buy and refresh prices
        </span>
      </section>
    );
  }

  return (
    <section className="panel chart-panel" aria-label="Portfolio value">
      <div className="chart-meta">
        <h2>Portfolio value (SEK)</h2>
        {incompleteDays > 0 ? (
          <span className="status-chip warning compact">
            {incompleteDays} days had missing inputs
          </span>
        ) : null}
      </div>
      <TimeSeriesChart
        data={points}
        ariaLabel="Portfolio value over time in SEK"
        visibleStart={query.data?.start_date ?? undefined}
        height={280}
      />
    </section>
  );
}

function TopMoversPanel({ rows }: { rows: GainsRow[] }) {
  const { gainers, losers } = topMovers(rows);
  if (gainers.length === 0 && losers.length === 0) {
    return null;
  }

  return (
    <section className="panel asset-panel" aria-label="Top movers">
      <h2>Top movers</h2>
      <div className="movers-grid">
        <MoverList title="Gainers" movers={gainers} />
        <MoverList title="Losers" movers={losers} />
      </div>
    </section>
  );
}

function MoverList({ title, movers }: { title: string; movers: MoverRow[] }) {
  return (
    <div className="mover-list">
      <h3>{title}</h3>
      {movers.length === 0 ? (
        <p className="asset-subtle">-</p>
      ) : (
        <ul>
          {movers.map((mover) => (
            <li key={mover.instrument.id}>
              <Link
                className="instrument-link"
                to={`/asset/${mover.instrument.id}`}
              >
                {mover.instrument.symbol}
              </Link>
              <span
                className={mover.percent >= 0 ? "up number" : "down number"}
              >
                {mover.percent >= 0 ? "+" : ""}
                {formatGroupedNumber(mover.percent.toFixed(2))}%
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function AllocationPanel({ rows }: { rows: GainsRow[] }) {
  const [dimension, setDimension] = useState<AllocationDimension>("instrument");
  const { slices, excludedCount } = useMemo(
    () => allocationBreakdown(rows, dimension),
    [rows, dimension],
  );
  const palette = [
    "var(--chart-1)",
    "var(--chart-2)",
    "var(--chart-3)",
    "var(--chart-4)",
    "var(--chart-5)",
    "var(--chart-6)",
  ];

  return (
    <section className="panel allocation-panel" aria-label="Allocation">
      <div className="panel-header">
        <h2>Allocation</h2>
        <fieldset className="segmented-control">
          <legend className="sr-only">Allocation dimension</legend>
          {(["instrument", "currency", "type"] as AllocationDimension[]).map(
            (dim) => (
              <button
                key={dim}
                type="button"
                className={dimension === dim ? "active" : undefined}
                aria-pressed={dimension === dim}
                onClick={() => setDimension(dim)}
              >
                {dim[0].toUpperCase() + dim.slice(1)}
              </button>
            ),
          )}
        </fieldset>
      </div>

      {slices.length === 0 ? (
        <p className="board-state muted">No valued holdings to allocate.</p>
      ) : (
        <div className="allocation-body">
          <div
            className="allocation-bar"
            role="img"
            aria-label="Allocation segments"
          >
            {slices.map((slice, index) => (
              <span
                key={slice.key}
                className="allocation-segment"
                style={{
                  width: `${slice.weightPercent}%`,
                  background: palette[index % palette.length],
                }}
                title={`${slice.label} ${slice.weightPercent.toFixed(1)}%`}
              />
            ))}
          </div>
          <table className="allocation-table">
            <tbody>
              {slices.map((slice, index) => (
                <tr key={slice.key}>
                  <td>
                    <span
                      className="allocation-swatch"
                      style={{ background: palette[index % palette.length] }}
                    />
                    {slice.label}
                  </td>
                  <td className="number">
                    SEK {formatGroupedNumber(slice.valueBase.toFixed(2))}
                  </td>
                  <td className="number">{slice.weightPercent.toFixed(1)}%</td>
                </tr>
              ))}
            </tbody>
          </table>
          {excludedCount > 0 ? (
            <span className="status-chip warning compact">
              {excludedCount} excluded (no market value)
            </span>
          ) : null}
        </div>
      )}
    </section>
  );
}
