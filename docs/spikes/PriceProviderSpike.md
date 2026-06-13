# Price Provider Spike

**Date:** 2026-06-13
**Scope:** Phase 0 representative EOD and FX provider check for one USD instrument and one EUR instrument.

## Summary

Use Yahoo Finance's unofficial chart endpoint as the v1 primary equity EOD provider, and use Frankfurter v2 as the FX provider for SEK conversion. Keep Twelve Data as the first fallback equity provider if Yahoo becomes unreliable or unacceptable, because it documents Euronext Amsterdam coverage and has a free personal API key path, but it requires a key for actual time-series requests.

This is a personal self-hosted app choice, not a general commercial licensing recommendation. Re-check provider terms before any hosted or distributed use.

## Representative Instruments

| Instrument | Market from export | Currency | Yahoo symbol | Twelve Data mapping |
|---|---:|---:|---|---|
| Microsoft Corporation | NASDAQ | USD | `MSFT` | `MSFT`, NASDAQ/XNGS |
| ASML Holding N.V. | EURONEXT | EUR | `ASML.AS` | `ASML`, Euronext/XAMS |

## Candidate Results

### Yahoo Finance chart endpoint

Authentication: none observed for the tested chart URLs.

Request URLs tested:

```text
https://query1.finance.yahoo.com/v8/finance/chart/MSFT?range=5d&interval=1d
https://query1.finance.yahoo.com/v8/finance/chart/ASML.AS?range=5d&interval=1d
https://query1.finance.yahoo.com/v8/finance/chart/MSFT?period1=1780848000&period2=1781308800&interval=1d
https://query1.finance.yahoo.com/v8/finance/chart/USDSEK=X?range=5d&interval=1d
https://query1.finance.yahoo.com/v8/finance/chart/EURSEK=X?range=5d&interval=1d
```

Observed response shape:

- Top-level `chart.result[0].meta` includes `currency`, `symbol`, exchange names, timezone, and market price metadata.
- Daily candles are arrays under `chart.result[0].timestamp` and `chart.result[0].indicators.quote[0]`.
- `open`, `high`, `low`, `close`, and `volume` arrays align by index with `timestamp`.
- `adjclose` is present for the tested equity and FX symbols.

Observed normalized output from the 5-day equity calls:

| Provider | Symbol | Date basis | Currency | Latest observed close |
|---|---|---|---:|---:|
| Yahoo | `MSFT` | exchange daily timestamp | USD | 390.739990234375 |
| Yahoo | `ASML.AS` | exchange daily timestamp | EUR | 1629.5999755859375 |

Strengths:

- No key required in the observed requests.
- Covers both representative instruments with clear Yahoo-specific symbols.
- Covers daily history via `range` or `period1`/`period2`.
- Also exposes `USDSEK=X` and `EURSEK=X`, useful as an emergency FX fallback.

Concerns:

- Endpoint is unofficial for this use and can change without notice.
- Yahoo's API terms reserve discretionary rate limits and restrict excessive or abusive usage.
- Provider may serve split-adjusted historical prices. Valuation should multiply provider close by derived quantity for the valuation date, while later chart buy markers may need split-ratio adjustment.
- Tests should use recorded fixtures rather than live calls.

### Frankfurter v2 FX API

Authentication: none.

Request URLs tested:

```text
https://api.frankfurter.dev/v2/rates?base=USD&quotes=SEK
https://api.frankfurter.dev/v2/rates?base=EUR&quotes=SEK
https://api.frankfurter.dev/v2/rates?from=2026-06-08&to=2026-06-12&base=USD&quotes=SEK
https://api.frankfurter.dev/v2/rates?from=2026-06-08&to=2026-06-12&base=EUR&quotes=SEK
```

Observed response shape:

- Latest and time-series responses are arrays of rows.
- Each row includes `date`, `base`, `quote`, and `rate`.
- The rate is directly in quote currency per one base unit, matching the planned canonical `currency -> SEK` storage shape.

Observed normalized output:

| Provider | Pair | Date | Rate |
|---|---|---:|---:|
| Frankfurter | USD/SEK | 2026-06-13 | 9.4719 |
| Frankfurter | EUR/SEK | 2026-06-13 | 10.9552 |

Strengths:

- No key required.
- Simple canonical `base`/`quote` model.
- Supports historical ranges.
- Open source and self-hostable if public access becomes unsuitable.

Concerns:

- This solves FX only, not equity prices.
- The API's default provider set is blended unless constrained by provider parameters. For deterministic accounting rules, decide in Phase 0.7 whether to pin to a provider such as ECB or accept blended central-bank rates.

### Twelve Data

Authentication: API key required for time series and exchange-rate requests. The demo key allowed symbol search but returned `401` for tested time-series and FX calls.

Request URLs tested:

```text
https://api.twelvedata.com/symbol_search?symbol=ASML&apikey=demo
https://api.twelvedata.com/time_series?symbol=MSFT&interval=1day&outputsize=5&apikey=demo
https://api.twelvedata.com/time_series?symbol=ASML&exchange=XAMS&interval=1day&outputsize=5&apikey=demo
https://api.twelvedata.com/exchange_rate?symbol=USD/SEK&apikey=demo
```

Observed response shape from symbol search:

- `data[]` includes `symbol`, `instrument_name`, `exchange`, `mic_code`, `exchange_timezone`, `instrument_type`, `country`, and `currency`.
- `ASML` was listed on Euronext with MIC `XAMS`, timezone `Europe/Amsterdam`, instrument type `Common Stock`, country `Netherlands`, and currency `EUR`.

Strengths:

- Documents EOD and time-series endpoints.
- Documents Euronext Amsterdam coverage.
- Uses exchange/MIC metadata that maps cleanly to stored provider-symbol rows.
- Better long-term fallback candidate than another scraper-style source.

Concerns:

- Requires a key before actual EOD/history can be verified against `MSFT` and `ASML`.
- Free plan limits need to be confirmed with a real account before relying on it for full backfill.

### Alpha Vantage

Authentication: API key required. The demo key returned an informational response for `MSFT`, `ASML.AMS`, symbol search, and USD/SEK FX calls rather than usable data.

Documentation notes:

- The free service is documented as capped at 25 requests per day.
- Daily stock and FX endpoints exist, but the representative symbols were not verified without a real key.

Assessment:

- Not recommended as the first fallback for this app because the documented free daily cap is tight for portfolio-wide backfill and daily refresh testing.

### Stooq

Authentication: none advertised for historical downloads, but the tested CSV endpoint returned a JavaScript browser verification page from this environment. An attempted proof-of-work verification flow still ended in `Access denied`.

Request URLs tested:

```text
https://stooq.com/q/d/l/?s=msft.us&i=d
https://stooq.com/q/d/l/?s=asml.nl&i=d
https://stooq.pl/q/d/l/?s=msft.us&i=d
```

Assessment:

- Not recommended as a backend provider for v1. Even if data is free, the browser challenge makes unattended jobs brittle.

## Missing Price Representation

Missing prices should not be stored as zero. The app should represent missing provider results explicitly:

- Persist successful prices with provider, provider symbol, date, currency, close, and retrieval timestamp.
- Represent a failed or absent price at the read/job-result boundary as `missing`, with reason categories such as `market_closed`, `not_listed`, `provider_error`, `rate_limited`, and `symbol_unmapped`.
- In valuation, carry the last known prior close only through an explicit rule such as `last_close_before(date)`, and surface staleness in the UI.

## Decision

Primary v1 equity provider: Yahoo Finance chart endpoint.

Primary v1 FX provider: Frankfurter v2, storing `USD -> SEK` and `EUR -> SEK` rates directly.

Fallback strategy:

1. Use recorded fixtures for deterministic tests and local development.
2. If Yahoo fails or becomes unacceptable, add Twelve Data behind the same provider trait after verifying `MSFT` and `ASML` time-series calls with a real free key.
3. If both live equity providers fail, allow manual price CSV import before adding a paid provider.

## Provider Trait Requirements

A later provider boundary should support:

- `latest_eod(provider_symbol) -> Option<DailyClose>`
- `daily_history(provider_symbol, start_date, end_date) -> Vec<DailyClose>`
- `latest_fx(base_currency, quote_currency) -> Option<FxRate>`
- `fx_history(base_currency, quote_currency, start_date, end_date) -> Vec<FxRate>`
- Structured missing/error reasons distinct from successful empty market data.
- Provider-specific symbol mappings stored outside instrument identity.
- Fixture-backed tests that do not call live services.

## Sources Checked

- Yahoo Developer API Terms of Use: `https://legal.yahoo.com/us/en/yahoo/terms/product-atos/apiforydn/index.html`
- Frankfurter v2 API documentation: `https://frankfurter.dev/`
- Twelve Data documentation and Euronext Amsterdam exchange page: `https://twelvedata.com/docs`, `https://twelvedata.com/exchanges/XAMS`
- Alpha Vantage documentation and support page: `https://www.alphavantage.co/documentation/`, `https://www.alphavantage.co/support/`
- Stooq historical data page: `https://stooq.com/db/h/`
