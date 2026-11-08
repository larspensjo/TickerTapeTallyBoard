import type { CSSProperties } from "react";
import type { quarterChartView } from "./companyFiguresPanelViewModel";

export function QuarterBarChart({
  title,
  chart,
}: {
  title: string;
  chart: ReturnType<typeof quarterChartView>;
}) {
  return (
    <section className="quarter-chart" aria-label={title}>
      <h3>{title} per quarter (report currency)</h3>
      <div
        className="quarter-chart-grid"
        style={{ "--quarters": chart.slotCount } as CSSProperties}
      >
        {chart.bars.map((bar) => (
          <div className="quarter-slot" key={bar.periodEnd}>
            <div
              className="quarter-plot"
              role="img"
              aria-label={bar.label}
              title={[bar.label, bar.title].filter(Boolean).join("; ")}
            >
              <span
                className="quarter-axis"
                style={{ bottom: `${chart.axis}%` }}
              />
              <span
                className={`quarter-bar${bar.missing ? " missing" : ""}${bar.negative ? " negative" : ""}`}
                style={{ height: `${bar.height}%`, bottom: `${bar.bottom}%` }}
              />
            </div>
            <span className="quarter-value number" title={bar.text}>
              {bar.text}
            </span>
            <span className="quarter-date number" title={bar.periodEnd}>
              {bar.dateText}
            </span>
          </div>
        ))}
      </div>
    </section>
  );
}
