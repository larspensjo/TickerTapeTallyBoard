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

## 2026-06-17: Import Exclusion Commit Semantics
Decision: Import commit exclusion drops whole assets before validation and reports commit counts for the retained, effective set. Written transaction counts are derived from mapped rows, while deferred dividends are counted from retained asset groups because they are never written. Duplicate detection still uses the original raw file hash, not the post-exclusion effective set.
Context: Whole-asset exclusion allows a user to commit valid assets from a file while dropping an asset with mapper or ledger errors. Phase 5 review clarified that dividend counts must follow the retained assets even though source counts remain unchanged internally.
Consequences: Re-importing the same raw file after a partial commit is treated as a duplicate unless `allow_duplicate=true`; using that override can reinsert already-committed assets. A future import redo or partial-commit model should use normalized effective-row snapshots or a filtered hash if users need to import excluded assets later from the same file.

## 2026-06-16 - Avanza CSV import
Decision: Avanza "AllTradesReport" CSV is a first-class import source alongside Sharesight, sharing a source-neutral import core (per-source parser+mapper -> row outcomes -> shared planner/writer). Instrument identity is ISIN (nullable on `instruments`, partial-unique); Avanza creates instruments with `exchange = "AVANZA"`, `symbol = ISIN`. Dividends are parsed and counted but not written; fractional/fund quantities are skipped with a warning; unsettled rows are imported (FX absent -> `missing_fx`); unknown transaction types are skipped, not fatal. Splits are netted per `(date, ISIN)` and never create an instrument. A whole-asset deselect/exclude path applies before validation so a user can commit the remainder of a file. No cross-source reconciliation; the user converges on one source.
Context: Avanza is the real source of truth; Sharesight data was copied from it.
Consequences: `import_batches.source` allows `AVANZA`; `source_currency` is persisted from the row (native for unsettled rows); overlapping settled/unsettled re-imports can double-count, so mitigation is per-batch rollback and deselect. Cross-source merge and fractional quantities remain out of scope.

## 2026-06-18 - Closed Positions In Gains
Decision: The Gains view shall optionally include fully closed instruments as realized-gain rows derived from weighted-average ledger cost at each sell. The default remains open positions only, and the Gains summary remains based on current open positions so current portfolio value is not mixed with historical sale proceeds.
Context: Sharesight exposes an "include closed positions" toggle, but this app's current Gains summary and top portfolio total are defined around open holdings and unrealized attribution.
Consequences: Closed rows reuse the existing Gains columns for realized cost, sale proceeds, and realized gain where FX is available. Partially sold positions that remain open still show their current open-position unrealized gain only; period selection, dividend income, and fuller realized-gain attribution remain separate future work.

## 2026-06-18 - Gains Totals Are View-Scoped
Decision: The Gains page has its own totals strip for capital gain, income, currency gain, and total return. These totals follow the active Gains filter, so they include closed realized rows only when "Include closed positions" is enabled. The top portfolio summary remains current-position based.
Context: Sharesight presents performance totals directly above the investment table, and those totals change with closed-position inclusion. This is useful on the Gains page but should not redefine the app-wide current portfolio value.
Consequences: Gains API responses expose a `totals` object separate from `summary`. Income is explicitly unavailable until dividend/income accounting is implemented, rather than displayed as zero.

## 2026-06-19 - Client-Side Routing And Asset Detail Page
Decision: The frontend adopts `react-router-dom`. The board (`/`), import (`/import`), and a new per-asset detail page (`/asset/:id`) become real routes, replacing the in-memory `appView` switch; the portfolio totals band, dev status strip, and app-bar actions are scoped to the board route. The asset page is a full-page, linkable destination (browser Back/Forward, bookmark, reload) reached by making the shared `InstrumentCell` a link, intended to grow into a per-asset editing surface. Its first iteration is a read-only data shell composed only from existing endpoints (`/api/instruments`, `/api/gains?include_closed=true`, `/api/holdings`, `/api/transactions`, `/api/prices/status`) with no backend changes; it reserves a band for a future price chart and omits per-asset income/total return because income is not yet tracked.
Context: Clicking an asset should surface all of its information in one place, and the view is expected to host future edits and a price chart. A full page implies a real, linkable destination, which the current reducer-only navigation cannot provide.
Consequences: Navigation moves from `appView` reducer state to routes; `boardView`, `boardFilter`, `includeClosedPositions`, and `formOpen` stay board-local. The price-history endpoint plus chart, per-asset income/total return, and asset-page editing are deferred to their own specs. See `docs/plans/Spec.asset-detail-page.md`.

## 2026-06-19: Sharesight-Style Performance Returns Method

Decision: The Gains view uses Modified Dietz money-weighted performance returns for totals and row-level `capital_gain`, `currency_gain`, and `total_return` fields, replacing cost-basis percentages in the comparison-oriented Gains table. The denominator uses calendar-day weights. Component percentages (capital, currency) share the Modified Dietz denominator but are not individually annualised; only `total_return_percent` is annualised when average years invested ≥ 1. Period years are approximated as `period_days / 365.25`.
Context: Sharesight documents its Performance Report as dollar-weighted / money-weighted using a Modified Dietz variation. Cost-basis ratios were misleading when compared against Sharesight exports.
Consequences: The Gains API accepts `start_date` and `end_date` query params; missing prices/FX surface as explicit unavailable reasons, never zero. Income/dividends remain unavailable until Phase 5. The existing row-level `unrealized_*`, `price_effect_base`, and `fx_effect_base` fields remain current-position values for surfaces that need open-exposure semantics.

## 2026-06-19: Period Reconstruction Boundary Convention

Decision: For period performance, transactions strictly before `start_date` form the opening position; transactions with `trade_date ∈ [start_date, end_date]` are period cash flows. A buy on the first day of the report is a cash inflow, not an opening balance. Transactions after `end_date` are excluded entirely (ledger truncation).
Context: This matches the spec's initial recommendation and the intuition that a purchase on the start date is a decision made within the reporting window.
Consequences: When `start_date` matches a buy date, the begin market value is zero for those shares. Calibrate this boundary against a Sharesight export before claiming full compatibility.

## 2026-06-19: Split-Adjusted Quantities For Period Valuation

Decision: `split_factor(transactions, opening_qty)` computes the cumulative factor given the quantity already held before the first transaction in the slice. For in-period splits, opening_qty = start_position.quantity; for post-period splits, opening_qty = end_position.quantity. Split factor for a split with delta d when quantity is q: factor = (q+d)/q.
Context: Yahoo historical prices are split-adjusted. A pre-split ledger quantity without adjustment would understate market value when multiplied by a split-adjusted price. Starting running_qty at zero would silently miscalculate factors for pre-period holders.
Consequences: Instruments with splits in or after the report range will have adjusted quantities in the performance computation. Instruments without splits are unaffected (factor = 1).

## 2026-06-19: Performance Amount Formula (Market-Value Identity)

Decision: total_return = end_mv − begin_mv − net_flows. Capital gain = same formula evaluated at constant end_fx. Currency gain = total_return − capital_gain. No held-quantity tracking or FIFO matching required.
Context: A stateful "held vs sold" tracking approach risks negative held quantities when in-period sells exceed the start position. The market-value identity is equivalent and avoids this class of errors.
Consequences: Decomposition into capital and currency gain is correct for all cases including buy-then-sell within the period, mixed pre/in-period sells, and inception mode.

## 2026-06-20: Performance Totals Include Report-Period Closed Activity

Decision: Modified Dietz Gains totals include every instrument with report-period exposure or cash flows, including instruments that are fully closed by the report end date. The "Include closed positions" toggle controls row rendering only; unavailable instruments are excluded from the aggregate totals and counted in `totals.excluded_rows` instead of making the whole totals band unavailable.
Context: Period performance should include realized gains and losses inside the selected period, and one missing start price or FX rate should not blank otherwise computable portfolio totals.
Consequences: Gains totals may include closed-position performance even when the closed row is hidden. The incomplete chip indicates how many instruments were excluded from performance aggregation due to missing valuation inputs.

## 2026-06-20: Closed Position Market Value Is Current Exposure

Decision: Fully closed Gains rows report `market_value_native` and `market_value_base` as zero current exposure. Realized sale proceeds are exposed through separate `proceeds_native` and `proceeds_base` fields.
Context: Closed rows previously reused the market-value fields for sale proceeds, which made the Gains table show non-zero "Market value" for positions with quantity zero.
Consequences: Market-value totals remain current-exposure totals even when closed rows are visible. UI surfaces that need realized proceeds must read the explicit proceeds fields instead of inferring them from market value.

## 2026-06-20: Canonical Performance Period For Additive Subtotals

Decision: Every Gains response uses one canonical `report_period.start_date` for Modified Dietz performance. In all-time mode this is the earliest included transaction date in the portfolio; explicit `start_date` queries use that requested date. Row-level performance denominators are computed against this same canonical start so visible-row table footers can sum row amount and denominator contributions.
Context: Modified Dietz percentages are not additive, and denominators computed from different instrument-specific inception dates cannot be safely summed. A Gains footer that filters visible rows needs additive backend-provided row contributions, not frontend-inferred per-row percentages.
Consequences: `report_period.start_date` is populated in all-time mode when the portfolio has included transactions. Frontend subtotals may sum row `total_return_base` and `performance_denominator_base`, then apply the shared report period annualisation rule. Rows whose support fields are unavailable or whose start date does not match the response period are excluded from that subtotal and counted as incomplete.

## 2026-06-21: Selectable Performance Return Method — refines 2026-06-19

Decision: The Gains portfolio total return is user-selectable and persisted via `localStorage` (key `gains.returnMethod`): Money-weighted (XIRR, default), Simple (total return ÷ opening value + gross purchases in period), and Modified Dietz (the prior single-shot method, retained as a labeled comparison/legacy option). XIRR is shown as the cumulative period return `(1+rate)^(days/365.25) − 1`, solved in period-time units so short periods with large returns converge. Per-asset row percentages use a current-position simple return (unrealized gain ÷ cost basis; capital = price effect ÷ cost basis, currency = FX effect ÷ cost basis), independent of the selected total method. The filtered visible-rows subtotal no longer reports a percentage — the method-specific percentage lives in the portfolio totals band only. XIRR annualized rate is `Option` (None when the rate overflows Decimal's range; only the cumulative period return is displayed). The XIRR float solve is display-only and never feeds the money pipeline.
Context: Investigation (`docs/Investigation.PerformancePercentageMethods.md`) showed single-shot Modified Dietz produced ~1,130% vs Sharesight ~421% for a churned holding. XIRR is the default because it is cash-flow neutral — a buy, sell, or transfer at current fair value does not shift the percentage until prices change — Lars's stated hard requirement (verified by the `money_weighted_is_cash_flow_neutral_for_same_day_trade` unit test). The solver was written in period-time fractions (not annualized rate) so short periods with large returns (e.g. 29× over 9 days, which annualizes beyond any finite scan cap) still converge to the correct cumulative return. Formal portfolio-level comparison with Sharesight is deferred; the portfolio is too nascent (single holding, inception to date) for a meaningful multi-period equivalence check. XIRR's neutrality property is the primary rationale for the default regardless of Sharesight match.
Consequences: All three methods are ex-dividend until income is tracked (matching existing gain amounts). Component percentages for XIRR use a proportional split of the total (no single natural denominator). The `performance_denominator_base` field on rows is now cost basis, not a Modified Dietz denominator. Method persistence is client-side localStorage; revisit if a server-side settings store is added. The `gains.returnMethod` localStorage key defaults to `"xirr"`. Unknown `method` query values return `400 invalid_method`. This refines the 2026-06-19 Modified Dietz decision (MD is no longer the only method or the default) and the 2026-06-20 Canonical Performance Period decision (the visible-rows footer no longer aggregates a percentage).

## 2026-06-21 - Per-instrument price-history endpoint conventions
Decision: `GET /api/instruments/{id}/prices` returns one point per stored close date (no synthesis), converts to SEK using the latest FX rate on or before each point's date (carry-forward, loading the full FX pair history so a rate dated before the window still applies), and with no `from`/`to` params returns the instrument's full stored history. A missing or disabled Yahoo price mapping yields `200` with an empty `points` array; cached prices are never served while the mapping is missing or disabled. Native `close` and `fx.rate` serialize at full stored precision; `close_base` uses the 2-decimal money format. `from > to` returns `400 invalid_date_range`, sharing the refresh endpoint's error code.
Context: First slice of Phase 4 charts; feeds the reserved price chart on the asset detail page while reusing valuation's mapping-gating and FX conventions.
Consequences: Portfolio value-over-time and position-market-value-over-time series remain separate future specs. The carry-forward-before-window behaviour relies on `fx_rates::list_for_pair` loading the full pair history, not a windowed range.

## 2026-06-22 - Portfolio value-history endpoint conventions
Decision: `GET /api/portfolio/value-history` reconstructs portfolio value per date on the fly from the ledger, cached prices, and cached FX, with no stored snapshots. The spine is the sorted union of stored price and FX dates on or after the earliest BUY trade date; FX-only dates are included so base-currency value can move on FX-only days. Each instrument's split-adjusted position is valued at the carry-forward close times carry-forward FX on or before the date, normalizing held quantity by the factors of all splits with effective_date after the date. Cached prices are used only behind an enabled Yahoo mapping. A date where every active position lacks price or FX is omitted; points with partial missing inputs are kept and marked `incomplete` with `included_count` and `excluded_count`. `value_base` uses the 2-decimal money format. `from > to` returns `400 invalid_date_range`. Stored-data invariant violations are surfaced as `500 internal` with row or instrument context, never silently dropped as missing market data.
Context: The dashboard value-over-time chart needs a portfolio series that honors the ledger as source of truth and reuses valuation mapping, carry-forward, and split-adjustment conventions.
Consequences: No schema change and no materialized derived truth. The series never contains a spurious zero from an all-excluded date. Allocation and top movers continue to derive from `GET /api/gains`, not this endpoint.

## 2026-06-22 - Dashboard is the landing page
Decision: `/` renders the dashboard with summary tiles, portfolio value chart, top movers, and allocation; the detailed board lives at `/board`. Asset Back links and the import board-tab handoff target `/board`. Top movers rank open rows by available `day_change_percent` (gainers descending, losers ascending, ties by symbol then name); allocation weights use display-only frontend float math over `market_value_base`.
Context: The overview should be the first screen, with detailed holdings/gains/transaction tables one level deeper.
Consequences: Deep links to `/board` are stable. The unrealized-change summary tile is current-exposure oriented (`summary.unrealized_gain_*`), not method-dependent total return.

## 2026-06-22 - Chart Axis And Price-Series Semantics
Decision: Charts must use a zero Y-axis baseline and a calendar-linear X-axis over their visible date range. Portfolio value charts are SEK value charts and start at the portfolio's earliest transaction date. Per-asset price charts are native-currency price charts, not SEK-converted valuation charts; they include native transaction prices on transaction dates when provider close history has gaps, while stored provider closes remain the primary market-price source for provider dates.
Context: Phase 4 chart review and follow-up visual checks showed misleading chart behavior when sparse provider price dates were spaced as adjacent points, when asset prices were plotted in SEK while transactions were shown in native currency, and when transaction dates inside provider-history gaps were absent from the line.
Consequences: Shared chart components must preserve calendar spacing, even when this requires whitespace points between data dates. Asset price chart labels and Y values follow the instrument currency. Missing provider history should not collapse elapsed time or hide available transaction-price anchors, but transaction prices remain user/trade anchors rather than a substitute for complete market price backfill.

## 2026-06-22 - Buy/Sell Markers On The Asset Price Chart
Decision: The per-asset price chart overlays Buy and Sell trade markers (Buy = green up-arrow below the bar, Sell = red down-arrow above the bar) using Lightweight Charts' native `setMarkers`, with a crosshair-driven hover tooltip showing date, quantity, average price, and fee. Markers are always shown (no toggle) and cover Buy and Sell only; Dividend and Split are not marked. Same-day, same-side trades merge into one marker: quantities sum, price is quantity-weighted, and sell magnitude uses `abs(quantity)`. Trades whose `currency` differs from the instrument's native currency are skipped (mirroring the native-currency transaction-price anchors). The fee is labeled in its own `brokerage_currency` (typically SEK even for a USD instrument), not the chart currency; merged fees only sum when their fee currencies match, otherwise the fee is omitted rather than mislabeled. Marker derivation is a pure, unit-tested selector (`tradeMarkers`) feeding a presentational chart wrapper.
Context: Trade context on the price chart helps relate price movement to one's own buys and sells. An initial implementation reused the instrument currency for the fee and filtered out sells because sell quantities are stored negative.
Consequences: Sell markers render correctly from negative ledger quantities. Fee currency is honored per the brokerage-currency convention. Adding Dividend/Split markers, same-day fee-currency conversion, or a marker visibility toggle remains future work.

## 2026-06-22 - Gains Breakdown Is A Waterfall; Realized Gain On Open Rows
Decision: The asset view's "Gains breakdown" panel is a stacked waterfall built from one `GainsRow`: cost basis → price effect → FX effect → subtotal (market value for open, proceeds for closed) → realized gain (open only) → a dividends placeholder → total return. The total-return bar is a delta floating from the cost-basis baseline so its height equals the return. To support the open-position terminus and a coherent percent column, `GET /api/gains` now serializes `realized_gain_base` and `realized_cost_basis_base` on open rows (= `performance.realized.gain_base` / `.cost_basis_base`, literal `0.00` when never sold); closed rows carry both too (realized gain, and the sold cost basis = their `cost_basis_base`). The per-step "% of cost" column uses population-matched denominators: price/FX effects vs held cost basis, the realized step vs the sold cost basis, and total return vs total capital deployed (held + sold cost basis for open; the closed row's `cost_basis_base` already is the full sold cost basis). A zero numerator renders `0.00%`, never `0/0`. The geometry exposes a `minValue`/`maxValue` domain so a realized loss larger than the held cost basis still renders in-track. The "% of cost" column and the open-position total-return amount (unrealized + realized) are display-only frontend float math under the 2026-06-18 decision, never money of record. Dividends remain an inert placeholder driven by the existing `income_not_tracked` reason — no per-instrument income plumbing yet.
Context: The prior two-line breakdown showed only the unrealized split. The waterfall makes the buildup legible and was chosen from three mockups (`docs/Design.GainsBreakdownWaterfall.md`). The population-matched denominator and below-zero geometry resolve review findings #3 and #4. Fees-as-a-step and real dividend income are deferred follow-on phases.
Consequences: `GainsRow` gains `realized_gain_base` and `realized_cost_basis_base` fields (additive). The asset breakdown reads realized and renders a total-return terminus; the Gains table's existing `total_return_base` open-row semantics (current-position unrealized, 2026-06-21) are unchanged. The waterfall adds four neutral-bar design tokens to the dark theme. Fees split and per-instrument dividend income remain separate future work; when income lands, the dividends placeholder becomes a contributing step.

## 2026-06-23 - Frontend Automated Test Strategy

Decision: Extend the Vitest runner (introduced with the charts work) to cover the frontend's pure logic: the three `useReducer` reducers (import, board, add-transaction), the `assetViewModel` finders/derivations, the `valuationDisplay` pure helpers, the consolidated `parseFiniteNumber`, and the `api/client.ts` error/parse layer. Reducers are exported under explicit names so tests import the `(state, action) => state` contract directly; components keep their internal `useReducer` wiring. Pure-logic tests run in the default `node` Vitest environment; DOM/component tests opt in per-file via a `// @vitest-environment jsdom` pragma (the global default stays `node` for speed). All tests are gated through `npm run check` alongside `tsc` and Biome.
Context: `Agents.md` mandates pure, unit-testable reducers and prefers tests of reducer behavior and public contracts; these were untested before this work.
Consequences: The duplicated `parseFiniteNumber` was consolidated into one exported helper in `valuationDisplay.tsx` (one source of truth). No new dependencies; no runtime behavior change beyond that behavior-preserving extraction. Node's `Response` constructor rejects null-body status codes (204) so client tests use plain mock objects matching the `parse()` interface rather than `new Response()`.

## 2026-06-23 - Dividend Transaction Model

Decision: Dividends use the existing `LedgerTransaction` field layout: `quantity` = shares eligible (positive integer), `price` = dividend per share in the instrument's native currency (positive), `currency` = instrument currency (required), `fx_rate_to_base` = optional SEK conversion (missing FX is stored but makes `income_base` unavailable, consistent with buy/sell behavior). Brokerage is rejected. Position quantity effect is zero (dividends do not change share count). `income_base` is added to `PeriodAmounts` as the sum of all period dividend cash flows in base currency; `currency_gain = total_return − capital_gain − income` so all three components remain additive. Dividend cash flows are negative in Modified Dietz and XIRR series (investor receives the cash). Import adapters (Avanza, Sharesight) continue to skip dividends until a future plan enables import.
Context: ISK account; no per-dividend tax calculation needed. Reusing existing fields avoids a schema migration. Missing FX is accepted at write time (lazy — user can supply later) rather than rejected eagerly, matching the existing buy/sell contract. The import-skip path is unchanged to keep this plan self-contained.
Consequences: Manually-entered dividends are immediately surfaced in Gains income column and the waterfall. Import adapters need a separate follow-on plan to map parsed dividend rows to the new validation path.

