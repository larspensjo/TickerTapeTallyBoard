# Avanza Full-History Replace Import Plan

> **For agentic workers:** Implement this plan task-by-task, in order. Steps use checkbox (`- [ ]`) syntax for tracking. Each task ends by staging its files (`git add ...`) for review -- do **not** `git commit`. Follow each task's verification step before moving on.

**Goal:** Make Avanza full-history imports safe as the default workflow. When a previous Avanza import exists, importing a newer Avanza All Trades CSV should replace that previous Avanza-imported ledger snapshot while keeping instruments, price history, FX rates, provider symbol mappings, market-data runs, and manual transactions.

**Why:** Avanza exports are naturally full-history snapshots. Today, a new export with all historical rows plus latest transactions has a new raw file hash, so the importer treats it as a new batch and can append duplicate historical transactions. The preferred user workflow is to re-import the full Avanza history whenever new transactions arrive or importer support improves, such as when dividends become writable.

**Core behavior:** Avanza `Import` defaults to **refresh previous Avanza import** when a previous Avanza batch exists. The refresh is atomic: unchanged imported rows keep their transaction ids, removed/changed/new rows are reconciled in one database transaction, and final ledgers are re-derived before commit. Non-ledger reference data is preserved.

**Out of scope:**
- General row-level broker synchronization across arbitrary sources.
- Deleting unused instruments or market data cleanup.
- Dividend ledger design itself. This plan leaves space for dividend rows to be included once that feature exists, but it does not design dividend amount fields, income attribution, or dividend UI.
- Command-log redo integration. If command undo is implemented first, adapt the write service to use the command layer instead of adding a parallel mutation path.

**Implementation constraints:**
- Preserve the existing unidirectional import flow: parse/prepare -> plan -> validated write -> derived ledger state.
- Keep backend SQL repository queries as static SQL strings; with this repo's SQLx version, `format!`-built query strings can fail `SqlSafeStr` even when interpolation is local constants only.
- Keep entry-point files thin. Put write orchestration in import/core or command services, not in route handlers.
- Stage each completed task for review, but do not commit.

---

## Current Behavior Summary

- `POST /api/import/avanza/preview` parses the file and builds a plan against the existing ledger.
- `POST /api/import/avanza/commit` writes every mapped buy/sell/split row into a new `import_batches` row unless the **raw file hash** exactly matches an existing batch and `allow_duplicate` is false.
- `transactions` has no unique imported-row identity. Re-exporting all history with a changed file hash can duplicate old rows.
- `POST /api/import/rollback/{batch_id}` deletes only transactions tagged with that batch, then deletes the batch row. It intentionally leaves instruments and all market-data tables alone.
- `write_batch` already owns one SQL transaction for batch creation, instrument resolution, transaction insertion, affected-ledger derivation, and commit; refresh should reuse this atomic shape.
- `rollback` is the closest existing delete-and-rederive template, but refresh keeps the batch row and replaces only that batch's imported transaction set.
- Position derivation and ledger reads depend on `(trade_date, id)` ordering. Preserving ids for unchanged imported rows is therefore part of correctness, not only cosmetic stability.
- `AVANZA` is already an allowed import source; no migration is needed just to target Avanza batches.

---

## Target Semantics

### Default Avanza Import

When previewing an Avanza file:
- If there is no previous Avanza batch, the UI behaves like a normal first import.
- If a previous Avanza batch exists, the preview states that committing will refresh that batch's imported transactions.
- If the same raw file was already imported as the previous Avanza batch, the default action is still refresh/no-double rather than append. It may be a no-op except for timestamp/hash bookkeeping, but it must not create a second copy of transactions.

When committing an Avanza file in refresh mode:
- The backend uses a caller-supplied expected batch id from preview, not an implicit hidden choice only made at commit time.
- The backend verifies that the expected batch still exists and has source `AVANZA`.
- The backend replaces the ledger rows belonging to that batch with the newly mapped rows in one SQL transaction.
- Instruments are resolved/upserted as today and remain in place.
- Existing instrument ids should be reused by ISIN/symbol resolution, so price history and provider mappings remain attached.
- The affected ledgers are re-derived after replacement and before commit.
- If final derivation fails, the entire refresh is rejected and the old imported rows remain unchanged.

### Append Escape Hatch

Keep an explicit advanced path for exceptional cases:
- Append as a new batch is allowed only through a deliberate UI action such as `Append as new batch...`.
- The existing `allow_duplicate=true` semantics may remain for this path, but the UI must make clear that append can duplicate holdings.
- Sharesight behavior remains unchanged unless a future plan generalizes replace semantics to it.

### Excluding Assets During Refresh

If the user unchecks an asset during an Avanza refresh:
- The old imported rows for that asset are removed with the old batch snapshot.
- The unchecked asset is not written from the new file.
- This means unchecked assets disappear from the Avanza-imported ledger unless manual rows still exist for them.
- The UI must use clear wording, because this differs from an append import where exclude simply omits new rows.

---

## Settled Decisions

> Settled 2026-06-22 after review (`docs/reviews/Review.avanza-full-history-replace-import.md`). The review findings are woven into the tasks below.

1. **Refresh batch identity: in-place.** Refresh the existing Avanza batch **in place** by keeping the same `import_batches.id`, updating its `raw_file_hash` and `imported_at`, and replacing its transaction set. This is the only choice consistent with preserving transaction ids (Decision 2). Loss of prior-hash audit is accepted as out of scope; a separate refresh log can be added later if ever needed.
2. **Preserving transaction ids: multiset match.** Because `derive_position` folds in `(trade_date, id)` order, a naive delete-all/insert-all refresh reorders same-day manual rows relative to re-imported rows and can flip over-sell validation or same-day cost. Match old and new rows as a **multiset of canonical fields** (after instrument resolution): preserve ids for unchanged rows (lowest old ids first, deterministically), delete unmatched old rows, insert genuinely-new rows. Treat any field correction as delete+insert (no id-preserving in-place field updates). Identical re-import becomes a metadata-only no-op. The canonical matcher must be transaction-type aware rather than assuming only buy/sell/split, so future writable dividend rows naturally participate. Residual caveat: a *changed* historical row on the same day as a manual row can still move; promote an explicit ordering column to a fast-follow only if that edge proves real.
3. **Previous batch selection: newest Avanza by id + warning.** Use the newest live `AVANZA` batch by `id` as the default replacement candidate (note: distinct from `find_by_hash`, which returns the *oldest* hash match). Surface the chosen id in the preview. When more than one live Avanza batch exists, add a non-blocking warning; the default path never creates a second Avanza batch -- only the confirmed append escape hatch does. Multi-batch consolidation/cleanup is deferred; do not silently delete extras.
4. **Manual rows depending on old imports: reuse `derive_position`.** The atomic final validation is the safety net -- no new dependency-tracking machinery. The one must-fix is the affected-instrument set: re-derive `union(instruments of the old batch's rows, instruments of the new mapped rows)` so removals/exclusions are validated too (F1). On any ledger error the entire refresh is rejected, the old batch is preserved, and the API should return a clear conflict such as `refresh_would_invalidate_ledger` with instrument context when available.
5. **DecisionLog timing.** Add the DecisionLog entry during implementation (Phase 4), not now. The entry must record Avanza full-history refresh semantics, the preserve-reference-data boundary, and the Decision 2 id-preservation choice with its residual same-day caveat.

---

## File Structure

**Backend likely modified:**
- `backend/src/api/import.rs` -- preview/commit DTOs and Avanza refresh routing.
- `backend/src/api/mod.rs` -- only if route names change; prefer keeping existing routes.
- `backend/src/db/import_batches.rs` -- find latest batch by source, count batches by source for preview warnings, validate/update batch metadata in transaction.
- `backend/src/db/transactions.rs` -- list imported rows for a batch, delete unmatched rows with static SQL, insert preserved rows with explicit ids if using delete-all/reinsert, and insert replacement rows with an existing batch id.
- `backend/src/import/core/writer.rs` -- extract reusable write/resolve logic so append and refresh share validation and instrument resolution.
- `backend/tests/import_api.rs` -- integration coverage for no-duplicate refresh and preserved reference data.
- `backend/Cargo.toml` -- patch version bump if behavior ships.

**Frontend likely modified:**
- `frontend/src/api/types.ts` -- preview/result fields for refresh candidate and replacement mode.
- `frontend/src/api/queries.ts` -- commit request params/body for refresh versus append.
- `frontend/src/components/ImportView.tsx` -- default Avanza refresh UI, advanced append confirmation, exclude wording.
- `frontend/package.json` -- patch version bump if behavior ships.

**Docs likely modified:**
- `docs/DecisionLog.md` -- accepted semantics after implementation.
- This plan moved to `docs/plans/archive/` after implementation.

---

## Phase 1 -- Backend Refresh Primitive

Build the backend capability first, behind existing API behavior where possible. The goal is an atomic function that can replace one Avanza import batch's transaction set without touching reference data.

### Task 1.1: Import batch repository helpers

**Files:**
- Modify: `backend/src/db/import_batches.rs`

- [ ] Add `find_latest_by_source(pool, source)` returning the newest batch for a source, ordered by `id DESC`.
- [ ] Add `count_by_source(pool, source)` or equivalent if preview needs to warn when multiple live Avanza batches exist.
- [ ] Add `find_in_tx(conn, batch_id)` or an equivalent transaction-scoped lookup.
- [ ] Add `update_metadata_in_tx(conn, batch_id, imported_at, raw_file_hash)` for in-place refresh.
- [ ] Keep the source check centralized so refresh cannot accidentally target a Sharesight batch.

**Verification:**
- Add focused repository or API integration assertions for latest-source lookup if there is already a convenient DB test module.
- Run from `backend/`: `cargo test import_batches` or the nearest focused test target.
- Stage changed backend files.

### Task 1.2: Transaction repository helpers for batch refresh

**Files:**
- Modify: `backend/src/db/transactions.rs`

- [ ] Add a query that returns every transaction for an import batch ordered by `(trade_date, id)` or `id`, including all fields needed to compare canonical imported rows.
- [ ] Add helpers to delete selected transaction ids inside a caller-managed transaction. **(F4)** SQLx 0.9 rejects `format!`-built SQL, so a dynamic `DELETE ... WHERE id IN (?, ...)` is not allowed. Prefer looping a single static `DELETE FROM transactions WHERE id = ?` per unmatched old id. If implementation instead deletes the whole batch via the existing `delete_batch_in_tx`, re-insert kept rows with explicit ids.
- [ ] If using the delete-all-then-reinsert strategy, add a helper to insert a kept row with an **explicit** `id` so unchanged rows retain their same-day ordering identity.
- [ ] Do not preserve ids by updating corrected rows in place. Under Decision 2, a field correction is represented as unmatched old row deleted plus unmatched new row inserted.
- [ ] Reuse `insert_in_tx` for genuinely new rows; it already accepts an `import_batch_id`.

**Verification:**
- Add repository-level tests only if the helpers are non-trivial and can be exercised without duplicating API coverage.
- Run from `backend/`: `cargo test transactions` or the focused import API tests once Phase 1.3 exists.
- Stage changed backend files.

### Task 1.3: Extract append writer internals for reuse

**Files:**
- Modify: `backend/src/import/core/writer.rs`
- Possibly modify: `backend/src/api/import.rs`

- [ ] Split the current `write_batch` flow into reusable pieces:
  - create/update batch metadata;
  - resolve instruments for mapped rows;
  - validate proposed rows into signed ledger rows;
  - insert/update/delete transaction rows;
  - re-derive affected ledgers before commit.
- [ ] Preserve the existing two-pass instrument resolution: resolve Buy/Sell instruments first, then resolve Splits against already-known instruments. Split-only refreshes must not regress.
- [ ] Preserve current append behavior for Sharesight and first Avanza imports.
- [ ] Keep transaction ownership clear: public append and refresh functions should each own one SQL transaction; lower helpers should accept `&mut SqliteConnection`.

**Verification:**
- Run from `backend/`: `cargo test import_api`.
- Confirm existing import tests still pass before adding refresh behavior.
- Stage changed backend files.

### Task 1.4: Implement `refresh_batch` for Avanza

**Files:**
- Modify: `backend/src/import/core/writer.rs`
- Modify: `backend/src/api/import.rs`
- Modify: `backend/src/db/import_batches.rs`
- Modify: `backend/src/db/transactions.rs`

- [ ] Add a writer entry point such as `refresh_batch(state, source, expected_batch_id, hash, mapped)`.
- [ ] Verify `expected_batch_id` exists, has source `AVANZA`, and is still the latest live Avanza batch. If a newer Avanza batch appeared after preview, fail with a clear conflict instead of refreshing a batch the user did not preview.
- [ ] Resolve instruments for all mapped rows using the same ISIN/symbol rules as append.
- [ ] Build canonical row keys for old imported rows and new mapped rows after instrument resolution. Include at least: `instrument_id`, transaction type/kind, trade date, signed quantity, price, currency, FX, brokerage, brokerage currency, source value, source currency, and note.
- [ ] Keep the canonical-key code transaction-type aware and forward-compatible with future writable dividend rows; do not bake in a buy/sell/split-only assumption.
- [ ] Match old/new rows as a multiset so duplicate identical source rows can preserve stable ids deterministically.
- [ ] Preserve unchanged matched rows in place. Treat changed historical rows as delete+insert rather than an id-preserving in-place update.
- [ ] Delete unmatched old rows.
- [ ] Insert unmatched new rows with the existing batch id.
- [ ] Update the existing import batch's `raw_file_hash` and `imported_at`.
- [ ] Re-derive every affected instrument ledger in the same transaction. **(F1)** The affected set must be `union(instruments of the old batch's rows, instruments of the new mapped rows)` so excluded/removed assets are re-derived too, not just instruments present in the new file.
- [ ] Roll back everything on any parse, identity, validation, or ledger error, and map ledger invalidation to a clear API error.

**Verification:**
- Add integration tests in `backend/tests/import_api.rs`:
  - First Avanza import writes normally.
  - Second Avanza full-history import with an extra latest row refreshes instead of doubling historical rows.
  - Same exact Avanza file imported again through refresh does not double rows. **(F7)** Assert idempotency explicitly: capture `COUNT(*)` and `MAX(id)` of `transactions` before/after and assert both are unchanged (metadata-only no-op).
  - Same-day stable order: an imported buy plus a same-day manual buy (higher id) plus a later imported sell; refresh with the same history preserves the manual row's same-day position and yields an identical `derive_position` result.
  - Existing instruments are reused by ISIN and are not deleted.
  - Seed a price row and provider symbol for an imported instrument; refresh; assert those rows still exist and point to the same instrument id.
  - Seed a later manual sell that remains valid with the refreshed full-history file; refresh succeeds.
  - Omit/exclude the old buy required by a later manual sell; refresh is rejected and the old imported rows remain unchanged.
  - Stale candidate conflict: preview returns batch N; a newer Avanza batch appears before commit; replace commit returns `replace_candidate_changed` or equivalent.
  - Split resolution parity: split rows continue to resolve only against instruments from Buy/Sell rows or existing ledger state; refresh does not create instruments from split-only input.
- Run from `backend/`: `cargo test import_api`.
- Stage changed backend files and tests.

---

## Phase 2 -- API Contract And Preview Metadata

Expose refresh intent explicitly so the frontend is not guessing and the backend is protected against stale preview/commit races.

### Task 2.1: Extend preview response

**Files:**
- Modify: `backend/src/api/import.rs`
- Modify: `frontend/src/api/types.ts`

- [ ] Add nullable fields to `ImportPreview`, for example:
  - `replace_candidate_batch_id: number | null`;
  - `replace_candidate_source: "AVANZA" | null` if useful;
  - optional `replace_candidate_warning` or equivalent if multiple live Avanza batches exist;
  - optional `commit_mode_default: "append" | "replace"` if the UI should not infer source-specific behavior.
- [ ] For Avanza preview, set `replace_candidate_batch_id` to the latest existing Avanza batch id, if any.
- [ ] If more than one live Avanza batch exists, include a non-blocking warning such as `Multiple Avanza imports found; refreshing batch N, others are left untouched`.
- [ ] For Sharesight preview, leave it null for now.
- [ ] Keep `duplicate_of_batch_id` as raw-file-hash information; it answers a different question.

**Verification:**
- Add/adjust backend tests:
  - Avanza preview before first import has no replace candidate.
  - Avanza preview after first import returns the prior Avanza batch id.
  - Avanza preview with multiple live Avanza batches returns the newest id and a non-blocking warning.
  - Sharesight preview remains unchanged/null.
- Run from `backend/`: `cargo test import_api`.
- Run from `frontend/`: `npm run check` once types are updated.
- Stage changed files.

### Task 2.2: Extend commit request for explicit mode

**Files:**
- Modify: `backend/src/api/import.rs`
- Modify: `frontend/src/api/queries.ts`
- Modify: `frontend/src/api/types.ts` if needed

- [ ] Add commit params such as `mode=replace|append` and `replace_batch_id=<id>`, or a clearer pair such as `replace_batch_id` plus `append=true`. Prefer an explicit mode; avoid overloading `allow_duplicate` for refresh.
- [ ] For `source=avanza` and `mode=replace`, require `replace_batch_id`.
- [ ] For append mode, retain the existing duplicate-file guard unless `allow_duplicate=true` is explicitly supplied.
- [ ] If the replace candidate disappeared after preview, return a clear conflict such as `replace_batch_not_found` or `replace_candidate_changed`.
- [ ] If a different newer Avanza batch appeared after preview, return a conflict rather than silently replacing a batch the user did not preview.

**Verification:**
- Add backend tests for missing/stale/wrong-source `replace_batch_id`, including the case where preview returned batch N but a newer Avanza batch appeared before commit.
- Run from `backend/`: `cargo test import_api`.
- Run from `frontend/`: `npm run check`.
- Stage changed files.

---

## Phase 3 -- Frontend Import UX

Make the safe Avanza path the default while keeping append as an explicit advanced choice.

### Task 3.1: Default refresh messaging

**Files:**
- Modify: `frontend/src/components/ImportView.tsx`

- [ ] When source is Avanza and `replace_candidate_batch_id` is present, show a status chip like `Will refresh Avanza batch N`.
- [ ] Surface any multi-batch preview warning near the refresh status without blocking the default refresh path.
- [ ] Change the primary button label from `Commit import` to `Refresh Avanza import` in this state.
- [ ] Make the normal primary action call commit with `mode=replace` and `replace_batch_id`.
- [ ] If the raw file is also a duplicate of the same batch, do not force the old duplicate confirmation for the default refresh path.
- [ ] Keep the existing duplicate warning for append mode.

**Verification:**
- Add reducer/view-model tests if ImportView's state machine is already exported by the frontend-test plan. If not, keep this to TypeScript coverage and manual UI testing for this phase.
- Run from `frontend/`: `npm run check`.
- Stage changed files.

### Task 3.2: Explicit append escape hatch

**Files:**
- Modify: `frontend/src/components/ImportView.tsx`
- Modify: `frontend/src/api/queries.ts`

- [ ] Add a secondary/outline action for Avanza refresh previews: `Append as new batch...`.
- [ ] Require a second confirmation state before append, using wording that it can duplicate historical transactions and holdings.
- [ ] On confirmed append, call commit with append mode and existing `allow_duplicate` behavior only when needed.
- [ ] Keep this action visually secondary to the safe refresh default.

**Verification:**
- Run from `frontend/`: `npm run check`.
- Human UI check recommended: preview an Avanza file after a prior import and verify the default path is refresh while append requires an extra confirmation.
- Stage changed files.

### Task 3.3: Exclude wording in refresh mode

**Files:**
- Modify: `frontend/src/components/ImportView.tsx`

- [ ] When refresh mode is active, add concise text near the asset table or action area explaining that unchecked assets will be removed from the refreshed Avanza-imported ledger snapshot.
- [ ] Keep skipped rows locked as today.
- [ ] Ensure errors on selected assets still block refresh; errors only on unchecked assets should not block.

**Verification:**
- Run from `frontend/`: `npm run check`.
- Human UI check recommended: uncheck an asset in refresh preview and confirm the warning text is visible and understandable.
- Stage changed files.

---

## Phase 4 -- Documentation, Versions, And Decision Log

Capture the accepted semantics once the implementation behavior is real.

### Task 4.1: DecisionLog entry

**Files:**
- Modify: `docs/DecisionLog.md`

- [ ] Add a new entry at the end, dated with the implementation date, similar to:

```md
## YYYY-MM-DD: Avanza Full-History Refresh Import
Decision: Avanza import treats a newer full-history export as a refresh of the previous Avanza import by default. Refresh replaces that Avanza batch's imported ledger rows atomically after final ledger validation, while preserving instruments, prices, FX rates, provider symbols, market-data runs, and manual transactions. Appending an Avanza file as a separate batch remains an explicit advanced action.
Context: Avanza exports are full-history snapshots; appending a newer export duplicates historical transactions because duplicate detection by raw file hash only catches identical files.
Consequences: The import UI defaults to refresh for Avanza after the first import. Excluding an asset during refresh removes that asset's old Avanza-imported rows from the refreshed snapshot. Refresh must validate the final ledger and leave the old snapshot unchanged on failure. Unchanged imported rows preserve transaction ids by multiset canonical matching; changed historical rows are delete+insert and may still move relative to same-day manual rows until an explicit ordering column exists.
```

- [ ] Mention transaction-id preservation and the residual same-day ordering caveat from Decision 2.

**Verification:**
- Read the new entry against existing import and rollback entries to ensure it does not contradict them; it should refine Avanza behavior specifically.
- Stage changed docs.

### Task 4.2: Version bumps

**Files:**
- Modify: `backend/Cargo.toml`
- Modify: `frontend/package.json`

- [ ] Bump backend patch version because the import API behavior changes.
- [ ] Bump frontend patch version because the import UI behavior changes.
- [ ] Do not edit lockfiles unless the package manager changes them as part of the normal workflow.

**Verification:**
- Run backend and frontend checks in Phase 5.
- Stage changed manifest files.

---

## Phase 5 -- Full Verification

Run the repository gates and do one realistic human workflow check.

### Automated gates

- [ ] From `backend/`: `cargo clippy --all-targets -- -D warnings`.
- [ ] From `backend/`: `cargo fmt`.
- [ ] From `frontend/`: `npm run check`.
- [ ] From `frontend/`: `npm run fmt`.
- [ ] Stage any formatting changes.

### Manual / human testing recommended

Use a disposable local database or a backup of the real database.

- [ ] Import an Avanza full-history file for the first time.
- [ ] Refresh prices/FX for at least one imported instrument.
- [ ] Import a newer Avanza full-history file containing all old rows plus at least one new transaction.
- [ ] Confirm transaction count reflects the new snapshot, not old rows doubled.
- [ ] Confirm existing instruments remain and price history still appears for them.
- [ ] Confirm holdings/cost basis are plausible after refresh.
- [ ] Try the explicit append path once in the disposable database and confirm the UI warns before creating a second batch.
- [ ] If dividend support is implemented by then, confirm old skipped dividend rows become writable after refresh without duplicating old buy/sell rows.

---

## Acceptance Criteria

1. A second Avanza full-history import defaults to refreshing the previous Avanza import, not appending duplicates.
2. Refresh is atomic: failure leaves the previous Avanza-imported ledger rows unchanged.
3. Instruments, prices, FX rates, provider symbol mappings, market-data runs, and manual transactions are preserved.
4. Unchanged imported rows preserve transaction ids; re-importing the identical Avanza file leaves transaction `COUNT(*)` and `MAX(id)` unchanged.
5. Final ledger validation catches invalid refreshed snapshots and reports a clear API/UI error.
6. Append remains available only as an explicit, confirmed advanced action.
7. Backend and frontend checks pass, formatting has been run, and all changed files are staged for review.

---

## Risks And Follow-Ups

- **Same-day ordering:** Unchanged imported rows preserve transaction ids, but changed historical rows are delete+insert and may move relative to same-day manual rows. Add an explicit ordering column in a follow-up migration if that edge proves important.
- **Multiple historical Avanza batches:** The default latest-batch choice is simple, but users who previously appended duplicates may need cleanup tooling. Do not silently delete multiple old batches in this plan.
- **Dividend support:** Once dividend rows are writable, refresh should naturally backfill them from the full-history export. Keep the matcher transaction-type aware so dividends can participate later; dividend schema and valuation behavior need their own design.
- **Command undo:** If command-based writes land before this work, refresh should become an import command with stored pre/post snapshots rather than a standalone API mutation.
