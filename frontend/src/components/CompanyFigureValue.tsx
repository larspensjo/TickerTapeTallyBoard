import type { CompanyFigureView } from "./companyFiguresViewModel";

/** All text and certainty decisions come from the pure view model. */
export function CompanyFigureValue({
  value,
  label,
}: {
  value: CompanyFigureView;
  label: string;
}) {
  return (
    <span className="company-figure" title={value.title || undefined}>
      <span className="sr-only">
        {label}: {value.accessibleLabel}
      </span>
      <span
        className={`number ${value.tone}${value.notApplicable ? " not-applicable" : ""}`}
        aria-hidden="true"
      >
        {value.prefix}
        {value.text}
      </span>
      {value.tag ? (
        <span className="report-tag" aria-hidden="true">
          {value.tag}
        </span>
      ) : null}
      {value.hint ? (
        <span className="report-hint" aria-hidden="true">
          {value.hint}
        </span>
      ) : null}
    </span>
  );
}
