import type { CSSProperties, ReactNode } from "react";
import {
  formatGroupedNumber,
  SummaryAvailabilityValue,
} from "./valuationDisplay";
import type {
  CapitalStack,
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

type CapitalTrackStyle = CSSProperties & {
  "--wf-capital-held-height": string;
  "--wf-capital-sold-height": string;
  "--wf-capital-total-height": string;
};

const BAR_HEIGHT_PX = 16;

function capitalDescription(stack: CapitalStack): string {
  const description = `Capital deployed: SEK ${formatGroupedNumber(stack.held)} held and SEK ${formatGroupedNumber(stack.sold)} previously sold.`;
  return stack.isBroken
    ? `${description} The sold-capital layer is shortened with a visual break.`
    : description;
}

function CapitalBase({
  stack,
  segment,
  minValue,
  maxValue,
}: {
  stack: CapitalStack;
  segment: StackedSegment;
  minValue: number;
  maxValue: number;
}) {
  const { left, width } = barGeometry(segment.span, minValue, maxValue);
  const description = capitalDescription(stack);
  const soldMultiple = stack.sold / stack.held;
  return (
    <div
      className="wf-capital-base"
      style={{ left: `${left}%`, width: `${width}%` }}
      role="img"
      aria-label={description}
      title={description}
      data-capital-stack="true"
      data-broken={stack.isBroken ? "true" : "false"}
    >
      <div className="wf-capital-layer held">
        <span className="wf-capital-layer-label" aria-hidden="true">
          held
        </span>
      </div>
      <div
        className={`wf-capital-layer sold${stack.isBroken ? " is-broken" : ""}`}
      >
        {stack.isBroken ? (
          <>
            <span className="wf-capital-overflow-label">
              sold ×{soldMultiple.toFixed(1)}
            </span>
            <span className="wf-capital-break" aria-hidden="true" />
          </>
        ) : stack.displayedSoldUnits >= 0.7 ? (
          <span className="wf-capital-layer-label" aria-hidden="true">
            sold
          </span>
        ) : null}
      </div>
    </div>
  );
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
  const stack = row.capitalStack;
  const trackStyle = stack
    ? ({
        "--wf-capital-held-height": `${stack.heldUnits * BAR_HEIGHT_PX}px`,
        "--wf-capital-sold-height": `${stack.displayedSoldUnits * BAR_HEIGHT_PX}px`,
        "--wf-capital-total-height": `${(stack.heldUnits + stack.displayedSoldUnits) * BAR_HEIGHT_PX}px`,
      } as CapitalTrackStyle)
    : undefined;
  return (
    <div
      className={`wf-track${stack ? " wf-capital-track" : ""}`}
      style={trackStyle}
    >
      {row.stackedSegments.map((seg) => {
        if (seg.direction === null && stack) {
          return (
            <CapitalBase
              key={seg.key}
              stack={stack}
              segment={seg}
              minValue={minValue}
              maxValue={maxValue}
            />
          );
        }
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
            className={`wf-row kind-${row.kind}${row.kind === "placeholder" ? " is-muted" : ""}${row.capitalStack ? " has-capital-stack" : ""}`}
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
