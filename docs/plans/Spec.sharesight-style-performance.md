# Spec - Sharesight-Style Performance Returns

**Status:** Draft design · **Date:** 2026-06-19 · **Owner:** Lars

## Summary

The Gains page should compute percentage returns in the same conceptual way as
Sharesight's Performance Report, not as the current simple
`gain / cost_basis` ratio.

The current app now reconciles the open+closed **amounts** closely after realized
sell gains from partially open instruments were included, but the percentages still
diverge because the app divides by accumulated weighted-average cost basis. Sharesight
documents its percentages as dollar-weighted / money-weighted performance returns,
using a variation of Modified Dietz, with date-range semantics and annualisation
rules.

This spec defines a phased path to make TickerTapeTallyBoard's Gains totals and rows
Sharesight-compatible enough to reconcile against the real portfolio.

## Goals

- Replace Gains percentage totals with Sharesight-style performance percentages.
- Add explicit report-date range semantics to Gains.
- Use report-start and report-end market values for positions that already existed
  before the range.
- Include realized gains from sells inside the range, including partial sells from
  instruments that remain open.
- Track income/dividends so total return can match Sharesight's capital + income +
  currency components.
- Keep cost-basis analytics available, but do not present them as Sharesight-style
  return percentages.
- Keep the performance engine pure and unit-testable.

## Non-goals

- Do not implement tax reporting or CGT-lot accounting.
- Do not change the existing weighted-average position/cost-basis model for holdings.
- Do not add cash-account balances in the first performance implementation.
- Do not promise byte-for-byte identity with proprietary Sharesight internals before
  calibration against exported report rows.

## Source Alignment

Sharesight public documentation establishes these requirements:

- Performance percentages are dollar-weighted / money-weighted and consider the size
  and timing of cash flows.
- Sharesight uses a variation of Modified Dietz rather than a simple cost-basis
  percentage.
- Custom date ranges use market prices at the report start and end; purchase price is
  not always the denominator.
- The Performance Report can include open and closed positions, including sales.
- Percentage Gains in the Performance Report are annualised percentage returns.

Reference URLs:

- <https://help.sharesight.com/performance_calculation_method/>
- <https://help.sharesight.com/performance_report/>

## Current Behavior

The backend currently computes Gains totals by summing available row amounts:

- `capital_gain_base`
- `currency_gain_base`
- `total_return_base`

Then it computes percentage fields as:

```text
percentage = amount / summed_cost_basis_base
```

That is a cost-basis analytics ratio. It is useful, but it is not a performance
return and should not be compared with Sharesight's Performance Report percentages.

## Target Model

Introduce a new pure domain module, tentatively:

```text
backend/src/domain/performance.rs
```

The module computes a report for:

```text
PerformanceInput {
  start_date,
  end_date,
  include_closed_positions,
  instruments,
  transactions,
  start_prices,
  end_prices,
  start_fx,
  end_fx,
}
```

`start_fx` and `end_fx` are in the app's canonical direction: SEK per one native
currency unit, for example USD -> SEK. This matches `fx_rates` and `fx_rate_to_base`.
Sharesight CSV exchange rates are inverted during import and must not be inverted
again in the performance engine.

The output has amount components and performance percentages:

```text
PerformanceReport {
  totals,
  rows,
  excluded_rows,
}

PerformanceTotals {
  capital_gain_base,
  income_base,
  currency_gain_base,
  total_return_base,
  capital_gain_percent,
  income_percent,
  currency_gain_percent,
  total_return_percent,
}
```

### Report Period Semantics

For each instrument, reconstruct the position at:

- `start_date` close, using transactions up to and including the report start
  boundary according to the chosen boundary rule.
- `end_date` close, using transactions up to and including the report end boundary.

Open question: Sharesight's exact trade-date boundary for start-day trades must be
confirmed against an export. The implementation should choose one rule explicitly and
test it:

```text
initial recommendation:
start_position = ledger after transactions strictly before start_date
period_flows   = transactions from start_date through end_date inclusive
end_position   = ledger after transactions through end_date inclusive
```

This matches the intuition that a buy on the start date is a period cash flow, not an
opening balance. Verify against Sharesight before finalising.

### Market Values

For an open quantity at the report start:

```text
begin_market_value_base = start_quantity * start_price_native * start_fx
```

For an open quantity at the report end:

```text
end_market_value_base = end_quantity * end_price_native * end_fx
```

Missing start/end price or FX makes the performance percentage unavailable for that
instrument unless the missing value is irrelevant because quantity is zero at that
boundary. Missing data remains explicit and is never coerced to zero.

For historical `end_date` values, freshness is relative to the report end date, not
today. A close/rate from the last market day on or before a historical end date is not
a stale current quote. For `end_date = today`, the existing launch-refresh and
staleness semantics still apply.

### Amount Components

For the report period, amount components should continue to decompose total return:

```text
total_return_base = capital_gain_base + income_base + currency_gain_base
```

Capital and currency components must use the existing native/FX split convention:

- Native price movement, including the cross-term, is capital/price effect.
- Currency movement is the FX effect.
- SEK brokerage and fees are assigned to the capital/price leg, not FX.

This requires a **new period attribution function**. The existing open-position
attribution in `valuation.rs` is a conceptual reference only; it is not a drop-in
because it baselines on purchase cost. A report-period return must baseline on
start-of-period market value for shares already held at `start_date`.

For a quantity held from the beginning to the end of the period:

```text
begin_native_value = quantity * start_price_native
end_native_value   = quantity * end_price_native

capital_effect_base  = (end_native_value - begin_native_value) * end_fx
currency_effect_base = begin_native_value * (end_fx - start_fx)

capital_effect_base + currency_effect_base
  = end_native_value * end_fx - begin_native_value * start_fx
```

This keeps the existing convention that the native-price move is valued at the ending
FX rate, so the cross-term lands in the capital/price leg and the FX leg is pure
currency movement.

In-period buys and sells are attributed from their actual trade price, trade FX, and
brokerage. Only fees on in-period transactions affect the report-period amount. A fee
paid before `start_date` is already embedded in the opening market value/cost history
and must not reduce the current period's capital gain again.

The period reconstruction must emit the begin position, end position, period cash
flows, and period realized components. Do not use `derive_position_performance`
directly for period performance because it accumulates realized gain over the whole
ledger.

### Splits And Adjusted Prices

The ledger stores splits as explicit quantity deltas, while Yahoo historical prices are
typically split-adjusted. A report range that crosses a split can double-count or
mis-scale value if pre-split ledger quantity is multiplied by split-adjusted prices.

The implementation must choose and test one convention before period valuation ships:

- preferred: convert reconstructed quantities to the same split-adjusted share basis as
  the provider price series before multiplying by provider prices; or
- fallback: store/use unadjusted prices for period performance.

Whichever convention is chosen, add a calibration test for an instrument with a split
inside the report range. Quantity-only split tests are not sufficient.

### Cash Flows For Performance Percent

For Modified Dietz style returns, define cash flows at the security/report level:

```text
buy      = contribution into the instrument
sell     = withdrawal from the instrument
dividend = income return, not a contribution
fee      = included in the relevant buy/sell cash flow or return component
```

The basic holding-period return form is:

```text
return = end_market_value - begin_market_value - net_external_cash_flows
denominator = begin_market_value + sum(weight_i * cash_flow_i)
return_percent = return / denominator
```

Where each cash-flow weight reflects how long the cash was invested in the period:

```text
weight_i = days_remaining_after_flow / total_period_days
```

Use calendar days first, because Sharesight's public docs do not specify trading-day
weights for Modified Dietz. Add a calibration test if Sharesight exports imply a
different convention.

Open question: Sharesight distinguishes fresh and recycled capital. The first
implementation can approximate by treating all buys as positive cash flows and all
sells as negative cash flows per instrument. Portfolio-level reconciliation may need a
later pass that nets same-period sale proceeds reinvested into other holdings.

Dividends are not external cash flows for return-denominator purposes. They add to the
income numerator, contribute to total return, and do not change quantity or cost basis.
This matches the current domain treatment of dividend rows as position no-ops, but the
write/API path currently rejects Dividend creation; Phase 5 must add that write path
before income can reconcile with Sharesight.

### Annualisation

The backend should return both:

```text
holding_period_percent
display_percent
display_percent_kind = "absolute" | "annualised"
```

Initial rule:

```text
if average_years_invested >= 1:
  display_percent = annualise(holding_period_percent, average_years_invested)
else:
  display_percent = holding_period_percent
```

The exact `average_years_invested` rule is an open calibration item. Sharesight says
returns under one year of Average Years Invested are not annualised, and once Average
Years Invested reaches one year or more it annualises. Use a simple weighted-capital
duration first, then compare with Sharesight.

Guard the annualisation formula:

- If `years <= 0`, return the holding-period percentage.
- If `1 + holding_period_return <= 0`, return the holding-period percentage with a
  non-annualised kind, or mark the annualised value unavailable with
  `annualisation_undefined`. Do not take a fractional power of a negative number.

### Component Percentages

Sharesight displays percentages for capital gain, income, currency gain, and total
return. Component annualisation must be handled carefully because annualisation is
non-linear: annualised capital + annualised income + annualised currency will not equal
annualised total return.

Initial rule:

```text
capital_gain_percent  = capital_gain_base / performance_denominator
income_percent        = income_base / performance_denominator
currency_gain_percent = currency_gain_base / performance_denominator
total_return_percent  = annualise_if_needed(total_return_base / performance_denominator)
```

The denominator is the same performance denominator for all four components. The three
component percentages are holding-period contribution percentages; only the headline
total return percentage may be annualised. The UI must label this so users do not
expect component percentages to sum to an annualised total.

Before locking this rule, calibrate it against Sharesight. If Sharesight annualises
component percentages separately, document the mismatch explicitly rather than hiding
the non-additivity.

## API Changes

Extend `GET /api/gains` additively:

```text
GET /api/gains?include_closed=true&start_date=2025-06-12&end_date=2026-06-19
```

Rules:

- If no dates are supplied, default to inception-to-`end_date` performance: start at
  the earliest transaction date, with zero opening market value and buys treated as
  period cash flows. This avoids a permanent second percentage code path that advertises
  a non-performance return.
- If dates are supplied, return performance percentages and report-period amounts.
- Use `end_date` as the valuation date instead of `Local::now()`.
- Reject `start_date > end_date` with `400`.
- Reject malformed dates with `400`.

Add fields to the response without removing existing amount fields:

```text
percentage_method: "modified_dietz"
display_percent_kind: "absolute" | "annualised"
report_period: { start_date, end_date }
```

If cost-basis percentages remain useful for analytics, expose them later under clearly
named fields such as `cost_basis_gain_percent`; do not reuse the Gains totals percent
slots for them.

## Frontend Changes

- Add a Gains date-range control matching the Sharesight comparison workflow:
  Today, 7D, 12M, YTD, All, and custom start/end.
- Make "All" map to the earliest transaction date through the selected end date.
- Display percentage method context in the Gains totals, for example:
  `Performance return` or `Annualised return`.
- Stop showing cost-basis percentages as if they were Sharesight-compatible returns.
- Continue showing amount components exactly as today.
- Keep `include_closed_positions` as part of the query key.

## Data Requirements

### Already available

- Ordered instrument ledger.
- Trade-date FX for buys/sells when imported.
- End-of-day prices and FX cache.
- Latest/open and realized gain decomposition.
- Generic lookup primitives for last price/FX on or before a date.

### Missing or incomplete

- Historical price/FX **coverage guarantee** for the report start date and end date.
  Lookup primitives exist, but data may not be cached far enough back for arbitrary
  ranges.
- Dividend/income write path and import/write support.
- Performance-period reconstruction helpers independent of current holdings.
- Sharesight export/report fixtures for calibration.

Gains is a read-only endpoint, so there is no command-log or undo interaction to
design for the performance calculation itself.

## Implementation Phases

### Phase 0 - Historical Backfill Guarantee

Ensure price and FX caches can cover the report start and end dates for the instruments
being reconciled.

Verification:

- A report range requests or validates price/FX coverage on or before `start_date` and
  `end_date`.
- Missing historical coverage surfaces as explicit unavailable reasons, not zero.
- Backfill for the current calibration portfolio covers the Sharesight comparison
  range.

Human testing:

- Confirm a known Sharesight range has start/end prices and FX for the largest holdings
  before comparing percentages.

### Phase 1 - Pure Period Reconstruction

Build pure domain helpers that reconstruct:

- position before `start_date`
- period transactions
- position at `end_date`

Verification:

- Unit tests for buys before/inside/after the range.
- Unit tests for partial sells and closed/reopened positions.
- Unit tests for splits before and inside the range.
- Valuation calibration test for a range crossing a split, using the chosen provider
  price-adjustment convention.

Human testing:

- Use one known instrument and compare start/end quantities against Sharesight.

### Phase 2 - Period Amount Components

Compute capital, currency, and total-return amounts for the report period using the new
start-of-period baseline attribution function.

Verification:

- Amount components sum exactly to total return.
- Existing open+closed amount reconciliation stays stable.
- Missing price/FX returns explicit unavailable reasons.
- Sells inside the period are included; sells outside the period are excluded.
- Pre-period fees are not charged again against in-period capital gain.

Human testing:

- Compare Gains monetary values against Sharesight for the portfolio and for several
  large contributors.

### Phase 3 - Modified Dietz Percentages

Compute performance denominator and non-annualised holding-period percentages.

Verification:

- Synthetic Modified Dietz fixtures with one buy, one sell, and one start position.
- Zero/negative denominator cases return unavailable with a reason.
- Portfolio totals use one common denominator for component percentages.

Human testing:

- Compare the current real portfolio date range against Sharesight. Record residual
  differences and classify them as data, income, annualisation, or method gaps.
- Expect an income-shaped residual until Phase 5. Do not tune method or annualisation
  to absorb missing dividends.

### Phase 4 - Annualisation And Date-Range UI

Add annualisation, percentage kind metadata, and the Gains date-range control.

Verification:

- Date parsing and invalid ranges.
- Query keys include dates and `include_closed_positions`.
- Historical ranges do not show false stale-current-data warnings merely because the
  end date is in the past.
- Annualisation handles `years <= 0` and `1 + return <= 0`.
- Frontend `npm run check` and `npm run fmt`.

Human testing:

- Test Today, 12M, All, and a custom Sharesight range.
- Confirm the displayed percent label says whether it is annualised.

### Phase 5 - Income/Dividends

Add dividend/income support so total return can include Sharesight's Income column.
This phase includes the dividend write/import/API validation path, not only aggregation
math; Dividend transactions are currently rejected by the manual API.

Verification:

- Dividend rows affect income and total return amounts.
- Dividend rows do not change position quantity or cost basis.
- Dividends are not treated as denominator cash flows.
- Imported dividend rows reconcile against Sharesight income totals.

Human testing:

- Compare Sharesight income and total return on the same date range.

## Availability And Edge Cases

- Missing start price blocks performance return for an instrument with non-zero start
  quantity.
- Missing end price blocks performance return for an instrument with non-zero end
  quantity.
- Missing FX blocks SEK performance for non-SEK quantities/cash flows.
- Missing start/end data uses distinct reasons such as `missing_start_price`,
  `missing_start_fx`, `missing_end_price`, and `missing_end_fx`; do not collapse these
  into denominator errors.
- A fully closed instrument with no start or end quantity can still have performance
  if it has in-period buy/sell cash flows.
- If the denominator is zero, negative, or effectively unavailable, return
  `zero_or_invalid_performance_denominator`.
- If all rows are unavailable, totals are unavailable rather than zero.
- Splits have no cash flow but affect quantity for start/end valuation.
- Brokerage remains a SEK fee and must not be treated as currency movement.

## Calibration Plan

Create a local, ignored reconciliation worksheet or script that compares:

- TTTB amount and percent totals.
- Sharesight screenshot/export totals.
- Top N holding rows by absolute return.
- Implied denominator per component.

Do not commit private raw exports. Version only sanitized aggregate findings if they
become useful design evidence.

## Open Questions

- Exact Sharesight Modified Dietz variant, especially fresh vs recycled capital.
- Exact start-date boundary treatment for trades on the first day of the report.
- Calendar-day vs trading-day cash-flow weights.
- Average Years Invested calculation for annualisation.
- Whether component percentages are annualised independently or share one annualised
  total-return denominator/rate convention.
- Whether dividends should be imported from Sharesight, Avanza, or entered manually
  first.

## Versioning

This is a backend and frontend feature. Bump both versions when implementing:

- backend `Cargo.toml`
- frontend `package.json`

## Documentation Updates When Accepted

When this spec is implemented and accepted:

- Archive or remove this plan/spec from `docs/plans/`.
- Add a durable decision log entry for Sharesight-style performance returns.
- Update durable Gains/FX design docs so repository documentation no longer refers to
  implementation phases.
