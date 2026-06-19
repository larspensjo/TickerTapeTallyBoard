import type { ReactNode } from "react";
import { Link, useParams } from "react-router-dom";
import {
  useGains,
  useHoldings,
  useInstruments,
  usePriceStatus,
  useTransactions,
} from "../api/queries";
import type {
  GainsRow,
  Instrument,
  PriceStatusInstrument,
  Transaction,
} from "../api/types";
import {
  type BreakdownView,
  breakdownView,
  deriveAssetData,
  headerStatus,
  parseInstrumentId,
  type Tiles,
  tilesView,
} from "./assetViewModel";
import { TransactionsTable } from "./TransactionsTable";
import {
  formatGroupedNumber,
  freshnessLabel,
  freshnessTone,
  SummaryAvailabilityValue,
} from "./valuationDisplay";

export function AssetView() {
  const { id: idParam } = useParams();
  const id = parseInstrumentId(idParam);

  const instrumentsQuery = useInstruments();
  const gainsQuery = useGains(true);
  const holdingsQuery = useHoldings();
  const transactionsQuery = useTransactions();
  const priceStatusQuery = usePriceStatus();

  const isPending =
    instrumentsQuery.isPending ||
    gainsQuery.isPending ||
    holdingsQuery.isPending ||
    transactionsQuery.isPending ||
    priceStatusQuery.isPending;

  const isError =
    instrumentsQuery.isError ||
    gainsQuery.isError ||
    holdingsQuery.isError ||
    transactionsQuery.isError ||
    priceStatusQuery.isError;

  if (isPending) {
    return (
      <div className="board-state">
        <div className="skeleton-bar" />
        <div className="skeleton-bar" />
        <div className="skeleton-bar" />
      </div>
    );
  }

  if (isError) {
    return (
      <div className="board-state error">
        <p className="down">Could not load asset data.</p>
        <button
          type="button"
          className="button outline"
          onClick={() => {
            void instrumentsQuery.refetch();
            void gainsQuery.refetch();
            void holdingsQuery.refetch();
            void transactionsQuery.refetch();
            void priceStatusQuery.refetch();
          }}
        >
          Retry
        </button>
      </div>
    );
  }

  const data = deriveAssetData({
    id,
    instruments: instrumentsQuery.data ?? [],
    gainsRows: gainsQuery.data?.rows ?? [],
    holdings: holdingsQuery.data ?? [],
    transactions: transactionsQuery.data ?? [],
    priceStatus: priceStatusQuery.data?.instruments ?? [],
  });

  if (data.kind === "not-found") {
    return (
      <div className="board-state muted asset-not-found">
        <p>Asset not found.</p>
        <Link className="button outline" to="/">
          ← Back to board
        </Link>
      </div>
    );
  }

  const instruments = instrumentsQuery.data ?? [];
  const gain = data.kind === "position" ? data.gain : null;

  return (
    <article className="asset-page">
      <AssetHeader
        instrument={data.instrument}
        gain={gain}
        priceStatus={data.priceStatus}
      />

      {data.kind === "position" ? (
        <>
          <AssetMetricTiles
            tiles={tilesView(data.gain, data.holding, data.transactions)}
          />
          <ReservedChartBand />
          <div className="asset-two-col">
            <AssetGainsBreakdown breakdown={breakdownView(data.gain)} />
            <AssetDataMapping gain={data.gain} priceStatus={data.priceStatus} />
          </div>
        </>
      ) : (
        <>
          <ReservedChartBand />
          <AssetDataMapping gain={null} priceStatus={data.priceStatus} />
        </>
      )}

      <AssetTransactions
        transactions={data.transactions}
        instruments={instruments}
      />
    </article>
  );
}

function AssetHeader({
  instrument,
  gain,
  priceStatus,
}: {
  instrument: Instrument;
  gain: GainsRow | null;
  priceStatus: PriceStatusInstrument | null;
}) {
  const status = headerStatus(gain, priceStatus);
  const meta = [
    instrument.symbol,
    instrument.exchange,
    instrument.currency,
    instrument.type,
  ]
    .filter((part) => part && part.length > 0)
    .join(" · ");

  return (
    <header className="asset-header">
      <Link className="asset-back" to="/">
        ← Back to board
      </Link>
      <div className="asset-title-row">
        <h1>{instrument.name || instrument.symbol}</h1>
        <span
          className={
            status.tone === "warning" ? "status-chip warning" : "status-chip"
          }
        >
          {status.label}
        </span>
      </div>
      <p className="asset-meta">{meta}</p>
    </header>
  );
}

function AssetMetricTiles({ tiles }: { tiles: Tiles }) {
  if (tiles.status === "open") {
    return (
      <section className="metric-tiles" aria-label="Position metrics">
        <MetricTile label="Market value">
          <SummaryAvailabilityValue
            value={tiles.marketValue}
            prefix="SEK "
            tone="plain"
          />
        </MetricTile>
        <MetricTile label="Unrealized">
          <SummaryAvailabilityValue
            value={tiles.unrealizedGain}
            prefix="SEK "
            tone="signed"
          />{" "}
          <SummaryAvailabilityValue
            value={tiles.unrealizedPercent}
            suffix="%"
            tone="signed"
          />
        </MetricTile>
        <MetricTile label="Day change">
          <SummaryAvailabilityValue
            value={tiles.dayChange}
            prefix="SEK "
            tone="signed"
          />{" "}
          <SummaryAvailabilityValue
            value={tiles.dayChangePercent}
            suffix="%"
            tone="signed"
          />
        </MetricTile>
        <MetricTile label="Quantity">
          <span className="number">{formatGroupedNumber(tiles.quantity)}</span>
        </MetricTile>
        <MetricTile label="Avg cost">
          <SummaryAvailabilityValue
            value={tiles.averageCost}
            prefix="SEK "
            tone="plain"
          />
        </MetricTile>
        <MetricTile label="Cost basis">
          <SummaryAvailabilityValue
            value={tiles.costBasis}
            prefix="SEK "
            tone="plain"
          />
        </MetricTile>
      </section>
    );
  }

  return (
    <section className="metric-tiles" aria-label="Position metrics">
      <MetricTile label="Realized gain">
        <SummaryAvailabilityValue
          value={tiles.realizedGain}
          prefix="SEK "
          tone="signed"
        />{" "}
        <SummaryAvailabilityValue
          value={tiles.realizedPercent}
          suffix="%"
          tone="signed"
        />
      </MetricTile>
      <MetricTile label="Proceeds">
        <SummaryAvailabilityValue
          value={tiles.proceeds}
          prefix="SEK "
          tone="plain"
        />
      </MetricTile>
      <MetricTile label="Cost basis">
        <SummaryAvailabilityValue
          value={tiles.costBasis}
          prefix="SEK "
          tone="plain"
        />
      </MetricTile>
      <MetricTile label="Shares sold">
        <span className="number">{formatGroupedNumber(tiles.sharesSold)}</span>
      </MetricTile>
    </section>
  );
}

function MetricTile({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="metric-tile">
      <span className="metric-tile-label">{label}</span>
      <span className="metric-tile-value">{children}</span>
    </div>
  );
}

function ReservedChartBand() {
  return (
    <section className="chart-band" aria-label="Price chart placeholder">
      <span className="chart-band-label">Price chart — coming soon</span>
    </section>
  );
}

function AssetGainsBreakdown({ breakdown }: { breakdown: BreakdownView }) {
  return (
    <section className="panel asset-panel" aria-label="Gains breakdown">
      <h2>Gains breakdown</h2>
      <dl className="breakdown-list">
        <BreakdownRow
          label="Capital (price effect)"
          value={breakdown.priceEffect}
        />
        <BreakdownRow label="Currency (FX effect)" value={breakdown.fxEffect} />
        <BreakdownRow
          label={breakdown.totalLabel}
          value={breakdown.total}
          ruled
        />
      </dl>
    </section>
  );
}

function BreakdownRow({
  label,
  value,
  ruled = false,
}: {
  label: string;
  value: BreakdownView["total"];
  ruled?: boolean;
}) {
  return (
    <div className={ruled ? "breakdown-row ruled" : "breakdown-row"}>
      <dt>{label}</dt>
      <dd>
        <SummaryAvailabilityValue value={value} prefix="SEK " tone="signed" />
      </dd>
    </div>
  );
}

function AssetDataMapping({
  gain,
  priceStatus,
}: {
  gain: GainsRow | null;
  priceStatus: PriceStatusInstrument | null;
}) {
  return (
    <section className="panel asset-panel" aria-label="Data and mapping">
      <h2>Data &amp; mapping</h2>
      <dl className="data-list">
        {gain ? (
          <>
            <DataRow label="Latest price">{latestPriceContent(gain)}</DataRow>
            <DataRow label="Latest FX">{latestFxContent(gain)}</DataRow>
          </>
        ) : null}
        <DataRow label="Provider">{providerContent(priceStatus)}</DataRow>
      </dl>
    </section>
  );
}

function DataRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="data-row">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function latestPriceContent(gain: GainsRow) {
  const price = gain.latest_price;
  if (!price) {
    return <span className="status-chip warning">No live price</span>;
  }

  return (
    <span className="data-value">
      <span className="number">
        {price.currency} {formatGroupedNumber(price.close)}
      </span>{" "}
      <span
        className={
          freshnessTone(price.freshness) === "warning"
            ? "status-chip warning compact"
            : "status-chip compact"
        }
      >
        {freshnessLabel(price.freshness)}
      </span>
    </span>
  );
}

function latestFxContent(gain: GainsRow) {
  const fx = gain.latest_fx;
  if (!fx) {
    return <span className="asset-subtle">—</span>;
  }

  return (
    <span className="data-value">
      <span className="number">{formatGroupedNumber(fx.rate)}</span>{" "}
      <span className="asset-subtle">
        {fx.quote}→{fx.base} · {fx.date}
      </span>{" "}
      <span
        className={
          freshnessTone(fx.freshness) === "warning"
            ? "status-chip warning compact"
            : "status-chip compact"
        }
      >
        {freshnessLabel(fx.freshness)}
      </span>
    </span>
  );
}

function providerContent(priceStatus: PriceStatusInstrument | null) {
  if (!priceStatus) {
    return <span className="asset-subtle">—</span>;
  }

  if (!priceStatus.mapping_enabled) {
    return <span className="status-chip warning">Mapping disabled</span>;
  }

  if (
    priceStatus.provider_symbol === null ||
    priceStatus.latest_price.status === "unmapped"
  ) {
    return <span className="status-chip warning">Unmapped</span>;
  }

  const provider = priceStatus.latest_price.provider ?? "—";

  return (
    <span className="data-value">
      <span className="number">{priceStatus.provider_symbol}</span>{" "}
      <span className="asset-subtle">{provider}</span>
      {priceStatus.latest_price.status === "missing" ? (
        <span className="status-chip warning compact">Missing price</span>
      ) : null}
      {priceStatus.latest_fx.status === "missing" ? (
        <span className="status-chip warning compact">Missing FX</span>
      ) : null}
      {priceStatus.latest_fx.status === "unmapped" ? (
        <span className="status-chip warning compact">FX unmapped</span>
      ) : null}
    </span>
  );
}

function AssetTransactions({
  transactions,
  instruments,
}: {
  transactions: Transaction[];
  instruments: Instrument[];
}) {
  return (
    <section className="panel asset-panel" aria-label="Transactions">
      <h2>Transactions for this asset</h2>
      {transactions.length === 0 ? (
        <p className="board-state muted">
          No transactions recorded for this asset.
        </p>
      ) : (
        <TransactionsTable
          transactions={transactions}
          instruments={instruments}
          showToolbar={false}
          showActions={false}
        />
      )}
    </section>
  );
}
