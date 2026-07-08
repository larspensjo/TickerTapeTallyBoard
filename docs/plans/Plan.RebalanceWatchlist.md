# Plan: Watchlist for rebalance-eligible new assets

**Status:** Draft plan, ready for external review.

## Goal

Let the user designate instruments they do not currently hold (closed positions
or brand-new ideas) with a conviction level so they join the shared
conviction-target pool and the rebalance planner can propose buying into them.

Membership is **derived, not stored**: the watchlist is exactly the set of
instruments whose derived `position.quantity == 0`. Closing a position moves it
onto the watchlist automatically; creating an instrument adds it automatically.
A watchlist instrument joins the target pool when it has a Low/Medium/High
conviction and a strictly-positive available price. Pool membership is **global**
— convicting a watchlist item shifts every open holding's target even when
watchlist rows are hidden — so the pool is computed the same way everywhere and
the Holdings toggle controls only which rows render, never the target math.

## Architecture and constraints

- Unidirectional flow (input → action → reducer → state → render); side effects
  isolated; pure, unit-testable reducers; pure selectors/view-models; thin entry
  points; DRY shared constants.
- One pool definition everywhere. The pool-membership predicate in
  `backend/src/domain/conviction.rs` is the single source of truth, consumed by
  Holdings target derivation and the rebalance planner alike.
- Conviction semantics per `docs/Design.ConvictionTargets.md` (its "Closed
  Positions" section already anticipates this feature).
- Planner spec `docs/Design.RebalancePlanner.md` is updated where contracts
  change.
- `POST /api/instruments` stays a pure, non-undoable DB mutation (DecisionLog
  2026-06-16); provider validation lives in the frontend against a read-only
  lookup surface, keeping offline tests and demo mode untouched.

### Verification commands (every phase)

- Backend, from `backend/`: `cargo build`, then
  `cargo clippy --all-targets -- -D warnings`, then `cargo fmt`.
- Frontend, from `frontend/`: `npm run check`, then `npm run fmt`.

Backend test crate runs under `cargo test`; frontend Vitest runs under
`npm run check`.

---

## Phase 1 — The "two zeros" pool-membership predicate and planner input contract

The core domain work. No loader or API change yet, so behavior stays identical
in production while the new code paths are exercised by unit tests.

### Pool-membership predicate (`backend/src/domain/conviction.rs`)

Today `pool_membership(conviction, market_value: MarketValueState)` excludes
every `Available(0)`. That single case conflates two distinct zeros:

- a **zero-quantity watchlist** instrument with a strictly-positive available
  price — must now be **admitted** at value 0; and
- a **held position whose price collapsed to zero** (or has no/zero price) —
  must stay **excluded**.

Because `market_value = quantity × price × fx` is 0 in both, the predicate
cannot distinguish them from `MarketValueState` alone. Grow its inputs to carry
the open quantity and whether a strictly-positive price is available (extend
`ConvictionTargetInput` with `open_quantity: i64` and a price-availability
signal, and thread the same into `pool_membership`). Admit rule:

- `Available(v)` with `v > 0` → admit (unchanged); or
- `Available(0)` with `open_quantity == 0` **and** a strictly-positive available
  price → admit at value 0 (the watchlist case); otherwise
- exclude.

Split `TargetReason::CurrentValueNotPositive` into the two intended outcomes:
the zero-qty-with-positive-price case is admitted (no exclusion reason), while
zero/absent price or a non-positive held value keeps an exclusion reason.
Distinguish "held value collapsed to zero" from "no usable price" so the
Holdings tooltip stays actionable.

Update every `pool_membership` test and add:

- admit a zero-value, zero-quantity, positive-price convicted holding;
- still exclude a held position whose price is zero/absent;
- still exclude a negative (short) value.

### Single-holding invariant (contract test + doc)

Once a zero-value member can join the pool, a portfolio with a single open
holding is no longer automatically `on_target`: the pool now also contains the
watchlist member, so the open holding has a real target gap. The existing
`single_eligible_asset_targets_full_pool_and_is_on_target` test describes the
old world. Add a new, explicitly named contract test for the new behavior (a
single open holding plus a convicted priced watchlist member splits the pool and
produces a nonzero gap), and update the "one eligible asset targets 100%" note
in `docs/Design.ConvictionTargets.md` and its blindspot bullet where the
assumption changes. Keep the old single-holding-only test but reframe it as
"single eligible member, no watchlist" if that scenario still holds, or replace
it — decide during implementation and keep both worlds documented.

### Planner input contract (`backend/src/domain/rebalance.rs`)

Relax `RebalanceCandidate` so `market_value_base` and `held_quantity` are no
longer required to be strictly positive; **price must still be strictly
positive**. A zero-value, zero-quantity candidate is buy-only (sell capacity 0,
which the existing `held_quantity` cap already enforces). The target math
`t_i = P'·w_i/W` already handles `v = 0`.

Audit the ladder for assumptions that a nonzero pool value or nonzero held
quantity always holds. In particular, `build_rung` carries
`debug_assert!(pool_value_base > Decimal::ZERO)`, and the feasibility check
`offset <= -pool_value_base` treats `offset == 0` against a zero pool as
`offset_exceeds_pool`.

**Settled decision (fold in): pure-watchlist pools with positive cash are
supported.** A pool whose entire value is zero-value watchlist members and a
**positive** offset (`P = 0`, `C > 0`) must produce a buy-only ladder that splits
the cash by conviction weight (`t_i = C·w_i/W`). Update the
`pool_value_base > 0` debug assertion (and any sibling assumptions in
redistribution/coverage) to allow a zero pool value when the post-offset pool
`P' = P + C` is strictly positive. The feasibility check keeps
`offset_exceeds_pool` semantics for **negative or zero** cash against a zero pool:
`C <= 0` with `P = 0` stays unavailable, because `P' <= 0` cannot fund any buy.
Coverage is `None` when `G = 0`; confirm a pure-buy pool still reports sensible
coverage from its nonzero gaps.

Add rung-level tests covering:

- a buy-only `v = 0, q = 0` candidate inside a **mixed** pool (at least one
  positive-value held candidate plus one zero-value watchlist candidate), through
  conviction-weighted redistribution, the all-zero fallback, and the
  residual-repair (greedy ±1) pass, with the shared invariants (`Σ d_i = C`, gaps
  sum, coverage) checkable; and
- a **pure-watchlist** pool (`P = 0`) with `C > 0`, asserting the cash splits by
  conviction weight and a buy ladder is produced; plus `C = 0` and `C < 0`
  against a zero pool both reporting `offset_exceeds_pool`.

### Docs in this phase

- `docs/Design.ConvictionTargets.md`: single-eligible-asset note and blindspot.
- `docs/Design.RebalancePlanner.md`: Inputs section — the strictly-positive
  market-value and held-quantity requirements relax to price-only, with the
  buy-only zero-value candidate described; and the Unavailable States / feasibility
  section — a pure-watchlist pool (`P = 0`) with `C > 0` is now feasible and
  splits cash by conviction weight, while `C <= 0` against a zero pool stays
  `offset_exceeds_pool`.

### Verification

- Backend build/clippy/fmt.
- New and updated unit tests in `conviction.rs` and `rebalance.rs` pass.
- No production behavior change yet (no zero-qty holding reaches the predicate
  because the loader still filters them out).

---

## Phase 2 — Holdings loader and API admit watchlist members

Make watchlist members actually enter the pool and appear as rows.

### Loader (`backend/src/api/valued_holdings.rs`)

`load_valued_open_holdings` is the single loader feeding both `/api/holdings`
and `/api/rebalance`, and it currently `continue`s on `position.quantity == 0`.
Give it an explicit mode parameter (open-only vs include-watchlist) rather than
making each caller filter, and rename it to reflect that it no longer only
returns open holdings (e.g. `load_valued_holdings(pool, date, scope)`), keeping
the name tied to stable behavior, not a plan phase. In include-watchlist mode it
also derives valuation for zero-quantity positions (value 0, but with the
latest price/FX snapshots so the price-available signal and planner price are
present). `ValuedOpenHolding::conviction_target_input` supplies the new
`open_quantity` and price-availability fields from Phase 1.

### Holdings response (`backend/src/api/holdings.rs`)

**Phase 2 keeps the response backward-compatible so the app runs unchanged
against the current frontend.** `/api/holdings` stays a bare JSON **array** of
holding objects in this phase; the only changes are additive per-row fields that
the current `useHoldings`/`HoldingsTable` (which consume `Holding[]`) ignore.
The envelope + hidden-watchlist count arrives in Phase 5, in lockstep with the
frontend type/query change, so no phase leaves the app un-runnable.

Explicit Phase 2 response shape:

- Each holding object gains `row_kind: "open" | "watchlist"` (additive; existing
  frontend ignores unknown fields).
- **Open rows are byte-compatible** with today's response: `cost_basis_native`,
  `average_cost_native`, and `base` are present exactly as now.
- **Watchlist rows** only appear when `include_watchlist=true`, which the current
  frontend never requests. `HoldingResponse::build` currently hard-errors on zero
  quantity via `average_cost_native().ok_or_else(internal error)`; make the native
  cost / average-cost / base-cost fields optional (`null`/absent) so watchlist rows
  serialize with blank cost instead of returning 500.
- `list()` derives targets over the **full pool** (open holdings plus convicted,
  priced watchlist members) regardless of the toggle, because pool membership is
  global. Add an `include_watchlist` query parameter (default `false`) that
  controls only which rows are returned:
  - toggle **off** (default): open rows only — same set of rows as today, but each
    open holding's target already reflects any convicted watchlist member in the
    pool.
  - toggle **on**: also return watchlist rows — every zero-quantity instrument
    (including `Other`/unpriced ones, which render blank), not only pool members.

The **hidden-watchlist-pool count** (for the "Targets include N watchlist
instruments" hint) requires a response envelope and is deferred to Phase 5, where
`/api/holdings` becomes `{ holdings: [...], hidden_watchlist_pool_count: N }` and
the frontend types/query change in the same phase.

Add API tests:

- a convicted, priced watchlist member shifts an open holding's target even with
  the toggle off (response is still a bare array of open rows);
- with the toggle on, watchlist rows serialize with blank cost/value and do not
  500;
- a zero-quantity `Other` or unpriced instrument appears (toggle on) but is not
  in the pool and carries the correct excluded/no-target status.

### Verification

- Backend build/clippy/fmt and the new API tests.
- The unchanged frontend still renders Holdings correctly against the Phase 2
  backend (bare-array contract preserved).

---

## Phase 3 — Rebalance planner admits buy-only watchlist candidates

### `backend/src/api/rebalance.rs`

`assemble_candidates` consumes the shared loader in include-watchlist scope so
zero-quantity, convicted, priced instruments become buy-only candidates
(`market_value_base = 0`, `held_quantity = 0`, strictly-positive `price_base`).
The internal-consistency guards that currently require a latest price/FX snapshot
still hold, because pool membership requires a positive available price.

**"New" badge data (settled decision).** Serialize a per-candidate marker so the
frontend can badge watchlist-sourced (zero-held) candidates. Add an additive
boolean (e.g. `is_new: bool`, true when `held_quantity == 0`) to the trade
response (`RebalanceTradeResponse`) and the balance response
(`RebalanceBalanceResponse`); untraded entries reuse the same via their balance
row. This stays purely additive and freshness-independent. The frontend view-model
and badge render land in Phase 5.

Add API tests:

- a pool with at least one open holding and one convicted watchlist member returns
  a plan whose rungs can propose a **buy** of the watchlist instrument, and the
  balance report includes it in input order;
- the watchlist candidate's trade and balance entries carry `is_new = true` while
  held candidates carry `is_new = false`;
- a pure-watchlist pool with a positive amount returns a buy ladder (mirrors the
  Phase 1 domain test at the API layer).

### Docs

- `docs/Design.RebalancePlanner.md`: note that the pool it consumes now includes
  watchlist members via the shared loader scope, and document the additive
  `is_new` marker on trade/balance responses (name the behavior, not the phase).

### Verification

- Backend build/clippy/fmt and the new API test.
- Recommended external human test: open the Rebalance page, convict a closed or
  new instrument, and confirm a positive amount proposes buying it.

---

## Phase 4 — Price refresh covers convicted watchlist instruments

### `backend/src/market_data/refresh.rs`

"Latest" refresh must fetch open positions **plus** zero-quantity instruments
whose conviction is Low/Medium/High. Setting a conviction is the "I care"
signal; closed instruments left at `Other` cost nothing and stay skipped.

Two loops currently skip never-traded instruments before the quantity check —
they `continue` when an instrument has no transaction group (`grouped.get`
returns `None`) — and both must be fixed:

- the `execute_refresh` target-collection loop; and
- the `status` readiness loop.

A never-traded instrument has no ledger group, so treat a missing group as an
empty ledger (quantity 0) rather than skipping. Then the "latest" filter becomes:
include when `position.quantity != 0` **or** the instrument's conviction is
Low/Medium/High. This requires reading conviction in the refresh path (it is on
`InstrumentRow`).

Add unit tests in `refresh.rs`:

- a never-traded instrument with a Low/Medium/High conviction and a mapped
  symbol is fetched in "latest" mode;
- a closed (zero-qty) instrument at `Other` is not fetched;
- a closed instrument that is convicted **is** fetched;
- the `status` readiness list includes a convicted never-traded instrument.

### Verification

- Backend build/clippy/fmt and the new refresh tests.

---

## Phase 5 — Frontend watchlist surfacing: Holdings toggle/rows/hint, Rebalance "new" badge, and cache invalidation

This phase couples the `/api/holdings` envelope change with its frontend
consumers so the app is never broken between phases.

### Holdings response envelope (`backend/src/api/holdings.rs`)

- Change `/api/holdings` from a bare array to
  `{ holdings: [...], hidden_watchlist_pool_count: N }`, where the count is the
  number of convicted, priced watchlist instruments in the pool that are **not**
  in the returned rows (i.e. hidden when the toggle is off). This backend change
  ships together with the frontend type/query change below.
- Add/adjust the API test asserting the count is reported with the toggle off and
  is zero with the toggle on (all pool members visible).

### Types and queries (`frontend/src/api/types.ts`, `frontend/src/api/queries.ts`)

- Add a `HoldingsResponse` envelope type (`holdings`, `hidden_watchlist_pool_count`);
  extend `Holding` for the `row_kind` discriminator and optional cost fields,
  mirroring the Phase 2/5 response.
- `useHoldings` accepts an `includeWatchlist` flag and sends
  `?include_watchlist=true`, with the flag in the query key so toggling refetches;
  it returns the envelope (update `HoldingsPage`/`HoldingsTable` to read
  `.holdings` and the count).
- Add the rebalance trade/balance `is_new` marker to the rebalance types.
- **Cache invalidation fix (review issue).** The Rebalance candidate pool depends
  on conviction, price freshness, and provider-symbol/mapping state. Add
  `["rebalance"]` to the invalidation set of every mutation that can change the
  pool: `useUpdateInstrumentConviction`, `useUpdateInstrumentConvictions`,
  `useRefreshPrices`, and the provider-symbol update mutation (and the
  add-instrument path in Phase 7). Without this, the Rebalance page can show a
  stale pool after a watchlist item becomes convicted or priceable.

### Holdings UI (`HoldingsPage.tsx`, `HoldingsTable.tsx`, `holdingsConviction.ts`)

- Add an "Include watchlist" toggle to the Holdings page, modeled on the Gains
  page's "Include closed positions" toggle. Default **off**. Keep the toggle
  state board-local (consistent with existing board-local view state), but allow
  it to be turned on programmatically (Phase 7 auto-enables it after an add).
- Watchlist rows render in the same table with blank Cost / Value / P&L and the
  same conviction editor and accept/apply flow as open rows.
- Blank Value sorts as 0 so watchlist entries cluster together when sorting on
  Value (extend the value sort key/selector accordingly; keep it a pure helper).
- When the toggle is off and `hidden_watchlist_pool_count > 0`, show a small hint
  near the toggle ("Targets include N watchlist instruments").
- Follow `docs/VisualDesign.DarkTheme.md` for the toggle, hint, and blank cells.

### Rebalance "new" badge (`RebalancePage.tsx`, `rebalanceViewModel.ts`)

- Extend the pure rebalance view-model to expose `is_new` for trade rows and
  balance entries, and render a small "new" badge on watchlist-sourced
  (zero-held) candidates, following `docs/VisualDesign.DarkTheme.md`.

### Tests

- Pure view-model/selector tests for the blank-value-sorts-as-0 behavior and the
  hint-count derivation.
- Pure `rebalanceViewModel` test that `is_new` maps to the badge for zero-held
  candidates and not for held ones.
- A role/text component test only for the toggle showing/hiding watchlist rows,
  if the behavior lives nowhere else.

### Verification

- Backend build/clippy/fmt (envelope change + test); frontend `npm run check`
  then `npm run fmt`.
- Recommended external human test: toggle on/off; confirm targets on open rows
  do not change when toggling (pool is global), watchlist rows show blank
  cost/value and cluster on Value sort, the hint appears when hidden pool members
  exist, the Rebalance page badges a proposed buy of an unheld asset, and the
  Rebalance page updates after convicting/pricing a watchlist item.

---

## Phase 6 — Read-only instrument lookup endpoint (mistyped-instrument guard backend)

The add-instrument dialog validates an instrument via a provider lookup before
creating it. No such read-only endpoint exists today (the `SymbolSearchProvider`
is only used internally by refresh seeding), so add one.

### `backend/src/api/` (new lookup handler) + route in `api/mod.rs`

- A read-only `GET` endpoint (e.g. `/api/instruments/lookup?query=...`) backed by
  the optional `SymbolSearchProvider` held by `MarketDataService`
  (`Option<Arc<dyn ...>>`, injected only for the live service). Expose a small
  service method on `MarketDataService` so the handler does not reach into
  provider internals; keep the domain/provider boundary intact.
- Graceful absent-provider story: when the provider is `None` (offline tests,
  demo mode, or a build without live providers), return an explicit
  `provider_unavailable`-style state rather than an error, so the dialog can fall
  back to allowing creation without a hard guard. This keeps offline tests and
  demo mode untouched.
- It is a GET, so it is not blocked by the demo read-only layer; confirm demo
  mode returns the graceful unavailable state.

### Tests

- Provider returns matches → endpoint returns them.
- Provider returns no supported match → endpoint returns an explicit
  "no match" result (drives the rejection UX in Phase 7).
- Provider absent → explicit `provider_unavailable` state, not a 500.

### Verification

- Backend build/clippy/fmt and the new endpoint tests.

---

## Phase 7 — Frontend Add-instrument dialog with lookup guard and feedback

### Dialog + reducer (new component under `frontend/src/components/`)

- An "Add instrument" button on the Holdings page opens a small dialog with the
  same fields as the Add Transaction form's new-instrument subform (symbol,
  exchange, name, type, currency), backed by the existing `useUpsertInstrument`
  mutation. **No conviction picker** in the dialog — creation and
  conviction-setting stay separate actions (explicit-apply principle).
- Pure reducer for dialog form state and the validate→create flow, tested
  directly (input → action → reducer → state), consistent with the existing
  add-transaction reducer.

### Lookup guard and feedback

Before calling create, validate via the Phase 6 lookup endpoint:

- **Mistyped instrument:** no suitable provider match → reject the create and
  surface the reason; do not call `POST /api/instruments`. The guard is a safety
  net, not a gate.
- **Provider unavailable (settled decision):** when the lookup endpoint reports
  `provider_unavailable` (offline, demo, or not configured), skip the hard guard
  and allow creation with a warning note — "could not verify instrument —
  provider unavailable". Creation is never blocked by an absent provider.
- **Upsert collision:** `POST /api/instruments` is upsert-like and returns
  `200`-instead-of-`201` for an existing `(exchange, symbol)`. The dialog detects
  the status and reports "already exists" (the existing row may be an open
  holding) instead of silently no-opping. This needs the client layer to expose
  the response status; add it to `api/client.ts`/`queries.ts` as needed.
- **Unpriceable ghost:** `create()` hardcodes `isin: None` and Yahoo auto-seed
  only knows certain exchanges, so a freshly created instrument may have no price
  mapping. After creation, surface mapping/price state (e.g. "no price mapping
  yet — configure provider symbol") using the existing price-status /
  provider-symbols surfaces, so the user knows the watchlist item will not be
  priced until a mapping is set.

### Reveal the new row (resolves review issue 3)

A newly created instrument has zero quantity, so with the watchlist toggle off it
would be invisible immediately after creation — stranding the user before the
separate conviction-setting step. On a successful add, the Holdings page
**auto-enables the include-watchlist toggle** and **highlights the newly created
row** (scroll into view + a transient highlight per `docs/VisualDesign.DarkTheme.md`),
so the instrument is immediately visible for the follow-up conviction edit. This
relies on the Phase 5 toggle being programmatically settable. On an upsert
collision (existing `(exchange, symbol)`), reveal/highlight the existing row
instead of reporting only text, since it may already be an open holding.

The add-instrument mutation path also invalidates `["holdings"]`, `["instruments"]`,
and `["rebalance"]` (the last per the review's cache-invalidation issue), so the
new row and any pool effects appear without a manual refresh.

### Tests

- Reducer + lookup-level test for the rejection behavior (mistyped → no create)
  and the provider-unavailable allow-with-warning path.
- Tests for the collision (200) and unpriceable-ghost feedback paths at the
  reducer / client layer, preferring pure logic over render assertions.
- A pure test that a successful add produces the "reveal" intent (toggle-on +
  target row id), kept in the reducer/selector rather than asserting DOM.

### Docs and versioning (final phase)

- `docs/DecisionLog.md`: new entry recording that the watchlist is derived from
  `quantity == 0` (no stored flag), that the pool-membership predicate
  distinguishes the two zeros, that pool membership is global (toggle is
  display-only), that a pure-watchlist pool with positive cash produces a
  conviction-weighted buy ladder while non-positive cash against a zero pool stays
  `offset_exceeds_pool`, and that the mistyped-instrument guard lives in the
  frontend against a read-only lookup endpoint (allow-with-warning when the
  provider is unavailable) while `POST /api/instruments` stays a pure DB mutation.
- Version bumps for user-visible behavior and API-surface change:
  `frontend/package.json` (currently `0.17.0`) and `backend/Cargo.toml`
  (currently `0.11.0`).
- Confirm `docs/Design.ConvictionTargets.md` and `docs/Design.RebalancePlanner.md`
  edits from Phases 1–3 are consistent with the shipped behavior.

### Verification

- Frontend `npm run check` then `npm run fmt`; backend checks still green.
- Recommended external human test: add a brand-new instrument via the dialog;
  confirm a mistyped symbol is rejected, an existing `(exchange, symbol)` reports
  "already exists", and a created-but-unmapped instrument surfaces the missing
  price-mapping state. Then convict it and confirm it enters the pool and the
  planner.

---

## Documents to update

- `docs/Design.ConvictionTargets.md` — single-eligible-asset note and blindspot
  (Phase 1).
- `docs/Design.RebalancePlanner.md` — Inputs contract relaxation and the
  watchlist-inclusive pool it consumes (Phases 1, 3).
- `docs/DecisionLog.md` — new entry (Phase 7).
- `frontend/package.json`, `backend/Cargo.toml` — version bumps (Phase 7).

## Settled decisions (folded in from review)

The three prior open questions are now resolved and written into the phases:

1. **"New" badge on the Rebalance page** — yes. Watchlist-sourced (zero-held)
   candidates carry an additive `is_new` marker in the rebalance API
   (Phase 3) and render a small badge on trade rows and balance entries via the
   pure view-model (Phase 5), following the dark-theme design language.
2. **Pure-watchlist pool with cash** — supported. A zero-value pool with a
   positive amount produces a buy-only ladder that splits the cash by conviction
   weight; negative or zero cash against a zero pool stays `offset_exceeds_pool`.
   Implemented via the relaxed `pool_value_base > 0` assertion and feasibility
   check (Phase 1) and documented in `docs/Design.RebalancePlanner.md`.
3. **Absent-provider guard UX** — allow creation with a warning ("could not
   verify instrument — provider unavailable"); the guard is a safety net, not a
   gate (Phases 6–7).

## Open Questions

None remaining.
