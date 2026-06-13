# Currency And FX Rules

Date: 2026-06-13

## Scope

These rules define v1 currency handling for the single SEK-based portfolio.
They are accounting and valuation rules for the application, not tax-reporting
rules.

The portfolio is held in a Swedish ISK account. v1 therefore does not model
capital-gains tax or dividend tax. Cost basis is retained for portfolio
analytics, reconciliation, and later performance explanations, not for Swedish
tax calculation.

## Canonical Currency Model

- Base currency is SEK.
- Transaction prices and market prices remain in their native instrument
  currency.
- FX rates are stored as quote-currency-to-SEK rates, for example `USD -> SEK`
  means SEK per 1 USD.
- The app converts to SEK at read/valuation time. It does not persist only a
  pre-converted SEK value.
- Historical FX must be available from the earliest trade date needed for
  valuation, not only for the latest date.

## Provider Rules

- Equity EOD/history provider: Yahoo Finance chart endpoint, per the market-data
  decision.
- FX provider: Frankfurter v2 pinned to `providers=ECB` when supported for the
  needed date and pair.
- Frankfurter returns rates in the app's canonical shape when requested as
  `base=<currency>&quotes=SEK`.
- Missing FX or price data is represented as missing data with a reason. It is
  never stored as zero.

## Sharesight Import Rules

Sharesight's `Exchange Rate` in the All Trades CSV is interpreted as instrument
currency per SEK. Import therefore stores:

```text
fx_rate_to_base = 1 / sharesight_exchange_rate
```

where `fx_rate_to_base` is SEK per 1 unit of the instrument currency.

Sharesight `Value` is a source/reconciliation field, not a primary ledger input.
For a buy row, `Value` is the SEK cash debit from the account and includes the
SEK brokerage. The ledger still derives its own components from native price,
native currency, trade-date FX, and SEK fee fields.

The imported ledger entry keeps at least these separate monetary components:

- Native unit price and instrument currency.
- Quantity with an explicit transaction type.
- Trade-date `fx_rate_to_base`.
- Brokerage amount and brokerage currency.
- Source `Value` as an audit field.

Brokerage in the current export is SEK. It is stored as a SEK-denominated fee
and must not be converted through the trade FX rate.

## Worked Buy Example

A private Sharesight buy row was checked locally against this arithmetic shape.
The exact row values are omitted from versioned documentation because the export
contains private portfolio data.

```text
type: Buy
instrument_currency: USD or EUR
brokerage_currency: SEK
source_exchange_rate: instrument currency per SEK

native_gross = quantity * native_unit_price
fx_rate_to_base = 1 / source_exchange_rate
converted_gross_sek = native_gross * fx_rate_to_base
cash_debit_sek = converted_gross_sek + brokerage_sek
```

For the checked buy interpretation, Sharesight `Value` corresponds to
`cash_debit_sek`, subject to residual differences from the rounded exchange
rate exported in the CSV.

Synthetic arithmetic vector for Phase 1 unit tests:

```text
quantity = 10
native_unit_price = 12.50 USD
source_exchange_rate = 0.100000 USD per SEK
brokerage = 9.60 SEK

native_gross = 10 * 12.50 = 125.00 USD
fx_rate_to_base = 1 / 0.100000 = 10.000000 SEK per USD
converted_gross_sek = 125.00 * 10.000000 = 1250.00 SEK
cash_debit_sek = 1250.00 + 9.60 = 1259.60 SEK
source_value = 1259.60 SEK
```

## Display And Decimal Rules

- Persist parsed money, prices, quantities, and FX as exact decimals.
- Do not round before persistence except when normalizing provider precision that
  is already limited by the source.
- Display SEK totals and fees to 2 decimal places.
- Display share prices and FX with enough precision to explain calculations;
  UI formatting can be adjusted per surface, but must not feed back into stored
  values.

## Cost Basis

Buy cost basis in SEK is derived from native gross value converted at trade-date
FX plus SEK brokerage. Sell handling should reduce position/cost basis according
to the chosen portfolio accounting method in Phase 1, using the stored native
trade, FX, and fee fields rather than Sharesight's all-zero
`Cost base per share (SEK)` column.

Because this portfolio is an ISK account, cost basis is not a tax engine input in
v1. It is used for analytics and reconciliation only.

## Explicit Non-Goals For v1

- No cash balance ledger.
- No deposits or withdrawals.
- No tax calculation.
- No currency-gain attribution UI, although the schema preserves the native/FX
  split required to add it later.
