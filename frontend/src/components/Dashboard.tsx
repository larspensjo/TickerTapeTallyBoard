import { usePortfolioValueHistory } from "../api/queries";
import { TimeSeriesChart } from "./TimeSeriesChart";

export function Dashboard() {
  const valueHistory = usePortfolioValueHistory();

  return (
    <section className="dashboard" aria-label="Portfolio dashboard">
      <DashboardValueChart query={valueHistory} />
    </section>
  );
}

function DashboardValueChart({
  query,
}: {
  query: ReturnType<typeof usePortfolioValueHistory>;
}) {
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

  const history = query.data?.points ?? [];
  const points = history.map((point) => ({
    time: point.date,
    value: Number(point.value_base),
  }));
  const incompleteDays = history.filter((point) => point.incomplete).length;

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
        height={280}
      />
    </section>
  );
}
