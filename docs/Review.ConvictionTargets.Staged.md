# Conviction Targets Staged Review

Date: 2026-07-06
Scope: staged changes for `docs/plans/Plan.ConvictionTargets.Implementation.md`

## Actionable Findings

### P1 - Backend test gate currently fails on dividend transaction API tests

- Evidence: `cargo test` from `backend/` fails 2 tests:
  - `api::transactions::tests::dividend_round_trips_through_list`
  - `api::transactions::tests::dividend_with_brokerage_is_rejected`
- Relevant code:
  - `backend/src/api/transactions.rs:594` sends a dividend request using `price`.
  - `backend/src/domain/transaction.rs:241` rejects dividends that carry `price`; it expects `dividend_per_share`.
  - `backend/src/api/transactions.rs:621` sends both `price` and `brokerage`, so validation returns `dividend_must_not_carry_price` before `dividend_must_not_carry_brokerage`.
- Why it matters: the required backend gate cannot pass in the current workspace, even though this appears outside the conviction-target staged files.
- Suggested fix: align the transaction API tests and/or API compatibility mapping with the current dividend contract. If `price` is intentionally still accepted as the API field for dividends, translate it to `dividend_per_share` before domain validation; otherwise update the tests to send `dividend_per_share` and adjust the brokerage test so it isolates brokerage validation.
- Verification: rerun `cargo test` from `backend/`.

### P2 - Deselecting a closing asset in import preview still requires an irrelevant conviction choice

- Evidence: `frontend/src/components/ImportView.tsx:293` checks every `preview.conviction_close_positions` entry, and `frontend/src/components/ImportView.tsx:437` blocks commit when any preview-listed position lacks a choice. The blocker does not consider `state.selected`. The UI section at `frontend/src/components/ImportView.tsx:797` also renders all preview-listed closing positions.
- Why it matters: the plan says deselected assets do not trigger the guard. Backend commit recomputes the closing set after `exclude`, but the frontend can still block a valid Avanza refresh/append until the user makes a meaningless keep/clear choice for an asset they deselected.
- Suggested fix: derive the active closing-decision set from preview positions filtered by selected assets, and use that same filtered set for `allConvictionChoicesMade`, `convictionCommitParams`, and `ConvictionCloseSection`.
- Verification: add a frontend reducer/helper test where a preview has a closing convicted position, that asset is deselected, and commit is not blocked and serializes no conviction decision for it.

### P2 - Holdings table added columns without increasing layout budget, truncating Instrument names

- Evidence: `frontend/src/styles.css:458` still gives `.holdings-table table` a `min-width` of `960px`, but `frontend/src/components/HoldingsTable.tsx:537` adds four new columns after the existing seven. The screenshot shows the first Instrument cell compressed to `A...`; `frontend/src/styles.css:614` then ellipsizes the instrument link under that pressure.
- Why it matters: the primary identifier is no longer readable on the main Holdings page after the conviction columns are added.
- Suggested fix: give the holdings table explicit column width rules or a larger min-width that reflects all 11 columns. Keep Instrument wide enough for names and let horizontal scrolling absorb the extra target columns.
- Verification: browser-check Holdings with long names at desktop and mobile widths; the Instrument name should not truncate at the default desktop viewport.

### P3 - Target exclusion loses specific valuation reasons

- Evidence: `backend/src/api/holdings.rs:206` maps every unavailable market value to `MarketValueState::Unavailable`, and `backend/src/domain/conviction.rs:217` serializes that as only `valuation_unavailable`.
- Why it matters: the plan asks to reuse existing valuation availability reasons where practical. A convicted holding missing price, FX, or stale data currently exposes the same generic target reason, making the target-status tooltip less actionable than the valuation field beside it.
- Suggested fix: carry the underlying market-value unavailable reasons into target derivation or merge them into the target response while retaining the target-specific `valuation_unavailable` category.
- Verification: add a holdings API test for a convicted row with missing price/FX and assert the target unavailable reasons include the underlying valuation reason.

## Checks Run

- `git diff --check --staged`: passed.
- `npm run check` from `frontend/`: passed.
- `cargo test` from `backend/`: failed on the two dividend transaction API tests listed above.
