# Import Delta Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a CSV file contains rows already in the database, detect them by fingerprint, suppress false ledger errors, and show only new transactions in the import preview — with already-imported assets listed in a separate collapsed section.

**Architecture:** The plan builder (`build_plan`) computes per-instrument fingerprint bags from `PlanContext.existing_ledgers` before the main row loop. Each `Mapped` row whose fingerprint matches a DB entry is counted as "already imported" (not added to the ledger or errors, but tracked in a separate `already_imported_*` counter on `AssetGroup`). Assets with zero new rows are moved to `ImportPlan.already_imported_assets`. The API DTO and TypeScript types mirror this split, and the frontend renders a collapsible "Already imported" table below the main asset list.

**Tech Stack:** Rust (backend), TypeScript + React (frontend), `rust_decimal::Decimal`, `chrono::NaiveDate`

## Global Constraints

- Build: `cargo build` from `backend/`; lint+format: `cargo clippy --all-targets -- -D warnings` then `cargo fmt`
- Frontend: `npm run check` (tsc + Biome lint) then `npm run fmt` from `frontend/`
- Reducers stay pure; selectors stay in view-model layer; components only render
- No new dependencies

---

## File Map

| File | Change |
|------|--------|
| `backend/src/import/core/plan.rs` | Add fingerprint helpers, `already_imported_*` fields to `AssetGroup`, `already_imported_assets` to `ImportPlan`, detection logic in `build_plan` |
| `backend/src/api/import.rs` | Add `already_imported_*` fields to `AssetGroupDto`, `already_imported_assets` to `ImportPreview`, update DTO mapping |
| `frontend/src/api/types.ts` | Add `already_imported_*` fields to `ImportAssetGroup`, add `already_imported_assets` to `ImportPreview` |
| `frontend/src/components/ImportView.tsx` | Render `(+N already imported)` in mixed-asset count cells; add collapsible already-imported section |
| `frontend/src/components/ImportView.reducer.test.ts` | Update `makePreview` factory to include new required fields |

---

## Task 1: Backend — fingerprint detection and domain types

**Files:**
- Modify: `backend/src/import/core/plan.rs`

**Interfaces:**
- Produces: `AssetGroup.already_imported_buys/sells/splits/dividends: usize`, `ImportPlan.already_imported_assets: Vec<AssetGroup>`

---

- [ ] **Step 1: Write failing tests**

Add the `BUY_ONLY` constant alongside `FRESH` in the test module (a single-row CSV with only the buy, no sell):

```rust
const BUY_ONLY: &str = concat!(
    "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
    "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
    "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"9,60\",SEK,\"0,100000\",\"1259,60\",All Trades,\n",
);
```

Then add these three tests inside the existing `#[cfg(test)] mod tests` block in `backend/src/import/core/plan.rs`:

```rust
#[test]
fn already_imported_buy_moves_to_already_imported_assets() {
    // Existing ledger has a buy for MSFT (qty 10, price 12.50)
    let existing_tx = LedgerTransaction {
        id: 1,
        trade_date: NaiveDate::from_ymd_opt(2026, 6, 12).unwrap(),
        kind: TransactionKind::Buy,
        quantity: 10,
        price: Some(dec!(12.50)),
        dividend_per_share: None,
        fx_rate_to_base: Some(dec!(0.1)),
        brokerage_base: dec!(9.60),
    };
    let mut existing_ledgers = BTreeMap::new();
    existing_ledgers.insert(1, vec![existing_tx]);
    let ctx = PlanContext {
        existing_instruments: vec![ExistingInstrument {
            id: 1,
            exchange: "NASDAQ".into(),
            symbol: "MSFT".into(),
            currency: "USD".into(),
            isin: None,
        }],
        existing_ledgers,
        max_existing_id: 1,
    };
    // CSV contains only that same buy (BUY_ONLY, not FRESH which also has a sell)
    let plan = plan_for(BUY_ONLY, ctx);
    // The buy row is already imported — so no new assets
    assert_eq!(plan.assets.len(), 0, "no new assets expected");
    assert_eq!(plan.already_imported_assets.len(), 1);
    let group = &plan.already_imported_assets[0];
    assert_eq!(group.already_imported_buys, 1);
    assert_eq!(group.buys, 0);
    assert_eq!(plan.counts.errors, 0);
}

#[test]
fn mixed_asset_has_already_imported_counts_and_no_position_error() {
    // Existing ledger: buy of 10 MSFT at 12.50
    let existing_tx = LedgerTransaction {
        id: 1,
        trade_date: NaiveDate::from_ymd_opt(2026, 6, 12).unwrap(),
        kind: TransactionKind::Buy,
        quantity: 10,
        price: Some(dec!(12.50)),
        dividend_per_share: None,
        fx_rate_to_base: Some(dec!(0.1)),
        brokerage_base: dec!(9.60),
    };
    let mut existing_ledgers = BTreeMap::new();
    existing_ledgers.insert(1, vec![existing_tx]);
    let ctx = PlanContext {
        existing_instruments: vec![ExistingInstrument {
            id: 1,
            exchange: "NASDAQ".into(),
            symbol: "MSFT".into(),
            currency: "USD".into(),
            isin: None,
        }],
        existing_ledgers,
        max_existing_id: 1,
    };
    // FRESH contains: buy 10 @ 12.50, sell 4 @ 12.60 — buy is already imported,
    // sell is new but valid because the existing ledger covers the position.
    let plan = plan_for(FRESH, ctx);
    assert_eq!(plan.already_imported_assets.len(), 0, "mixed asset stays in main list");
    assert_eq!(plan.assets.len(), 1);
    let group = &plan.assets[0];
    assert_eq!(group.already_imported_buys, 1, "buy counted as already imported");
    assert_eq!(group.sells, 1, "sell is new");
    assert_eq!(plan.counts.errors, 0, "no position error: existing ledger covers the sell");
}

#[test]
fn two_identical_existing_entries_matched_independently() {
    // Two identical buys in the existing ledger (legitimately same-day same-price fills)
    let tx = LedgerTransaction {
        id: 1,
        trade_date: NaiveDate::from_ymd_opt(2026, 6, 12).unwrap(),
        kind: TransactionKind::Buy,
        quantity: 10,
        price: Some(dec!(12.50)),
        dividend_per_share: None,
        fx_rate_to_base: Some(dec!(0.1)),
        brokerage_base: dec!(9.60),
    };
    let tx2 = LedgerTransaction { id: 2, ..tx.clone() };
    let mut existing_ledgers = BTreeMap::new();
    existing_ledgers.insert(1, vec![tx, tx2]);
    let ctx = PlanContext {
        existing_instruments: vec![ExistingInstrument {
            id: 1,
            exchange: "NASDAQ".into(),
            symbol: "MSFT".into(),
            currency: "USD".into(),
            isin: None,
        }],
        existing_ledgers,
        max_existing_id: 2,
    };
    // FRESH has: buy 10 @ 12.50, sell 4 @ 12.60. The buy matches one existing entry.
    // The second existing buy + the sell: position = 10 + 10 - 4 = 16 → no error.
    let plan = plan_for(FRESH, ctx);
    assert_eq!(plan.already_imported_assets.len(), 0);
    let group = &plan.assets[0];
    assert_eq!(group.already_imported_buys, 1);
    assert_eq!(group.sells, 1);
    assert_eq!(plan.counts.errors, 0);
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

```
cd backend && cargo test -p ticker-tape-tally-backend import::core::plan::tests::already_imported 2>&1 | head -30
```
Expected: compile error — `already_imported_assets` not found on `ImportPlan`, `already_imported_buys` not found on `AssetGroup`.

- [ ] **Step 3: Add `already_imported_*` fields to `AssetGroup` and `ImportPlan`**

In `backend/src/import/core/plan.rs`, modify `AssetGroup` and `ImportPlan`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetGroup {
    pub asset_key: String,
    pub name: String,
    pub currency: String,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
    pub already_imported_buys: usize,
    pub already_imported_sells: usize,
    pub already_imported_splits: usize,
    pub already_imported_dividends: usize,
    pub default_selected: bool,
    pub skipped_reason: Option<String>,
    pub warnings: Vec<RowNote>,
    pub errors: Vec<RowNote>,
    pub is_new_instrument: bool,
}
```

Update `AssetGroup` in `asset_group_mut` to initialise the new fields as `0`:

```rust
AssetGroup {
    asset_key: key.to_string(),
    name: key.to_string(),
    currency: String::new(),
    buys: 0,
    sells: 0,
    splits: 0,
    dividends: 0,
    already_imported_buys: 0,
    already_imported_sells: 0,
    already_imported_splits: 0,
    already_imported_dividends: 0,
    default_selected: true,
    skipped_reason: None,
    warnings: Vec::new(),
    errors: Vec::new(),
    is_new_instrument: false,
}
```

Modify `ImportPlan`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPlan {
    pub counts: PlanCounts,
    pub new_instruments: Vec<InstrumentKey>,
    pub assets: Vec<AssetGroup>,
    pub already_imported_assets: Vec<AssetGroup>,
    /// Mapped rows that are NOT already imported. Used by commit_source to write
    /// only genuinely new rows even on append, so the UI and backend agree.
    pub new_mapped_rows: Vec<MappedRow>,
    pub warnings: Vec<RowNote>,
    pub errors: Vec<RowNote>,
}
```

- [ ] **Step 4: Add the fingerprint type and helper functions**

Add these private functions before `build_plan` in `backend/src/import/core/plan.rs`:

```rust
/// Stable identity tuple used to detect rows already present in the DB.
/// Quantity is in its signed ledger form: Buy > 0, Sell < 0, Split = signed delta,
/// Dividend = raw share count (mirrors the writer, which stores proposed.quantity for dividends).
///
/// The fingerprint intentionally omits FX rate, brokerage, note, source value, and currency.
/// This is fuzzy-duplicate detection: same date/kind/qty/price rows are treated as already
/// imported even if ancillary fields differ. The tradeoff is that a genuine correction
/// (e.g. corrected FX rate on an existing row) would be suppressed. This is an acceptable
/// default; exact identity matching can be added later if needed.
type TxFingerprint = (chrono::NaiveDate, &'static str, i64, Option<Decimal>, Option<Decimal>);

fn ledger_fingerprint(tx: &LedgerTransaction) -> TxFingerprint {
    (
        tx.trade_date,
        tx.kind.as_db_str(),
        tx.quantity,
        tx.price,
        tx.dividend_per_share,
    )
}

fn proposed_fingerprint(proposed: &ProposedTransaction) -> TxFingerprint {
    // Mirror the writer's quantity convention: Buy/Dividend/Split store positive quantity;
    // Sell stores negated quantity. Dividend uses proposed.quantity (eligible share count),
    // which is what the writer stores — NOT 0.
    let signed = match proposed.kind {
        TransactionKind::Sell => -proposed.quantity,
        _ => proposed.quantity,
    };
    (
        proposed.trade_date,
        proposed.kind.as_db_str(),
        signed,
        proposed.price,
        proposed.dividend_per_share,
    )
}
```

- [ ] **Step 5: Implement detection in `build_plan`**

Replace the body of `build_plan` in `backend/src/import/core/plan.rs` with the following. The change adds a fingerprint bag pre-computation, an already-imported check at the top of the `Mapped` arm, and a final separation of fully-already-imported assets into their own list.

```rust
pub fn build_plan(prepared: &PreparedImport, ctx: &PlanContext) -> ImportPlan {
    let mut warnings: Vec<RowNote> = Vec::new();
    let mut errors: Vec<RowNote> = Vec::new();
    let mut new_instruments: Vec<InstrumentKey> = Vec::new();
    let mut mapped_rows: Vec<MappedRow> = Vec::new();
    // all_mapped_rows is populated from every Mapped outcome before the already-imported
    // check, so duplicate_row_warnings sees the full set even when one of a pair is
    // already imported.
    let mut all_mapped_rows: Vec<MappedRow> = Vec::new();

    let mut asset_order: Vec<String> = Vec::new();
    let mut assets: BTreeMap<String, AssetGroup> = BTreeMap::new();
    let mut ledgers: BTreeMap<String, Vec<LedgerTransaction>> = BTreeMap::new();
    let mut seeded: BTreeSet<String> = BTreeSet::new();
    let mut skipped = 0usize;

    // Build per-instrument fingerprint bags from the existing DB ledger.
    // Each entry is a multiset: fingerprint → remaining count available to match.
    let mut fingerprint_bags: BTreeMap<i64, BTreeMap<TxFingerprint, usize>> = ctx
        .existing_ledgers
        .iter()
        .map(|(&id, txs)| {
            let mut bag: BTreeMap<TxFingerprint, usize> = BTreeMap::new();
            for tx in txs {
                *bag.entry(ledger_fingerprint(tx)).or_insert(0) += 1;
            }
            (id, bag)
        })
        .collect();

    for (index, outcome) in prepared.outcomes.iter().enumerate() {
        match outcome {
            RowOutcome::Mapped(mapped) => {
                // Track every mapped row before any filtering, so duplicate_row_warnings
                // sees the full source set even when one of a pair is already imported.
                all_mapped_rows.push(mapped.clone());

                // Detect rows already present in the DB by fingerprint match.
                let existing_id = ctx
                    .existing_instruments
                    .iter()
                    .find(|e| e.matches(&mapped.instrument))
                    .map(|e| e.id);

                let already_imported = if let Some(id) = existing_id {
                    if let Some(bag) = fingerprint_bags.get_mut(&id) {
                        let fp = proposed_fingerprint(&mapped.proposed);
                        let count = bag.entry(fp).or_insert(0);
                        if *count > 0 {
                            *count -= 1;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                if already_imported {
                    // Track the row so the asset group is created with correct name/currency,
                    // but do not add it to the ledger or to mapped_rows for error checking.
                    let key = mapped.instrument.asset_key();
                    let group = asset_group_mut(
                        &mut assets,
                        &mut asset_order,
                        &key,
                        Some(&mapped.instrument.name),
                        Some(&mapped.instrument.currency),
                    );
                    match mapped.proposed.kind {
                        TransactionKind::Buy => group.already_imported_buys += 1,
                        TransactionKind::Sell => group.already_imported_sells += 1,
                        TransactionKind::Split => group.already_imported_splits += 1,
                        TransactionKind::Dividend => group.already_imported_dividends += 1,
                    }
                    continue;
                }

                mapped_rows.push(mapped.clone());
                process_mapped(
                    index,
                    mapped,
                    ctx,
                    &mut ledgers,
                    &mut seeded,
                    &mut new_instruments,
                    &mut warnings,
                    &mut errors,
                    &mut asset_order,
                    &mut assets,
                );
            }
            RowOutcome::Skip { asset_key, note } => {
                skipped += 1;
                warnings.push(note.clone());
                if let Some(key) = asset_key {
                    let group = asset_group_mut(&mut assets, &mut asset_order, key, None, None);
                    group.warnings.push(note.clone());
                }
            }
            RowOutcome::Excluded {
                asset_key,
                name,
                currency,
                note,
            } => {
                skipped += 1;
                if let Some(key) = asset_key {
                    let group = asset_group_mut(
                        &mut assets,
                        &mut asset_order,
                        key,
                        name.as_deref(),
                        currency.as_deref(),
                    );
                    group.skipped_reason = Some(note.message.clone());
                    group.default_selected = false;
                }
            }
            RowOutcome::Error { asset_key, note } => {
                errors.push(note.clone());
                if let Some(key) = asset_key {
                    let group = asset_group_mut(&mut assets, &mut asset_order, key, None, None);
                    group.errors.push(note.clone());
                }
            }
        }
    }

    // Use all_mapped_rows so that duplicate warnings fire even when one of the
    // duplicate pair was already imported (and thus absent from mapped_rows).
    duplicate_row_warnings(&all_mapped_rows, &mut warnings);
    ledger_errors(&mut ledgers, ctx, prepared, &mut errors);
    attach_asset_notes(&mut assets, &warnings, &errors, &mapped_rows);

    for group in assets.values_mut() {
        if group.buys + group.sells + group.splits + group.dividends == 0
            && (!group.warnings.is_empty() || !group.errors.is_empty())
        {
            group.skipped_reason = Some("no writable rows (all skipped)".to_string());
            group.default_selected = false;
        }
    }

    let counts = PlanCounts {
        rows: prepared.counts.rows,
        buys: prepared.counts.buys,
        sells: prepared.counts.sells,
        splits: prepared.counts.splits,
        dividends: prepared.counts.dividends,
        new_instruments: new_instruments.len(),
        skipped,
        warnings: warnings.len(),
        errors: errors.len(),
    };

    // Separate assets: those with zero new rows (all rows already imported) go
    // to already_imported_assets; mixed and fully-new assets stay in assets.
    let mut already_imported_assets: Vec<AssetGroup> = Vec::new();
    let assets: Vec<AssetGroup> = asset_order
        .into_iter()
        .filter_map(|key| {
            let group = assets.remove(&key)?;
            let has_new =
                group.buys + group.sells + group.splits + group.dividends > 0
                || group.skipped_reason.is_some()
                || !group.warnings.is_empty()
                || !group.errors.is_empty();
            let has_already_imported = group.already_imported_buys
                + group.already_imported_sells
                + group.already_imported_splits
                + group.already_imported_dividends
                > 0;
            if !has_new && has_already_imported {
                already_imported_assets.push(group);
                None
            } else {
                Some(group)
            }
        })
        .collect();

    ImportPlan {
        counts,
        new_instruments,
        assets,
        already_imported_assets,
        new_mapped_rows: mapped_rows,
        warnings,
        errors,
    }
}
```

- [ ] **Step 6: Run the new tests**

```
cd backend && cargo test -p ticker-tape-tally-backend import::core::plan::tests 2>&1 | tail -20
```
Expected: all tests pass, including the three new ones.

- [ ] **Step 7: Run clippy and fmt**

```
cd backend && cargo clippy --all-targets -- -D warnings 2>&1 | tail -20
cd backend && cargo fmt
```
Expected: no warnings or errors.

- [ ] **Step 8: Commit**

```
git add backend/src/import/core/plan.rs
git commit -m "feat(import): detect already-imported rows by fingerprint in build_plan"
```

---

## Task 2: Backend API — update DTO and `ImportPreview`

**Files:**
- Modify: `backend/src/api/import.rs`

**Interfaces:**
- Consumes: `AssetGroup.already_imported_buys/sells/splits/dividends`, `ImportPlan.already_imported_assets`
- Produces: `AssetGroupDto.already_imported_buys/sells/splits/dividends`, `ImportPreview.already_imported_assets: Vec<AssetGroupDto>`

---

- [ ] **Step 1: Add fields to `AssetGroupDto` and `ImportPreview`**

In `backend/src/api/import.rs`, update `AssetGroupDto`:

```rust
#[derive(Debug, Serialize)]
pub struct AssetGroupDto {
    pub asset_key: String,
    pub name: String,
    pub currency: String,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
    pub already_imported_buys: usize,
    pub already_imported_sells: usize,
    pub already_imported_splits: usize,
    pub already_imported_dividends: usize,
    pub default_selected: bool,
    pub skipped_reason: Option<String>,
    pub warnings: Vec<RowNoteDto>,
    pub errors: Vec<RowNoteDto>,
    pub is_new_instrument: bool,
}
```

Update `ImportPreview`:

```rust
#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub metadata: Option<PreviewMetadata>,
    pub counts: PreviewCounts,
    pub assets: Vec<AssetGroupDto>,
    pub already_imported_assets: Vec<AssetGroupDto>,
    pub new_instruments: Vec<NewInstrumentDto>,
    pub warnings: Vec<RowNoteDto>,
    pub errors: Vec<RowNoteDto>,
    pub duplicate_of_batch_id: Option<i64>,
    pub replace_candidate_batch_id: Option<i64>,
    pub replace_candidate_warning: Option<String>,
}
```

- [ ] **Step 2: Update the DTO mapping function**

Find the function that maps `ImportPlan` → `ImportPreview` (it is the `preview_source` helper or its inline mapping). The mapping for each `AssetGroup` to `AssetGroupDto` currently sets the basic fields. Locate the `AssetGroupDto { ... }` construction and add the four new fields. Also populate `already_imported_assets` on `ImportPreview`.

Search for the mapping with:
```
grep -n "AssetGroupDto" backend/src/api/import.rs
```

Then update the `AssetGroupDto` construction to include:
```rust
already_imported_buys: group.already_imported_buys,
already_imported_sells: group.already_imported_sells,
already_imported_splits: group.already_imported_splits,
already_imported_dividends: group.already_imported_dividends,
```

And update the `ImportPreview` construction to include:
```rust
already_imported_assets: plan
    .already_imported_assets
    .iter()
    .map(asset_group_to_dto)
    .collect(),
```

(If the mapping is inline rather than extracted into `asset_group_to_dto`, extract it to avoid repeating the mapping for `assets` and `already_imported_assets`.)

- [ ] **Step 3: Build to verify**

```
cd backend && cargo build 2>&1 | tail -20
```
Expected: compiles cleanly. Fix any missing field errors.

- [ ] **Step 4: Run clippy and fmt**

```
cd backend && cargo clippy --all-targets -- -D warnings 2>&1 | tail -20
cd backend && cargo fmt
```

- [ ] **Step 5: Commit**

```
git add backend/src/api/import.rs
git commit -m "feat(import): expose already_imported_assets in ImportPreview DTO"
```

---

## Task 2b: Backend — fix commit path to skip already-imported rows

**Files:**
- Modify: `backend/src/api/import.rs`

**Context:** `build_plan` now detects already-imported rows and stores genuinely new rows in `ImportPlan.new_mapped_rows`. But `commit_source` currently rebuilds mapped rows directly from `effective.outcomes`, bypassing that detection. Without this fix the UI shows rows as "already imported" but the backend still writes them, causing silent duplicates on append.

**Note:** Avanza `mode=replace` uses `refresh_batch` (a separate code path) and is not affected.

---

- [ ] **Step 1: Use `plan.new_mapped_rows` in `commit_source`**

In `backend/src/api/import.rs`, find `commit_source`. Replace the block that rebuilds `mapped` from `effective.outcomes`:

```rust
// BEFORE:
let mapped: Vec<MappedRow> = effective
    .outcomes
    .iter()
    .filter_map(|outcome| match outcome {
        RowOutcome::Mapped(mapped) => Some(mapped.clone()),
        _ => None,
    })
    .collect();
let batch_id = write_batch(state, source, &hash, &mapped).await?;
```

With:

```rust
// AFTER — use the already-filtered list from the plan so already-imported
// rows are not written again on append.
let batch_id = write_batch(state, source, &hash, &plan.new_mapped_rows).await?;
```

Remove the now-unused `mapped` binding and the `RowOutcome` import if it becomes dead.

- [ ] **Step 2: Add a regression integration test (or document the manual test)**

Add a doc comment on `commit_source` describing the expected behavior:

```
// Invariant: already-imported rows (fingerprint-matched by build_plan) are
// excluded from new_mapped_rows and therefore never written to write_batch.
// A CSV that is fully already-imported produces an empty write_batch call.
```

If an integration test harness is available, add a test that: (1) imports a file, (2) previews a superset file, (3) commits the superset in append mode, and (4) asserts the DB row count increased only by the genuinely new rows.

- [ ] **Step 3: Build and run clippy**

```
cd backend && cargo build 2>&1 | tail -20
cd backend && cargo clippy --all-targets -- -D warnings 2>&1 | tail -20
cd backend && cargo fmt
```

- [ ] **Step 4: Commit**

```
git add backend/src/api/import.rs backend/src/import/core/plan.rs
git commit -m "fix(import): exclude already-imported rows from append write_batch"
```

---

## Task 3: Frontend — types and UI

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/components/ImportView.tsx`
- Modify: `frontend/src/components/ImportView.reducer.test.ts`

**Interfaces:**
- Consumes: `ImportPreview.already_imported_assets`, `ImportAssetGroup.already_imported_buys/sells/splits/dividends`

---

- [ ] **Step 1: Update TypeScript types**

In `frontend/src/api/types.ts`, update `ImportAssetGroup`:

```typescript
export interface ImportAssetGroup {
  asset_key: string;
  name: string;
  currency: string;
  buys: number;
  sells: number;
  splits: number;
  dividends: number;
  already_imported_buys: number;
  already_imported_sells: number;
  already_imported_splits: number;
  already_imported_dividends: number;
  default_selected: boolean;
  skipped_reason: string | null;
  warnings: ImportRowNote[];
  errors: ImportRowNote[];
  is_new_instrument: boolean;
}
```

Update `ImportPreview`:

```typescript
export interface ImportPreview {
  metadata: { title: string; date_from: string; date_to: string } | null;
  counts: ImportCounts;
  assets: ImportAssetGroup[];
  already_imported_assets: ImportAssetGroup[];
  new_instruments: ImportNewInstrument[];
  warnings: ImportRowNote[];
  errors: ImportRowNote[];
  duplicate_of_batch_id: number | null;
  /** Batch id to target for a refresh commit. Null when no prior Avanza batch exists. */
  replace_candidate_batch_id: number | null;
  /** Non-blocking warning when multiple live Avanza batches exist. */
  replace_candidate_warning: string | null;
}
```

- [ ] **Step 2: Fix the reducer test factory**

In `frontend/src/components/ImportView.reducer.test.ts`, update `makePreview` so each asset in `assets.map()` includes the new fields, and add `already_imported_assets: []` to the returned object:

```typescript
function makePreview(
  assets: Array<{
    asset_key: string;
    default_selected: boolean;
    skipped_reason: string | null;
  }> = [],
): ImportPreview {
  return {
    metadata: null,
    counts: {
      rows: 10,
      buys: 5,
      sells: 2,
      splits: 1,
      dividends: 1,
      new_instruments: 0,
      skipped: 1,
      warnings: 0,
      errors: 0,
    },
    assets: assets.map((a) => ({
      ...a,
      name: a.asset_key,
      currency: "USD",
      buys: 1,
      sells: 0,
      splits: 0,
      dividends: 0,
      already_imported_buys: 0,
      already_imported_sells: 0,
      already_imported_splits: 0,
      already_imported_dividends: 0,
      warnings: [],
      errors: [],
      is_new_instrument: false,
    })),
    already_imported_assets: [],
    new_instruments: [],
    warnings: [],
    errors: [],
    duplicate_of_batch_id: null,
    replace_candidate_batch_id: null,
    replace_candidate_warning: null,
  };
}
```

- [ ] **Step 3: Run frontend checks to confirm the type changes compile**

```
cd frontend && npm run check 2>&1 | tail -30
```
Expected: type errors about missing fields in `ImportView.tsx` (since the asset table cells don't yet use the new fields, but the `ImportPreview` now requires `already_imported_assets`). Fix any structural errors before proceeding to the next step.

- [ ] **Step 4: Update `ImportView.tsx` — mixed-asset count cells**

In `frontend/src/components/ImportView.tsx`, update the asset table row to show `(+N already imported)` in each count cell when the count is non-zero. Replace the four count `<td>` cells inside `preview.assets.map(...)`:

```tsx
<td className="number">
  {formatGroupedNumber(asset.buys)}
  {asset.already_imported_buys > 0 && (
    <span className="muted">
      {" "}(+{formatGroupedNumber(asset.already_imported_buys)})
    </span>
  )}
</td>
<td className="number">
  {formatGroupedNumber(asset.sells)}
  {asset.already_imported_sells > 0 && (
    <span className="muted">
      {" "}(+{formatGroupedNumber(asset.already_imported_sells)})
    </span>
  )}
</td>
<td className="number">
  {formatGroupedNumber(asset.splits)}
  {asset.already_imported_splits > 0 && (
    <span className="muted">
      {" "}(+{formatGroupedNumber(asset.already_imported_splits)})
    </span>
  )}
</td>
<td className="number">
  {formatGroupedNumber(asset.dividends)}
  {asset.already_imported_dividends > 0 && (
    <span className="muted">
      {" "}(+{formatGroupedNumber(asset.already_imported_dividends)})
    </span>
  )}
</td>
```

- [ ] **Step 5: Update `ImportView.tsx` — already-imported section**

Add a `useState` import for `showAlreadyImported` at the top of `ImportView`. Then, after the closing `</>` of the `{preview.assets.length > 0 ? ... : null}` block (and before `{preview.errors.length > 0 ? ...}`), add:

First, add the state variable inside `ImportView`:
```tsx
const [showAlreadyImported, setShowAlreadyImported] = useState(false);
```

Then add the section after the assets table block:

```tsx
{preview.already_imported_assets.length > 0 ? (
  <>
    <button
      type="button"
      className="button secondary"
      onClick={() => setShowAlreadyImported((v) => !v)}
    >
      {showAlreadyImported ? "Hide" : "Show"} already imported (
      {preview.already_imported_assets.length} asset
      {preview.already_imported_assets.length === 1 ? "" : "s"})
    </button>
    {showAlreadyImported ? (
      <div className="table-wrap asset-table">
        <table>
          <thead>
            <tr>
              <th className="checkbox-head">
                <span className="sr-only">Select</span>
              </th>
              <th>Asset</th>
              <th>Currency</th>
              <th>Buys</th>
              <th>Sells</th>
              <th>Splits</th>
              <th>Dividends</th>
            </tr>
          </thead>
          <tbody>
            {preview.already_imported_assets.map((asset) => (
              <tr key={asset.asset_key}>
                <td className="checkbox-cell">
                  <input
                    type="checkbox"
                    className="asset-check"
                    checked={false}
                    disabled
                    aria-label={`Include ${asset.name}`}
                    onChange={() => undefined}
                  />
                </td>
                <td>
                  <div className="asset-name-cell">
                    <strong>{asset.name}</strong>
                    <div className="asset-meta-line">
                      <span>{asset.asset_key}</span>
                    </div>
                  </div>
                </td>
                <td>{asset.currency}</td>
                <td className="number">
                  {formatGroupedNumber(asset.already_imported_buys)}
                </td>
                <td className="number">
                  {formatGroupedNumber(asset.already_imported_sells)}
                </td>
                <td className="number">
                  {formatGroupedNumber(asset.already_imported_splits)}
                </td>
                <td className="number">
                  {formatGroupedNumber(asset.already_imported_dividends)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    ) : null}
  </>
) : null}
```

- [ ] **Step 6: Disable append commit when there are no writable assets**

In `frontend/src/components/ImportView.tsx`, find the commit button for non-replace (append) imports. Add a derived boolean and use it to disable the button:

```tsx
const noWritableAssets =
  preview.assets.length === 0 && preview.already_imported_assets.length > 0;
```

Then on the append commit button:

```tsx
disabled={/* existing conditions */ || noWritableAssets}
title={noWritableAssets ? "All rows are already imported — nothing to commit" : undefined}
```

This prevents the user from committing a file where every row is already in the database. `mode=replace` (Avanza refresh) is unaffected — it uses a separate button and path that intentionally re-applies the full file.

- [ ] **Step 7: Run frontend checks**

```
cd frontend && npm run check 2>&1 | tail -30
```
Expected: passes with no errors or warnings.

- [ ] **Step 8: Run frontend tests**

```
cd frontend && npm test 2>&1 | tail -30
```
Expected: all tests pass.

- [ ] **Step 9: Run frontend fmt**

```
cd frontend && npm run fmt
```

- [ ] **Step 10: Commit**

```
git add frontend/src/api/types.ts frontend/src/components/ImportView.tsx frontend/src/components/ImportView.reducer.test.ts
git commit -m "feat(import): show already-imported assets in collapsed section with delta counts"
```

---

## Self-Review

**Spec coverage check:**

| Requirement | Task |
|------------|------|
| Detect already-imported rows by (instrument + date + kind + quantity + price) fingerprint | Task 1, Steps 4–5 |
| Suppress false `sell_exceeds_position` errors from double-counting | Task 1, Step 5 (already-imported rows bypass `process_mapped` and `ledger_errors`) |
| Multiset matching for instruments with two identical legitimate transactions | Task 1, tests Step 1 + Step 5 bag logic |
| Assets where ALL rows already imported → go to `already_imported_assets` | Task 1, Step 5 (separation logic) |
| Mixed assets show `(+N already imported)` alongside new counts | Task 3, Step 4 |
| Frontend collapsed "Already imported" section | Task 3, Step 5 |
| Reducer test fixture updated | Task 3, Step 2 |
| Clippy + fmt pass | Task 1 Step 7, Task 2 Step 4 |

**Placeholder scan:** None found.

**Type consistency:**
- `already_imported_buys/sells/splits/dividends` used consistently in `AssetGroup` (Rust), `AssetGroupDto` (Rust), `ImportAssetGroup` (TS)
- `already_imported_assets` used consistently in `ImportPlan` (Rust), `ImportPreview` (Rust struct + TS interface)
- `proposed_fingerprint` / `ledger_fingerprint` defined in Task 1 Step 4 and used in Task 1 Step 5
- `TxFingerprint` type alias defined once in Task 1 Step 4 and used in Steps 4–5
