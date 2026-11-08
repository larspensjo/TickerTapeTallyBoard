import type { InstrumentFundamentals } from "../api/types";
import { CompanyFigureValue } from "./CompanyFigureValue";
import { companyFiguresPanelView } from "./companyFiguresPanelViewModel";
import { QuarterBarChart } from "./QuarterBarChart";

export function CompanyFiguresPanel({
  data,
}: {
  data: InstrumentFundamentals;
}) {
  const view = companyFiguresPanelView(data);
  return (
    <section
      className="panel asset-panel"
      aria-label="How the company is doing"
    >
      <h2>How the company is doing</h2>
      {view.awaiting ? (
        <p className="asset-subtle" role="status">
          {view.awaiting}
        </p>
      ) : null}
      <dl className="company-figure-tiles">
        {view.figures.map(({ label, value }) => (
          <div key={label}>
            <dt className="company-figure-label">{label}</dt>
            <dd>
              <CompanyFigureValue value={value} label={label} />
            </dd>
          </div>
        ))}
      </dl>
      {view.fxNote ? (
        <p className="asset-subtle number">{view.fxNote}</p>
      ) : null}
      <dl className="company-report-dates">
        <div>
          <dt className="company-figure-label">Next report</dt>
          <dd>
            <CompanyFigureValue value={view.nextReport} label="Next report" />
          </dd>
        </div>
        <div>
          <dt className="company-figure-label">Last report</dt>
          <dd>
            <CompanyFigureValue value={view.lastReport} label="Last report" />
            {view.observedWindow ? (
              <span className="asset-subtle number">{view.observedWindow}</span>
            ) : null}
            {view.lastPeriod ? (
              <span className="asset-subtle number">{view.lastPeriod}</span>
            ) : null}
          </dd>
        </div>
      </dl>
      {data.quarters.length > 0 ? (
        <div className="company-quarter-charts">
          <QuarterBarChart title="Sales" chart={view.sales} />
          <QuarterBarChart title="EPS" chart={view.eps} />
        </div>
      ) : null}
    </section>
  );
}
