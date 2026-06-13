# Phase 0 Plan: Spikes & Skeleton

**Status:** Draft
**Created:** 2026-06-12
**Source design:** [docs/Design.HighLevel.md](../Design.HighLevel.md)
**Primary private fixture:** `docs/AllTradesReport_2026-06-12.csv` exists locally for Phase 0, but must remain git-ignored because it contains private portfolio data.

## Purpose

Phase 0 should turn the high-level portfolio tracker design into a runnable, low-risk project skeleton and answer the import, market-data, currency, and documentation questions that would otherwise create churn in Phase 1 and Phase 2.

The phase ends when:

- The repo builds from a clean checkout.
- The frontend and backend can run in development with a minimal health/API flow.
- The real Sharesight All Trades CSV has been parsed by a throwaway spike.
- A primary end-of-day price provider has been chosen from at least two candidates.
- The base-currency and FX interpretation rules are documented.
- The decisions and remaining risks are captured in project docs.

## Inputs Already Confirmed

The current Sharesight export is a real, complete Phase 0 fixture and is sufficient to start importer investigation. It must not be committed to the repository.

Observed from `AllTradesReport_2026-06-12.csv`:

- Report range line says `between 2025-06-12 and 2026-06-12`.
- Data rows: 189.
- Types: 105 `Buy`, 83 `Sell`, 1 `Split`.
- Markets: NASDAQ, NYSE, EURONEXT, XETR, FRA.
- Instrument currencies: USD and EUR.
- Brokerage currency: SEK for every row.
- The report includes no dividend/income rows.
- Date format appears to be `dd/mm/yyyy`.
- Decimal format is comma decimal, with non-breaking spaces as thousands separators in some values.
- Sells use a Unicode minus sign in `Quantity` and negative `Value`.
- The split row uses type `Split`, zero price/value, and comment `Automated Trade - Split 1.0:5.0`.
- There is an unnamed column containing `All Trades`, separate from `Comments`.

## Resolved Decisions

These answers came from Lars after the first draft of this plan.

- Base currency for v1 is SEK.
- `AllTradesReport_2026-06-12.csv` is the complete Sharesight trade history for the portfolio.
- Dividends can be entered manually in v1; the import path only needs to handle securities transactions.
- v1 does not need cash, deposits, or withdrawals.
- v1 does not need fractional shares.
- v1 is a single-portfolio application.
- An unofficial Yahoo Finance source is acceptable for v1 if the provider spike supports it.
- The price-provider spike should use one USD asset and one EUR asset from the export.
- LAN trust is sufficient for v1; no authentication is needed in Phase 0.
- The project layout should be top-level `backend/` and `frontend/`, without a Rust workspace unless later reuse justifies it.
- Sharesight exports are private and must not be saved to the repository.
- For split import, use "quantity delta" as the working hypothesis and verify it against Sharesight's current position.

## Documentation Bootstrap Status

The repo now has the planning/supporting documents referenced by the repo instructions:

- `EngineeringDiary.md`
- `docs/DecisionLog.md`

Phase 0 should review and extend these documents as implementation work reveals better workflow details.

## Remaining External Input

No external input is currently blocking the Sharesight import spike.

Resolved during implementation:

1. **Which current Sharesight position should be used to verify the split row?**
   Lars reran the spike with the current Sharesight position for the split
   holding. The provided position matched the summed export quantity, confirming
   quantity-delta semantics.

## Standing Assumptions

- Use exact decimals for money from the first real implementation.
- Use integer quantities for v1 securities because fractional shares are not required.
- Keep authentication out of Phase 0 and v1 unless the deployment model changes.
- Keep `domain/` free of axum, sqlx, HTTP, and provider-specific types.

## Phase Structure

Phase 0 is split into small increments. Each increment should be independently reviewable and should leave the repo in a buildable or documentation-improved state.

### 0.1 Documentation Review (COMPLETE)

**Goal:** Make sure the bootstrapped project-management docs are useful before implementation decisions accumulate.

### 0.2 Repo Skeleton Decision (COMPLETE)

**Goal:** Choose and document the project layout before scaffolding code.

Layout:

```text
backend/
  Cargo.toml
  migrations/
  src/
    main.rs
    api/
    domain/
    providers/
    import/
    db/
frontend/
  package.json
  vite.config.ts
  src/
docs/
  plans/
```

Rationale:

- Matches the actual deployment boundary: one Rust backend binary and one TypeScript frontend.
- Avoids a Rust workspace until there are multiple Rust crates that need shared versioning or reuse.
- Keeps `domain/` available for pure ledger, valuation, FX, and split logic without axum, sqlx, or HTTP types.
- Leaves a later workspace migration mechanical if a CLI, reusable domain crate, or compile-time pressure justifies it.
- Avoids inheriting the unrelated `crates/qsf_browser_server/ui/` path from the generic repo instructions.

Tasks:

- Document the chosen layout in `docs/DecisionLog.md`.
- Update repo instructions later so backend checks run from `backend/` and frontend checks run from `frontend/`.
- Add a minimal backend crate using axum and tokio.
- Add a minimal Vite React TypeScript frontend.

Verification:

- `cargo build` succeeds from `backend/`.
- `cargo fmt --check` succeeds from `backend/` after initial scaffolding.
- `npm install` and `npm run check` succeed from `frontend/`.
- `npm run fmt -- --check` or the configured formatter check succeeds from `frontend/` if available.

### 0.3 Backend Skeleton (COMPLETE)

**Goal:** Establish a thin, runnable Rust backend that can host API routes and eventually static frontend assets.

Tasks:

- Create a backend binary with `main.rs` kept thin.
- Add an application module that wires router creation without embedding business logic in the entry point.
- Add endpoints:
  - `GET /api/health` returning status and version/build info.
  - `GET /api/import/sharesight/schema-preview` or a temporary equivalent for spike output if useful.
- Add structured logging using the repo-approved logging approach. If the current repo instruction names a logging crate that is unavailable, update the instruction during 0.1 and use the agreed replacement, preferably standard `tracing`/`tracing-subscriber`.
- Add config loading for host/port with sensible local defaults.
- Add CORS or Vite proxy compatibility for local development.

Verification:

- `cargo build` from `backend/`.
- `cargo clippy --all-targets -- -D warnings` from `backend/`.
- A local request to `http://127.0.0.1:8080/api/health` returns JSON.
- Backend shutdown via Ctrl+C is clean.

Human testing recommended:

- Open the health endpoint from another LAN device only if LAN firewall behavior matters during Phase 0.

### 0.4 Frontend Skeleton

**Goal:** Establish the actual app shell, not a marketing page.

Tasks:

- Create Vite + React + TypeScript app.
- Implement approved dark theme foundation from docs/VisualDesign.DarkTheme.md.
- Configure the dev server proxy so `/api` reaches the Rust backend.
- Add TanStack Query.
- Configure TypeScript checking and the chosen frontend formatter/linter. Use
  Biome if `npm run check` is expected to cover both TypeScript and linting;
  otherwise update the repo instructions during 0.1.
- Add a minimal app shell with a holdings/transactions-oriented first screen.
- Add a health/API status query so frontend-backend connectivity is visible during development.
- Keep TanStack Query responsible for server cache state. Model non-trivial
  client interaction state with explicit actions and reducer-style transitions
  so the UI follows the repo's unidirectional data-flow rule.
- Keep UI compact and operational; no landing-page hero.
- Add package scripts:
  - `npm run dev`
  - `npm run build`
  - `npm run check`
  - `npm run fmt`

Verification:

- `npm run check`.
- `npm run fmt`.
- `npm run build`.
- With backend running, frontend displays the API health result.
- Browser console has no errors.

Human testing recommended:

- Lars should open the app on desktop and phone/tablet viewport to confirm the basic shell direction before Phase 1 UI work.

### 0.5 Sharesight Import Spike

**Goal:** Prove the real export can be parsed robustly and identify exact mapping rules before building the import feature.

This spike can be a throwaway Rust command, a temporary backend test fixture, or a script. Prefer Rust if it informs the eventual parser; prefer a script if speed matters. Do not build a permanent import subsystem in Phase 0.

Tasks:

- Parse the title/report metadata line.
- Locate the header row instead of assuming it is always line 3.
- Parse rows into a temporary normalized structure:
  - market
  - code
  - name
  - transaction type
  - trade date
  - quantity
  - price
  - instrument currency
  - cost base per share in SEK
  - brokerage
  - brokerage currency
  - exchange rate
  - value
  - report/source column
  - comments
- Handle comma decimals.
- Handle non-breaking-space thousands separators.
- Handle Unicode minus signs.
- Validate all rows have known transaction types.
- Summarize unique instruments and markets.
- Summarize buy/sell/split counts.
- Verify whether all brokerage is SEK.
- Verify whether `Value` sign is consistent with `Type`.
- Treat split quantity as a quantity delta at the import/ledger layer as the working hypothesis.
- Verify split semantics by summing all quantity columns for the split holding across the complete export and comparing the result to Sharesight's current position.
- Use a Sharesight current position captured at the same time as the export for
  the split invariant, or explicitly account for any later `NOW` trades before
  comparing quantities.
- If the summed position matches Sharesight, keep delta semantics. If it overshoots by the split row amount, treat the export row as resulting-shares semantics. If neither matches, document the mismatch before implementing import.
- For engine logic, derive split ratio when needed as `(position_before + delta) / position_before` and validate it as a clean rational ratio, for example `2/1`, `4/1`, `7/1`, or `1/10`.
- Add a design note that providers such as Yahoo may serve split-adjusted historical prices while Sharesight transaction prices are unadjusted. Valuation should use `derived_quantity(date) * close(date)`; buy-price markers on charts will need ratio-adjusted marker prices later.
- Produce only sanitized spike output under versioned `docs/spikes/`, or put private row-level output under ignored `docs/spikes/private/`. Record the summary in `EngineeringDiary.md`.

Candidate transaction mapping for later phases:

| Sharesight type | Ledger type | Notes |
|---|---|---|
| `Buy` | `BUY` | Quantity positive; value in SEK appears positive. |
| `Sell` | `SELL` | Quantity and value are negative in export; ledger should normalize sign rules explicitly. |
| `Split` | `SPLIT` | Store the source quantity as a delta if the position invariant verifies it; derive ratio in engine code when needed. |

Questions the spike must answer:

- Does the `Exchange Rate` column mean foreign currency per SEK, SEK per foreign currency, or the inverse of the displayed conversion rate?
- Does `Value` include brokerage?
- Does `Cost base per share (SEK)` being `0,00` mean unavailable for all rows, or is this export configuration omitting it?
- Can `Market + Code` uniquely identify all instruments in the report?
- Are there rows with blank comments, duplicate rows, or same-day same-instrument partial fills that should remain separate ledger entries?
- Does the `NOW` split position invariant confirm quantity-delta semantics? Yes.

Verification:

- Parser handles all 189 current rows without panics or skipped data.
- Parsed counts match the observed counts in this plan.
- A generated sanitized summary is checked into docs or recorded in the diary.
- At least one sample row of each type is manually compared to the raw CSV.
- The raw CSV and any row-level private outputs remain untracked by git.

Human testing recommended:

- Lars should compare spike totals and sample rows against Sharesight's UI/export view.
- Lars should provide Sharesight's current `NOW` position for the split invariant check, or run the comparison in Sharesight during the spike review.

### 0.6 Price Provider Spike (COMPLETE)

**Goal:** Pick a primary EOD price provider and identify fallback/provider-trait requirements.

Candidate providers:

- Yahoo Finance unofficial endpoint or crate.
- Alpha Vantage or Twelve Data free tier.
- EODHD or another paid provider if reliability/coverage is worth it.
- ECB reference rates for FX if instrument-price provider FX is weak.

Representative instruments:

- USD asset: `MSFT` from NASDAQ.
- EUR asset: `ASML` from EURONEXT.

Tasks:

- For each candidate provider, fetch latest available EOD close for `MSFT` and `ASML`.
- Fetch a short historical range for at least one instrument.
- Fetch or identify FX source for USD/SEK and EUR/SEK.
- Record provider symbol mappings for each representative instrument.
- Record API authentication requirements.
- Record rate limits, licensing constraints, and reliability concerns.
- Decide how missing prices should be represented in the app.
- Pick a primary provider and one fallback strategy.

Provider evaluation criteria:

- Covers US stocks and EURONEXT assets for the current complete export.
- Supports EOD close and historical daily data.
- Has acceptable API terms for self-hosted personal use.
- Has stable symbol conventions that can be stored per instrument.
- Supports enough free or low-cost calls for daily refresh and backfill.
- Allows deterministic tests through recorded fixtures.

Verification:

- A spike note documents exact request URLs or client calls, response shape, and sample normalized output.
- At least two providers are compared against the same instruments.
- The chosen provider is recorded in `docs/DecisionLog.md`.

Human testing recommended:

- Lars should approve paid-provider use if the best option requires a subscription or API key.

### 0.7 Base Currency And FX Rules

**Goal:** Remove ambiguity from multi-currency math before Phase 1 schema and Phase 3 valuation.

Tasks:

- Record SEK as the confirmed base currency.
- Decide how to store exchange rates:
  - recommended canonical storage: quote currency to base currency rate, e.g. `USD -> SEK`.
- Interpret Sharesight `Exchange Rate` using sample row math.
- Decide whether imported transaction `fx_rate_to_base` should use Sharesight's value directly or an inverted/derived value.
- Decide how to model brokerage paid in SEK when the instrument is USD/EUR.
- Decide rounding rules for display and persisted decimals.
- Document cost-basis rules for buys and sells at a high level.
- Document that v1 ignores cash/deposit/withdrawal ledger flows and tracks securities transactions only.

Currency-gain attribution guardrail (forward-compatibility, even though the
breakdown itself is post-v1):

- Store native value and FX rate as separate fields; never persist only the
  SEK-converted value. A transaction keeps native `price` + `currency` plus a
  separate trade-date `fx_rate_to_base`; `prices` keeps native `close`, and
  `fx_rates` keeps dated rates. Convert at the read/valuation boundary, not at
  write time, so the currency component of return stays recoverable later.
- Do not import Sharesight's pre-computed `Cost base per share (SEK)` as the
  primary cost basis. Import the native price plus the export's `Exchange Rate`
  and derive SEK; importing the SEK column directly bakes in FX and discards the
  native/FX split.
- FX must be backfilled historically (back to the earliest trade date), not just
  the latest day, so a past valuation date can be converted at its own rate.
- Brokerage is paid in SEK while trades are USD/EUR. A SEK fee has no currency
  component, so it must not be run through the trade FX. Decide deliberately
  whether this needs a separate fees currency rather than letting one `currency`
  column force a choice.

Verification:

- A worked example from the CSV shows native trade value, brokerage, FX conversion, and SEK value.
- Rules are documented in `docs/DecisionLog.md` or a dedicated design note.
- The rules can be translated into unit tests in Phase 1.

Human testing recommended:

- Lars should confirm the worked example matches how Sharesight and the broker present the trade.

### 0.8 Static Serving And Dev Workflow

**Goal:** Make local development and future production serving concrete.

Tasks:

- Configure frontend dev proxy to backend API.
- Decide whether production serves frontend from disk initially or embeds assets later.
- Add backend static-file serving only if it does not distract from Phase 0; otherwise document it for Phase 5.
- Document local commands in `README.md`.

Verification:

- Fresh setup instructions work on Windows.
- Backend and frontend can be run simultaneously.
- `README.md` lists exact commands for build, checks, and dev servers.

Human testing recommended:

- Lars should follow the README once from a clean terminal to catch missing prerequisites.

### 0.9 CI Or Local Check Script

**Goal:** Make the expected checks repeatable.

Tasks:

- Add GitHub Actions CI if the repo is hosted on GitHub and CI is desired immediately.
- Or add a local `justfile`, PowerShell script, or documented command sequence if CI is not ready.
- Ensure checks include:
  - `cargo build` from `backend/`
  - `cargo clippy --all-targets -- -D warnings` from `backend/`
  - `cargo fmt --check` from `backend/`
  - frontend `npm run check`
  - frontend formatter check or `npm run fmt`

Verification:

- CI passes on the skeleton branch, or local check script passes from a clean shell.
- Check commands are documented in `README.md`.

Human testing recommended:

- Lars should decide whether GitHub Actions is wanted before code is pushed.

### 0.10 Phase 0 Closeout

**Goal:** Leave clear handoff material for Phase 1.

Tasks:

- Update `EngineeringDiary.md` with what was implemented and verified.
- Update `docs/DecisionLog.md` with:
  - repo layout
  - base currency
  - price provider choice
  - import parsing conventions
  - private fixture handling
- Update `Design.HighLevel.md` if Phase 0 materially changes the architecture.
- Reconcile `Design.HighLevel.md` with the import spike result: if CSV is the
  supported Sharesight import format, update the design to say so directly and
  remove XLS/`calamine` as an implied implementation requirement.
- Create or update the Phase 1 plan if needed.
- List unresolved questions explicitly.

Verification:

- All Phase 0 exit criteria are checked.
- Check commands have been run and results recorded.
- Remaining open questions are visible in docs, not buried in chat.

Human testing recommended:

- Lars should review closeout docs before Phase 1 schema work begins.

## Exit Criteria Checklist

- [ ] Documentation review completed.
- [ ] Repo instructions reconciled with the chosen backend/frontend layout and logging approach.
- [ ] Repo layout chosen and recorded.
- [ ] Rust backend skeleton builds.
- [ ] Frontend skeleton checks and builds.
- [ ] Backend `/api/health` works locally.
- [ ] Frontend can query backend health through dev proxy.
- [ ] Sharesight CSV spike parses all current rows.
- [ ] Sharesight import findings documented.
- [ ] Design import-format wording reconciled with the import spike result.
- [x] At least two price providers tested.
- [x] Primary price provider chosen and recorded.
- [ ] Base currency and FX rules documented with one worked CSV example.
- [ ] README contains local dev and check commands.
- [ ] Phase 0 diary entry written.
- [ ] Phase 1 risks and prerequisites documented.

## Suggested Order Of Work

1. Documentation review.
2. Repo skeleton decision.
3. Backend and frontend skeleton.
4. Sharesight import spike.
5. Price provider spike.
6. Base currency and FX rules.
7. README/check workflow.
8. Phase 0 closeout.

This order makes the repo runnable early, then spends the rest of the phase reducing the import and market-data risks that drive the later design.
