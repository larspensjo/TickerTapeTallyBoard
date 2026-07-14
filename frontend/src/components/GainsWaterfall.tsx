import type { ReactNode } from "react";
import { SummaryAvailabilityValue } from "./valuationDisplay";
import type {
  StackedSegment,
  WaterfallRow,
  WaterfallView,
} from "./waterfallViewModel";

function barGeometry(
  span: { from: number; to: number },
  minValue: number,
  maxValue: number,
): { left: number; width: number } {
  const domain = maxValue - minValue || 1;
  const lo = Math.min(span.from, span.to);
  const hi = Math.max(span.from, span.to);
  return {
    left: ((lo - minValue) / domain) * 100,
    width: Math.max(((hi - lo) / domain) * 100, 0.6),
  };
}

function barClass(row: WaterfallRow): string {
  if (row.kind === "base") return "wf-bar base";
  if (row.kind === "subtotal") return "wf-bar subtotal";
  if (row.kind === "total") return "wf-bar total";
  if (row.direction === "up") return "wf-bar up";
  if (row.direction === "down") return "wf-bar down";
  return "wf-bar flat";
}

function segmentClass(seg: StackedSegment): string {
  if (seg.direction === null) return "wf-bar base";
  if (seg.direction === "up") return "wf-bar up";
  if (seg.direction === "down") return "wf-bar down";
  return "wf-bar flat";
}

function Track({
  row,
  minValue,
  maxValue,
}: {
  row: WaterfallRow;
  minValue: number;
  maxValue: number;
}) {
  if (!row.span) {
    if (row.kind === "placeholder") {
      return <div className="wf-track" />;
    }
    return (
      <div className="wf-track">
        <div className="wf-bar unavailable" />
      </div>
    );
  }
  const { left, width } = barGeometry(row.span, minValue, maxValue);
  return (
    <div className="wf-track">
      <div
        className={barClass(row)}
        style={{ left: `${left}%`, width: `${width}%` }}
      />
    </div>
  );
}

function StackedTrack({
  row,
  minValue,
  maxValue,
}: {
  row: WaterfallRow;
  minValue: number;
  maxValue: number;
}) {
  if (!row.stackedSegments) {
    return <Track row={row} minValue={minValue} maxValue={maxValue} />;
  }
  return (
    <div className="wf-track">
      {row.stackedSegments.map((seg) => {
        const { left, width } = barGeometry(seg.span, minValue, maxValue);
        return (
          <div
            key={seg.key}
            className={segmentClass(seg)}
            style={{ left: `${left}%`, width: `${width}%` }}
          />
        );
      })}
    </div>
  );
}

function ValueCell({ row }: { row: WaterfallRow }) {
  if (row.kind === "placeholder") {
    return <span className="wf-placeholder">Not tracked yet</span>;
  }
  const tone =
    row.kind === "base" || row.kind === "subtotal" ? "plain" : "signed";
  return <SummaryAvailabilityValue value={row.value} tone={tone} />;
}

function PercentCell({ row }: { row: WaterfallRow }) {
  if (row.percent === null) {
    return <span className="wf-pct-empty" />;
  }
  if (row.percent.status !== "available") {
    return <span className="wf-pct muted">n/a</span>;
  }
  const sign = Number(row.percent.value) > 0 ? "+" : "";
  const tone =
    Number(row.percent.value) > 0
      ? "up"
      : Number(row.percent.value) < 0
        ? "down"
        : "flat";
  return (
    <span className={`wf-pct ${tone}`}>
      {sign}
      {row.percent.value}%
    </span>
  );
}

export function GainsWaterfall({
  view,
  title = "Gains breakdown",
  className = "panel asset-panel",
  headerRight,
}: {
  view: WaterfallView;
  title?: string;
  className?: string;
  headerRight?: ReactNode;
}) {
  const rootClassName = [className, "gains-waterfall"]
    .filter(Boolean)
    .join(" ");
  return (
    <section className={rootClassName} aria-label={title}>
      <div className="gains-waterfall-header">
        <h2>{title}</h2>
        {headerRight ? (
          <div className="gains-waterfall-header-right">{headerRight}</div>
        ) : null}
      </div>
      <div className="wf-head">
        <span className="wf-col-amount">{view.currency}</span>
        <span className="wf-col-pct">% of cost</span>
      </div>
      <div className="wf-rows">
        {view.rows.map((row) => (
          <div
            key={row.key}
            className={`wf-row kind-${row.kind}${row.kind === "placeholder" ? " is-muted" : ""}`}
          >
            <span className="wf-label">{row.label}</span>
            {row.kind === "total" ? (
              <StackedTrack
                row={row}
                minValue={view.minValue}
                maxValue={view.maxValue}
              />
            ) : (
              <Track
                row={row}
                minValue={view.minValue}
                maxValue={view.maxValue}
              />
            )}
            <span className="wf-value">
              <ValueCell row={row} />
            </span>
            <PercentCell row={row} />
          </div>
        ))}
      </div>
    </section>
  );
}
