# Conviction Targets Implementation Plan

Status: Draft plan for implementation
Date: 2026-07-06
Source design: [Design.ConvictionTargets.md](../Design.ConvictionTargets.md)

## Purpose

Implement conviction targets as user-managed instrument metadata and expose the
derived target status in Holdings and Asset Detail. Conviction remains outside
transaction, price, import, and performance accounting. Targets are derived from
the current eligible open-position pool and are recalculated for the full pool
after any conviction edit.

## Existing Context

- Backend holdings are derived in `backend/src/api/holdings.rs` from
  `instruments::list`, `transactions::all_for_holdings`, `derive_position`, and
  `value_position`.
- Instrument metadata currently lives in `backend/src/db/instruments.rs` and is
  serialized through `backend/src/api/instruments.rs`.
- Frontend Holdings uses `frontend/src/components/HoldingsTable.tsx` with pure
  helper functions for sorting, summaries, and display derivations.
- Asset Detail derives its page model in
  `frontend/src/components/assetViewModel.ts` and renders in
  `frontend/src/components/AssetView.tsx`.
- Mutating API routes are already rejected in demo mode by the API middleware;
  new mutating handlers should also call `reject_demo_mutation` before touching
  storage or providers, matching the existing defense-in-depth convention.
- Demo data is seeded in `backend/src/demo/mod.rs` and should include
  representative convictions so target indicators are visible.

## Implementation Assumptions

- Store conviction as a non-null `conviction` column on `instruments`.
  This matches the current single-portfolio model, gives every instrument an
  explicit default of `OTHER`, and keeps conviction untouched by transaction
  edits, import refresh, and import rollback.
- Do not create a separate portfolio-settings or allocation-policy table in this
  change. That would be useful only if multi-portfolio support lands later.
- V1 computes target values only for current open holdings. Closed instruments
  can store conviction and can be edited on Asset Detail, but they do not enter
  the target pool until they have a current positive quantity and usable
  valuation.
- Use backend decimal arithmetic for target values and gaps. Frontend may format
  and sort the already-derived strings, but must not recompute target values.
- The 5% tolerance band is a backend-owned display derivation so Holdings and
  Asset Detail cannot disagree.
- Version bumps are required when implementing: backend `Cargo.toml` and
  frontend `package.json`.

## Open Questions

- Target status colors: the existing dark theme has up/down/warning semantics.
  The plan maps below/underweight to `--up` by default because it implies room
  to buy, and above/overweight to `--down` because it implies trimming. If that
  interpretation feels too trade-suggestive, use neutral text plus signed gaps.

Accepted design consequences that do not need further design changes:

- Asset Detail reset baseline is the conviction loaded at navigation time and is
  kept until the route is left. Reset can therefore overwrite a conviction change
  made elsewhere while the page was open.
- Import zero-position handling uses explicit per-instrument choices in the
  import preview and commit request. The backend still recomputes the affected
  set at commit time and rejects missing choices.

## Target API Contract

### Instrument

Extend `InstrumentResponse`:

```json
{
  "id": 1,
  "symbol": "MSFT",
  "exchange": "NASDAQ",
  "name": "Microsoft",
  "type": "Stock",
  "currency": "USD",
  "conviction": "Medium"
}
```

Accepted API values: `Other`, `Low`, `Medium`, `High`.
Database values: `OTHER`, `LOW`, `MEDIUM`, `HIGH`.

### Holding Target

Extend each holding with:

```json
{
  "conviction_target": {
    "conviction": "Medium",
    "status": "below",
    "target_value_base": { "status": "available", "value": "200000.00" },
    "target_gap_base": { "status": "available", "value": "-25000.00" },
    "target_gap_percent": { "status": "available", "value": "-12.50" }
  }
}
```

Do not duplicate current value inside `conviction_target`; consumers should read
the same holding's existing `valuation.market_value_base` when they need to show
current value beside the target fields.

Statuses:

- `below`
- `on_target`
- `above`
- `no_target`
- `excluded_unavailable`
- `unavailable`

Availability reasons should reuse existing valuation reasons where possible and
add target-specific reasons such as `target_pool_empty`,
`target_pool_zero`, `valuation_unavailable`, `price_mapping_disabled`, and
`current_value_not_positive`.

`valuation` can be present and available, present but unavailable with reasons,
or absent because price mapping is disabled. Convicted holdings in either
unavailable case must return `excluded_unavailable`, retaining their stored
conviction. A convicted holding with available but zero or negative current
value also returns `excluded_unavailable` with `current_value_not_positive`.

### Mutations

Add a small instrument metadata endpoint:

- `PUT /api/instruments/{id}/conviction`
- Request: `{ "conviction": "Low" }`
- Response: updated `InstrumentResponse`

For Holdings apply-all behavior:

- `PUT /api/instruments/convictions` with
  `{ "changes": [{ "instrument_id": 1, "conviction": "High" }] }`.

The bulk endpoint is required for Holdings apply. It validates all ids and
values before writing, applies changes in one SQL transaction, and avoids
partial frontend fallback semantics.

## Target Derivation Rules

Add a pure backend module, for example `backend/src/domain/conviction.rs`.

Core types:

- `ConvictionLevel`: `Other`, `Low`, `Medium`, `High`
- `ConvictionWeight`: `None`, `1`, `2`, `4`
- `TargetStatus`
- `ConvictionTargetInput`: instrument id, conviction, open quantity, current
  market value availability
- `ConvictionTargetOutput`: target value, signed gap, signed gap percent,
  status, reasons

Algorithm:

1. Build one input per open holding.
2. Mark `Other` as `no_target`.
3. For Low/Medium/High, include only holdings with available current market
   value and positive value in the eligible target pool.
4. Sum eligible current market values into `target_pool_value`.
5. Sum eligible conviction weights into `target_weight`.
6. If the eligible pool is empty, target values are unavailable.
7. If the eligible pool value is zero, target values are unavailable.
8. For each eligible holding:
   `target_value = target_pool_value * asset_weight / target_weight`.
9. Compute `target_gap = current_asset_value - target_value`.
10. Compute `target_gap_percent` only when `target_value > 0`.
11. Apply the 5% display tolerance to gap percent:
    less than `-5` is `below`, greater than `5` is `above`, otherwise
    `on_target`.
12. For convicted holdings with unavailable valuation, absent valuation because
    price mapping is disabled, or available but non-positive valuation, return
    `excluded_unavailable`; retain the stored conviction.

Rounding:

- Keep internal calculations in `Decimal`.
- Serialize money values with the existing `money_string` helper.
- Serialize percentages with two decimals.
- Tests should allow the eligible gap sum to differ from zero only by normal
  cent rounding.

## Backend Work

### Persistence And Repository

Tasks:

- Add migration `0006_add_instrument_conviction.sql`.
- Add `conviction TEXT NOT NULL DEFAULT 'OTHER' CHECK (...)` to `instruments`.
- Update all instrument selects/inserts/returning clauses in
  `backend/src/db/instruments.rs`.
- Extend `InstrumentRow`.
- Keep `NewInstrument` conviction-free so import-driven instrument creation
  cannot import conviction by accident.
- Ensure all upsert paths create new instruments with `OTHER` by default and
  leave existing rows unchanged.
- Add repository functions:
  - `update_conviction(pool, instrument_id, conviction)`
  - `update_convictions(pool, changes)` for the bulk endpoint
- Add repository tests for default value, update, invalid DB value rejection,
  and upsert preserving existing conviction.

Verification:

- `cargo test db::instruments`
- `cargo test db::testing`
- Confirm a migrated in-memory DB has the new column and CHECK constraint.

### API Surface

Tasks:

- Add `ConvictionDto` in `backend/src/api/instruments.rs`.
- Extend `InstrumentResponse::from_row`.
- Add `PUT /api/instruments/{id}/conviction`.
- Add the required bulk endpoint for Holdings apply.
- Validate unknown ids with 404 and invalid conviction values with the normal
  JSON extraction or a clear `invalid_conviction` error.
- Call `reject_demo_mutation(&state)` in every new mutating handler before
  touching storage or providers.
- Add router demo-mode test coverage for the new mutation route(s).

Verification:

- API tests:
  - new instrument returns `Other`;
  - update returns changed conviction;
  - list reflects changed conviction;
  - unknown instrument is 404;
  - demo mode rejects update.

### Target Derivation

Tasks:

- Implement the pure conviction target module.
- Unit test the example from the design note:
  Low 100k, Medium 300k, High 300k, Other 500k gives targets 100k, 200k,
  400k, no target.
- Test below/on-target/above at the 5% boundary.
- Test unavailable valuation exclusion while preserving Low/Medium/High.
- Test absent valuation because price mapping is disabled.
- Test available but zero/non-positive valuation returns
  `excluded_unavailable`.
- Test empty pool and zero pool handling.
- Test one eligible asset targets 100% and is on target.
- Test signed gap semantics: positive = above target, negative = below target.

Verification:

- `cargo test domain::conviction`

### Holdings Endpoint Integration

Tasks:

- While building holdings, retain each valued holding's current market value.
- After all open holdings are collected, run the target derivation once for the
  full pool.
- Add the matching `conviction_target` response to each `HoldingResponse`.
- Do not calculate targets row-by-row.
- Reuse existing valuation availability reasons in target exclusion where
  practical.
- Treat `valuation: None` as price mapping disabled, not as zero value.
- Keep existing holdings ordering unchanged.

Verification:

- Holdings API tests:
  - target values match the design example;
  - editing one conviction changes targets for all eligible rows;
  - `Other` gets `no_target`;
  - convicted row with missing valuation gets `excluded_unavailable`;
  - convicted row with disabled price mapping gets `excluded_unavailable`;
  - convicted row with available but zero value gets `excluded_unavailable`;
  - all `Other` or no usable values returns unavailable/no-target states
    without treating missing values as zero.

### Asset Detail Data

Tasks:

- Prefer using the extended Holdings response for open-position target data.
- For closed or no-row instruments, Asset Detail can show stored conviction from
  `InstrumentResponse` and omit target values.
- If Asset Detail needs target data for an open position not present in Holdings,
  fix that inconsistency in Holdings rather than adding separate target math.

Verification:

- Backend does not need a separate endpoint if the frontend can derive all Asset
  Detail display from `useInstruments` plus `useHoldings`.

### Import Close-Position Guard

Tasks:

- In import preview planning, detect existing instruments touched by the
  post-exclusion import plan whose derived quantity transitions from positive
  now to zero after the proposed commit while stored conviction is
  Low/Medium/High.
- Reuse the same effective-ledger computation as the commit path being
  previewed:
  - append mode predicts `existing ledger + plan.new_mapped_rows`;
  - Avanza replace mode predicts `existing ledger - replaced batch rows for
    non-excluded instruments + new rows`, while instruments excluded from the
    refresh keep their old rows.
- The preview must know which commit mode the user is likely to use. For Avanza,
  compute against the replace candidate when one exists because the UI defaults
  to refresh in that case.
- Surface blocking preview items with instrument id, symbol, current conviction,
  and a required user choice:
  - keep conviction for future planning; or
  - change conviction to Other as part of commit.
- Extend import commit query parameters to carry these choices, because commit
  endpoints currently use the raw CSV bytes as the request body. Use asset keys,
  matching the existing `exclude` convention and frontend preview identity, for
  example `conviction_keep=<asset keys>` and
  `conviction_to_other=<asset keys>`.
- At commit time, recompute the affected post-exclusion close-position set and
  reject the commit if the union of `conviction_keep` and `conviction_to_other`
  does not cover it exactly.
- Commit atomically:
  - transaction writes/import refresh happen as today;
  - conviction changes to Other happen in the same SQL transaction only for
    instruments where the user chose that option;
  - keeping conviction writes nothing.
- Re-run ledger validation as today.
- Ensure import rollback never changes conviction.

Verification:

- Backend import tests:
  - preview blocks when an import closes a convicted position;
  - append and Avanza replace predictions match their respective commit ledgers;
  - excluded/deselected assets do not trigger the guard;
  - pre-existing closed convicted instruments untouched by the import do not
    trigger the guard;
  - commit without required choices fails;
  - commit with "keep" preserves conviction;
  - commit with "change to Other" updates conviction atomically;
  - rollback leaves conviction unchanged.

## Frontend Work

### Types, Queries, And Mutations

Tasks:

- Extend `Instrument` and `Holding` in `frontend/src/api/types.ts`.
- Add `Conviction` and `TargetStatus` union types.
- Add `updateInstrumentConviction` or bulk `updateInstrumentConvictions`
  mutation in `frontend/src/api/queries.ts`.
- On success, invalidate:
  - `["instruments"]`
  - `["holdings"]`
  - `["gains"]` only if a displayed Asset Detail page depends on gains-derived
    status nearby; otherwise skip because conviction is not performance data.
  - `["portfolio-value-history"]` is not required because conviction does not
    affect valuation history.
- Keep mutation handling explicit: saving conviction must not masquerade as a
  transaction or price refresh.

Verification:

- `npm run check` after types compile.
- Add client/mutation tests only if existing client test patterns make this
  practical without over-testing TanStack Query internals.

### Holdings View-Model And Table

Tasks:

- Extract target display helpers into a pure module or colocated pure functions:
  - conviction label;
  - status label;
  - target gap signed formatting;
  - target sort value;
  - search text inclusion;
  - dirty-change reducer for staged table edits.
- Add sortable columns:
  - Conviction
  - Target
  - Target gap
  - Target status
- Add a compact conviction selector per row.
- Stage row changes locally rather than saving immediately.
- Add an explicit Apply action in the table toolbar that is disabled until there
  are changes.
- Add Reset/Discard action for staged table edits.
- When Apply succeeds, clear staged changes and invalidate holdings so target
  values refresh for the full pool.
- In demo mode, render conviction values and target indicators, but disable edit
  controls consistently with other mutating UI.
- Include conviction and target status in the table filter search text.
- Update persisted sorting validation to include new sortable column ids.

Visual behavior:

- Use existing `status-chip` styles for `Other`, `on_target`, unavailable, and
  excluded states.
- Use `--up` text for underweight/below and `--down` text for overweight/above
  only on the signed target gap, not as large fills.
- Use `--warning-soft` only for unavailable/excluded chips because those are
  data-quality alerts.
- Keep row density close to the current table; target values are numeric and
  should use mono typography.

Verification:

- Frontend tests:
  - target helpers map statuses and signs correctly;
  - staged reducer records changes, clears after apply, and resets;
  - sorting-state validation accepts new target columns and rejects unknown ids;
  - search text includes conviction/status.
- Manual browser check:
  - changing one row and applying refreshes every target row;
  - discard restores visible staged selectors;
  - demo mode disables edit controls.

### Asset Detail Conviction Panel

Tasks:

- Add a compact Asset Detail panel for conviction and target data.
- Show stored conviction for open, closed, and no-position instruments.
- For open holdings, show target value, signed gap, gap percent, and status.
- For closed/no-row instruments, show conviction and "No current target" rather
  than treating conviction as Other.
- Save conviction immediately when changed.
- Capture a local baseline conviction when the page first loads for the current
  instrument id.
- Show Reset only when saved conviction differs from that baseline.
- Reset immediately saves the baseline conviction back to the instrument.
- Clear baseline when the instrument id changes or the component unmounts.
- Disable controls in demo mode.

Verification:

- Frontend tests:
  - `assetViewModel` finds target data for the current holding;
  - closed/no-row instruments still expose stored conviction;
  - baseline/reset state helper behaves across save and route-id changes.
- Manual browser check:
  - Asset Detail save updates Holdings after navigation/refetch;
  - Reset restores the original page-load conviction;
  - unavailable valuation is explicit.

### Import UI Guard

Tasks:

- Extend import preview types with close-position conviction decisions.
- Render a blocking section in the import preview when choices are required.
- For each affected instrument, show current conviction and radio/select choices:
  - keep conviction;
  - change to Other on commit.
- Disable Commit until every affected instrument has a choice.
- Include choices in commit query parameters using the backend asset-key
  convention.
- After commit, invalidate instruments and holdings.

Verification:

- Frontend tests for preview reducer/state:
  - commit disabled until all choices are made;
  - choices are serialized into commit query parameters;
  - existing import exclusion behavior remains unchanged.
- Manual human test recommended with a small fixture that closes a convicted
  position.

## Demo Data

Tasks:

- Extend `DemoInstrument` with conviction.
- Apply seeded convictions after `instruments::upsert` with the new conviction
  repository function instead of adding conviction to `NewInstrument`.
- Seed at least:
  - one `Other`;
  - one `Low`;
  - one `Medium`;
  - one `High`;
  - at least one visibly below or above target.
- Keep demo mode read-only after seeding, as it is today.
- Add tests that seeded Holdings returns non-Other convictions and available
  target indicators.

Verification:

- `cargo test demo`
- Launch demo manually and confirm the Holdings table shows target indicators.

## Recommended Implementation Order

### Step 1: Persistence And Contract

Deliver:

- migration;
- instrument repository updates;
- instrument API response includes conviction;
- mutation endpoint(s);
- tests.

Verification:

- `cargo test db::instruments`
- `cargo test api::instruments`
- `cargo test api::tests::demo_mode_rejects_mutating_routes`

Human testing:

- Optional API smoke with `curl` or browser devtools to confirm update/list.

### Step 2: Pure Target Derivation

Deliver:

- backend pure target module;
- design-example tests;
- tolerance and missing-valuation tests.

Verification:

- `cargo test domain::conviction`

Human testing:

- Not required; this step is pure logic.

### Step 3: Holdings API Targets

Deliver:

- Holdings response includes `conviction_target`;
- target derivation runs once per full holdings response;
- API tests for pool-wide recalculation and unavailable states.

Verification:

- `cargo test api::holdings`

Human testing:

- Optional JSON inspection of `/api/holdings`.

### Step 4: Frontend Holdings Editing

Deliver:

- types and mutation hooks;
- Holdings columns;
- staged edit/apply/discard flow;
- sorting/search updates;
- tests.

Verification:

- `npm run check`

Human testing:

- Recommended. The table workflow is the highest-risk interaction in V1:
  apply one changed row, confirm all targets update, discard staged changes, and
  verify demo mode disables editing.

### Step 5: Asset Detail Editing

Deliver:

- Asset Detail conviction panel;
- immediate save;
- local reset baseline;
- target display for open positions;
- tests.

Verification:

- `npm run check`

Human testing:

- Recommended. Confirm the reset baseline behavior after navigating to an asset,
  saving a different conviction, resetting, and navigating away/back.

### Step 6: Import Close-Position Guard

Deliver:

- backend preview/commit guard;
- frontend import choice UI;
- import tests;
- no rollback conviction changes.

Verification:

- relevant backend import tests;
- `npm run check`;
- manual import fixture test.

Human testing:

- Required before considering this complete. This touches a high-impact import
  workflow and needs a real or realistic file that closes a convicted holding.

### Step 7: Demo Data, Versions, And Full Gates

Deliver:

- demo convictions;
- visible demo target indicators;
- backend and frontend version bumps;
- final formatting and full checks.

Verification:

- From `backend/`: `cargo clippy --all-targets -- -D warnings`
- From `backend/`: `cargo fmt`
- From `frontend/`: `npm run check`
- From `frontend/`: `npm run fmt`
- Manual demo launch with `scripts/start.ps1 -Demo`.

Human testing:

- Recommended final smoke: normal mode read path, demo mode read-only path, and
  one conviction edit in Holdings plus one on Asset Detail.

## Non-Goals For This Implementation

- Cash-aware buy/sell/rebalance planning.
- Dashboard allocation by conviction.
- Gains table conviction columns.
- Configurable target weights or tolerance.
- Risk modeling by sector, country, currency, volatility, or correlation.
- Multi-portfolio conviction policy.

Adding `conviction` to shared `InstrumentResponse` also means embedded
instrument DTOs such as `GainsRow.instrument` receive the field. Gains should not
add conviction-led columns in this implementation, but this shared DTO exposure
is expected rather than accidental.

## Completion Criteria

- Every instrument stores exactly one conviction and new instruments default to
  `Other`.
- Import refresh, import rollback, ledger edits, and closing a position never
  silently delete or reset stored conviction.
- Holdings shows conviction, target value, signed target gap, and target status.
- Holdings saves conviction changes only through an explicit Apply action.
- Asset Detail saves conviction immediately and supports reset to the
  navigation-time baseline.
- Target calculations are backend-derived from the full eligible pool and are
  refreshed across all rows after any edit.
- Unavailable valuation states are explicit and are never treated as zero or
  silently converted to `Other`.
- Demo mode shows representative target indicators and remains read-only.
