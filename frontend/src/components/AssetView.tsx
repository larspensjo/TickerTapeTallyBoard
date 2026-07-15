import { type ReactNode, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  useDeleteInstrument,
  useGains,
  useHoldings,
  useInstrumentPrices,
  useInstruments,
  usePriceStatus,
  useTransactions,
  useUpdateInstrumentConviction,
} from "../api/queries";
import type {
  Conviction,
  ConvictionTarget,
  GainsRow,
  Holding,
  Instrument,
  PriceStatusInstrument,
  Transaction,
} from "../api/types";
import { AsyncBoundary } from "./AsyncBoundary";
import {
  canDeleteInstrument,
  convictionPanelView,
  convictionResetVisible,
  costBasisLineValue,
  deleteInstrumentDisabledReason,
  deriveAssetData,
  headerStatus,
  parseInstrumentId,
  type SplitEvent,
  splitEvents,
  type Tiles,
  tilesView,
} from "./assetViewModel";
import { GainsWaterfall } from "./GainsWaterfall";
import {
  CONVICTION_OPTIONS,
  convictionLabel,
  isTargetAlert,
  targetStatusLabel,
} from "./holdingsConviction";
import {
  instrumentPriceSeries,
  tradeMarkers,
} from "./instrumentChartViewModel";
import { type ChartTradeMarker, TimeSeriesChart } from "./TimeSeriesChart";
import { TransactionsTable } from "./TransactionsTable";
import { useAppMode } from "./useAppMode";
import {
  formatGroupedNumber,
  freshnessLabel,
  freshnessTone,
  reasonSummary,
  SummaryAvailabilityValue,
  signedTone,
} from "./valuationDisplay";
import { waterfallView } from "./waterfallViewModel";

export function AssetView() {
  const { id: idParam } = useParams();
  const id = parseInstrumentId(idParam);
  const navigate = useNavigate();

  const instrumentsQuery = useInstruments();
  // method is intentionally unset: the asset page shows method-independent current-position
  // row data (unrealized gain ÷ cost basis). If it later needs a method-dependent portfolio
  // total, share the persisted loadReturnMethod() helper from GainsTable.
  const gainsQuery = useGains({ includeClosedPositions: true });
  const holdingsQuery = useHoldings();
  const transactionsQuery = useTransactions();
  const priceStatusQuery = usePriceStatus();
  const pricesQuery = useInstrumentPrices(id);
  const deleteInstrument = useDeleteInstrument();

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
    return <AsyncBoundary isPending />;
  }

  if (isError) {
    return (
      <AsyncBoundary
        isError
        errorMessage="Could not load asset data."
        onRetry={() => {
          void instrumentsQuery.refetch();
          void gainsQuery.refetch();
          void holdingsQuery.refetch();
          void transactionsQuery.refetch();
          void priceStatusQuery.refetch();
        }}
      />
    );
  }

  const data = deriveAssetData({
    id,
    instruments: instrumentsQuery.data ?? [],
    gainsRows: gainsQuery.data?.rows ?? [],
    holdings: holdingsQuery.data?.holdings ?? [],
    transactions: transactionsQuery.data ?? [],
    priceStatus: priceStatusQuery.data?.instruments ?? [],
  });

  if (data.kind === "not-found") {
    return (
      <div className="board-state muted asset-not-found">
        <p>Asset not found.</p>
        <Link className="button outline" to="/holdings">
          ← Back to holdings
        </Link>
      </div>
    );
  }

  const instrument = data.instrument;
  const instruments = instrumentsQuery.data ?? [];
  const gain = data.kind === "position" ? data.gain : null;
  const holding = data.kind === "position" ? data.holding : null;
  const splits = splitEvents(data.transactions);
  const canDelete = canDeleteInstrument({
    holding,
    transactions: data.transactions,
  });
  const deleteDisabledReason = deleteInstrumentDisabledReason({
    holding,
    transactions: data.transactions,
  });

  async function handleDeleteInstrument() {
    if (!canDelete) {
      return;
    }
    if (
      !window.confirm(
        "Delete this never-traded instrument? This cannot be undone.",
      )
    ) {
      return;
    }

    try {
      await deleteInstrument.mutateAsync(instrument.id);
      navigate("/holdings");
    } catch {
      // Mutation state renders the error message below the header.
    }
  }

  return (
    <article className="asset-page">
      <AssetHeader
        instrument={instrument}
        gain={gain}
        priceStatus={data.priceStatus}
        canDelete={canDelete}
        deleteDisabledReason={deleteDisabledReason}
        deletePending={deleteInstrument.isPending}
        onDelete={handleDeleteInstrument}
      />

      {data.kind === "position" ? (
        <>
          <AssetMetricTiles
            tiles={tilesView(data.gain, data.holding, data.transactions)}
          />
          <AssetPriceChart
            query={pricesQuery}
            transactions={data.transactions}
            costBasisLine={costBasisLineValue(data.holding)}
          />
          <div className="asset-two-col">
            <GainsWaterfall
              view={waterfallView(data.gain)}
              title="Gains breakdown"
              className="panel asset-panel"
            />
            <AssetDataMapping
              key={instrument.id}
              instrument={instrument}
              holding={data.holding}
              gain={data.gain}
              priceStatus={data.priceStatus}
            />
            <AssetSplits events={splits} />
          </div>
        </>
      ) : (
        <>
          <AssetPriceChart
            query={pricesQuery}
            transactions={data.transactions}
          />
          <AssetDataMapping
            key={instrument.id}
            instrument={instrument}
            holding={null}
            gain={null}
            priceStatus={data.priceStatus}
          />
          <AssetSplits events={splits} />
        </>
      )}

      <AssetTransactions
        transactions={data.transactions}
        instruments={instruments}
      />
      {deleteInstrument.isError ? (
        <p className="asset-subtle down" role="alert">
          Could not delete instrument: {deleteInstrument.error.message}
        </p>
      ) : null}
    </article>
  );
}

function AssetHeader({
  instrument,
  gain,
  priceStatus,
  canDelete,
  deleteDisabledReason,
  deletePending,
  onDelete,
}: {
  instrument: Instrument;
  gain: GainsRow | null;
  priceStatus: PriceStatusInstrument | null;
  canDelete: boolean;
  deleteDisabledReason: string | null;
  deletePending: boolean;
  onDelete: () => void;
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
      <Link className="asset-back" to="/holdings">
        ← Back to holdings
      </Link>
      <div className="asset-title-row">
        <h1>{instrument.name || instrument.symbol}</h1>
        <div className="asset-header-actions">
          <span
            className={
              status.tone === "warning" ? "status-chip warning" : "status-chip"
            }
          >
            {status.label}
          </span>
          <button
            type="button"
            className="button outline danger"
            disabled={!canDelete || deletePending}
            title={
              canDelete ? "Delete instrument" : (deleteDisabledReason ?? "")
            }
            onClick={onDelete}
          >
            {deletePending ? "Deleting..." : "Delete instrument"}
          </button>
        </div>
      </div>
      <p className="asset-meta">{meta}</p>
    </header>
  );
}

function AssetConvictionSection({
  instrument,
  holding,
}: {
  instrument: Instrument;
  holding: Holding | null;
}) {
  const { canMutate } = useAppMode();
  const save = useUpdateInstrumentConviction();
  const { conviction, target } = convictionPanelView(instrument, holding);

  // The merged details panel remounts per instrument id (key at the call site), so this
  // captures the navigation-time conviction once and keeps it across saves as
  // the reset baseline.
  const [baseline] = useState<Conviction>(conviction);
  const showReset = convictionResetVisible(baseline, conviction);

  const disabled = !canMutate || save.isPending;
  // Show the in-flight selection immediately; the select is otherwise bound to
  // the saved value, which only updates once the instruments refetch lands.
  const shownConviction =
    save.isPending && save.variables ? save.variables.conviction : conviction;

  return (
    <>
      <div className="conviction-controls">
        <label className="conviction-field">
          <span className="conviction-field-label">Conviction</span>
          <select
            className="conviction-select"
            value={shownConviction}
            disabled={disabled}
            aria-label={`Conviction for ${instrument.symbol}`}
            onChange={(event) =>
              save.mutate({
                instrumentId: instrument.id,
                conviction: event.target.value as Conviction,
              })
            }
          >
            {CONVICTION_OPTIONS.map((option) => (
              <option key={option} value={option}>
                {convictionLabel(option)}
              </option>
            ))}
          </select>
        </label>
        {showReset ? (
          <button
            type="button"
            className="button secondary"
            disabled={disabled}
            onClick={() =>
              save.mutate({
                instrumentId: instrument.id,
                conviction: baseline,
              })
            }
          >
            Reset
          </button>
        ) : null}
      </div>
      {save.isError ? (
        <p className="asset-subtle down" role="alert">
          Could not save conviction: {save.error.message}
        </p>
      ) : null}
      <AssetConvictionTarget target={target} />
    </>
  );
}

function AssetConvictionTarget({
  target,
}: {
  target: ConvictionTarget | null;
}) {
  if (!target || target.status === "no_target") {
    return <p className="asset-subtle">No current target</p>;
  }

  if (isTargetAlert(target.status)) {
    const reasons =
      target.target_value_base.status === "unavailable"
        ? target.target_value_base.reasons
        : [];
    return (
      <p className="asset-subtle">
        <span
          className="status-chip warning"
          title={reasons.length > 0 ? reasonSummary(reasons) : undefined}
        >
          {targetStatusLabel(target.status)}
        </span>{" "}
        Target unavailable
      </p>
    );
  }

  const gap = target.target_gap_base;
  const gapPercent = target.target_gap_percent;
  // Target gaps use plain sign colouring: above target (appreciated more than
  // its peers) is green, below is red — same convention as the Holdings bar.
  const gapTone = gap.status === "available" ? signedTone(gap.value) : "flat";

  return (
    <dl className="data-list">
      <DataRow label="Target value">
        <SummaryAvailabilityValue
          value={target.target_value_base}
          prefix="SEK "
          tone="plain"
        />
      </DataRow>
      <DataRow label="Target gap">
        {gap.status === "available" ? (
          <span className="data-value">
            <span className={`number ${gapTone}`}>
              SEK {formatGroupedNumber(gap.value)}
            </span>{" "}
            {gapPercent.status === "available" ? (
              <span className={`number ${gapTone}`}>
                ({formatGroupedNumber(gapPercent.value)}%)
              </span>
            ) : null}
          </span>
        ) : (
          <span className="asset-subtle">—</span>
        )}
      </DataRow>
      <DataRow label="Status">
        <span className="status-chip">{targetStatusLabel(target.status)}</span>
      </DataRow>
    </dl>
  );
}

function AssetMetricTiles({ tiles }: { tiles: Tiles }) {
  if (tiles.status === "open") {
    return (
      <section className="metric-tiles" aria-label="Position metrics">
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

function AssetPriceChart({
  query,
  transactions,
  costBasisLine,
}: {
  query: ReturnType<typeof useInstrumentPrices>;
  transactions: Transaction[];
  costBasisLine?: number;
}) {
  const series = useMemo(
    () => instrumentPriceSeries(query.data, transactions),
    [query.data, transactions],
  );
  const visibleStart = useMemo(
    () => firstTransactionDate(transactions),
    [transactions],
  );
  const currency = query.data?.currency;
  const markers = useMemo(
    () =>
      currency
        ? [
            ...tradeChartMarkers(transactions, currency),
            ...splitChartMarkers(splitEvents(transactions)),
          ]
        : [],
    [transactions, currency],
  );

  if (query.isPending) {
    return (
      <section className="chart-band" aria-label="Price chart">
        <div className="skeleton-bar" />
      </section>
    );
  }

  if (query.isError) {
    return (
      <section className="chart-band error" aria-label="Price chart">
        <p className="down">Could not load price history.</p>
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

  if (series.points.length === 0) {
    return (
      <section className="chart-band muted" aria-label="Price chart">
        <span className="chart-band-label">
          No price history yet — refresh prices
        </span>
      </section>
    );
  }

  return (
    <section className="panel chart-panel" aria-label="Price chart">
      <div className="chart-meta">
        <h2>Price history ({query.data?.currency ?? "native"})</h2>
      </div>
      <TimeSeriesChart
        data={series.points}
        ariaLabel={`Instrument price history in ${query.data?.currency ?? "native currency"}`}
        visibleStart={visibleStart}
        markers={markers}
        costBasisLine={costBasisLine}
      />
    </section>
  );
}

function tradeChartMarkers(
  transactions: Transaction[],
  currency: string,
): ChartTradeMarker[] {
  return tradeMarkers(transactions, currency).map((marker) => {
    const rows = [
      { label: "Date", value: marker.time },
      {
        label: "Quantity",
        value: formatGroupedNumber(String(marker.quantity)),
      },
      {
        label: "Avg price",
        value: `${formatGroupedNumber(marker.avgPrice.toFixed(2))} ${marker.currency}`,
      },
    ];
    if (marker.fee !== null && marker.feeCurrency !== null) {
      rows.push({
        label: "Fee",
        value: `${formatGroupedNumber(marker.fee.toFixed(2))} ${marker.feeCurrency}`,
      });
    }

    return {
      time: marker.time,
      side: marker.side,
      price: marker.avgPrice,
      title: marker.side === "buy" ? "Buy" : "Sell",
      rows,
    };
  });
}

function splitChartMarkers(events: SplitEvent[]): ChartTradeMarker[] {
  return events.map((event) => ({
    time: event.tradeDate,
    side: "split",
    title: "Split",
    rows: [
      { label: "Date", value: event.tradeDate },
      { label: "Ratio", value: event.ratioLabel },
      {
        label: "Before",
        value: formatGroupedNumber(event.beforeQuantity),
      },
      {
        label: "After",
        value: formatGroupedNumber(event.afterQuantity),
      },
      {
        label: "Delta",
        value: formatSignedQuantity(event.quantityDelta),
      },
    ],
  }));
}

function firstTransactionDate(transactions: Transaction[]): string | undefined {
  return transactions.reduce<string | undefined>((firstDate, transaction) => {
    if (firstDate === undefined || transaction.trade_date < firstDate) {
      return transaction.trade_date;
    }

    return firstDate;
  }, undefined);
}

function AssetSplits({ events }: { events: SplitEvent[] }) {
  if (events.length === 0) {
    return null;
  }

  const latest = events[events.length - 1];
  const totalDelta = events.reduce(
    (sum, event) => sum + event.quantityDelta,
    0,
  );

  return (
    <section className="panel asset-panel asset-splits" aria-label="Splits">
      <h2>Splits</h2>
      <dl className="data-list split-summary">
        <DataRow label="Count">
          <span className="number">{formatGroupedNumber(events.length)}</span>
        </DataRow>
        <DataRow label="Latest">
          <span className="data-value">
            <span className="number">{latest.ratioLabel}</span>
            <span className="asset-subtle">{latest.tradeDate}</span>
          </span>
        </DataRow>
        <DataRow label="Net delta">
          <span className="number">{formatSignedQuantity(totalDelta)}</span>
        </DataRow>
      </dl>
      <div className="split-event-list">
        {events.map((event) => (
          <article className="split-event" key={event.id}>
            <div>
              <span className="split-date number">{event.tradeDate}</span>
              <span className="asset-subtle">
                {formatGroupedNumber(event.beforeQuantity)} →{" "}
                {formatGroupedNumber(event.afterQuantity)}
              </span>
            </div>
            <div className="split-event-metrics">
              <span className="status-chip compact">{event.ratioLabel}</span>
              <span className="number">
                {formatSignedQuantity(event.quantityDelta)}
              </span>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function AssetDataMapping({
  instrument,
  holding,
  gain,
  priceStatus,
}: {
  instrument: Instrument;
  holding: Holding | null;
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
      <div className="asset-panel-section">
        <h3>Conviction</h3>
        <AssetConvictionSection instrument={instrument} holding={holding} />
      </div>
    </section>
  );
}

function formatSignedQuantity(value: number): string {
  const formatted = formatGroupedNumber(Math.abs(value));
  if (value > 0) return `+${formatted}`;
  if (value < 0) return `-${formatted}`;
  return formatted;
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
