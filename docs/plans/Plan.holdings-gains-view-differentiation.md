# Implementation Plan - Holdings and Gains View Differentiation

**Goal:** Implement the accepted Holdings / Gains / Transactions view model from
`docs/Design.HoldingsGainsViews.md` so Holdings answers "where is my money?" and
Gains answers "what made or lost money?"

**Outcome:** Gains exposes a price-effect and FX-effect attribution split that
adds up to total unrealized gain, while Holdings becomes exposure-first with
portfolio weight, explicit currency, and market-value sorting.

**Current context:** Market data, FX, valuation availability, Holdings, and Gains
already exist. `backend/src/domain/valuation.rs` computes pure valuation rows and
summaries. `backend/src/api/gains.rs` serializes the Gains API. The frontend
tables are `frontend/src/components/GainsTable.tsx` and
`frontend/src/components/HoldingsTable.tsx`, with shared API contracts in
`frontend/src/api/types.ts`.

**Non-goals:** Do not split day change into price/FX effects, do not add a period
selector, and do not add dividend, fee, or realized-gain attribution lines. Those
are explicitly deferred by the accepted design.

---

## Open Questions And Review Checkpoints

1. **Holdings cost-basis display currency.**
   - Plan default: keep Holdings cost basis and average cost per share in the
     instrument currency, because Gains owns the SEK attribution view and
     Holdings should foreground exposure and currency.
   - Review checkpoint: confirm this reads correctly in the UI. If the table
     needs SEK cost basis instead, adjust labels and values before merging.

2. **Portfolio percent denominator.**
   - Plan default: include only rows with available `market_value_base`, matching
     the design. Rows with unavailable valuation show `--`.
   - Review checkpoint: manually verify that visible percentages sum to roughly
     100% over rows with available market value, with expected rounding drift
     only.
   - Note: this can intentionally diverge from the app header's Portfolio total
     if that total still comes from `GainsSummary.market_value_base`, because
     the summary includes only rows with both market value and base cost basis.
     Holdings is exposure-first, so market-value availability alone is the right
     denominator for this column.

3. **Summary inclusion rule for attribution.**
   - Plan default: add summary price/FX effects over the same rows already
     included in `ValuationSummary` today, so the summary remains internally
     consistent with `unrealized_gain_base`.
   - Review checkpoint: ensure excluded rows still surface through the existing
     `excluded_rows` count and unavailable cells.

---

## Phase 1 - Backend Attribution Domain

**Purpose:** Add the price-effect and FX-effect math in the pure valuation domain.

**Changes:**
- Extend `ValuedHolding` with:
  - `price_effect_base: Availability<Decimal>`
  - `fx_effect_base: Availability<Decimal>`
- Extend `ValuationSummary` with the same two fields.
- In `value_position`, compute:
  - `gross_base = cost_basis_base - fee_component_base`
  - `price_effect_base = (market_value_native - cost_basis_native) * latest_fx - fee_component_base`
  - `fx_effect_base = cost_basis_native * latest_fx - gross_base`
- Use the direct `fx_effect_base` form above instead of computing through
  `avg_purchase_fx = gross_base / cost_basis_native`. The division form is
  algebraically equivalent, but `rust_decimal` division can round when the ratio
  does not terminate, which would break exact equality tests for
  `price_effect_base + fx_effect_base == unrealized_gain_base`.
- Read `fee_component_base` from `BaseCostBasis::Available` without changing the
  `value_position` function signature.
- Return unavailable effects when market value, base cost basis, latest FX, or
  non-zero native cost basis requirements are not met.
- Include the new effects in the row-level `reasons` aggregation if they are
  unavailable.
- In `summarize_holdings`, sum the effects over rows included in the existing
  summary totals and expose unavailable summary fields when no rows qualify.

**Tests:**
- Add focused unit tests in `backend/src/domain/valuation.rs` for:
  - USD holding where `price_effect_base + fx_effect_base == unrealized_gain_base`.
  - SEK holding where FX effect is exactly zero and price effect equals total gain.
  - Multi-lot buys at different FX rates.
  - Non-zero brokerage with unchanged price and unchanged FX, proving brokerage
    lands in price effect and not FX effect.
    Use a helper parameter or a manual `BaseCostBasis::Available` override so
    the test exercises `position.base.fee_component_base`, which is what
    `value_position` reads.
  - Zero native cost basis returns `ZeroCostBasis`.
    Confirm the existing display wording for `ZeroCostBasis` stays generic
    enough for both zero base cost basis and zero native cost basis.
  - Missing-FX contamination propagates the existing base-cost unavailable reason.
  - Summary effects sum to row effects and to summary total gain.

**Verification:**
- Run `cargo test valuation` from `backend/` during the phase.
- Confirm no provider, database, or refresh code changes are needed.

---

## Phase 2 - Gains API Contract

**Purpose:** Expose the attribution fields additively through `/api/gains`.

**Changes:**
- Extend `backend/src/api/gains.rs` `GainRow` with:
  - `price_effect_base`
  - `fx_effect_base`
- Extend `SummaryResponse` with the same two fields.
- Serialize both fields with the existing `serialize_availability` and
  `money_string` helpers.
- Keep existing fields for compatibility during this change; frontend cleanup
  happens in a later phase.

**Tests:**
- Extend Gains API tests to assert:
  - New row fields exist and carry expected available values for a valued holding.
  - New summary fields exist and match the sum of included rows.
  - Unavailable attribution serializes with reason arrays rather than zero.

**Verification:**
- Run `cargo test gains` from `backend/`.
- Inspect a sample `/api/gains` response and confirm the new fields are additive.

---

## Phase 3 - Frontend Types And Gains Table

**Purpose:** Make Gains attribution-first and remove exposure columns that now
belong in Holdings.

**Changes:**
- Update `frontend/src/api/types.ts`:
  - Add `price_effect_base` and `fx_effect_base` to `GainsRow`.
  - Add `price_effect_base` and `fx_effect_base` to `GainsSummary`.
- Update `frontend/src/components/GainsTable.tsx` columns to:
  - `Instrument`
  - `Cost basis (SEK)`
  - `Market value (SEK)`
  - `Total gain + %`
  - `Price effect`
  - `FX effect`
  - `Day change + %`
  - `Status`
- Remove Gains columns for `Qty`, `Latest close`, `Market value (native)`, and
  native cost-basis detail.
- Make `Price effect` and `FX effect` signed, sortable, and searchable.
- Render `Total gain + %` and `Day change + %` as combined stacked cells. Keep
  each combined column sortable by its base SEK amount:
  - `Total gain + %` sorts by `unrealized_gain_base`
  - `Day change + %` sorts by `day_change_base`
- Keep default sort as `unrealized_gain_base` descending.
- Keep unavailable rendering through `AvailabilityValueCell`.
- Update the `numericColumns` set and search-string builder for the new effect
  fields and removed display columns.
- Keep `latest_price` and `latest_fx` in the API type and row data even after
  dropping the `Latest close` column, because the Status column still derives
  freshness from those snapshots.
- If the summary band in `frontend/src/App.tsx` is updated during this phase,
  keep it compact and avoid turning Gains into a duplicate of Holdings.

**Tests:**
- `npm run check` should catch the API type changes and table contract mistakes.
- Add or update frontend component tests only if the project already has a table
  test harness by implementation time; otherwise rely on type/lint plus manual UI
  verification for this UI-only change.

**Verification:**
- Run `npm run check` from `frontend/`.
- Recommended human check: open Gains and verify it reads as attribution, not
  exposure. Confirm a known USD holding has a plausible split and the effect
  columns sort correctly.

---

## Phase 4 - Holdings Exposure Table

**Purpose:** Make Holdings focus on current exposure, size, currency, and portfolio
weight.

**Changes:**
- Compute `Portfolio %` in `frontend/src/components/HoldingsTable.tsx` from the
  sum of available `valuation.market_value_base` values.
- Exclude unavailable rows from the denominator and show `--` for their portfolio
  percentage.
- Treat `valuation == null` the same as unavailable valuation for this
  denominator and display rule; `backend/src/api/holdings.rs` can omit valuation
  when price mapping is not enabled.
- Change the table columns to:
  - `Instrument`
  - `Qty`
  - `Avg cost/share`
  - `Cost basis`
  - `Current value (SEK)`
  - `Portfolio %`
  - `Currency`
  - `P&L hint`
- Make `Current value (SEK)` the visual focus of the valuation cells.
- Keep P&L compact and secondary, using the existing unrealized/day-change values.
- Change default sort from instrument ascending to `market_value_base` descending.
- Preserve the existing dark-theme table styling and avoid adding explanatory
  in-app text.

**Tests:**
- `npm run check` for type/lint coverage.
- Add focused helper tests only if portfolio-percent calculation is factored into
  a pure helper during implementation.

**Verification:**
- Run `npm run check` from `frontend/`.
- Recommended human check: open Holdings and confirm portfolio percentages sum to
  roughly 100% over rows with available market value, unavailable rows show `--`,
  and the first row is the largest available SEK market value.

---

## Phase 5 - Version Bump, Styling Pass, And Full Validation

**Purpose:** Finish the visible behavior change and verify the full application.

**Changes:**
- Bump `backend/Cargo.toml` version.
- Bump `frontend/package.json` version.
- Apply any small CSS refinements needed in `frontend/src/styles.css` so the new
  table columns stay readable on the existing dark theme.
- Add an `EngineeringDiary.md` entry for the implemented application change. Do
  not reference this plan from the diary entry.
- Archive this plan after implementation by moving it to `docs/plans/archive/`,
  creating that directory when the first implemented plan is archived.

**Verification:**
- From `backend/`:
  - `cargo build`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo fmt`
- From `frontend/`:
  - `npm run check`
  - `npm run fmt`
- Run a local app smoke test with backend and frontend active.
- Recommended human check: compare Holdings, Gains, and Transactions and confirm
  each view answers its intended question without obvious column duplication.

---

## Suggested Implementation Order

Implement and review phases sequentially. Phases 1 and 2 can be validated entirely
with backend tests. Phases 3 and 4 should be kept separate enough that the table
role change is easy to review. Phase 5 should be the only phase that changes
versions and final formatting.
