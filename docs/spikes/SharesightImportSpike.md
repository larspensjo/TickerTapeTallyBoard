# Sharesight Import Spike

**Date:** 2026-06-13  
**Fixture:** private `docs/AllTradesReport_2026-06-12.csv`  
**Spike command:** `cargo run --example sharesight_import_spike` from `backend/`

This note is sanitized. It intentionally omits row-level values, position sizes,
instrument names, and instrument codes from the private Sharesight export.

## Parser Result

- Report title recognized as an All Trades report for 2025-06-12 to 2026-06-12.
- Header row was found by content at CSV row 3 instead of assuming a fixed line.
- Parsed 189 data rows from CSV rows 4-192.
- Transaction counts matched the planning observation: 105 buys, 83 sells, 1 split.
- Markets found: EURONEXT, FRA, NASDAQ, NYSE, XETR.
- Instrument currencies found: EUR, USD.
- Unique `Market + Code` instruments: 30.
- `Market + Code` identity conflicts: 0.
- Brokerage currency values: SEK only.
- Source/report column values: `All Trades` only.

## Validation Findings

- Unknown transaction types: 0.
- Value sign mismatches against type rules: 0.
- Quantity sign mismatches against type rules: 0.
- Rows with non-zero `Cost base per share (SEK)`: 0.
- Blank comments: 188.
- Duplicate full rows: 0.
- Same-day same-instrument same-type groups with multiple rows: 1 group, 2 rows.

The importer should keep same-day same-instrument rows as separate ledger entries.
There is at least one partial-fill-shaped group, and no evidence in this export
that duplicate rows need to be collapsed.

## Decimal And CSV Rules

The spike parser successfully handled:

- Comma decimal separators.
- Non-breaking-space and ordinary-space thousands separators.
- Unicode minus signs in sell quantities and values.
- The unnamed report/source column between `Value` and `Comments`.
- `dd/mm/yyyy` trade dates.

## FX And Value Interpretation

The closest observed aggregate value model was:

```text
Value = native gross / Exchange Rate + Brokerage
```

Residuals are expected because the exported `Exchange Rate` appears rounded:

| Candidate model | Average absolute residual | Max absolute residual |
|---|---:|---:|
| `native gross / exchange rate` | 71.56 SEK | 296.70 SEK |
| `native gross / exchange rate + brokerage` | 64.35 SEK | 266.94 SEK |
| `native gross / exchange rate - brokerage` | 78.78 SEK | 326.46 SEK |
| `native gross * exchange rate` | 70073.93 SEK | 297002.55 SEK |
| `native gross * exchange rate + brokerage` | 70073.05 SEK | 296980.13 SEK |
| `native gross * exchange rate - brokerage` | 70074.80 SEK | 297024.97 SEK |

Interpretation:

- The export's `Exchange Rate` is instrument currency per SEK.
- The app's canonical `fx_rate_to_base` should use the inverse: SEK per instrument currency.
- Brokerage inclusion in `Value` is a working hypothesis, not a settled finding from the aggregate residuals alone. The residual improvement from adding brokerage is smaller than the FX-rounding noise band, so Phase 1 should first reconcile at least one buy and one sell manually against the raw export or Sharesight UI.
- Brokerage is already SEK-denominated and must not be converted through the trade FX rate.
- `Cost base per share (SEK)` is unusable in this export because every row is zero.

## Split Handling

- Split rows found: 1.
- Treating the split quantity as a delta derives a clean 5/1 ratio.
- Lars reran the spike with the current Sharesight position for the split holding.
- The provided current position matched the summed quantity, so delta semantics are confirmed for this export.

Repro command shape:

```powershell
cd backend
cargo run --example sharesight_import_spike -- --split-current-position <CURRENT_NOW_POSITION>
```

## Price Adjustment Note

Market data providers such as Yahoo may serve split-adjusted historical prices
while Sharesight transaction prices are unadjusted. Valuation should use
`derived_quantity(date) * close(date)`. Buy-price chart markers will need
ratio-adjusted marker prices later.

## Open Question

- Does `Value` include brokerage for both buys and sells? Reconcile one buy and one sell manually before Phase 1 import bakes this into transaction math.
