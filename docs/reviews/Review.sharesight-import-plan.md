# Sharesight Import Plan Review

Reviewed:
- `docs/plans/Plan.sharesight-import.md`
- `docs/plans/Design.sharesight-import-design.md`
- Current backend/frontend contracts where the plan depends on them.

## Findings

### High: Phase 1 cannot compile after wiring the module tree

`Plan.sharesight-import.md:170-175` tells the implementer to create `sharesight/mod.rs` with `pub mod mapper; pub mod parser; pub mod plan;`, but only `parser.rs` exists at that point. The parser test command at `Plan.sharesight-import.md:457-460` will fail before running parser tests because Rust requires `mapper.rs` and `plan.rs` to exist.

Fix: either create empty `mapper.rs` and `plan.rs` stubs before the module declaration, or declare only `parser` until the later tasks add the other files.

### High: Production planner code uses a dev-only dependency

Task 1.2 explicitly keeps `rust_decimal_macros` in `[dev-dependencies]` at `Plan.sharesight-import.md:140`, but the production `plan.rs` sketch imports `rust_decimal_macros::dec` at `Plan.sharesight-import.md:742-751`. That will not compile for the library target.

Fix: move `rust_decimal_macros` to normal dependencies, or avoid the macro in production constants by constructing `Decimal` with a const-compatible API.

### High: Split quantities are mapped through `abs()`

The mapper description says Buy/Sell use positive magnitude and Split uses signed delta (`Plan.sharesight-import.md:467`), matching the existing domain contract. The implementation sketch uses one `integral_magnitude` helper for every kind and calls `row.quantity.abs()` at `Plan.sharesight-import.md:681-695`; the Split branch then stores that absolute value at `Plan.sharesight-import.md:660-663`.

That would turn a negative split delta into a positive split and corrupt reverse-split imports. Fix by preserving the sign for Split and add a negative-split unit test.

### High: Commit and rollback transaction snippets will not compile as written

The plan says `*_in_tx` functions take `&mut sqlx::SqliteConnection` (`Plan.sharesight-import.md:1593`), but later calls pass `&mut tx` at `Plan.sharesight-import.md:1835-1837`, `1852-1854`, `1880-1882`, `1901-1903`, and `2147-2153`. Those should pass `&mut *tx` or the helper signatures should accept a generic executor compatible with `Transaction`.

The same commit sketch constructs `NewImportTransaction` without the required `import_batch_id` field (`Plan.sharesight-import.md:1639-1653`, `1882-1895`), so it also fails struct initialization.

### Medium: Rollback leaves the batch row behind

The design and acceptance language treat rollback as undoing an import batch, but the planned rollback only deletes transactions (`Plan.sharesight-import.md:2087-2097`, `2137-2157`). The `import_batches` row and `raw_file_hash` remain, so preview/commit will still report the rolled-back file as a duplicate. That makes a normal re-import after undo require `allow_duplicate=true`.

Fix: either delete the `import_batches` row after deleting its transactions, or add an explicit rolled-back state and make duplicate detection ignore rolled-back batches.

### Medium: Duplicate import override is not an explicit confirmation

The design requires an explicit "Import anyway" confirmation when a duplicate is detected (`Design.sharesight-import-design.md:287-289`). The UI sketch changes the primary button text to "Import anyway" and immediately calls `onCommit(isDuplicate)` (`Plan.sharesight-import.md:2556-2588`). That is a single-click bypass of the duplicate guard, not a confirmation.

Fix: add a reducer state such as `duplicateConfirming`, a modal, or a second required click after the user chooses to override.

### Medium: Commit error status is inconsistent

The plan's assumptions say commit hard errors return `422` (`Plan.sharesight-import.md:31`), but parse failures are mapped to `400` (`Plan.sharesight-import.md:1790-1792`) and duplicate imports are tested as `409` (`Plan.sharesight-import.md:1961-1963`). The design only requires the shared `ApiError` shape, so the plan should settle the status contract before implementation.

Fix: either update the assumption and frontend expectations to document `400`/`409`, or make all import hard errors return `422` as stated.

## Test Gaps

- The commit rollback test at `Plan.sharesight-import.md:1979-1994` fails before any transaction starts because pre-planning catches the oversell. It proves "no writes before commit", not rollback of an in-transaction failure.
- Add API-level tests for re-import after rollback once the batch-row semantics are clarified.
- Add a mapper test for negative Split delta to lock the signed-delta contract.

## Notes

- The no-migration assumption is correct against `backend/migrations/0001_create_ledger_core.sql`.
- The planned public `db::memory_pool` helper is reasonable for integration tests; it should delegate to the existing test helper or share the same implementation to avoid drift.
