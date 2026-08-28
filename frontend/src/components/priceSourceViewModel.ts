import type { GainsRow, PriceStatusInstrument } from "../api/types";

type PriceSourceGain = Pick<GainsRow, "latest_price">;

export interface PriceSourceRow {
  provider: string;
  label: string;
  identifier: string;
  assetClass?: string;
  enabled: boolean;
  effective: boolean;
}

export function priceSourceLabel(code: string): string {
  switch (code) {
    case "YAHOO":
      return "Yahoo";
    case "NASDAQ_NORDIC":
      return "Nasdaq Nordic";
    case "MANUAL":
      return "Hand-entered";
    default:
      return code
        .toLowerCase()
        .split("_")
        .filter((part) => part.length > 0)
        .map((part) => `${part[0].toUpperCase()}${part.slice(1)}`)
        .join(" ");
  }
}

export function priceSourceRows(
  priceStatus: PriceStatusInstrument,
  gain: PriceSourceGain | null = null,
): PriceSourceRow[] {
  const effectiveSource = effectivePriceSourceCode(priceStatus, gain);

  return priceStatus.price_sources.map((source) => ({
    provider: source.provider,
    label: priceSourceLabel(source.provider),
    identifier: source.provider_symbol,
    ...(source.asset_class ? { assetClass: source.asset_class } : {}),
    enabled: source.enabled,
    effective: source.provider === effectiveSource,
  }));
}

export function effectivePriceSourceRow(
  priceStatus: PriceStatusInstrument | null,
  gain: PriceSourceGain | null,
): PriceSourceRow | null {
  if (!priceStatus) {
    return null;
  }

  return (
    priceSourceRows(priceStatus, gain).find((source) => source.effective) ??
    null
  );
}

export function priceAvailabilityLabel(
  priceStatus: null,
  gain: PriceSourceGain | null,
): null;
export function priceAvailabilityLabel(
  priceStatus: PriceStatusInstrument,
  gain: PriceSourceGain | null,
): string;

export function priceAvailabilityLabel(
  priceStatus: PriceStatusInstrument | null,
  gain: PriceSourceGain | null,
): string | null {
  if (!priceStatus) {
    return null;
  }

  if (priceStatus.price_sources.length === 0) {
    return "No price source";
  }

  if (!priceStatus.price_sources.some((source) => source.enabled)) {
    return "Price sources disabled";
  }

  return (
    effectivePriceSourceRow(priceStatus, gain)?.label ??
    "No effective price source"
  );
}

function effectivePriceSourceCode(
  priceStatus: PriceStatusInstrument,
  gain: PriceSourceGain | null,
): string | null {
  return gain?.latest_price?.source ?? priceStatus.effective_price_source;
}
