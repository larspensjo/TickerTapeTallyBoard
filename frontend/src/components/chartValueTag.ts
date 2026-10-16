import type {
  ISeriesApi,
  ISeriesPrimitive,
  ISeriesPrimitiveAxisView,
  SeriesAttachedParameter,
  SeriesType,
  Time,
} from "lightweight-charts";

/**
 * A right-axis tag whose text is supplied verbatim, for a value the library
 * must not format itself. A series' own last-value tag takes its text from the
 * shared price scale's formatter, which rounds half-up and so can disagree with
 * a backend display string at a midpoint (112.125 shown as 112.13 where the
 * backend says 112.12). This primitive draws the tag at the value's level with
 * the given text instead.
 *
 * Kept free of runtime `lightweight-charts` imports so it can be unit-tested
 * against a stand-in series.
 */
export function fixedTextValueTag({
  value,
  text,
  color,
  textColor,
}: {
  value: number;
  text: string;
  color: string;
  textColor: string;
}): ISeriesPrimitive<Time> {
  let series: ISeriesApi<SeriesType, Time> | null = null;
  let coordinate: number | null = null;

  const refresh = () => {
    coordinate = series?.priceToCoordinate(value) ?? null;
  };

  const axisView: ISeriesPrimitiveAxisView = {
    coordinate: () => coordinate ?? 0,
    text: () => text,
    textColor: () => textColor,
    backColor: () => color,
    visible: () => coordinate !== null,
  };

  return {
    attached(param: SeriesAttachedParameter<Time>) {
      series = param.series;
      refresh();
    },
    detached() {
      series = null;
      coordinate = null;
    },
    updateAllViews: refresh,
    priceAxisViews: () => [axisView],
  };
}
