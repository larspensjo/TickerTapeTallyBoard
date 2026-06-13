# Decision Log

Purpose: durable record of deliberate commitments — the source of truth for what the
project has agreed to do going forward.

## How to use
### How to add new entries
- One entry per decision. Decisions are commitments, not summaries of work.
- Implementation summaries and bug-fix postmortems belong in `EngineeringDiary.md`.
- Reversals of prior decisions get their own entry referencing the original.
- Keep entries concise and reference concrete artifacts.
- New entries go to the end of the file.

### When to add new entries
- Architecture commitments
- Technology or library choices
- Naming, structural, or coding conventions adopted project-wide
- Safety and scope boundaries
- Experiment outcomes promoted into accepted design
- Reusable rules derived from incidents
- The decisions are sometimes updated during planning, not during implementation (unless something unexpected happened).
- Creating a plan isn't decision point if it was already obviously part of the project.

### How to use the decision log during development
- Do not modify older entries if they were commited.

## Entry Template
```
## YYYY-MM-DD - <decision title>
Decision: <the rule, in present tense>
Context: <why this was decided now>
Consequences: <what this constrains or implies going forward>
```

## 2026-06-12: Phase 0 Planning Decisions
Decision: v1 uses SEK as the base currency, tracks one portfolio, imports securities transactions only, uses integer share quantities, trusts the LAN without authentication, and allows manual dividend entry.
Context: These choices keep the first implementation focused on the complete Sharesight All Trades export and the core ledger/valuation path.
Consequences: Phase 0 and Phase 1 do not need account/cash flows, multi-portfolio schema, fractional quantity support, dividend import, or authentication UI/API work.

## 2026-06-12: Repository Layout
Decision: Use top-level `backend/` and `frontend/` directories without a Rust workspace for v1.
Context: The project currently needs one Rust binary and one TypeScript app. A workspace can be introduced later if a CLI, reusable domain crate, or compile-time pressure justifies it.
Consequences: Backend commands run from `backend/`; frontend commands run from `frontend/`. Domain logic should live under `backend/src/domain/` and remain free of axum, sqlx, HTTP, and provider-specific types.

## 2026-06-12: Private Sharesight Exports
Decision: Raw Sharesight exports must not be committed to the repository.
Context: The export contains private portfolio data.
Consequences: `.gitignore` excludes `docs/AllTradesReport*.csv`, `docs/fixtures/private/`, and `docs/spikes/private/`. Versioned docs may include sanitized aggregate findings only.

## 2026-06-12: Split Import Working Model
Decision: Treat Sharesight split quantity as a quantity delta at the import/ledger layer until verified by the Phase 0 invariant check.
Context: The export contains one split row. The spike will sum quantities for the split holding across the complete export and compare that result with Sharesight's current position.
Consequences: If the invariant confirms delta semantics, the ledger stores the source delta and the engine derives ratios when needed. Price-history and buy-marker logic must account for provider split adjustment behavior.

## 2026-06-12: Price Provider Spike Scope
Decision: Unofficial Yahoo Finance data is acceptable for v1 if the Phase 0 provider spike supports it. The spike should compare candidate providers using one USD asset, `MSFT`, and one EUR asset, `ASML`.
Context: v1 only needs end-of-day data for the complete current export, not realtime market data.
Consequences: Phase 0 can include Yahoo as a serious candidate while still documenting symbol mappings, limitations, and fallback options.

## 2026-06-13: Backend Logging Stack
Decision: Backend runtime logging uses the local `engine_logging` facade backed by `log` and `simplelog`.
Context: The skeleton needs simple terminal logging plus an append-only `engine.log` without introducing structured tracing requirements before the API and job model exist. Keeping calls behind `engine_logging` preserves a migration path if later work needs spans, request IDs, or richer sinks.
Consequences: Backend code should log through `engine_logging` rather than calling logging libraries directly. Revisit the backing library before adding structured request, import job, provider, or database operation telemetry.

## 2026-06-13: Displayed Application Versions
Decision: The frontend displays its `package.json` version and the backend displays its Cargo package version from `GET /api/health` as separate version values.
Context: The application has independent frontend and backend build artifacts, and a single hardcoded UI version would hide skew during local development and deployment.
Consequences: Release work should keep the two manifest versions aligned when shipping one product release, while the UI remains able to expose mismatches during development or partial deployment.

## 2026-06-13: Keep Native Value And FX Separate For Currency-Gain Attribution
Decision: Persist native value and the exchange rate as separate fields and never store only the SEK-converted value. Transactions keep native `price` + `currency` plus a separate trade-date `fx_rate_to_base`; `prices` keep native `close`; `fx_rates` keep dated rates; conversion happens at the read/valuation boundary. Imported cost basis is derived from the native price and the export's `Exchange Rate`, not from Sharesight's pre-computed `Cost base per share (SEK)`.
Context: Currency-gain attribution (decomposing an asset's total return into capital, income, and currency components) is post-v1, but it is only possible if native value and FX are recoverable independently. Collapsing to SEK at write time would lock out the breakdown and force a schema rewrite later.
Consequences: Phase 1 schema and import must keep native and FX fields distinct. FX must be backfilled historically (back to the earliest trade date), not just the latest day. Brokerage paid in SEK on a USD/EUR trade must not be run through the trade FX, which may require a separate fees currency rather than a single transaction `currency` column. The attribution math itself remains a derived calculation added later in the valuation module.

## 2026-06-13: Sharesight CSV Import Conventions
Decision: The Sharesight import path reads the All Trades CSV format, locates the header row by content, parses comma decimals with non-breaking-space thousands separators and Unicode minus signs, treats the unnamed column after `Value` as the report/source column, and keeps each row as a separate ledger entry. The exported `Exchange Rate` is interpreted as instrument currency per SEK, so imported `fx_rate_to_base` stores its inverse. Treat `Value` brokerage inclusion as a working hypothesis until one buy and one sell are reconciled manually against the raw export or Sharesight UI; SEK brokerage still remains a separate SEK-denominated fee. The all-zero `Cost base per share (SEK)` column is not used as imported cost basis.
Context: The Phase 0 Sharesight import spike parsed the complete private All Trades CSV and compared aggregate validation rules and value/FX models.
Consequences: Phase 1 import should normalize native price/currency, the inverted FX rate, SEK brokerage, signed quantities, and signed source values explicitly. Same-day same-instrument rows must not be merged during import. Do not bake brokerage inclusion into import math until the manual buy/sell reconciliation is recorded. Split quantity uses delta semantics.

## 2026-06-13: Primary Market Data Sources
Decision: Use the unofficial Yahoo Finance chart endpoint as the v1 primary equity EOD/history provider and Frankfurter v2 as the v1 FX provider for SEK conversion. Store provider-specific equity symbols separately from instrument identity; the Phase 0 representative mappings are Yahoo `MSFT` for NASDAQ Microsoft and Yahoo `ASML.AS` for Euronext Amsterdam ASML. Keep Twelve Data as the first keyed fallback candidate, and keep manual price CSV import as the fallback if free live equity APIs become unreliable or unacceptable. Missing prices are represented explicitly with a reason, never as zero.
Context: The Phase 0 price provider spike tested no-key/free-friendly candidates for `MSFT`, `ASML`, USD/SEK, and EUR/SEK. Yahoo returned daily equity candles for both representative instruments without a key. Frankfurter returned current and historical SEK FX rows without a key. Stooq was blocked by browser verification, Alpha Vantage required a real key and has a tight documented free daily limit, and Twelve Data required a real key for time-series calls but confirmed ASML on Euronext/XAMS through symbol search.
Consequences: Phase 1/3 provider code should hide Yahoo-specific response shapes behind a provider boundary, use recorded fixtures for deterministic tests, store symbol mappings per provider, and keep missing-data states visible to valuation and UI code. Re-check terms and usage limits before any hosted or distributed deployment.

## 2026-06-13: Base Currency And FX Rules
Decision: v1 uses SEK as base currency and stores FX canonically as SEK per one unit of the quote/instrument currency, for example `USD -> SEK`. Sharesight's exported `Exchange Rate` is interpreted as instrument currency per SEK and is inverted on import. Sharesight `Value` is retained as a source/audit field rather than a primary ledger input; for buys it represents the SEK cash debit and includes SEK brokerage. Brokerage is stored as its own SEK-denominated fee and is not converted through trade FX. Frankfurter FX requests should pin `providers=ECB` when supported for the needed date and pair.
Context: The Phase 0 import spike established the Sharesight FX direction, and Lars confirmed the buy-side account cash interpretation. The Phase 0 market-data spike chose Frankfurter as the FX provider.
Consequences: Phase 1 schema should keep native price/currency, trade-date `fx_rate_to_base`, brokerage amount/currency, and source value separate. Valuation converts at read time using historical FX. Missing FX is an explicit missing-data state, not zero. See `docs/CurrencyAndFxRules.md`.

## 2026-06-13: ISK Tax Scope
Decision: v1 assumes the tracked stocks and ETFs are held in a Swedish ISK account and does not calculate capital-gains tax or dividend tax. Cost basis is kept for portfolio analytics, reconciliation, and performance explanation only.
Context: The portfolio's tax wrapper means realized gains and dividends do not need per-transaction tax calculations in the application.
Consequences: Phase 1 and v1 valuation should avoid tax-reporting claims and should not add tax lots, tax events, or dividend withholding logic unless the account scope changes later.
