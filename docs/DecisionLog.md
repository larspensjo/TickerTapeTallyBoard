# Decision Log

Purpose: durable record of deliberate commitments — the source of truth for what the
project has agreed to do going forward.

## How to use
### How to add new entries
- One entry per decision. Decisions are commitments, not summaries of work.
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

## 2026-06-13: Static Frontend Serving
Decision: The backend serves built frontend assets from disk for the initial production-style path, using `../frontend/dist` by default and `TTTB_STATIC_DIR` as an override. Embedded frontend assets are deferred until packaging needs justify them.
Context: The app needs concrete local development and future serving behavior without adding packaging complexity. Vite already handles development serving and proxies `/api` to the backend.
Consequences: Local development continues to run Vite and the backend as separate processes. A production-style smoke run builds the frontend first, then runs the backend from `backend/`; API routes remain under `/api`, while frontend routes are served by the built SPA fallback when the static directory exists.

## 2026-06-14: Cost-Basis Accounting Method
Decision: Portfolio cost basis uses weighted-average cost per instrument. A buy adds to quantity and native cost basis; a sell reduces quantity and every cost-basis component proportionally at the current average, leaving average cost per share unchanged. FIFO tax-lots remain out of scope for v1.
Context: `docs/CurrencyAndFxRules.md` deferred "the chosen portfolio accounting method"; this resolves it. The ISK tax scope means cost basis is for analytics and reconciliation only.
Consequences: Sell handling and holdings derivation use a single blended average. Per-lot recovery of historical cost is not possible without a future FIFO model.

## 2026-06-14: Ledger Ordering
Decision: Position derivation processes a single instrument's transactions in `(trade_date, id)` order. `id` is the monotonic insert/import tiebreaker; same-day same-instrument rows stay distinct.
Context: Weighted-average cost is order-sensitive when buys and sells interleave on the same trade date. A deterministic order is required for reproducible holdings.
Consequences: A later explicit import-sequence column can replace the tiebreaker without changing the contract. Writes are validated by re-deriving the full ordered ledger.

## 2026-06-14: Instrument Identity
Decision: Instrument identity is `UNIQUE (exchange, symbol)`. Currency is an attribute, not part of identity. Creating an instrument is upsert-like: an existing `(exchange, symbol)` returns the existing row unchanged rather than creating a duplicate.
Context: Inline instrument creation from the transaction form must not fragment a holding into duplicate instruments.
Consequences: Provider-specific symbols are stored separately when added later. Renames/currency corrections to an existing instrument need an explicit update path (not part of upsert).

## 2026-06-14: Missing-FX Contamination Rule
Decision: A position's SEK cost basis (`cost_basis_base` / `average_cost_base`) is available only while every buy contributing to the currently open quantity had a known trade-date FX rate. The first missing-FX buy folded into the blended average makes the position's base cost unavailable (with reasons and offending transaction ids) until the position fully closes (`quantity → 0`) and is rebuilt from buys that all have FX. Native cost basis is always derivable.
Context: Under one blended weighted-average, shares bought without FX cannot be isolated, so a partial SEK basis would be misleading. Missing data must be explicit, never zero. This refines the 2026-06-13 Base Currency And FX Rules commitment that missing FX is an explicit state, not zero.
Consequences: Holdings expose an explicit unavailable state. Per-lot recovery is deferred with FIFO tax-lots.

## 2026-06-14: Ledger-Write Validity Invariant
Decision: Every transaction write (create, full-replace, delete) must leave the affected instrument's `(trade_date, id)`-ordered ledger derivable — no step may drive quantity below zero and every split must remain valid — otherwise the write is rejected. Missing FX is not a violation (it derives successfully with an unavailable base).
Context: Holdings are derived purely from the ledger and must always be computable. Back-dated edits and deletes can otherwise invalidate later rows.
Consequences: Corrections may need to be applied in dependency order (e.g. remove a dependent sell before the buy it draws from). Holdings derivation can assume a consistent stored ledger.

## 2026-06-14: Transaction Type Constraint And Manual Entry Scope
Decision: The `transactions.type` CHECK constraint allows `BUY|SELL|SPLIT|DIVIDEND`, but manual entry and the API support only Buy, Sell, and Split; a Dividend create request is rejected (`dividend_not_supported`) until dividend fields and validation exist. This narrows the 2026-06-12 Phase 0 Planning Decisions commitment that v1 "allows manual dividend entry": manual dividend entry is deferred, not yet available.
Context: Keeping DIVIDEND in the constraint avoids a future table-rebuild migration when dividend support is designed, while current behaviour stays limited to the types that have defined fields. The earlier scope assumed manual dividend entry would ship, but no dividend fields or validation have been designed yet.
Consequences: The schema is forward-compatible for dividends. Until dividend fields land, no Dividend rows are created, and derivation treats any Dividend row as a position no-op. When dividend support is added, revisit this entry and the 2026-06-12 Phase 0 Planning Decisions entry.

## 2026-06-14: Backend Persistence Stack
Decision: The backend persists to SQLite via sqlx using runtime `query_as` and `FromRow` (no compile-time macros, no `.sqlx/` offline metadata, no `SQLX_OFFLINE`). Money, prices, FX, and quantities-as-decimals are stored as TEXT and round-tripped through `rust_decimal` strings; integer share quantities as INTEGER; dates as ISO-8601 TEXT. Schema correctness is covered by DB integration tests that run the real migrations. Migrations are forward-only and additive.
Context: For a single-binary local SQLite app where SQLite stores decimals as TEXT, compile-time query macros add tooling friction for little type-safety gain, and the existing `cargo clippy --all-targets -- -D warnings` workflow must stay free of a live database.
Consequences: All SQL lives in `db/` repositories. Reads decode TEXT decimals/dates into domain types; a decode failure is an internal error. Revisit the macro approach only if a richer database or compile-time guarantees are later justified.

## 2026-06-15: Sharesight Import Endpoints And Atomicity
Decision: Sharesight CSV import uses a read-only `preview` endpoint and an atomic `commit` endpoint that both consume raw CSV bytes. Commit writes one sqlx transaction containing the import batch row, upserted instruments, and transactions in CSV order, then re-derives each affected ledger before commit. Duplicate-file detection is based on `raw_file_hash`; a repeated file is rejected with `duplicate_import` unless `allow_duplicate=true` is set.
Context: The import flow needs a non-mutating dry run, safe retries, and deterministic same-day ordering between preview and commit.
Consequences: The frontend must keep the file between preview and commit. Import rollback work can delete batches by `import_batch_id` later without changing the duplicate-guard contract.

## 2026-06-15: Import Batch Rollback Semantics
Decision: Rolling back an import batch deletes every transaction tagged with that `import_batch_id` in one transaction, then re-derives each affected instrument's remaining ledger. If any remaining row is no longer derivable, the rollback is rejected and nothing is removed. When re-derivation passes, the now-empty `import_batches` row carrying `raw_file_hash` is deleted in the same transaction so the same file can be re-imported afterwards without tripping the duplicate guard.
Context: Imports must be reversible, but a back-dated import can become load-bearing for later manual edits.
Consequences: A user must remove dependent manual transactions before rolling back an import they depend on. Instruments created by the batch are left in place as harmless empty instruments. Empty imported batches are not retained for duplicate detection after rollback.

## 2026-06-16: Import Duplicate-Row Warning Semantics
Decision: The import preview warns with `duplicate_row` only when two or more CSV rows are identical in instrument, trade date, direction, quantity, price, and value — the signature of a duplicated export line. Multiple same-day, same-direction trades that differ in quantity or price are treated as legitimate partial fills and raise no warning.
Context: The earlier rule keyed only on instrument + date + direction, so it flagged every normal multi-fill day as a warning, inflating the warning count with noise and obscuring the genuinely suspicious duplicate-line case.
Consequences: Partial fills import silently. The duplicate guard catches accidental repeated rows but not deliberate identical re-entries; a true duplicate is surfaced as a warning, not an error, so import is still allowed.

## 2026-06-16: Command-Based Writes And Backend-Validated Undo
Decision: User-visible mutations (manual transaction create/replace/delete and Sharesight import commit) are executed through backend command handlers backed by a persisted command log, not by direct repository calls scattered across API handlers. The transaction ledger stays the source of truth for holdings; the command log only records how data changed and powers undo/activity UX. Undo is itself a validated backend write that re-derives the affected ledgers and may be rejected (`undo_blocked`) when it would violate the 2026-06-14 Ledger-Write Validity Invariant. Existing resource endpoints keep their current response shapes first and let the frontend refetch command history; command metadata is added to responses additively later, never as a breaking change.
Context: The import rollback path already proved that durable, backend-validated reversal beats frontend-only undo. Generalising it as one command service avoids every future mutation inventing its own history, UI, and blocked-state behaviour. See `docs/Design.CommandUndo.md`.
Consequences: A command module sits below `api/` and above `db/`; handlers parse DTOs while commands own the SQL transaction, domain validation, command record, and undo metadata. Holdings are never derived from the command log. Undo can legitimately fail when later rows depend on the change, so corrections may still need dependency-ordered edits.

## 2026-06-16: Undo/Redo Scope, UX Safety, And Redo Snapshots
Decision: The first command/undo version ships global "undo latest" only. Selective undo/redo — viewing the do/undo stack, selecting a specific entry, and undoing or redoing it — and redo in general are planned but deferred past the undo MVP. Because history can grow long and a stray `Ctrl+Z` could unwind major unrelated work, undo and redo must always be backed by an explicit, visible stack UI: a popup that shows the do/undo stack, is navigable with `Ctrl+Z`/`Ctrl+Y`, and lets the user select an entry, so it is always obvious what happened or will happen before acting. Redo of a Sharesight import re-applies stored normalized row snapshots rather than re-reading the file; persisting that private import data in the command log is accepted.
Context: Undo lowers the cost of mistakes only if the system is honest and visible about what it will reverse. Blind keyboard undo against an unbounded history is the main human-error risk. Storing normalized import rows is the only way to support import redo without forcing the user to re-supply the file.
Consequences: `Ctrl+Z` outside text inputs targets the newest applied undoable command; redo and selective undo wait for a later phase but the schema must preserve payloads (including import row snapshots) so they can be added without a rebuild. The undo/redo stack popup is a first-class UI surface, not an afterthought toast.

## 2026-06-16: Command History Retention, Cleanup, And Instrument Removal On Undo
Decision: Command records are retained indefinitely for now and treated as UX/activity state, not a formal audit log; rejected command attempts are not written as command rows. A future general cleanup function will remove unused instruments and prune old undo history together. Undoing a manual command that created a brand-new instrument also removes that instrument when the command created it and no remaining row references it; undo never deletes a pre-existing instrument. This intentionally differs from the 2026-06-15 Import Batch Rollback Semantics, which leave batch-created empty instruments in place.
Context: At single-user local scale, keeping every command row is simpler and keeps undo/effects references trivially correct; bounding belongs in the UI (recent-activity list) and in a later cleanup pass, not in the hot write path. A quick manual add-then-undo should leave no orphan instrument, but the undo payload must record whether the instrument was newly created versus reused.
Consequences: Undo must capture instrument-creation provenance. Pruning the command log is a deliberate later feature that must never drop a record still reachable as an undo/redo target. Import rollback and manual-command undo deliberately treat empty instruments differently; revisit both rules when the general cleanup function lands.

## 2026-06-16: Atomic Manual-Transaction Endpoint And Command Undo Preconditions
Decision: Manual entry with a new instrument is one atomic command exposed by extending `POST /api/transactions` additively to accept either an `instrument_id` or a `new_instrument` object, leaving the response shape unchanged; standalone `POST /api/instruments` stays a normal, non-undoable resource mutation. Undo is gated on expected-current-state preconditions: it verifies the command's effects are still present (current rows match the command's stored post-state by id and row snapshot/hash) before applying the inverse — returning `409 undo_blocked` otherwise, `409 not_latest_undoable` for non-latest command-id undo in the MVP, and `422` with the existing ledger error code when the validated inverse would itself break ledger rules. Sharesight import commands store normalized inserted-row snapshots and block undo when the batch no longer matches; the legacy `rollback/{batch_id}` best-effort path is retained only for pre-command batches. The first command schema omits a stored error column, carries a `payload_version` with tagged-JSON payloads, cascades `command_effects` on command delete, and makes retries idempotent via `client_command_id` plus `client_payload_hash` for conflict detection.
Context: The review in `docs/reviews/Review.CommandUndo.md` flagged that the atomic-write path, undo preconditions, latest-only constraint, idempotency, payload versioning, and effect cleanup needed settling before the design becomes an implementation plan.
Consequences: Import commands must persist normalized row snapshots (accepted private-data storage). Undo handlers must compare current rows against stored post-state before writing the inverse. Redo branch state stays additive and intentionally out of the first schema. Revisit the legacy rollback path once all live batches are command-backed.

## 2026-06-16: Market Data Staleness Display Rules
Decision: Valuation always uses the last available close/rate on or before the valuation date. The application first attempts to retrieve the current price; if that fetch succeeds the data is fresh. If the fetch fails and the cached price/FX is older than 2 trading days, the UI shows a clear visual warning (a prominent stale chip). If the fetch fails but the cached data is within 2 trading days, the UI shows only a minor icon. Staleness is measured in trading days (weekends/holidays do not count), and a stale value is always shown, never hidden or rendered as zero. This sets the concrete threshold left open in `docs/plans/Plan.phase-3-prices-fx-gains.md`.
Context: A calendar-day threshold would either flag every normal weekend or mask multi-day provider gaps. Trading-day staleness with a 2-day window distinguishes a normal market closure from a genuine fetch problem, and the two-tier minor-icon/warning split keeps routine closures quiet while making real gaps obvious.
Consequences: The valuation module needs a trading-day-aware staleness policy and must emit a tri-state freshness signal (fresh, minor-stale, warning-stale) rather than a single boolean. The frontend renders a minor icon versus a prominent chip based on that signal. A trading-day count needs a market-calendar approximation (at minimum weekend-aware); revisit holiday handling if false warnings appear.

## 2026-06-16: Market Data Refresh Triggering
Decision: Market-data refresh is not scheduled or time-based. Instead the application fetches fresh prices/FX as a background job on application launch, showing an animated loading indicator while it runs, and exposes a manual refresh action for sessions that stay open long enough for data to age. Only one refresh runs at a time. This replaces the scheduled-refresh default proposed in `docs/plans/Plan.phase-3-prices-fx-gains.md`.
Context: The home PC is not active around the clock, so a weekday-evening cron-style refresh would frequently miss or run against a closed machine. Refreshing at launch matches actual usage, and a manual button covers long-lived sessions.
Consequences: Phase 3 ships a launch-triggered background refresh plus a manual refresh action; no tokio scheduler or configured refresh time is added. The single-flight guard and last-refresh status surfacing still apply. Reconsider a scheduler only if the deployment model changes to an always-on host.

## 2026-06-16: Market Data Provider Dispatch And HTTP Client
Decision: Phase 3 market-data providers use object-safe async traits through `async-trait`, with provider handles stored as `Arc<dyn PriceProvider + Send + Sync>` and `Arc<dyn FxRateProvider + Send + Sync>`. Live HTTP calls use `reqwest` with `default-features = false` and the `json` and `rustls-tls` features.
Context: The refresh API and launch refresh need testable provider injection without live network calls in normal tests. Native async trait methods in Rust 2021 are not dyn-compatible enough for this use without a dispatch choice, and the backend did not yet have an HTTP client dependency.
Consequences: Backend implementation adds `async-trait` and `reqwest`. Provider traits stay small and object-safe. Tests inject fake providers or fixture parsers, while production startup injects Yahoo and Frankfurter clients.

## 2026-06-16: Market Data Service Injection And Single-Flight Refresh
Decision: `AppState` carries a cloneable market-data service handle in addition to the SQLite pool. The service owns injected provider handles and a refresh single-flight guard shared by launch refresh and manual refresh. API handlers and launch jobs call the service; they do not construct live providers directly. Test constructors inject fake providers.
Context: The current `AppState` only contains `SqlitePool`, but Phase 3 needs the same refresh logic to be reachable from HTTP handlers, startup background work, and offline API tests. The launch/manual refresh model also requires one shared place to prevent concurrent refresh runs.
Consequences: `AppState` grows beyond persistence state. `AppState::for_tests()` or an equivalent builder must provide fake market-data providers. A second refresh request reports the current running status instead of starting another provider run.

## 2026-06-16: Market Data Staleness Calendar Approximation
Decision: Phase 3 implements trading-day staleness by counting Monday-Friday business days between the cached market-data date and the valuation date. Exchange-specific public holidays are not modeled until a market-calendar source is added.
Context: The staleness display rule requires trading-day counting, but the repository has no market calendar. A weekday approximation is testable and avoids weekend false warnings, while keeping the implementation small for v1.
Consequences: Valuation tests pin weekday behavior and document the holiday limitation. Holiday weeks can overcount staleness and may show a warning one or more days early; add exchange calendars later if this creates noisy UI warnings.

## 2026-06-17: Board View Roles And Gain Attribution
Decision: The three board views each answer one question and must not duplicate each other. Holdings answers "where is my money?" and emphasizes exposure: position size, current value, cost basis, currency, and portfolio weight, defaulting to market value descending. Gains answers "what made/lost money?" and emphasizes performance attribution — splitting each holding's gain into a price effect and an FX effect — defaulting to total gain descending. Transactions answers "what happened?" (buys, sells, dividends, fees, dates) defaulting to date descending. The price/FX split is a derived calculation in the valuation domain from existing native value and FX data, with the cross-term and brokerage fees both assigned to price effect (native gain valued at the current FX rate, fees subtracted there) so the FX effect stays pure currency movement and never reflects a fee; the two effects always sum to the existing unrealized gain. Day-change splitting, a time-period selector, and dividend/fee/realized-gain attribution are deferred to later phases.
Context: The Holdings and Gains views had grown to show nearly the same columns (market value, cost basis, unrealized gain, day change), so the second view added little. The 2026-06-13 decision to keep native value and FX separate makes a currency-aware attribution possible without new storage. See `docs/Design.HoldingsGainsViews.md` for the durable design.
Consequences: Gains adds price-effect and FX-effect fields on each row and in the summary, surfaced additively through the gains API; Holdings adds a portfolio-weight column and drops the gains-style latest-close duplication while keeping quantity (position size is core to the Holdings exposure framing). Attribution stays a pure, unit-testable valuation calculation. The deferred items (day split, period selector, dividends/realized gains) each remain separate future phases, not part of this change.

## 2026-06-17: ISIN Backfill On Matching Instruments
Decision: When an import or create path supplies an ISIN and finds an existing symbol-matched instrument with a null `isin`, the repository backfills that row's `isin` instead of creating a duplicate. If symbol and ISIN lookups resolve to different instruments, the write fails as an identity conflict.
Context: Avanza instruments are identified by ISIN, but the database can already contain symbol-matched rows from earlier work. The repository needs a deterministic way to converge onto one row without silently splitting identity.
Consequences: ISIN-aware writes can reuse or repair an existing row when the match is unambiguous. Mixed or conflicting identity data becomes a hard error rather than an implicit merge.

## 2026-06-18: Display-Only Weights May Use Frontend Float Math
Decision: Presentational, derived display values that are not money of record — starting with the Holdings `Portfolio %` column — may be computed in JavaScript floating point on the frontend by parsing decimal strings, rather than going through the exact `rust_decimal`/TEXT money pipeline. They are display approximations, are allowed to show rounding drift, and must not feed back into stored values or the money pipeline.
Context: The 2026-06-14 Backend Persistence Stack decision commits money, prices, FX, and quantities to exact decimals round-tripped as strings. Holdings `Portfolio %` is a derived weight for visual sizing only and is intentionally computed client-side from summed market values, which diverges from that convention and could otherwise be "corrected" into the money pipeline by a future reader.
Consequences: Frontend weight/percentage display can stay simple and local. Such values must never be persisted, sent back to the backend as authoritative, or used where exact equality matters. Any figure that becomes money of record must move to the exact-decimal pipeline. Revisit if display rounding ever becomes user-visibly misleading.
