import type { TargetGapBar } from "./holdingsConviction";

/** The diverging target-gap bar, labelled by its tooltip for assistive tech. */
export function TargetGapBarView({ bar }: { bar: TargetGapBar }) {
  return (
    <div
      className="target-gap-track"
      role="img"
      aria-label={bar.tooltip}
      title={bar.tooltip}
    >
      <span className="target-gap-axis" />
      {bar.side !== "on_target" ? (
        <span
          className={`target-gap-fill ${bar.side}`}
          style={{ width: `${bar.widthPercent}%` }}
        />
      ) : null}
    </div>
  );
}
