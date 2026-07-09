# Plan: Explicit "Brokerage costs" row in the Gains breakdown waterfall

## Goal

Add an explicit "Brokerage costs" row to the Gains breakdown waterfall on the
instrument asset view so fee drag becomes visible instead of being silently baked
into cost basis and realized gain. When done, the waterfall shows brokerage as a
true negative (red "down") bar in both open- and closed-position modes, and the
Total return figure is numerically identical to today.

This realizes the "fees-as-a-step" follow-on work that the 2026-06-22 waterfall
decision explicitly deferred.

## Design summary (settled in the brief)

- **Gross decomposition, display-only.** The waterfall's Cost basis, Price effect
  and Realized gain rows become fee-free (gross); FX effect is already fee-free; a
  new negative "Brokerage costs" row absorbs the fees. The identity
  `gross cost basis + gross price effect + FX effect = market value` holds exactly
  (fee terms cancel), so the Market value / Proceeds subtotal survives and Total
  return is unchanged.
- **Placement:** "Brokerage costs" immediately after Realized gain (open) or after
  the Proceeds subtotal (closed), before Dividend income, framed as realized loss.
- **Scope:** waterfall-only. The domain ledger money of record stays net. Holdings
  table, asset summary, Gains table, and all totals keep today's fee-inclusive
  numbers. Every pre-existing serialized `GainRow` money field stays byte-identical.
- **Brokerage row value = raw ledger sum** of `brokerage_base` across the
  instrument's transactions. Always computable (never FX-gated). When cost basis is
  FX-unavailable the gross decomposition rows go unavailable but the Brokerage costs
  row still shows the true total.
- **Zero-fee instruments** show the row with a calm `0.00`.
- **Accepted cross-surface divergence:** the asset waterfall "Price effect" (gross,
  fee-free) differs from the Gains table "Price effect" column (fee-inclusive) by
  the fee amount; no label changes. Closed-mode Proceeds becomes gross and therefore
  no longer matches Avanza's net settlement amount.

### Settled follow-up decisions (from Codex review + user answers)

- **Percent denominator stays NET.** Effect-row and total-return percentages keep
  today's fee-inclusive "total capital deployed" (held + sold cost basis)
  denominator; the total-return `%` stays byte-identical. Bars float from the
  displayed gross cost basis while percentages remain on the net denominator — a
  deliberate split, meaning "return on money actually paid out, fees included".
- **The Brokerage costs row has no unavailable state.** Its value is an
  always-`available` `MoneyValue` (the raw ledger sum is computable once
  transactions load). Mechanically this is an ordinary available `MoneyValue`
  flowing through `SummaryAvailabilityValue` in `ValueCell` with tone "signed" and
  direction "down"; the `wf-bar unavailable` path is never taken for this row.
- **Closed-mode gross Proceeds must NOT be `proceeds_base + brokerage_total_base`.**
  That double-counts buy brokerage, which lives in cost basis, not proceeds
  (`backend/src/domain/position.rs`: buy brokerage added to cost basis; sell
  brokerage subtracted from proceeds). Gross Proceeds = `proceeds_base + sell_brokerage`
  only. A sell-brokerage-only primitive is therefore required (see field set
  below); the "gross-identity-only" alternative does not avoid it, because the
  closed gross **cost-basis anchor** also needs the same fee split.

### Serialized-field decision (stated explicitly, per brief requirement)

**No existing serialized field changes meaning; all stay net. Four additive
fields are introduced and the gross rows are derived in the pure frontend
view-model.**

Rationale: the requirement is that every pre-existing `GainRow` money field remain
byte-identical, and the architecture rule keeps view-derivation in pure
selectors/view-models. Repurposing `unrealized_price_effect_base` to mean "gross"
would silently overload a field that closed-mode already reuses for realized net
price effect, and would push a presentation concern (gross vs net) into the
serialization layer. Instead the API exposes the raw ingredients and the waterfall
view-model composes gross rows. New fields:

- `held_fee_component_base` (H) — the open position's `fee_component_base`
  (allocated buy fees on **held** shares). Shares cost-basis availability
  (FX-gated).
- `realized_fee_base` (R) — allocated buy fee on **sold** shares + sell brokerage.
  Shares realized cost-basis availability (FX-gated).
- `realized_sell_brokerage_base` (S) — raw sum of `brokerage_base` over **sell**
  transactions only. **Always available** (raw SEK, not FX-gated).
- `brokerage_total_base` (T) — raw sum of `brokerage_base` over **all** the
  instrument's transactions. **Always available.**

Identity: `T = H + R`, and `R = A + S` where `A` = allocated buy fee on sold
shares = `R − S`. `T` and `S` are exposed as their own always-available fields
because `H`/`R`/`A` are FX-gated and must not gate the brokerage row or the
closed-mode Proceeds fix.

Gross rows the view-model derives — **open mode**:
- Cost basis (held) gross = `cost_basis_base − H`
- Price effect gross = `unrealized_price_effect_base + H`
- Realized gain gross = `realized_gain_base + R`
- Brokerage costs = `−T`

Gross rows the view-model derives — **closed mode** (the corrected math):
- Cost basis (sold) gross = `cost_basis_base − A` = `cost_basis_base − (R − S)`
- Price effect gross = `price_effect_base + R`
- FX effect unchanged
- Proceeds subtotal gross = `proceeds_base + S` (sell brokerage only — **not** `T`)
- Brokerage costs = `−T` (with `H = 0` for a closed position, `T = R`)

Reconciliation (all FX available): buy fees are ratio-allocated between held and
sold, so `H + R − T = 0` and Total return is unchanged. Closed identity check:
`gross cost basis + gross price effect + FX = (cost_basis − A) + (price_effect + R) + fx = proceeds_base + S`
= gross Proceeds, so the subtotal and geometry stay consistent.

---

## Phase 1 — Domain: realized fee accumulator + raw brokerage total

Pure `backend/src/domain/` work, no API/DB/UI changes. Smallest self-contained
slice.

**Changes (`backend/src/domain/position.rs`):**
- Add `RealizedGain.fee_base: BaseAmount` (R), accumulating
  `allocated_fee_component_base + tx.brokerage_base` for each sell. In
  `accumulate_realized_gain`, the value currently subtracted inline into
  `price_effect_base` is captured into `fee_base`; `price_effect_base`,
  `gain_base`, `cost_basis_base`, `proceeds_base` stay numerically identical. When
  a sell (or the buy-side base) is FX-unavailable, mark `fee_base` unavailable in
  the same branches as the sibling fields.
- Add `RealizedGain.sell_brokerage_base: Decimal` (S) — a raw sum of `tx.brokerage_base`
  over the position's **sell** transactions, accumulated at the top of
  `accumulate_realized_gain` **before** any FX early-return, so it is always
  available even when the sell/buy FX is missing.
- Add `PositionPerformance.brokerage_total_base: Decimal` (T) — a raw fold over
  **all** transactions' `brokerage_base` (independent of FX and of buy/sell/split
  kind), always available. Compute it in `derive_position_performance`.
- Leave the existing FX-gating of `fee_component_base` in `apply_buy` **as is**.
  The brokerage row and the closed-Proceeds fix deliberately source raw SEK sums
  (`T`, `S`), so held/realized fee components (`H`, `R`) may legitimately be
  unavailable while those rows still render — no need to un-gate them.

**Tests (in-module):**
- Extend the existing `position_performance_accumulates_realized_sell_gain` test
  (which pins `proceeds_base`, `cost_basis_base`, `price_effect_base`,
  `fx_effect_base`, `gain_base`) to also assert `fee_base`, `sell_brokerage_base`,
  and `brokerage_total_base`, and confirm the pinned fields are unchanged — this is
  the byte-identical regression guard at the domain layer.
- New: partial-sell case carrying **both** fee kinds (allocated buy fee on sold
  shares + sell brokerage) — the motivating Broadcom shape — asserting
  `gain_base + fee_base` equals the gross realized gain,
  `held fee + realized fee == brokerage_total_base`, and
  `fee_base − sell_brokerage_base` equals the allocated buy fee on sold shares (A).
- New: `brokerage_total_base` and `sell_brokerage_base` are Available and correct
  even when a buy or sell lacks FX (base/realized cost basis unavailable).
- New: `fee_base` goes Unavailable when sell FX is missing and when the buy-side
  base is unavailable, while `sell_brokerage_base` stays Available.

**Verify:** from `backend/` — `cargo build`, `cargo test domain::position`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt`. No human testing.

---

## Phase 2 — API: expose the four fee fields, keep everything else byte-identical

**Changes:**
- `backend/src/domain/valuation.rs`: expose the held fee on `ValuedHolding` as
  `fee_component_base: Availability<Decimal>` (the local `Option<Decimal>` already
  computed in `value_position`; Unavailable when base cost basis is). Unrealized
  price/FX effect math stays unchanged (still net).
- `backend/src/api/gains/types.rs`: add `held_fee_component_base`,
  `realized_fee_base`, `realized_sell_brokerage_base`, `brokerage_total_base:
  AvailabilityResponse` to `GainRow`.
- `backend/src/api/gains/rows.rs`: populate the four fields in both
  `open_gain_row` and `closed_gain_row`. `brokerage_total_base` (T) and
  `realized_sell_brokerage_base` (S) are threaded in / read from `RealizedGain` and
  always serialized Available. `held_fee_component_base` (H) is
  `ValuedHolding.fee_component_base` on open rows and `0.00`/unavailable
  appropriately on closed rows (no held shares). `realized_fee_base` (R) serializes
  `RealizedGain.fee_base` on both.
- `backend/src/api/gains.rs`: pass `performance.brokerage_total_base` to both row
  builders; `RealizedGain.sell_brokerage_base` is already on the `realized` value
  both builders receive.
- Confirm no ledger re-query is needed: `gains.rs` already holds the per-instrument
  `ledger`, and `brokerage_total_base` / `sell_brokerage_base` ride along on
  `PositionPerformance` / `RealizedGain` from Phase 1, so
  `backend/src/db/transactions.rs` is untouched.

**Tests (`backend/src/api/gains/tests.rs`):**
- Regression: for the three canonical shapes — never-sold open, partially-sold
  open (both fee kinds), fully closed — assert every pre-existing serialized money
  field (`cost_basis_base`, `price_effect_base`, `fx_effect_base`,
  `market_value_base`, `proceeds_base`, `realized_gain_base`,
  `realized_cost_basis_base`, `total_return_base`, `capital_gain_base`,
  `currency_gain_base`, `unrealized_*`) is unchanged vs current expected strings.
- New: the four new fields serialize with correct values and availability;
  `brokerage_total_base` and `realized_sell_brokerage_base` are Available even when
  cost basis is unavailable (a missing-FX buy).
- New: reconciliation at the API level — `held_fee + realized_fee` equals
  `brokerage_total` when FX is fully available, and
  `realized_fee − realized_sell_brokerage` equals the allocated buy fee on sold
  shares.

**Verify:** from `backend/` — `cargo build`, `cargo test api::gains`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt`. No human testing (no UI
consumer yet).

---

## Phase 3 — Frontend: gross decomposition, Brokerage costs row, geometry

The user-visible slice.

**Changes (`frontend/src/api/types.ts`):**
- Add `held_fee_component_base?`, `realized_fee_base?`,
  `realized_sell_brokerage_base?`, `brokerage_total_base?` to `GainsRow` (optional,
  like the existing `unrealized_*_effect_base`).

**Missing-field fallback (settled).** Frontend and backend are versioned and
deployed together, so a mismatch is not an expected runtime state. If
`brokerage_total_base` is absent (older backend), the view-model **falls back to
today's net waterfall**: no Brokerage costs row, net cost basis / price effect /
realized rows, exactly the current behavior. This is graceful degradation rather
than an error, keeping the panel functional against any stale backend. The
gross-decomposition branch activates only when the fee fields are present.

**Changes (`frontend/src/components/waterfallViewModel.ts`):** derivation lives
here (not in the component), per the architecture rule.
- Add a pure helper to subtract/add `MoneyValue`s with availability propagation
  (mirroring the existing `displaySum`), used to derive gross cost basis, gross
  price effect, and gross realized gain.
- **Open mode:** Cost basis (held) uses gross; Price effect uses gross; FX effect
  unchanged; Market value subtotal unchanged; Realized gain uses gross; then push a
  new `brokerage` effect row with value `−brokerage_total_base` (direction "down")
  before Dividend income; income and Total return unchanged.
- **Closed mode:** Cost basis (sold) gross = `cost_basis_base − (R − S)`; Price
  effect gross = `price_effect_base + R`; FX unchanged; Proceeds subtotal gross =
  `proceeds_base + S` (**sell brokerage only** — never `+ T`, which would
  double-count buy brokerage); `brokerage` row (`−T`) after the Proceeds subtotal;
  income and Total return unchanged.
- **Baseline/geometry:** the Total return delta bar and the `buildStackedSegments`
  gray base float from the **displayed gross cost basis** (bars float from the row
  the user sees). Pass gross cost basis as the `baseline` argument; the percent
  `denominator` stays the existing **net** "total capital deployed" (settled — see
  Settled follow-up decisions), so the total-return `%` is byte-identical.
- **Stacked segments:** add `"brokerage"` to the effect-keys list as a "down"
  segment — open: `["price","fx","realized","brokerage","income"]`; closed:
  `["price","fx","brokerage","income"]`. Confirm the down segment overlays the loss
  zone the same way the realized-loss path already does.
- **Brokerage row percent:** vs total capital deployed (held + sold cost basis),
  display-only; zero-fee renders `0.00` (reuse the existing zero-numerator calm
  path). The row is **always available** (never an unavailable/`status` bar), even
  when the gross decomposition rows are unavailable.

**Changes (`frontend/src/components/GainsWaterfall.tsx` / `styles.css`):**
- Expected to need **no** component or style change: the new row is an ordinary
  "effect" row with direction "down", already rendered red by the existing
  `wf-bar down` class. Confirm during implementation; only touch `styles.css` if a
  visual gap appears (reuse existing tokens per `docs/VisualDesign.DarkTheme.md`).

**Tests (`frontend/src/components/waterfallViewModel.test.ts`):**
- Reconciliation for three shapes — never-sold open, partially-sold open (both fee
  kinds; the Broadcom case), fully closed — asserting the displayed effect rows sum
  to the unchanged Total return.
- **Closed position with both buy and sell brokerage** (matching the existing
  closed fixture: buy fee 20, sell fee 5, `proceeds_base` 13195.00): assert the
  gross Proceeds subtotal is **13200.00** (= `proceeds_base + S`), explicitly
  **not** 13220.00 (`proceeds_base + T`), and that gross cost basis, gross price
  effect, FX and Proceeds telescope consistently.
- Brokerage row present with value `−T` and direction "down"; zero-fee shows `0.00`
  and stays available.
- Brokerage row still available and correct when `cost_basis_base` is unavailable
  (gross rows unavailable) — verifies the always-available contract.
- Missing-field fallback: when `brokerage_total_base` is absent, the view-model
  emits today's net waterfall with no Brokerage costs row.
- Stacked total-return segments include the brokerage down-segment for both modes.

**Version bumps (user-visible + API surface change):**
- `frontend/package.json`: `0.19.0` → `0.20.0`.
- `backend/Cargo.toml`: `0.13.0` → `0.14.0`.

**Verify:** from `frontend/` — `npm run check`, then `npm run fmt`.
**External human testing recommended here:** open a partially-sold instrument (the
Broadcom case), a fully closed instrument, and a zero-fee instrument; confirm the
red "Brokerage costs" bar appears in the right slot, the Total return figure is
unchanged from before, and the layout matches the dark theme.

---

## Phase 4 — Decision log + doc reconciliation

- Add a `docs/DecisionLog.md` entry (template below). This is noteworthy: it
  promotes the deferred fees-as-a-step, commits the gross-decomposition +
  raw-ledger-sum sourcing, and records the accepted cross-surface divergence.
- Confirm no durable design doc needs edits: `docs/Design.HighLevel.md` and
  `docs/CurrencyAndFxRules.md` describe net money-of-record semantics that are
  unchanged; the ephemeral `docs/Design.GainsBreakdownWaterfall.md` referenced by
  the 2026-06-22 entry has already been deleted per repo convention. Update only if
  a surviving doc describes the waterfall row set.

**Verify:** doc review only.

**Proposed DecisionLog entry:**

```
## 2026-07-09 - Waterfall Shows Brokerage As An Explicit Gross-Decomposition Step
Decision: The asset-view Gains breakdown waterfall decomposes fees into an explicit
negative "Brokerage costs" step (after Realized gain / Proceeds, before Dividend
income) and shows fee-free (gross) Cost basis, Price effect, and Realized gain
rows; Total return stays numerically identical. The step's value is the raw sum of
ledger brokerage, so it is always shown even when FX gaps make the gross rows
unavailable. The API adds additive `held_fee_component_base`, `realized_fee_base`,
`realized_sell_brokerage_base`, and `brokerage_total_base` fields; every
pre-existing GainRow money field stays net and byte-identical, and gross rows are
derived in the frontend view-model. Waterfall percentages keep the existing net
(fee-inclusive) denominator, so the total-return `%` is unchanged.
Context: Fees were invisible, baked into cost basis and realized gain; the
2026-06-22 waterfall decision deferred fees-as-a-step.
Consequences: The asset waterfall's gross "Price effect" and gross closed "Proceeds"
intentionally diverge from the fee-inclusive Gains table columns and from Avanza's
net settlement amount; this is a display-only decomposition and never changes money
of record. Holdings, asset summary, Gains table, and totals keep fee-inclusive
numbers.
```

---

## Documents to update

- `docs/DecisionLog.md` — new entry above (required).
- `docs/Design.HighLevel.md` — review only; edit only if it enumerates waterfall
  rows.

## Version bumps

- `frontend/package.json` 0.19.0 → 0.20.0; `backend/Cargo.toml` 0.13.0 → 0.14.0
  (Phase 3).

## Open Questions

None remaining. The three questions raised in the first draft were settled by the
Codex review and the user's answers:

- Percent denominator stays **net** (byte-identical total-return `%`).
- The Brokerage costs row has **no unavailable state** (always-available
  `MoneyValue`).
- Closed-mode gross Proceeds uses `proceeds_base + sell_brokerage` (via the new
  `realized_sell_brokerage_base` field), **not** `proceeds_base + brokerage_total`.
