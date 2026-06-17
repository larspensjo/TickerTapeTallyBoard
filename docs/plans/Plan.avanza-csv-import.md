# Avanza CSV Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Import an Avanza "AllTradesReport" CSV into the existing ledger by reusing the pure domain validator and the established preview → commit → rollback flow, behind a source-neutral shared import core.

**Architecture:** Option A — shared core, per-source parser + mapper. Each source's parser+mapper turns bytes into source-neutral *row outcomes* (`Mapped`, `Skip`, `Error`) plus a small header; the shared planner and writer consume those outcomes. Instrument identity becomes ISIN-aware. A whole-asset deselect/exclude path lets the user drop problematic assets at commit time.

**Tech Stack:** Rust (axum 0.8, sqlx 0.9 + SQLite, rust_decimal, chrono, csv), React 19 + TypeScript + TanStack Query (frontend).

---

## Source of truth & conventions

- Design spec: `docs/superpowers/specs/Design.avanza-csv-import.md`. Read it before starting.
- Backend build/test from `backend/`: `cargo build`, `cargo test`, then `cargo clippy --all-targets -- -D warnings` and `cargo fmt`.
- Frontend from `frontend/`: `npm run check` then `npm run fmt`.
- Per repo workflow (`Agents.md`): add an `EngineeringDiary.md` entry per logical change as you implement; a `docs/DecisionLog.md` entry is added in Phase 10 (it may be added during planning). Diary entries must stand alone and must not reference this plan.
- Each phase ends green (build + tests + clippy + fmt) and is independently committable.

## Open Questions / Decisions to confirm

The design is approved and prescriptive; these are the resolutions this plan adopts where the design left a degree of freedom. **Confirmed by Lars 2026-06-16.**

1. **Shared-core module layout.** This plan creates `backend/src/import/core/` holding the source-neutral types and logic — `outcome.rs` (`RowOutcome`, `PreparedImport`, `PlanHeader`, `InstrumentKey`, `MappedRow`, `RowNote`), `plan.rs` (`build_plan`, `PlanContext`, `ImportPlan`, `PlanCounts`, asset grouping), and `writer.rs` (`write_batch`). The Sharesight-specific planner code in `backend/src/import/sharesight/plan.rs` moves into `core/` and its tests route through the new Sharesight adapter. Module names describe stable behavior, per `Agents.md`.
2. **Shared decimal normalization** lives in `backend/src/import/text.rs` (`normalize_decimal`). Both parsers call it.
3. **Rollback route.** Rollback is already source-agnostic. This plan adds a shared `POST /api/import/rollback/{batch_id}` and points the frontend at it. The existing `POST /api/import/sharesight/rollback/{batch_id}` is kept as a thin alias so nothing breaks mid-migration (removed only if you choose).
4. **Count semantics.** `PreviewCounts`/`ImportCounts` gain `dividends` and `skipped`. `buys`/`sells`/`splits`/`dividends` count *source rows by classified type* (so the design's verified totals hold: 99 + 103 + 35 + 2 = 239 = `rows`). `skipped` counts rows dropped before writing (fractional buy/sell, unsupported type, fractional/zero net split) and excludes dividends (counted separately). On the commit result, counts reflect the *effective post-exclusion* set actually written.
5. **`exclude` is implemented in the shared commit path**, so both Sharesight and Avanza commits honor it. The asset-grouping in preview is likewise produced for both sources.
6. **Version bumps:** backend `0.3.1 → 0.4.0`, frontend `0.4.2 → 0.5.0` (notable feature). Done in Phase 9.

## Plan-review resolutions (2026-06-16)

Decisions adopted after the plan review (`docs/reviews/Review.avanza-csv-import-plan.md`); the relevant phases below already reflect them:

- **Skip/Error outcomes carry their asset identity.** `RowOutcome::Skip`/`Error` carry `asset_key: Option<String>` so a hard error or skip for a *known* asset (it could identify its ISIN / `exchange:symbol` but not produce a `MappedRow`) is attached to that asset's preview group **and** is dropped by `exclude_assets`. This makes "deselect an asset whose rows carry a hard error" work for mapper-stage errors, not just ledger errors on mapped rows.
- **Source parser errors stay global and non-excludable.** A parse failure short-circuits before any `PreparedImport` exists (preview returns a single global error; commit returns `400`). Only per-row outcomes with a known `asset_key` are excludable; `asset_key: None` outcomes (e.g. an Avanza row with a blank ISIN) remain global.
- **Symbol-only `AVANZA` instrument with `isin = NULL` is upgraded in place.** When `find_by_isin` misses but an `(exchange='AVANZA', symbol='<ISIN>')` row exists with `isin = NULL`, the writer backfills its `isin` and reuses it (it is the same instrument; `symbol == ISIN`). A symbol row whose `isin` is a *different* non-null ISIN is an `instrument_identity_conflict`. This preserves the ISIN identity guarantee instead of silently leaving `isin` NULL.
- **Phases 3 and 4 are one commit boundary** (the API cannot compile between them); see the banner on Phase 3.
- **`ImportResult` carries `warnings` from Phase 4 on**, defaulting to an empty vector when there are no commit warnings.
- **The duplicate-row key includes transaction direction** (`proposed.kind`), matching the existing Sharesight behavior.

## Review resolutions (2026-06-17)

A second review (`docs/reviews/Review.avanza-csv-import-plan.md`) is folded into the phases below:

- **Fixture date ordering (was an oversell).** The Avanza fixture's Apple Buy is now dated before its Sell, so the clean fixture stays at `errors == 0` (Phase 8 Task 8.2). The accompanying note had the chronology backwards; it is corrected.
- **One shared `ParseError`.** A single `ParseError` lives in `core::outcome`; both the Sharesight and Avanza parsers use it, dropping the Phase 8 normalization adapter (Phases 3, 6, 8).
- **Commit counts are uniform.** Phase 5 replaces `commit_source`'s body, so *every* commit reports `effective_counts` (rows actually written); an empty `exclude` makes `effective == prepared`. Preview keeps source-level counts (Phases 4, 5).
- **Writer error paths log.** `write_batch` logs identity conflicts, `split_without_position`, and ledger failures via `engine_logging` with the batch source + ISIN/symbol, since the central `ApiError` logger only logs `5xx` (Phase 4).
- **Split-never-creates-instrument is a deliberate behavior change** for a standalone Sharesight split; called out in the Phase 4 diary/commit.
- **No-ops (verified, no change):** `TransactionKind::as_db_str` and its `PartialEq`/`Eq` derives already exist (`backend/src/domain/transaction.rs`); the seven `NewInstrument` sites are re-verified by `grep` at implementation time (Phase 1 Task 1.3).

## File map

Created:
- `backend/migrations/0003_add_instrument_isin.sql`
- `backend/migrations/0004_widen_import_batch_source.sql`
- `backend/src/import/text.rs` — shared decimal normalization.
- `backend/src/import/core/mod.rs`, `core/outcome.rs`, `core/plan.rs`, `core/writer.rs` — source-neutral types (including the shared `ParseError` used by both parsers), planner, writer.
- `backend/src/import/avanza/mod.rs`, `avanza/parser.rs`, `avanza/mapper.rs` — Avanza adapter.
- `backend/tests/fixtures/avanza_synthetic.csv` — fixture mirroring the real export shapes.

Modified:
- `backend/src/db/instruments.rs` — `isin` column; `find_by_isin`; ISIN-aware upsert.
- `backend/src/import/mod.rs` — declare `text`, `core`, `avanza` modules.
- `backend/src/import/sharesight/{mod.rs,parser.rs,mapper.rs}` — use shared types + `text`; add a `to_prepared` adapter; remove `plan.rs`.
- `backend/src/api/import.rs` — generalized preview/commit/rollback over `PreparedImport`; per-source handlers; `exclude`.
- `backend/src/api/mod.rs` — new Avanza + shared-rollback routes.
- `backend/tests/import_api.rs` — new Avanza API tests.
- `backend/Cargo.toml` — version bump.
- `frontend/src/api/{types.ts,queries.ts}` — extended counts, source toggle, exclude, shared rollback.
- `frontend/src/components/ImportView.tsx` — source toggle + asset checkboxes + exclude.
- `frontend/package.json` — version bump.
- `docs/DecisionLog.md`, `docs/Design.HighLevel.md`, `EngineeringDiary.md`.

---

## Phase 1 — Schema & repository: ISIN identity + AVANZA source

**Outcome:** `instruments` has a nullable, partial-unique `isin`; `import_batches.source` allows `'AVANZA'`; the instruments repo can match by ISIN and upsert with ISIN. Sharesight paths are untouched (NULL isin behaves exactly as before).

**Files:**
- Create: `backend/migrations/0003_add_instrument_isin.sql`
- Create: `backend/migrations/0004_widen_import_batch_source.sql`
- Modify: `backend/src/db/instruments.rs`
- Test: `backend/src/db/instruments.rs` (`#[cfg(test)]` module), `backend/tests/import_api.rs` (source CHECK)

### Task 1.1: Migration — add `isin`

- [ ] **Step 1: Write the migration**

Create `backend/migrations/0003_add_instrument_isin.sql`:

```sql
-- Instrument identity by ISIN. Nullable so existing Sharesight rows are untouched.
-- The partial unique index guarantees one instrument per ISIN while allowing many NULLs.
ALTER TABLE instruments ADD COLUMN isin TEXT;

CREATE UNIQUE INDEX idx_instruments_isin
    ON instruments (isin)
    WHERE isin IS NOT NULL;
```

- [ ] **Step 2: Verify it applies**

Run: `cargo test --lib db::` (any db test boots `memory_pool`, which runs migrations)
Expected: PASS (migrations apply cleanly).

### Task 1.2: Migration — widen `import_batches.source`

SQLite cannot alter a CHECK in place, so this is a table rebuild. `import_batches` is referenced by `transactions.import_batch_id`; with foreign keys enabled (they are — see `db/mod.rs` `memory_pool` and `db/pool.rs`), dropping the old table while children reference it violates FK. The pragma that disables FK enforcement is a no-op inside a transaction, so this migration must run **outside** a transaction via the sqlx `-- no-transaction` directive.

- [ ] **Step 1: Write the migration**

Create `backend/migrations/0004_widen_import_batch_source.sql`:

```sql
-- no-transaction
-- Widen import_batches.source to allow 'AVANZA'. SQLite cannot alter a CHECK in
-- place, so rebuild the table. FK enforcement must be off during the swap, which
-- requires running outside a transaction (the directive above).
PRAGMA foreign_keys=OFF;

CREATE TABLE import_batches_new (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    source        TEXT NOT NULL CHECK (source IN ('SHARESIGHT', 'CSV', 'MANUAL', 'AVANZA')),
    imported_at   TEXT NOT NULL,
    raw_file_hash TEXT
);

INSERT INTO import_batches_new (id, source, imported_at, raw_file_hash)
    SELECT id, source, imported_at, raw_file_hash FROM import_batches;

DROP TABLE import_batches;

ALTER TABLE import_batches_new RENAME TO import_batches;

PRAGMA foreign_keys=ON;
```

- [ ] **Step 2: Add a test that AVANZA is now accepted**

Add to `backend/tests/import_api.rs`:

```rust
#[tokio::test]
async fn import_batches_accepts_avanza_source() {
    let state = test_state().await;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO import_batches (source, imported_at, raw_file_hash) VALUES ('AVANZA', ?, ?) RETURNING id",
    )
    .bind("2026-06-16T00:00:00Z")
    .bind("deadbeef")
    .fetch_one(&state.pool)
    .await
    .expect("AVANZA source should be accepted");
    assert!(id >= 1);
}
```

- [ ] **Step 3: Run it**

Run: `cargo test --test import_api import_batches_accepts_avanza_source`
Expected: PASS. If it fails with "no such directive"/transaction errors, confirm sqlx 0.9 honors `-- no-transaction` (it does); the directive must be the first line.

- [ ] **Step 4: Confirm existing Sharesight API tests still pass**

Run: `cargo test --test import_api`
Expected: PASS (the rebuild preserves existing rows and the FK).

### Task 1.3: Repository — `isin` on rows + `find_by_isin` + ISIN-aware upsert

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)] mod tests` of `backend/src/db/instruments.rs` (create the module if absent), using `crate::db::memory_pool`:

```rust
#[cfg(test)]
mod tests {
    use super::{find_by_isin, upsert, NewInstrument};
    use crate::db::memory_pool;

    fn avanza(isin: &str) -> NewInstrument {
        NewInstrument {
            symbol: isin.to_string(),
            exchange: "AVANZA".to_string(),
            name: "Example".to_string(),
            kind: "STOCK".to_string(),
            currency: "SEK".to_string(),
            isin: Some(isin.to_string()),
        }
    }

    #[tokio::test]
    async fn upsert_then_find_by_isin_round_trips() {
        let pool = memory_pool().await.expect("pool");
        let (row, created) = upsert(&pool, &avanza("US1234567890")).await.expect("upsert");
        assert!(created);
        assert_eq!(row.isin.as_deref(), Some("US1234567890"));

        let found = find_by_isin(&pool, "US1234567890").await.expect("find");
        assert_eq!(found.map(|r| r.id), Some(row.id));
    }

    #[tokio::test]
    async fn multiple_null_isin_instruments_coexist() {
        let pool = memory_pool().await.expect("pool");
        for symbol in ["MSFT", "AAPL"] {
            upsert(
                &pool,
                &NewInstrument {
                    symbol: symbol.to_string(),
                    exchange: "NASDAQ".to_string(),
                    name: symbol.to_string(),
                    kind: "STOCK".to_string(),
                    currency: "USD".to_string(),
                    isin: None,
                },
            )
            .await
            .expect("null-isin upsert should succeed");
        }
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib db::instruments`
Expected: FAIL to compile (`NewInstrument` has no `isin`, no `find_by_isin`).

- [ ] **Step 3: Implement the repository changes**

In `backend/src/db/instruments.rs`:

Add `isin` to every column list and to the structs. Replace the SQL consts and structs:

```rust
const LIST_SQL: &str =
    "SELECT id, symbol, exchange, name, type, currency, isin FROM instruments ORDER BY exchange, symbol";
const FIND_SQL: &str =
    "SELECT id, symbol, exchange, name, type, currency, isin FROM instruments WHERE id = ?";
const FIND_BY_EXCHANGE_SYMBOL_SQL: &str =
    "SELECT id, symbol, exchange, name, type, currency, isin FROM instruments WHERE exchange = ? AND symbol = ?";
const FIND_BY_ISIN_SQL: &str =
    "SELECT id, symbol, exchange, name, type, currency, isin FROM instruments WHERE isin = ?";
const INSERT_SQL: &str = "INSERT INTO instruments (symbol, exchange, name, type, currency, isin) \
     VALUES (?, ?, ?, ?, ?, ?) RETURNING id, symbol, exchange, name, type, currency, isin";
```

```rust
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct InstrumentRow {
    pub id: i64,
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub currency: String,
    pub isin: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NewInstrument {
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    pub kind: String,
    pub currency: String,
    pub isin: Option<String>,
}
```

Bind `isin` in both INSERT paths (pool `upsert` and `upsert_in_tx`): after `.bind(&new.currency)` add `.bind(&new.isin)`.

Add the finders (pool + in-tx):

```rust
pub async fn find_by_isin(
    pool: &SqlitePool,
    isin: &str,
) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(FIND_BY_ISIN_SQL)
        .bind(isin)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_by_isin_in_tx(
    conn: &mut SqliteConnection,
    isin: &str,
) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(FIND_BY_ISIN_SQL)
        .bind(isin)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row)
}
```

- [ ] **Step 4: Add `isin` to every existing `NewInstrument` construction site**

Adding the field is a breaking change for the phase's own build/clippy gate, so update **all seven** current literals (verified by `grep -rn "NewInstrument {" backend/src`). Add `isin: None,` to each — none of these create ISIN-bearing instruments:

- `backend/src/api/import.rs:278` (Sharesight `write_batch`; rewritten in Phase 3, `isin: None` is correct until then)
- `backend/src/api/instruments.rs:89` (manual instrument create)
- `backend/src/api/prices.rs:97`
- `backend/src/api/provider_symbols.rs:150`
- `backend/src/db/prices.rs:322`
- `backend/src/db/provider_symbols.rs:256`
- `backend/src/market_data/refresh.rs:1058`

> If the manual instrument-create API (`api/instruments.rs`) should accept an ISIN later, that is a separate feature; here it stays `None`.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib db::instruments`
Expected: PASS.

- [ ] **Step 6: Build + clippy + fmt**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add backend/migrations backend/src/db/instruments.rs backend/src/api backend/src/db backend/src/market_data backend/tests/import_api.rs
git commit -m "feat(import): ISIN instrument identity and AVANZA batch source"
```

Add an `EngineeringDiary.md` entry for the schema + repository change.

---

## Phase 2 — Shared decimal normalization helper

**Outcome:** Decimal normalization (comma→dot, Unicode minus, NBSP/space stripping) lives in one place; the Sharesight parser delegates to it. Pure refactor — no behavior change.

**Files:**
- Create: `backend/src/import/text.rs`
- Modify: `backend/src/import/mod.rs`, `backend/src/import/sharesight/parser.rs`

### Task 2.1: Extract `normalize_decimal`

- [ ] **Step 1: Write the helper with tests**

Create `backend/src/import/text.rs`:

```rust
//! Shared text helpers for CSV imports.

/// Normalize a numeric cell into a `Decimal`-parsable string:
/// trims, maps the Unicode minus (U+2212) to ASCII `-`, comma decimal to dot,
/// and strips spaces, NBSP (U+00A0) and narrow NBSP (U+202F) thousands marks.
/// Returns an empty string for a blank cell.
pub fn normalize_decimal(value: &str) -> String {
    value
        .trim()
        .replace('\u{2212}', "-")
        .replace(',', ".")
        .replace([' ', '\u{00a0}', '\u{202f}'], "")
}

#[cfg(test)]
mod tests {
    use super::normalize_decimal;

    #[test]
    fn maps_comma_unicode_minus_and_strips_thousands() {
        assert_eq!(normalize_decimal("1\u{00a0}259,60"), "1259.60");
        assert_eq!(normalize_decimal("\u{2212}504,00"), "-504.00");
        assert_eq!(normalize_decimal("  12,50 "), "12.50");
        assert_eq!(normalize_decimal(""), "");
    }
}
```

- [ ] **Step 2: Declare the module**

In `backend/src/import/mod.rs` add near the top (after `pub mod sharesight;`):

```rust
pub mod text;
```

- [ ] **Step 3: Point the Sharesight parser at it**

In `backend/src/import/sharesight/parser.rs`, delete the private `normalize_decimal` and `normalize_text` fns and replace their use. Change the call site in `decimal_field`/`optional_decimal_field` from `normalize_decimal(raw)` to `crate::import::text::normalize_decimal(raw)`. (Leave `sanitize_report_title` as is.)

- [ ] **Step 4: Run the Sharesight parser tests**

Run: `cargo test --lib import::sharesight::parser && cargo test --lib import::text`
Expected: PASS (existing parser tests cover NBSP/comma/Unicode-minus).

- [ ] **Step 5: clippy + fmt + commit**

```bash
git add backend/src/import
git commit -m "refactor(import): share decimal normalization across parsers"
```

---

## Phase 3 — Source-neutral core (planner) + generalized writer/API

> **Phases 3 and 4 are a single commit boundary.** Generalizing `build_plan` changes the signature that `api/import.rs` calls, so the crate cannot build between them — treat Phase 3 (the planner core) and Phase 4 (the writer + API rewrite) as two readable parts of one phase and **commit only at the end of Phase 4**. Run unit tests at the Phase 3 checkpoint to confirm the core logic, then proceed straight into Phase 4. (Strict green-per-commit alternative: introduce the new `core` modules with `#[allow(dead_code)]` in Phase 3 while leaving the old `sharesight/plan.rs` + `api/import.rs` in place, then delete the old path and remove the allow in Phase 4. The merged-commit approach below is simpler and is what this plan assumes.)

**Outcome (end of Phase 3 part):** A shared `import::core` module defines `RowOutcome`/`MappedRow`/`InstrumentKey` (ISIN-aware) and a `build_plan` that consumes a `PreparedImport` (header + classified counts + outcomes). The Sharesight path is refactored to produce a `PreparedImport`; the ported planner unit tests pass through the adapter. The API does not compile until Phase 4 lands.

**Files:**
- Create: `backend/src/import/core/mod.rs`, `core/outcome.rs`, `core/plan.rs`
- Modify: `backend/src/import/mod.rs`, `backend/src/import/sharesight/{mod.rs,mapper.rs,parser.rs}`, add `backend/src/import/sharesight/adapter.rs`
- Delete: `backend/src/import/sharesight/plan.rs` (logic moves to `core/plan.rs`; its tests move to `adapter.rs`/`core/plan.rs`)

### Task 3.1: Define the shared outcome types

- [ ] **Step 1: Create `core/outcome.rs`**

```rust
use rust_decimal::Decimal;

use crate::domain::ProposedTransaction;

/// Instrument identity + display fields from one row. ISIN, when present, is the
/// match key; otherwise `(exchange, symbol)` is used (Sharesight).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstrumentKey {
    pub exchange: String,
    pub symbol: String,
    pub name: String,
    pub currency: String,
    pub isin: Option<String>,
}

impl InstrumentKey {
    /// Stable grouping key: ISIN when present, else lowercased `exchange:symbol`.
    pub fn asset_key(&self) -> String {
        match &self.isin {
            Some(isin) => isin.clone(),
            None => format!(
                "{}:{}",
                self.exchange.to_lowercase(),
                self.symbol.to_lowercase()
            ),
        }
    }
}

/// A row mapped to a proposed ledger transaction plus audit/warning context.
#[derive(Clone, Debug, PartialEq)]
pub struct MappedRow {
    pub source_row_number: usize,
    pub instrument: InstrumentKey,
    pub proposed: ProposedTransaction,
    pub source_value: Option<Decimal>,
    /// Currency of `source_value`; persisted verbatim (SEK for Sharesight,
    /// `Transaktionsvaluta` for Avanza). `None` for rows with no monetary value.
    pub source_currency: Option<String>,
    /// Free-text note persisted to `transactions.note` (Sharesight `Comments`;
    /// `None` for Avanza, which has no comment column).
    pub note: Option<String>,
    /// True when a Buy/Sell had a blank or non-positive FX rate.
    pub fx_warning: bool,
}

/// A note attached to a row: a stable code + message + optional source row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowNote {
    pub row: Option<usize>,
    pub code: &'static str,
    pub message: String,
}

/// One source row, classified into a downstream outcome. `Skip`/`Error` carry the
/// row's `asset_key` when it is known (ISIN, or `exchange:symbol`) so the note can
/// be attached to its preview group and dropped by `exclude_assets`; `None` means
/// the row cannot name an asset (e.g. a blank ISIN) and the note stays global.
#[derive(Clone, Debug, PartialEq)]
pub enum RowOutcome {
    Mapped(MappedRow),
    /// Counted and reported but not written (dividend, fractional fund, unknown type).
    Skip {
        asset_key: Option<String>,
        note: RowNote,
    },
    /// A hard error that blocks writing this asset.
    Error {
        asset_key: Option<String>,
        note: RowNote,
    },
}

/// Source-row classification counts (by transaction type, before netting/skipping).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceKindCounts {
    pub rows: usize,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
}

/// Minimal report header. Avanza synthesizes this; Sharesight reads it from metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanHeader {
    pub title: String,
    pub date_from: chrono::NaiveDate,
    pub date_to: chrono::NaiveDate,
}

/// Everything a source adapter produces for the shared planner/writer.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedImport {
    pub header: PlanHeader,
    pub counts: SourceKindCounts,
    pub outcomes: Vec<RowOutcome>,
}

/// A parse-stage failure with optional row context and a stable code. One shared
/// type so every source parser fails the same way and the API handles one error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub row: Option<usize>,
    pub code: &'static str,
    pub message: String,
}

impl ParseError {
    pub fn header(message: impl Into<String>) -> Self {
        Self { row: None, code: "header_not_found", message: message.into() }
    }

    pub fn row(row: usize, code: &'static str, message: impl Into<String>) -> Self {
        Self { row: Some(row), code, message: message.into() }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.row {
            Some(row) => write!(f, "row {row}: {} ({})", self.message, self.code),
            None => write!(f, "{} ({})", self.message, self.code),
        }
    }
}

impl std::error::Error for ParseError {}
```

> The skip code `"dividend_deferred"` is the one code the planner treats as "already counted as a dividend, do not add to `skipped`". All other skip codes increment `skipped`.

- [ ] **Step 2: Create `core/mod.rs`**

```rust
pub mod outcome;
pub mod plan;
```

(`writer` is added in Phase 4.)

- [ ] **Step 3: Declare `core` in `import/mod.rs`**

Add `pub mod core;` to `backend/src/import/mod.rs`.

### Task 3.2: Move/generalize `build_plan` into `core/plan.rs`

This is the existing `sharesight/plan.rs` logic, generalized to consume `&PreparedImport` instead of `&ParsedReport`, ISIN-aware instrument matching, asset grouping, and the new counts.

- [ ] **Step 1: Write `core/plan.rs`**

```rust
use std::collections::BTreeMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::domain::{self, LedgerTransaction};
use crate::import::core::outcome::{
    InstrumentKey, MappedRow, PlanHeader, PreparedImport, RowNote, RowOutcome, SourceKindCounts,
};

const RECONCILIATION_FLOOR_SEK: Decimal = dec!(300);
const RECONCILIATION_RATE: Decimal = dec!(0.01);

/// DB-derived context the pure planner needs.
#[derive(Clone, Debug, Default)]
pub struct PlanContext {
    pub existing_instruments: Vec<ExistingInstrument>,
    pub existing_ledgers: BTreeMap<i64, Vec<LedgerTransaction>>,
    pub max_existing_id: i64,
}

#[derive(Clone, Debug)]
pub struct ExistingInstrument {
    pub id: i64,
    pub exchange: String,
    pub symbol: String,
    pub currency: String,
    pub isin: Option<String>,
}

impl ExistingInstrument {
    /// True when this stored instrument is the identity referenced by `key`.
    fn matches(&self, key: &InstrumentKey) -> bool {
        match (&self.isin, &key.isin) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => {
                self.exchange.eq_ignore_ascii_case(&key.exchange)
                    && self.symbol.eq_ignore_ascii_case(&key.symbol)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPlan {
    pub counts: PlanCounts,
    pub new_instruments: Vec<InstrumentKey>,
    pub assets: Vec<AssetGroup>,
    pub warnings: Vec<RowNote>,
    pub errors: Vec<RowNote>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanCounts {
    pub rows: usize,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
    pub new_instruments: usize,
    pub skipped: usize,
    pub warnings: usize,
    pub errors: usize,
}

/// Per-asset preview grouping for the deselect UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetGroup {
    pub asset_key: String,
    pub name: String,
    pub currency: String,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
    pub default_selected: bool,
    /// Set when the asset cannot be written at all (e.g. every row skipped).
    pub skipped_reason: Option<String>,
    pub warnings: Vec<RowNote>,
    pub errors: Vec<RowNote>,
    pub is_new_instrument: bool,
}

pub fn build_plan(prepared: &PreparedImport, ctx: &PlanContext) -> ImportPlan {
    let mut warnings: Vec<RowNote> = Vec::new();
    let mut errors: Vec<RowNote> = Vec::new();
    let mut new_instruments: Vec<InstrumentKey> = Vec::new();
    let mut skipped = 0usize;

    // Per-asset accumulation, keyed by asset_key, preserving first-seen order.
    let mut asset_order: Vec<String> = Vec::new();
    let mut assets: BTreeMap<String, AssetGroup> = BTreeMap::new();

    let mut ledgers: BTreeMap<String, Vec<LedgerTransaction>> = BTreeMap::new();
    let mut seeded: std::collections::BTreeSet<String> = Default::default();
    let mut mapped_rows: Vec<MappedRow> = Vec::new();

    for (i, outcome) in prepared.outcomes.iter().enumerate() {
        match outcome {
            RowOutcome::Skip { asset_key, note } => {
                warnings.push(note.clone());
                if note.code != "dividend_deferred" {
                    skipped += 1;
                }
                // Attach to the named asset so the deselect UI can flag/exclude it.
                if let Some(key) = asset_key {
                    let group = asset_group_mut(&mut assets, &mut asset_order, key, None, None);
                    if note.code == "dividend_deferred" {
                        group.dividends += 1;
                    }
                    group.warnings.push(note.clone());
                }
            }
            RowOutcome::Error { asset_key, note } => {
                errors.push(note.clone());
                if let Some(key) = asset_key {
                    let group = asset_group_mut(&mut assets, &mut asset_order, key, None, None);
                    group.errors.push(note.clone());
                }
            }
            RowOutcome::Mapped(mapped) => {
                mapped_rows.push(mapped.clone());
                process_mapped(
                    i,
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
        }
    }

    duplicate_row_warnings(&mapped_rows, &mut warnings);
    ledger_errors(&mut ledgers, ctx, prepared, &mut errors);

    // Attach planner-generated notes (missing_fx, reconciliation, duplicate, ledger
    // errors) to assets via their mapped source rows. Per-row Skip/Error notes were
    // already attached above by their carried asset_key, so they are not re-attached
    // here (their source rows are not in `mapped_rows`).
    attach_asset_notes(&mut assets, &warnings, &errors, &mapped_rows);

    // An asset that contributes no writable row (e.g. an all-fractional fund or a
    // dividend-only asset) is shown but not selectable.
    for group in assets.values_mut() {
        if group.buys + group.sells + group.splits == 0
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

    let assets = asset_order
        .into_iter()
        .filter_map(|key| assets.remove(&key))
        .collect();

    ImportPlan {
        counts,
        new_instruments,
        assets,
        warnings,
        errors,
    }
}
```

Then port the row-processing helpers from the old `sharesight/plan.rs`, adjusting to the ISIN key. Implement:

```rust
/// Get or create an asset group, preserving first-seen order. `name`/`currency`
/// upgrade the placeholder values (used when the group was first created from a
/// Skip/Error outcome that only knew the asset_key) once a mapped row supplies them.
fn asset_group_mut<'a>(
    assets: &'a mut BTreeMap<String, AssetGroup>,
    asset_order: &mut Vec<String>,
    key: &str,
    name: Option<&str>,
    currency: Option<&str>,
) -> &'a mut AssetGroup {
    let group = assets.entry(key.to_string()).or_insert_with(|| {
        asset_order.push(key.to_string());
        AssetGroup {
            asset_key: key.to_string(),
            name: key.to_string(),
            currency: String::new(),
            buys: 0,
            sells: 0,
            splits: 0,
            dividends: 0,
            default_selected: true,
            skipped_reason: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            is_new_instrument: false,
        }
    });
    if let Some(name) = name.filter(|n| !n.is_empty()) {
        group.name = name.to_string();
    }
    if let Some(currency) = currency.filter(|c| !c.is_empty()) {
        group.currency = currency.to_string();
    }
    group
}

#[allow(clippy::too_many_arguments)]
fn process_mapped(
    i: usize,
    mapped: &MappedRow,
    ctx: &PlanContext,
    ledgers: &mut BTreeMap<String, Vec<LedgerTransaction>>,
    seeded: &mut std::collections::BTreeSet<String>,
    new_instruments: &mut Vec<InstrumentKey>,
    warnings: &mut Vec<RowNote>,
    errors: &mut Vec<RowNote>,
    asset_order: &mut Vec<String>,
    assets: &mut BTreeMap<String, AssetGroup>,
) {
    let key = mapped.instrument.asset_key();

    let group = asset_group_mut(
        assets,
        asset_order,
        &key,
        Some(&mapped.instrument.name),
        Some(&mapped.instrument.currency),
    );
    match mapped.proposed.kind {
        domain::TransactionKind::Buy => group.buys += 1,
        domain::TransactionKind::Sell => group.sells += 1,
        domain::TransactionKind::Split => group.splits += 1,
        domain::TransactionKind::Dividend => group.dividends += 1,
    }

    if let Some(existing) = ctx
        .existing_instruments
        .iter()
        .find(|e| e.matches(&mapped.instrument))
    {
        if !existing
            .currency
            .eq_ignore_ascii_case(&mapped.instrument.currency)
            && !mapped.instrument.currency.is_empty()
        {
            errors.push(RowNote {
                row: Some(mapped.source_row_number),
                code: "currency_mismatch",
                message: format!(
                    "row currency {} differs from stored {}",
                    mapped.instrument.currency, existing.currency
                ),
            });
        }
        if seeded.insert(key.clone()) {
            ledgers.insert(
                key.clone(),
                ctx.existing_ledgers
                    .get(&existing.id)
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    } else {
        seeded.insert(key.clone());
        ledgers.entry(key.clone()).or_default();
        // A split must never create an instrument (no currency of its own).
        if mapped.proposed.kind != domain::TransactionKind::Split
            && !new_instruments.contains(&mapped.instrument)
        {
            new_instruments.push(mapped.instrument.clone());
            group.is_new_instrument = true;
        }
    }

    match domain::validate(&mapped.proposed) {
        Ok(signed) => {
            let provisional_id = ctx.max_existing_id + 1 + i as i64;
            ledgers.entry(key.clone()).or_default().push(LedgerTransaction {
                id: provisional_id,
                trade_date: mapped.proposed.trade_date,
                kind: mapped.proposed.kind,
                quantity: signed,
                price: mapped.proposed.price,
                fx_rate_to_base: mapped.proposed.fx_rate_to_base,
                brokerage_base: mapped.proposed.brokerage_base.unwrap_or(Decimal::ZERO),
            });
            if mapped.fx_warning {
                warnings.push(RowNote {
                    row: Some(mapped.source_row_number),
                    code: "missing_fx",
                    message: "Exchange Rate blank or non-positive; SEK base unavailable"
                        .to_string(),
                });
            }
            reconciliation_warning(signed, mapped, warnings);
        }
        Err(validation) => errors.push(RowNote {
            row: Some(mapped.source_row_number),
            code: validation.code(),
            message: validation.message().to_string(),
        }),
    }
}

fn reconciliation_warning(signed: i64, mapped: &MappedRow, warnings: &mut Vec<RowNote>) {
    if !matches!(
        mapped.proposed.kind,
        domain::TransactionKind::Buy | domain::TransactionKind::Sell
    ) {
        return;
    }
    if let (Some(fx), Some(price), Some(source_value)) = (
        mapped.proposed.fx_rate_to_base,
        mapped.proposed.price,
        mapped.source_value,
    ) {
        let signed_native_gross = Decimal::from(signed) * price;
        let brokerage = mapped.proposed.brokerage_base.unwrap_or(Decimal::ZERO);
        let derived = signed_native_gross * fx + brokerage;
        let residual = (source_value - derived).abs();
        let threshold = reconciliation_threshold(source_value);
        if residual > threshold {
            warnings.push(RowNote {
                row: Some(mapped.source_row_number),
                code: "reconciliation_residual",
                message: format!(
                    "derived SEK off by {} (> {})",
                    residual.round_dp(2),
                    threshold.round_dp(2)
                ),
            });
        }
    }
}

fn reconciliation_threshold(source_value: Decimal) -> Decimal {
    let proportional = RECONCILIATION_RATE * source_value.abs();
    proportional.max(RECONCILIATION_FLOOR_SEK)
}

fn duplicate_row_warnings(mapped: &[MappedRow], warnings: &mut Vec<RowNote>) {
    // Direction (`kind`) is part of the key, matching the existing Sharesight rule
    // that a duplicate is identical in instrument, date, direction, quantity, price,
    // and value. `TransactionKind` is not `Ord`, so key on its stable DB string.
    type DuplicateKey = (
        String,
        &'static str,
        String,
        Decimal,
        Option<Decimal>,
        Option<Decimal>,
    );
    let mut groups: BTreeMap<DuplicateKey, Vec<usize>> = BTreeMap::new();
    for row in mapped {
        groups
            .entry((
                row.instrument.asset_key(),
                row.proposed.kind.as_db_str(),
                row.proposed.trade_date.to_string(),
                Decimal::from(row.proposed.quantity),
                row.proposed.price,
                row.source_value,
            ))
            .or_default()
            .push(row.source_row_number);
    }
    for rows in groups.values().filter(|rows| rows.len() > 1) {
        warnings.push(RowNote {
            row: rows.first().copied(),
            code: "duplicate_row",
            message: format!("identical row appears {} times", rows.len()),
        });
    }
}

fn ledger_errors(
    ledgers: &mut BTreeMap<String, Vec<LedgerTransaction>>,
    ctx: &PlanContext,
    prepared: &PreparedImport,
    errors: &mut Vec<RowNote>,
) {
    for ledger in ledgers.values_mut() {
        ledger.sort_by_key(|tx| (tx.trade_date, tx.id));
        if let Err(ledger_error) = domain::derive_position(ledger) {
            let id = ledger_error.transaction_id();
            let row = if id > ctx.max_existing_id {
                provisional_source_row(prepared, (id - ctx.max_existing_id - 1) as usize)
            } else {
                None
            };
            errors.push(RowNote {
                row,
                code: ledger_error.code(),
                message: ledger_message(ledger_error),
            });
        }
    }
}

/// The Nth outcome's source row number, if it is a `Mapped` outcome.
fn provisional_source_row(prepared: &PreparedImport, index: usize) -> Option<usize> {
    match prepared.outcomes.get(index) {
        Some(RowOutcome::Mapped(m)) => Some(m.source_row_number),
        _ => None,
    }
}

fn attach_asset_notes(
    assets: &mut BTreeMap<String, AssetGroup>,
    warnings: &[RowNote],
    errors: &[RowNote],
    mapped: &[MappedRow],
) {
    let row_to_asset: BTreeMap<usize, String> = mapped
        .iter()
        .map(|m| (m.source_row_number, m.instrument.asset_key()))
        .collect();
    for note in warnings {
        if let Some(key) = note.row.and_then(|r| row_to_asset.get(&r)) {
            if let Some(group) = assets.get_mut(key) {
                group.warnings.push(note.clone());
            }
        }
    }
    for note in errors {
        if let Some(key) = note.row.and_then(|r| row_to_asset.get(&r)) {
            if let Some(group) = assets.get_mut(key) {
                group.errors.push(note.clone());
            }
        }
    }
}

fn ledger_message(error: crate::domain::LedgerError) -> String {
    use crate::domain::LedgerError::*;
    match error {
        SellExceedsPosition {
            available,
            requested,
            ..
        } => format!("Sell of {requested} exceeds available position of {available}."),
        SplitWithoutPosition { .. } => "A split requires an existing position.".to_string(),
        SplitDrivesNonPositive {
            resulting_quantity, ..
        } => format!("Split would drive the position to {resulting_quantity}."),
        BuyMissingPrice { .. } => "A buy requires a native price.".to_string(),
    }
}
```

> Why `provisional_id = max_existing_id + 1 + i`: it keys the provisional ledger id off the *outcome index* `i`, exactly as the old planner used the row index, so imported rows still sort after existing same-day rows. Skip/Error outcomes consume an index slot but push nothing, which is harmless.

- [ ] **Step 2: Build (expect downstream breakage)**

Run: `cargo build`
Expected: FAIL — `sharesight/plan.rs` and `api/import.rs` still reference the old types. Fixed in Task 3.3 and Phase 4.

### Task 3.3: Sharesight adapter producing `PreparedImport`

> **Lift `ParseError` to the shared type first.** In `sharesight/parser.rs`, delete the local `ParseError` struct and its `header`/`row`/`Display`/`Error` impls and add `use crate::import::core::outcome::ParseError;`. `parse_report` now returns `Result<ParsedReport, core::outcome::ParseError>`; the `ParseError::header(..)`/`ParseError::row(..)` call sites are unchanged (the shared type keeps the same constructors). The Avanza parser (Phase 6) reuses this same type, so Phase 8 needs no `ParseError` normalization adapter.

- [ ] **Step 1: Update `sharesight/mapper.rs` to build shared types**

Change `map_row` to return `crate::import::core::outcome::MappedRow` with `InstrumentKey { isin: None, .. }` and `source_currency`/`source_value` set to match today's writer behavior (SEK for buy/sell, `None` otherwise). Replace the `InstrumentKey`/`MappedRow` definitions in this file with imports from `core::outcome`, and update the buy/sell arm:

```rust
use crate::import::core::outcome::{InstrumentKey, MappedRow};
use crate::import::sharesight::parser::{ParsedKind, ParsedRow};
use crate::domain::{ProposedTransaction, TransactionKind};
// ... keep MapError, integral_*, invert_fx, sek_brokerage unchanged ...

pub fn map_row(row: &ParsedRow) -> Result<MappedRow, MapError> {
    let instrument = InstrumentKey {
        exchange: row.market.trim().to_string(),
        symbol: row.code.trim().to_string(),
        name: row.name.trim().to_string(),
        currency: row.instrument_currency.trim().to_string(),
        isin: None,
    };
    let kind = match row.kind {
        ParsedKind::Buy => TransactionKind::Buy,
        ParsedKind::Sell => TransactionKind::Sell,
        ParsedKind::Split => TransactionKind::Split,
    };
    match row.kind {
        ParsedKind::Buy | ParsedKind::Sell => {
            let magnitude = integral_magnitude(row)?;
            let fx_rate_to_base = invert_fx(row.exchange_rate);
            let fx_warning = fx_rate_to_base.is_none();
            let brokerage_base = sek_brokerage(row)?;
            Ok(MappedRow {
                source_row_number: row.source_row_number,
                instrument,
                proposed: ProposedTransaction {
                    kind,
                    trade_date: row.trade_date,
                    quantity: magnitude,
                    price: Some(row.price),
                    currency: Some(row.instrument_currency.trim().to_string()),
                    fx_rate_to_base,
                    brokerage_base,
                },
                source_value: Some(row.value),
                source_currency: Some("SEK".to_string()),
                note: non_empty_comment(row),
                fx_warning,
            })
        }
        ParsedKind::Split => Ok(MappedRow {
            source_row_number: row.source_row_number,
            instrument,
            proposed: ProposedTransaction {
                kind,
                trade_date: row.trade_date,
                quantity: integral_signed(row)?,
                price: None,
                currency: None,
                fx_rate_to_base: None,
                brokerage_base: None,
            },
            source_value: Some(row.value),
            source_currency: None,
            note: non_empty_comment(row),
            fx_warning: false,
        }),
    }
}

/// The trimmed `Comments` cell as an optional note.
fn non_empty_comment(row: &ParsedRow) -> Option<String> {
    let trimmed = row.comments.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
```

Update the in-file mapper tests: remove the `mapped.kind` assertions (the field moved to `proposed.kind`) and read `mapped.proposed.kind`; `source_value` is now `Some(...)`.

- [ ] **Step 2: Create `sharesight/adapter.rs`**

```rust
use crate::import::core::outcome::{PlanHeader, PreparedImport, RowNote, RowOutcome, SourceKindCounts};
use crate::import::sharesight::mapper::map_row;
use crate::import::sharesight::parser::{ParsedKind, ParsedReport};

/// Turn a parsed Sharesight report into source-neutral outcomes for the planner.
pub fn to_prepared(report: &ParsedReport) -> PreparedImport {
    let mut counts = SourceKindCounts {
        rows: report.rows.len(),
        ..Default::default()
    };
    let mut outcomes = Vec::with_capacity(report.rows.len());
    for parsed in &report.rows {
        match parsed.kind {
            ParsedKind::Buy => counts.buys += 1,
            ParsedKind::Sell => counts.sells += 1,
            ParsedKind::Split => counts.splits += 1,
        }
        match map_row(parsed) {
            Ok(mapped) => outcomes.push(RowOutcome::Mapped(mapped)),
            Err(err) => outcomes.push(RowOutcome::Error {
                // A mapper error still knows its asset (`market:code`), so it can be
                // attached to that asset and excluded. Mirror `InstrumentKey::asset_key`
                // for the no-ISIN case.
                asset_key: Some(format!(
                    "{}:{}",
                    parsed.market.trim().to_lowercase(),
                    parsed.code.trim().to_lowercase()
                )),
                note: RowNote {
                    row: Some(err.row),
                    code: err.code,
                    message: err.message,
                },
            }),
        }
    }
    PreparedImport {
        header: PlanHeader {
            title: report.metadata.title.clone(),
            date_from: report.metadata.date_from,
            date_to: report.metadata.date_to,
        },
        counts,
        outcomes,
    }
}
```

- [ ] **Step 3: Update `sharesight/mod.rs`**

```rust
pub mod adapter;
pub mod mapper;
pub mod parser;
```

(Remove the `plan` module.)

- [ ] **Step 4: Move the Sharesight planner tests**

Delete `backend/src/import/sharesight/plan.rs`. Recreate its `tests` module in `core/plan.rs` (or in `sharesight/adapter.rs`), routing through the adapter. Replace the old helper:

```rust
fn plan_for(csv: &str, ctx: PlanContext) -> ImportPlan {
    let report = crate::import::sharesight::parser::parse_report(csv.as_bytes()).expect("parses");
    let prepared = crate::import::sharesight::adapter::to_prepared(&report);
    build_plan(&prepared, &ctx)
}
```

Keep each existing assertion. Update `ExistingInstrument { ... }` literals to add `isin: None,`. The `currency_mismatch` test still asserts `plan.errors` contains `currency_mismatch` and `counts.new_instruments == 0`.

- [ ] **Step 5: Run the ported planner + parser/mapper unit tests (checkpoint, no commit)**

Run: `cargo test --lib import::core:: && cargo test --lib import::sharesight::`
Expected: PASS for the core planner tests and the Sharesight parser/mapper tests. A full `cargo build` will still fail because `api/import.rs` calls the old `build_plan` signature — that is expected and is closed in Phase 4. This is the mid-phase checkpoint described in the banner; **do not commit here.** Proceed directly to Phase 4.

---

## Phase 4 — Generalized writer + preview/commit/API (closes the Phase 3 commit)

> Completes the single commit boundary opened in Phase 3. The crate is red until this phase lands; the commit at the end covers both phases.

**Outcome:** `api/import.rs` drives the shared `PreparedImport` path for Sharesight: a generalized `write_batch` that upserts instruments ISIN-aware (backfilling a NULL `isin` on a symbol-only AVANZA row), persists `source_currency` from the row, guards `instrument_identity_conflict`, and never creates an instrument from a split. Preview returns `assets` and the extended counts. Build is green again; all Sharesight API tests pass unchanged.

**Files:**
- Create: `backend/src/import/core/writer.rs`
- Modify: `backend/src/import/core/mod.rs`, `backend/src/api/import.rs`, `backend/src/api/mod.rs`

### Task 4.1: Generalized writer

- [ ] **Step 1: Create `core/writer.rs`**

```rust
use std::collections::{BTreeMap, BTreeSet};

use axum::http::StatusCode;

use crate::api::error::ApiError;
use crate::db::instruments::{self, NewInstrument};
use crate::db::transactions::{self, NewImportTransaction};
use crate::db::import_batches;
use crate::domain;
use crate::import::core::outcome::{InstrumentKey, MappedRow, PreparedImport, RowOutcome};
use crate::import::now_iso8601;
use crate::state::AppState;

fn identity_conflict(isin: &str, symbol: &str) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "instrument_identity_conflict",
        format!("ISIN {isin} and symbol {symbol} resolve to different instruments"),
    )
}

/// Resolve or create the instrument id for a buy/sell `key`, ISIN-aware.
///
/// Cases when the key carries an ISIN (Avanza always does):
/// - ISIN match wins. If a different instrument also occupies `(AVANZA, ISIN)`,
///   that is an `instrument_identity_conflict` (a mixed-source DB).
/// - No ISIN match but an `(AVANZA, ISIN)` row exists with `isin = NULL`: backfill
///   its `isin` and reuse it (same instrument; `symbol == ISIN`). This preserves the
///   ISIN identity guarantee instead of silently leaving `isin` NULL.
/// - No ISIN match but the symbol row carries a *different* non-null ISIN: conflict.
/// - Otherwise create a new instrument carrying the ISIN.
async fn resolve_buy_sell_instrument(
    conn: &mut sqlx::sqlite::SqliteConnection,
    key: &InstrumentKey,
) -> Result<i64, ApiError> {
    if let Some(isin) = &key.isin {
        let by_isin = instruments::find_by_isin_in_tx(conn, isin).await?;
        let by_symbol =
            instruments::find_by_exchange_symbol_in_tx(conn, &key.exchange, &key.symbol).await?;
        match (by_isin, by_symbol) {
            (Some(a), Some(b)) if a.id != b.id => {
                return Err(identity_conflict(isin, &key.symbol));
            }
            (Some(a), _) => return Ok(a.id),
            (None, Some(b)) => match &b.isin {
                None => {
                    // Upgrade the pre-existing symbol-only AVANZA row in place.
                    instruments::set_isin_in_tx(conn, b.id, isin).await?;
                    return Ok(b.id);
                }
                Some(_other) => return Err(identity_conflict(isin, &key.symbol)),
            },
            (None, None) => {} // fall through to create
        }
    }
    let (row, _) = instruments::upsert_in_tx(
        conn,
        &NewInstrument {
            symbol: key.symbol.clone(),
            exchange: key.exchange.clone(),
            name: key.name.clone(),
            kind: "STOCK".to_string(),
            currency: key.currency.clone(),
            isin: key.isin.clone(),
        },
    )
    .await?;
    Ok(row.id)
}

/// Look up the instrument id for a split `key` without creating one.
async fn resolve_split_instrument(
    conn: &mut sqlx::sqlite::SqliteConnection,
    key: &InstrumentKey,
    created: &BTreeMap<String, i64>,
) -> Result<i64, ApiError> {
    if let Some(id) = created.get(&key.asset_key()).copied() {
        return Ok(id);
    }
    if let Some(isin) = &key.isin {
        if let Some(existing) = instruments::find_by_isin_in_tx(conn, isin).await? {
            return Ok(existing.id);
        }
    } else if let Some(existing) =
        instruments::find_by_exchange_symbol_in_tx(conn, &key.exchange, &key.symbol).await?
    {
        return Ok(existing.id);
    }
    Err(ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "split_without_position",
        "A split requires an existing position.".to_string(),
    ))
}

/// Write one atomic batch from already-mapped rows. `mapped` is the effective
/// (post-exclusion) set; the caller has already validated it has no hard errors.
pub async fn write_batch(
    state: &AppState,
    source: &str,
    hash: &str,
    mapped: &[MappedRow],
) -> Result<i64, ApiError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;

    let batch_id = import_batches::insert_in_tx(&mut tx, source, &now_iso8601(), hash).await?;

    // First pass: resolve/create instruments for buy/sell rows.
    let mut created: BTreeMap<String, i64> = BTreeMap::new();
    let mut affected: BTreeSet<i64> = BTreeSet::new();
    for row in mapped {
        if row.proposed.kind == domain::TransactionKind::Split {
            continue;
        }
        let key = row.instrument.asset_key();
        if !created.contains_key(&key) {
            let id = resolve_buy_sell_instrument(&mut tx, &row.instrument).await?;
            created.insert(key, id);
        }
    }

    // Second pass: insert every transaction.
    for row in mapped {
        let instrument_id = if row.proposed.kind == domain::TransactionKind::Split {
            resolve_split_instrument(&mut tx, &row.instrument, &created).await?
        } else {
            created[&row.instrument.asset_key()]
        };
        affected.insert(instrument_id);

        let signed = domain::validate(&row.proposed).map_err(ApiError::from)?;
        let brokerage_currency = row.proposed.brokerage_base.map(|_| "SEK".to_string());
        transactions::insert_in_tx(
            &mut tx,
            &NewImportTransaction {
                instrument_id,
                kind: row.proposed.kind,
                trade_date: row.proposed.trade_date,
                quantity: signed,
                price: row.proposed.price,
                currency: row.proposed.currency.clone(),
                fx_rate_to_base: row.proposed.fx_rate_to_base,
                brokerage: row.proposed.brokerage_base,
                brokerage_currency,
                source_value: row.source_value,
                source_currency: row.source_currency.clone(),
                note: row.note.clone(),
                import_batch_id: batch_id,
            },
        )
        .await?;
    }

    for instrument_id in affected {
        let ledger = transactions::ledger_for_instrument_in_tx(&mut tx, instrument_id).await?;
        domain::derive_position(&ledger).map_err(ApiError::from)?;
    }

    tx.commit()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(batch_id)
}
```

> `note` is carried on `MappedRow` (set from Sharesight `Comments`; `None` for Avanza) so the existing Sharesight behavior of persisting comments to `transactions.note` is preserved.

> **Log the writer's hard-error paths** (per `Agents.md`): `ApiError::into_response` only logs `5xx`, so the writer's `422`s would otherwise be silent. Before returning, emit `crate::engine_error!` with the batch `source` and the offending instrument's ISIN/symbol in: `resolve_buy_sell_instrument` (the `instrument_identity_conflict` branches), `resolve_split_instrument` (`split_without_position`), and the final `derive_position` failure. Example: `crate::engine_error!("import write_batch [{source}]: identity conflict isin={isin} symbol={}", key.symbol);`. Thread `source` into the resolver helpers (or log at the `write_batch` call sites) so the message names the failing operation and instrument.

- [ ] **Step 2: Add the in-tx symbol finder and ISIN backfill used above**

In `backend/src/db/instruments.rs` add (and the `SET_ISIN_SQL` const near the other SQL consts):

```rust
const SET_ISIN_SQL: &str = "UPDATE instruments SET isin = ? WHERE id = ?";

pub async fn find_by_exchange_symbol_in_tx(
    conn: &mut SqliteConnection,
    exchange: &str,
    symbol: &str,
) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(FIND_BY_EXCHANGE_SYMBOL_SQL)
        .bind(exchange)
        .bind(symbol)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row)
}

/// Backfill the `isin` of an existing instrument inside a caller-managed
/// transaction. Used when a symbol-only AVANZA row is matched by an ISIN import.
pub async fn set_isin_in_tx(
    conn: &mut SqliteConnection,
    id: i64,
    isin: &str,
) -> Result<(), RepoError> {
    sqlx::query(SET_ISIN_SQL)
        .bind(isin)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}
```

- [ ] **Step 3: Export the writer**

In `core/mod.rs` add `pub mod writer;`.

### Task 4.2: Rewrite `api/import.rs` over `PreparedImport`

- [ ] **Step 1: Replace the DTOs + handlers**

Rework `backend/src/api/import.rs` so preview/commit accept a `PreparedImport` from a source-specific helper. Key shape:

```rust
use crate::import::core::outcome::{
    InstrumentKey, MappedRow, ParseError, PreparedImport, RowNote, RowOutcome,
};
use crate::import::core::plan::{build_plan, AssetGroup, ExistingInstrument, ImportPlan, PlanContext};
use crate::import::core::writer::write_batch;
use crate::import::sharesight::adapter::to_prepared as sharesight_prepared;
use crate::import::sharesight::parser::parse_report;

#[derive(Debug, Serialize, Default)]
pub struct PreviewCounts {
    pub rows: usize,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
    pub new_instruments: usize,
    pub skipped: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Serialize)]
pub struct AssetGroupDto {
    pub asset_key: String,
    pub name: String,
    pub currency: String,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
    pub default_selected: bool,
    pub skipped_reason: Option<String>,
    pub warnings: Vec<RowNoteDto>,
    pub errors: Vec<RowNoteDto>,
    pub is_new_instrument: bool,
}

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub metadata: Option<PreviewMetadata>,
    pub counts: PreviewCounts,
    pub assets: Vec<AssetGroupDto>,
    pub new_instruments: Vec<NewInstrumentDto>,
    pub warnings: Vec<RowNoteDto>,
    pub errors: Vec<RowNoteDto>,
    pub duplicate_of_batch_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CommitParams {
    #[serde(default)]
    pub allow_duplicate: bool,
    /// Comma-separated asset_keys to exclude before validating/writing.
    #[serde(default)]
    pub exclude: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub batch_id: i64,
    pub counts: PreviewCounts,
    /// Commit-time warnings (e.g. unknown `exclude` keys). Empty when there are none.
    pub warnings: Vec<RowNoteDto>,
}
```

> `ImportResult` carries `warnings` from this phase on, so every construction site (here and the exclusion path in Phase 5) sets it — empty where there are no commit warnings. The frontend `ImportResult` type (Phase 9) and the committed-result card include the field.

Add `NewInstrumentDto` an `isin: Option<String>` field; extend `new_instrument_dto`. Extend `counts_dto` to copy `dividends` and `skipped`.

The Sharesight handlers become thin wrappers over shared helpers:

```rust
pub async fn sharesight_preview(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<Json<ImportPreview>, ApiError> {
    preview_source(&state, &bytes, parse_sharesight).await
}

pub async fn sharesight_commit(
    State(state): State<AppState>,
    Query(params): Query<CommitParams>,
    bytes: Bytes,
) -> Result<Json<ImportResult>, ApiError> {
    commit_source(&state, &bytes, "SHARESIGHT", &params, parse_sharesight).await
}

fn parse_sharesight(bytes: &[u8]) -> Result<PreparedImport, ParseError> {
    parse_report(bytes).map(|report| sharesight_prepared(&report))
}
```

Implement the shared `preview_source` / `commit_source` generic over a `parse: impl Fn(&[u8]) -> Result<PreparedImport, ParseError>`:

```rust
async fn preview_source(
    state: &AppState,
    bytes: &[u8],
    parse: impl Fn(&[u8]) -> Result<PreparedImport, ParseError>,
) -> Result<Json<ImportPreview>, ApiError> {
    let hash = raw_file_hash(bytes);
    let duplicate_of_batch_id =
        import_batches::find_by_hash(&state.pool, &hash).await?.map(|b| b.id);

    let prepared = match parse(bytes) {
        Ok(prepared) => prepared,
        Err(error) => return Ok(Json(parse_error_preview(error, duplicate_of_batch_id))),
    };
    let ctx = load_plan_context(state).await?;
    let plan = build_plan(&prepared, &ctx);

    Ok(Json(ImportPreview {
        metadata: Some(PreviewMetadata {
            title: prepared.header.title.clone(),
            date_from: prepared.header.date_from.to_string(),
            date_to: prepared.header.date_to.to_string(),
        }),
        counts: counts_dto(&plan),
        assets: plan.assets.iter().map(asset_group_dto).collect(),
        new_instruments: plan.new_instruments.iter().map(new_instrument_dto).collect(),
        warnings: plan.warnings.iter().map(row_note_dto).collect(),
        errors: plan.errors.iter().map(row_note_dto).collect(),
        duplicate_of_batch_id,
    }))
}
```

`load_plan_context` now copies `isin` into `ExistingInstrument`:

```rust
existing_instruments.push(ExistingInstrument {
    id: row.id,
    exchange: row.exchange.clone(),
    symbol: row.symbol.clone(),
    currency: row.currency.clone(),
    isin: row.isin.clone(),
});
```

`commit_source` (without exclusion yet — exclusion lands in Phase 5):

```rust
async fn commit_source(
    state: &AppState,
    bytes: &[u8],
    source: &str,
    params: &CommitParams,
    parse: impl Fn(&[u8]) -> Result<PreparedImport, ParseError>,
) -> Result<Json<ImportResult>, ApiError> {
    let hash = raw_file_hash(bytes);
    let prepared = parse(bytes).map_err(|e| ApiError::bad_request(e.code, e.message))?;
    let ctx = load_plan_context(state).await?;
    let plan = build_plan(&prepared, &ctx);
    reject_on_errors(&plan)?; // 422-with-details helper extracted from the old commit body

    if let Some(existing) = import_batches::find_by_hash(&state.pool, &hash).await? {
        if !params.allow_duplicate {
            return Err(duplicate_conflict(existing.id));
        }
    }

    let mapped: Vec<MappedRow> = prepared
        .outcomes
        .iter()
        .filter_map(|o| match o {
            RowOutcome::Mapped(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    let batch_id = write_batch(state, source, &hash, &mapped).await?;
    Ok(Json(ImportResult {
        batch_id,
        counts: counts_dto(&plan),
        warnings: Vec::new(),
    }))
}
```

> The `counts: counts_dto(&plan)` returned here is **interim** — Phase 5 replaces this body so every commit (excluded or not) reports `effective_counts` (the rows actually written). It is shown this way only so Phase 4 compiles and its tests pass before exclusion lands.

> Extract two helpers from the old commit body and use them here: `reject_on_errors(&plan) -> Result<(), ApiError>` (returns the existing `422` `UNPROCESSABLE_ENTITY` with the `errors` details payload when `!plan.errors.is_empty()`) and `duplicate_conflict(batch_id) -> ApiError` (the existing `409` `duplicate_import` with the `duplicate_of_batch_id` detail). Phase 5 reuses both.

Generalize `rollback` to take any source (it already is). Update `parse_error_preview` to set `assets: Vec::new()`.

- [ ] **Step 2: Update routes**

In `backend/src/api/mod.rs`, rename the Sharesight handlers and add the shared rollback:

```rust
.route("/import/sharesight/preview", post(import::sharesight_preview))
.route("/import/sharesight/commit", post(import::sharesight_commit))
.route("/import/rollback/{batch_id}", post(import::rollback))
.route("/import/sharesight/rollback/{batch_id}", post(import::rollback))
```

- [ ] **Step 3: Build, then run the full Sharesight suite**

Run: `cargo build && cargo test --test import_api && cargo test --lib import::`
Expected: PASS. The existing API tests assert `counts.rows/buys/sells/splits/new_instruments/errors`; the new fields default to 0/are additive, so they still pass.

- [ ] **Step 4: Add a test asserting `assets` + new counts appear**

Add to `backend/tests/import_api.rs`:

```rust
#[tokio::test]
async fn preview_groups_assets_and_reports_extended_counts() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/sharesight/preview", SYNTHETIC).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["counts"]["dividends"].as_u64().is_some());
    assert!(body["counts"]["skipped"].as_u64().is_some());
    let assets = body["assets"].as_array().expect("assets array");
    assert_eq!(assets.len(), 2); // MSFT, ASML
    assert!(assets.iter().all(|a| a["default_selected"] == true));
}
```

Run: `cargo test --test import_api preview_groups_assets`
Expected: PASS.

- [ ] **Step 5: clippy + fmt + commit (closes the Phase 3+4 boundary)**

This is the first green commit since Phase 2; it covers both Phase 3 (planner core) and Phase 4 (writer + API).

```bash
git add backend/src/import backend/src/api backend/src/db/instruments.rs backend/tests/import_api.rs
git commit -m "refactor(import): source-neutral planner/writer with asset grouping"
```

Add a single `EngineeringDiary.md` entry covering the shared-core refactor (planner + writer + API). Call out one deliberate behavior change: a standalone split (no same-batch buy/sell and no existing position) no longer creates an instrument — the shared writer routes splits through `resolve_split_instrument`, which errors with `split_without_position` instead of the old Sharesight `upsert`-for-all-rows path that could create an orphan instrument before `derive_position` rejected it. Net effect is unchanged (the import still fails) but it fails earlier and leaves no orphan.

---

## Phase 5 — Whole-asset exclusion at commit

**Outcome:** Commit accepts `?exclude=key1,key2`, drops those assets *before* validating, gates only the remaining set on hard errors, reports counts for the written set, and warns on unknown exclude keys. This lets a user commit the rest of a file while dropping an asset whose rows have a hard error.

**Files:**
- Modify: `backend/src/import/core/plan.rs` (a filtered re-plan helper), `backend/src/api/import.rs`
- Test: `backend/tests/import_api.rs`

### Task 5.1: Filter outcomes by asset, then re-plan

- [ ] **Step 1: Add a helper to drop excluded assets from a `PreparedImport`**

In `core/plan.rs`:

```rust
use crate::import::core::outcome::RowOutcome;

/// Drop every outcome belonging to an excluded asset — `Mapped` rows by their
/// instrument's `asset_key`, and `Skip`/`Error` rows by their carried `asset_key`.
/// This is what lets a user deselect an asset whose rows carry a *mapper-stage*
/// hard error (not just a ledger error on a mapped row). Outcomes with no
/// `asset_key` (e.g. a row with a blank ISIN) cannot be excluded and are kept.
pub fn exclude_assets(prepared: &PreparedImport, exclude: &std::collections::BTreeSet<String>) -> PreparedImport {
    let outcomes = prepared
        .outcomes
        .iter()
        .filter(|o| match o {
            RowOutcome::Mapped(m) => !exclude.contains(&m.instrument.asset_key()),
            RowOutcome::Skip { asset_key, .. } | RowOutcome::Error { asset_key, .. } => {
                asset_key.as_ref().is_none_or(|k| !exclude.contains(k))
            }
        })
        .cloned()
        .collect();
    PreparedImport {
        header: prepared.header.clone(),
        counts: prepared.counts, // source totals unchanged; effective counts come from the re-plan
        outcomes,
    }
}

/// Asset keys the import can name — from mapped rows and from Skip/Error outcomes
/// that identified an asset — so `exclude` input can be validated and a key that
/// only ever appears on an errored row still counts as known (not "unknown").
pub fn known_asset_keys(prepared: &PreparedImport) -> std::collections::BTreeSet<String> {
    prepared
        .outcomes
        .iter()
        .filter_map(|o| match o {
            RowOutcome::Mapped(m) => Some(m.instrument.asset_key()),
            RowOutcome::Skip { asset_key, .. } | RowOutcome::Error { asset_key, .. } => {
                asset_key.clone()
            }
        })
        .collect()
}
```

> `is_none_or` is stable since Rust 1.82; if the toolchain is older, write `asset_key.as_ref().map_or(true, |k| !exclude.contains(k))`.

- [ ] **Step 2: Apply exclusion in `commit_source`**

Replace the body of `commit_source` after parsing:

```rust
let exclude: std::collections::BTreeSet<String> = params
    .exclude
    .as_deref()
    .unwrap_or("")
    .split(',')
    .map(str::trim)
    .filter(|s| !s.is_empty())
    .map(str::to_string)
    .collect();

let known = known_asset_keys(&prepared);
let unknown: Vec<String> = exclude.difference(&known).cloned().collect();

let effective = exclude_assets(&prepared, &exclude);
let ctx = load_plan_context(state).await?;
let plan = build_plan(&effective, &ctx);
reject_on_errors(&plan)?; // only the remaining assets are gated

if let Some(existing) = import_batches::find_by_hash(&state.pool, &hash).await? {
    if !params.allow_duplicate {
        return Err(duplicate_conflict(existing.id));
    }
}

let mapped: Vec<MappedRow> = effective
    .outcomes
    .iter()
    .filter_map(|o| match o { RowOutcome::Mapped(m) => Some(m.clone()), _ => None })
    .collect();
let batch_id = write_batch(state, source, &hash, &mapped).await?;

let warnings: Vec<RowNoteDto> = unknown
    .iter()
    .map(|key| RowNoteDto {
        row: None,
        code: "unknown_exclude_key",
        message: format!("exclude key {key:?} matched no asset"),
    })
    .collect();

// Effective counts: recompute written buys/sells/splits from the written set.
let counts = effective_counts(&plan, &mapped);
Ok(Json(ImportResult { batch_id, counts, warnings }))
```

`ImportResult` already carries `warnings` (defined in Phase 4), so no struct change is needed here — the non-exclusion path sets it empty, this path fills it with any `unknown_exclude_key` notes.

This body replaces the interim `counts: counts_dto(&plan)` from Phase 4, so **every** commit now reports `effective_counts` — there is no separate non-exclusion code path. An empty `exclude` makes `effective == prepared`, so the counts simply reflect the full written set. (Preview still reports source-level counts from `counts_dto`, matching the design's split: preview = "what's in the file", commit = "what was inserted".)

Add `effective_counts(plan: &ImportPlan, mapped: &[MappedRow]) -> PreviewCounts`: start from `counts_dto(plan)` for the re-planned set (its `skipped`/`errors`/`new_instruments` already reflect the post-exclusion outcomes), then overwrite `rows` and the per-type tallies with what was actually written, counted from `mapped` by `proposed.kind`. (`dividends` stays the re-planned count; dividends are never written, so the result reports them only for context.)

```rust
fn effective_counts(plan: &ImportPlan, mapped: &[MappedRow]) -> PreviewCounts {
    let mut counts = counts_dto(plan);
    counts.rows = mapped.len();
    counts.buys = kind_count(mapped, domain::TransactionKind::Buy);
    counts.sells = kind_count(mapped, domain::TransactionKind::Sell);
    counts.splits = kind_count(mapped, domain::TransactionKind::Split);
    counts
}

fn kind_count(mapped: &[MappedRow], kind: domain::TransactionKind) -> usize {
    mapped.iter().filter(|m| m.proposed.kind == kind).count()
}
```

- [ ] **Step 3: Tests**

Add to `backend/tests/import_api.rs` (the synthetic file's ASML row has a hard-error variant; use a CSV where one asset oversells while another is clean):

```rust
const TWO_ASSETS_ONE_BAD: &str = concat!(
    "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
    "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
    "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"1250,00\",All Trades,\n",
    "XETR,ASML,ASML Holding,Sell,12/06/2026,−4,\"600,00\",EUR,\"0,00\",\"0,00\",SEK,\"0,100000\",\"−2400,00\",All Trades,\n",
);

#[tokio::test]
async fn commit_excludes_a_bad_asset_and_writes_the_rest() {
    let state = test_state().await;
    // Without exclusion the ASML oversell blocks the whole file.
    let (blocked, _) =
        send_bytes(&state, "/api/import/sharesight/commit", TWO_ASSETS_ONE_BAD.as_bytes()).await;
    assert_eq!(blocked, StatusCode::UNPROCESSABLE_ENTITY);

    // Excluding the ASML asset_key lets MSFT commit.
    let (ok, body) = send_bytes(
        &state,
        "/api/import/sharesight/commit?exclude=xetr:asml",
        TWO_ASSETS_ONE_BAD.as_bytes(),
    )
    .await;
    assert_eq!(ok, StatusCode::OK);
    assert_eq!(body["counts"]["rows"], 1);

    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert_eq!(holdings.as_array().expect("array").len(), 1);
}

#[tokio::test]
async fn unknown_exclude_key_warns_but_commits() {
    let state = test_state().await;
    let (ok, body) =
        send_bytes(&state, "/api/import/sharesight/commit?exclude=nope:none", SYNTHETIC).await;
    assert_eq!(ok, StatusCode::OK);
    let warnings = body["warnings"].as_array().expect("warnings array");
    assert!(warnings.iter().any(|w| w["code"] == "unknown_exclude_key"));
}

// Regression for the plan review: a *mapper-stage* hard error (here a non-SEK
// brokerage, which fails in the Sharesight mapper before a MappedRow exists) must
// still be excludable by asset_key so the other, clean asset commits.
#[tokio::test]
async fn commit_excludes_an_asset_with_a_mapper_stage_error() {
    let state = test_state().await;
    let csv = concat!(
        "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
        "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
        "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"1250,00\",All Trades,\n",
        // Non-SEK brokerage -> mapper Error (non_sek_brokerage), not a ledger error.
        "XETR,ASML,ASML Holding,Buy,12/06/2026,3,\"600,00\",EUR,\"0,00\",\"5,00\",EUR,\"0,100000\",\"1805,00\",All Trades,\n",
    );

    // The mapper error blocks the whole file when not excluded.
    let (blocked, blocked_body) =
        send_bytes(&state, "/api/import/sharesight/commit", csv.as_bytes()).await;
    assert_eq!(blocked, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(blocked_body["error"]["code"], "non_sek_brokerage");

    // Excluding the ASML asset drops its Error outcome; MSFT still commits.
    let (ok, body) = send_bytes(
        &state,
        "/api/import/sharesight/commit?exclude=xetr:asml",
        csv.as_bytes(),
    )
    .await;
    assert_eq!(ok, StatusCode::OK);
    assert_eq!(body["counts"]["rows"], 1);

    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert_eq!(holdings.as_array().expect("array").len(), 1);
}
```

Run: `cargo test --test import_api commit_excludes && cargo test --test import_api unknown_exclude_key`
Expected: PASS.

- [ ] **Step 4: clippy + fmt + commit**

```bash
git add backend/src/import backend/src/api backend/tests/import_api.rs
git commit -m "feat(import): whole-asset exclusion at commit"
```

---

## Phase 6 — Avanza parser

**Outcome:** `backend/src/import/avanza/parser.rs` reads the semicolon-delimited, comma-decimal Avanza export into `ParsedAvanzaReport { rows, date_from, date_to }`, classifying the four kinds and emitting an `unsupported_type` marker (not an error) for anything else.

**Files:**
- Create: `backend/src/import/avanza/mod.rs`, `backend/src/import/avanza/parser.rs`
- Modify: `backend/src/import/mod.rs`

### Task 6.1: Parser

- [ ] **Step 1: Declare the module**

`backend/src/import/avanza/mod.rs`:

```rust
pub mod mapper;
pub mod parser;
```

Add `pub mod avanza;` to `backend/src/import/mod.rs`.

- [ ] **Step 2: Write the parser**

`backend/src/import/avanza/parser.rs`:

```rust
use chrono::NaiveDate;
use csv::ReaderBuilder;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::import::core::outcome::ParseError;
use crate::import::text::normalize_decimal;

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedAvanzaReport {
    pub rows: Vec<ParsedAvanzaRow>,
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvanzaKind {
    Buy,
    Sell,
    Dividend,
    Split,
    /// Any other "Typ av transaktion"; carried so the mapper can skip-warn.
    Unsupported,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedAvanzaRow {
    pub source_row_number: usize,
    pub trade_date: NaiveDate,
    pub raw_kind: String,
    pub kind: AvanzaKind,
    pub name: String,
    pub quantity: Decimal,
    pub price: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub transaction_currency: String,
    pub brokerage: Option<Decimal>,
    pub fx_rate: Option<Decimal>,
    pub instrument_currency: String,
    pub isin: String,
}

// `ParseError` is the shared `crate::import::core::outcome::ParseError` (imported
// above); its `header(..)`/`row(..)` constructors are used below.

const COLUMNS: &[&str] = &[
    "Datum", "Konto", "Typ av transaktion", "Värdepapper/beskrivning", "Antal",
    "Kurs", "Belopp", "Transaktionsvaluta", "Courtage", "Valutakurs",
    "Instrumentvaluta", "ISIN", "Resultat",
];

struct Header {
    datum: usize,
    typ: usize,
    namn: usize,
    antal: usize,
    kurs: usize,
    belopp: usize,
    transaktionsvaluta: usize,
    courtage: usize,
    valutakurs: usize,
    instrumentvaluta: usize,
    isin: usize,
}

pub fn parse_report(bytes: &[u8]) -> Result<ParsedAvanzaReport, ParseError> {
    let mut reader = ReaderBuilder::new()
        .delimiter(b';')
        .has_headers(true)
        .flexible(true)
        .from_reader(bytes);

    let header_record = reader
        .headers()
        .map_err(|e| ParseError::header(format!("CSV header read error: {e}")))?
        .clone();
    let header = parse_header(&header_record)?;

    let mut rows = Vec::new();
    let mut min_date: Option<NaiveDate> = None;
    let mut max_date: Option<NaiveDate> = None;

    for (zero_based, record) in reader.records().enumerate() {
        // +2: one for the header row, one for 1-based numbering.
        let row_number = zero_based + 2;
        let record = record
            .map_err(|e| ParseError::row(row_number, "csv_read", format!("CSV read error: {e}")))?;
        if record.iter().all(|f| f.trim().is_empty()) {
            continue;
        }
        let row = parse_row(row_number, &record, &header)?;
        min_date = Some(min_date.map_or(row.trade_date, |d: NaiveDate| d.min(row.trade_date)));
        max_date = Some(max_date.map_or(row.trade_date, |d: NaiveDate| d.max(row.trade_date)));
        rows.push(row);
    }

    Ok(ParsedAvanzaReport { rows, date_from: min_date, date_to: max_date })
}

fn parse_header(record: &csv::StringRecord) -> Result<Header, ParseError> {
    let idx = |name: &str| {
        record
            .iter()
            .position(|f| f.trim() == name)
            .ok_or_else(|| ParseError::header(format!("required column {name:?} not found")))
    };
    // Sanity: the first column must be Datum, identifying an Avanza export.
    if record.get(0).map(str::trim) != Some("Datum") {
        return Err(ParseError::header("not an Avanza AllTradesReport (missing Datum header)"));
    }
    let _ = COLUMNS; // documents the expected column set
    Ok(Header {
        datum: idx("Datum")?,
        typ: idx("Typ av transaktion")?,
        namn: idx("Värdepapper/beskrivning")?,
        antal: idx("Antal")?,
        kurs: idx("Kurs")?,
        belopp: idx("Belopp")?,
        transaktionsvaluta: idx("Transaktionsvaluta")?,
        courtage: idx("Courtage")?,
        valutakurs: idx("Valutakurs")?,
        instrumentvaluta: idx("Instrumentvaluta")?,
        isin: idx("ISIN")?,
    })
}

fn parse_row(
    row_number: usize,
    record: &csv::StringRecord,
    header: &Header,
) -> Result<ParsedAvanzaRow, ParseError> {
    let get = |i: usize| record.get(i).map(str::trim).unwrap_or("");
    let raw_kind = get(header.typ).to_string();
    let kind = classify(&raw_kind);
    let trade_date = NaiveDate::parse_from_str(get(header.datum), "%Y-%m-%d")
        .map_err(|e| ParseError::row(row_number, "invalid_date", format!("invalid Datum: {e}")))?;

    Ok(ParsedAvanzaRow {
        source_row_number: row_number,
        trade_date,
        raw_kind,
        kind,
        name: get(header.namn).to_string(),
        quantity: decimal(get(header.antal))
            .ok_or_else(|| ParseError::row(row_number, "invalid_decimal", "invalid Antal"))?,
        price: optional_decimal(get(header.kurs)),
        amount: optional_decimal(get(header.belopp)),
        transaction_currency: get(header.transaktionsvaluta).to_string(),
        brokerage: optional_decimal(get(header.courtage)),
        fx_rate: optional_decimal(get(header.valutakurs)),
        instrument_currency: get(header.instrumentvaluta).to_string(),
        isin: get(header.isin).to_string(),
    })
}

fn classify(raw: &str) -> AvanzaKind {
    match raw {
        "Köp" => AvanzaKind::Buy,
        "Sälj" => AvanzaKind::Sell,
        "Utdelning" => AvanzaKind::Dividend,
        "Split värdepapper" => AvanzaKind::Split,
        _ => AvanzaKind::Unsupported,
    }
}

fn decimal(raw: &str) -> Option<Decimal> {
    let n = normalize_decimal(raw);
    if n.is_empty() { None } else { Decimal::from_str(&n).ok() }
}

fn optional_decimal(raw: &str) -> Option<Decimal> {
    decimal(raw)
}
```

- [ ] **Step 3: Parser tests**

Add a `#[cfg(test)] mod tests` covering: semicolon + comma decimals; ISO dates; the four kinds + unknown→`Unsupported`; unsettled blank-FX/blank-courtage row; SEK instrument (no Valutakurs); fractional `Antal`. Example skeleton:

```rust
#[cfg(test)]
mod tests {
    use super::{parse_report, AvanzaKind};
    use rust_decimal_macros::dec;

    const SAMPLE: &str = concat!(
        "Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n",
        "2026-06-10;ISK;Köp;Apple Inc;5;200,00;-10000,00;SEK;9,00;9,50;USD;US0378331005;\n",
        "2026-06-09;ISK;Sälj;Apple Inc;-2;210,00;3990,00;SEK;9,00;9,50;USD;US0378331005;120,00\n",
        "2026-06-08;ISK;Utdelning;Apple Inc;0;;50,00;SEK;;9,50;USD;US0378331005;\n",
        "2026-06-07;ISK;Köp;Volvo B;3;250,00;750,00;SEK;0,00;;SEK;SE0000115446;\n",
        "2026-06-06;ISK;Köp;ASML;1;800,00;-800,00;EUR;;;EUR;NL0010273215;\n",
        "2026-06-05;ISK;Övrigt;Cash;0;;100,00;SEK;;;SEK;;\n",
    );

    #[test]
    fn parses_kinds_decimals_and_dates() {
        let report = parse_report(SAMPLE.as_bytes()).expect("parses");
        assert_eq!(report.rows.len(), 6);
        assert_eq!(report.rows[0].kind, AvanzaKind::Buy);
        assert_eq!(report.rows[0].price, Some(dec!(200.00)));
        assert_eq!(report.rows[0].quantity, dec!(5));
        assert_eq!(report.rows[1].kind, AvanzaKind::Sell);
        assert_eq!(report.rows[2].kind, AvanzaKind::Dividend);
        assert_eq!(report.rows[5].kind, AvanzaKind::Unsupported);
        // SEK instrument has blank Valutakurs.
        assert_eq!(report.rows[3].fx_rate, None);
        assert_eq!(report.rows[3].instrument_currency, "SEK");
        // Unsettled EUR row: blank FX and blank courtage.
        assert_eq!(report.rows[4].fx_rate, None);
        assert_eq!(report.rows[4].brokerage, None);
        assert_eq!(report.date_from.unwrap().to_string(), "2026-06-05");
        assert_eq!(report.date_to.unwrap().to_string(), "2026-06-10");
    }
}
```

Run: `cargo test --lib import::avanza::parser`
Expected: PASS.

- [ ] **Step 4: clippy + fmt + commit**

```bash
git add backend/src/import/avanza backend/src/import/mod.rs
git commit -m "feat(import): Avanza CSV parser"
```

---

## Phase 7 — Avanza mapper (adapter to `PreparedImport`)

**Outcome:** `backend/src/import/avanza/mapper.rs` exposes `to_prepared(&ParsedAvanzaReport) -> PreparedImport`, applying the design's mapping rules: buy/sell mapping with FX rules and `source_currency` from `Transaktionsvaluta`, dividend → skip, unsupported → skip, fractional → skip, and split netting by `(date, ISIN)`.

**Files:**
- Create/replace: `backend/src/import/avanza/mapper.rs`

### Task 7.1: Mapper + split netting

- [ ] **Step 1: Write the mapper**

`backend/src/import/avanza/mapper.rs`:

```rust
use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::domain::{ProposedTransaction, TransactionKind};
use crate::import::avanza::parser::{AvanzaKind, ParsedAvanzaReport, ParsedAvanzaRow};
use crate::import::core::outcome::{
    InstrumentKey, MappedRow, PlanHeader, PreparedImport, RowNote, RowOutcome, SourceKindCounts,
};

const TITLE: &str = "Avanza All Trades";

pub fn to_prepared(report: &ParsedAvanzaReport) -> PreparedImport {
    let mut counts = SourceKindCounts { rows: report.rows.len(), ..Default::default() };
    let mut outcomes: Vec<RowOutcome> = Vec::new();

    // ISIN -> the instrument key carried by a buy/sell row (currency known).
    let mut instrument_by_isin: BTreeMap<String, InstrumentKey> = BTreeMap::new();
    for row in &report.rows {
        if matches!(row.kind, AvanzaKind::Buy | AvanzaKind::Sell) && !row.isin.is_empty() {
            instrument_by_isin
                .entry(row.isin.clone())
                .or_insert_with(|| buy_sell_instrument(row));
        }
    }

    // Split netting: group split rows by (date, ISIN).
    let mut split_groups: BTreeMap<(NaiveDate, String), (Decimal, usize, String)> = BTreeMap::new();

    for row in &report.rows {
        match row.kind {
            AvanzaKind::Buy => {
                counts.buys += 1;
                outcomes.push(map_buy_sell(row, TransactionKind::Buy));
            }
            AvanzaKind::Sell => {
                counts.sells += 1;
                outcomes.push(map_buy_sell(row, TransactionKind::Sell));
            }
            AvanzaKind::Dividend => {
                counts.dividends += 1;
                outcomes.push(RowOutcome::Skip {
                    asset_key: asset_key_of(&row.isin),
                    note: RowNote {
                        row: Some(row.source_row_number),
                        code: "dividend_deferred",
                        message: format!("dividend for {} not written in this version", row.name),
                    },
                });
            }
            AvanzaKind::Split => {
                counts.splits += 1;
                let entry = split_groups
                    .entry((row.trade_date, row.isin.clone()))
                    .or_insert((Decimal::ZERO, row.source_row_number, row.name.clone()));
                entry.0 += row.quantity;
                entry.1 = entry.1.min(row.source_row_number);
            }
            AvanzaKind::Unsupported => {
                outcomes.push(RowOutcome::Skip {
                    asset_key: asset_key_of(&row.isin),
                    note: RowNote {
                        row: Some(row.source_row_number),
                        code: "unsupported_type",
                        message: format!("transaction type {:?} is not supported", row.raw_kind),
                    },
                });
            }
        }
    }

    for ((trade_date, isin), (net, row, name)) in split_groups {
        outcomes.push(map_split(trade_date, &isin, net, row, &name, &instrument_by_isin));
    }

    PreparedImport {
        header: PlanHeader {
            title: TITLE.to_string(),
            date_from: report.date_from.unwrap_or(trade_date_fallback()),
            date_to: report.date_to.unwrap_or(trade_date_fallback()),
        },
        counts,
        outcomes,
    }
}

fn trade_date_fallback() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch date")
}

fn buy_sell_instrument(row: &ParsedAvanzaRow) -> InstrumentKey {
    InstrumentKey {
        exchange: "AVANZA".to_string(),
        symbol: row.isin.clone(),
        name: row.name.clone(),
        currency: row.instrument_currency.clone(),
        isin: Some(row.isin.clone()),
    }
}

/// An Avanza row's excludable asset key is its ISIN, or `None` when the ISIN is
/// blank (e.g. a cash/"Övrigt" row) so the skip/error note stays global.
fn asset_key_of(isin: &str) -> Option<String> {
    let trimmed = isin.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn map_buy_sell(row: &ParsedAvanzaRow, kind: TransactionKind) -> RowOutcome {
    if !row.quantity.fract().is_zero() {
        return RowOutcome::Skip {
            asset_key: asset_key_of(&row.isin),
            note: RowNote {
                row: Some(row.source_row_number),
                code: "non_integer_quantity",
                message: format!("quantity {} is not an integer (fund?)", row.quantity),
            },
        };
    }
    let Some(magnitude) = row.quantity.abs().to_i64() else {
        return RowOutcome::Skip {
            asset_key: asset_key_of(&row.isin),
            note: RowNote {
                row: Some(row.source_row_number),
                code: "non_integer_quantity",
                message: format!("quantity {} does not fit in i64", row.quantity),
            },
        };
    };

    // FX: present positive Valutakurs is SEK-per-native. SEK instrument + blank
    // Valutakurs => fx = 1 (no warning). Blank + native != SEK => None (warn).
    let (fx_rate_to_base, fx_warning) = match row.fx_rate {
        Some(rate) if rate > Decimal::ZERO => (Some(rate), false),
        _ if row.instrument_currency.eq_ignore_ascii_case("SEK") => (Some(Decimal::ONE), false),
        _ => (None, true),
    };

    let brokerage_base = match row.brokerage {
        Some(b) if b > Decimal::ZERO => Some(b),
        _ => None,
    };

    RowOutcome::Mapped(MappedRow {
        source_row_number: row.source_row_number,
        instrument: buy_sell_instrument(row),
        proposed: ProposedTransaction {
            kind,
            trade_date: row.trade_date,
            quantity: magnitude,
            price: row.price,
            currency: Some(row.instrument_currency.clone()),
            fx_rate_to_base,
            brokerage_base,
        },
        source_value: row.amount,
        source_currency: Some(row.transaction_currency.clone()),
        note: None,
        fx_warning,
    })
}

fn map_split(
    trade_date: NaiveDate,
    isin: &str,
    net: Decimal,
    row: usize,
    name: &str,
    instrument_by_isin: &BTreeMap<String, InstrumentKey>,
) -> RowOutcome {
    if net.is_zero() {
        return RowOutcome::Skip {
            asset_key: asset_key_of(isin),
            note: RowNote {
                row: Some(row),
                code: "split_zero_net",
                message: format!("net split delta for {name} is zero"),
            },
        };
    }
    if !net.fract().is_zero() {
        return RowOutcome::Skip {
            asset_key: asset_key_of(isin),
            note: RowNote {
                row: Some(row),
                code: "non_integer_quantity",
                message: format!("net split delta {net} is not an integer"),
            },
        };
    }
    let Some(delta) = net.to_i64() else {
        return RowOutcome::Skip {
            asset_key: asset_key_of(isin),
            note: RowNote {
                row: Some(row),
                code: "non_integer_quantity",
                message: format!("net split delta {net} does not fit in i64"),
            },
        };
    };

    // Prefer the same-file buy/sell instrument; otherwise carry an ISIN-only key
    // with empty currency. The writer resolves it to an existing instrument or
    // errors (a split never creates an instrument).
    let instrument = instrument_by_isin.get(isin).cloned().unwrap_or(InstrumentKey {
        exchange: "AVANZA".to_string(),
        symbol: isin.to_string(),
        name: name.to_string(),
        currency: String::new(),
        isin: Some(isin.to_string()),
    });

    RowOutcome::Mapped(MappedRow {
        source_row_number: row,
        instrument,
        proposed: ProposedTransaction {
            kind: TransactionKind::Split,
            trade_date,
            quantity: delta,
            price: None,
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
        },
        source_value: None,
        source_currency: None,
        note: None,
        fx_warning: false,
    })
}
```

- [ ] **Step 2: Mapper tests**

Add a `#[cfg(test)] mod tests` covering each rule from the design:
- buy/sell with native price + FX (Valutakurs present) → `fx_rate_to_base == Some(Valutakurs)`, `source_currency == "SEK"`.
- unsettled buy (blank FX, native EUR) → `fx_rate_to_base == None`, `fx_warning == true`, `source_currency == "EUR"` (the native `Transaktionsvaluta`), **not** SEK.
- SEK instrument, blank Valutakurs → `fx_rate_to_base == Some(1)`, no warning.
- dividend → `RowOutcome::Skip` with code `dividend_deferred`; `counts.dividends == 1`.
- unknown type → `Skip` code `unsupported_type`.
- fractional `Antal` → `Skip` code `non_integer_quantity`.
- split netting: rows `+70` and `−14` same date+ISIN → one `Mapped` Split with `quantity == 56`.
- split with no same-file buy/sell → a Split `Mapped` whose `instrument.currency` is empty (writer/planner turns it into `split_without_position` later — assert here only that no instrument currency was invented).

Use a helper that builds `ParsedAvanzaReport` directly (don't round-trip through CSV) for unit clarity. Example for split netting:

```rust
#[test]
fn split_rows_are_netted_to_one_delta() {
    let report = report_with(vec![
        split_row(10, "2026-01-02", "NOW1", 70),
        split_row(11, "2026-01-02", "NOW1", -14),
    ]);
    let prepared = to_prepared(&report);
    let splits: Vec<_> = prepared
        .outcomes
        .iter()
        .filter_map(|o| match o {
            RowOutcome::Mapped(m) if m.proposed.kind == TransactionKind::Split => Some(m),
            _ => None,
        })
        .collect();
    assert_eq!(splits.len(), 1);
    assert_eq!(splits[0].proposed.quantity, 56);
}
```

Run: `cargo test --lib import::avanza::mapper`
Expected: PASS.

- [ ] **Step 3: clippy + fmt + commit**

```bash
git add backend/src/import/avanza/mapper.rs
git commit -m "feat(import): Avanza mapper with split netting and FX rules"
```

---

## Phase 8 — Avanza API routes + end-to-end fixture tests

**Outcome:** `POST /api/import/avanza/preview` and `/api/import/avanza/commit` work through the shared path; a synthetic fixture exercises unsettled, split, dividend, fund, SEK and multi-currency shapes.

**Files:**
- Modify: `backend/src/api/import.rs`, `backend/src/api/mod.rs`
- Create: `backend/tests/fixtures/avanza_synthetic.csv`
- Test: `backend/tests/import_api.rs`

### Task 8.1: Wire Avanza handlers

- [ ] **Step 1: Add the parse adapter + handlers in `api/import.rs`**

```rust
use crate::import::avanza::mapper::to_prepared as avanza_prepared;
use crate::import::avanza::parser::parse_report as parse_avanza_report;

fn parse_avanza(bytes: &[u8]) -> Result<PreparedImport, ParseError> {
    parse_avanza_report(bytes).map(|report| avanza_prepared(&report))
}

pub async fn avanza_preview(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<Json<ImportPreview>, ApiError> {
    preview_source(&state, &bytes, parse_avanza).await
}

pub async fn avanza_commit(
    State(state): State<AppState>,
    Query(params): Query<CommitParams>,
    bytes: Bytes,
) -> Result<Json<ImportResult>, ApiError> {
    commit_source(&state, &bytes, "AVANZA", &params, parse_avanza).await
}
```

> Both parsers return the shared `core::outcome::ParseError` (lifted in Phase 3), so `parse_avanza` needs no normalization — it maps the parsed report straight into a `PreparedImport`.

- [ ] **Step 2: Routes**

In `backend/src/api/mod.rs` add:

```rust
.route("/import/avanza/preview", post(import::avanza_preview))
.route("/import/avanza/commit", post(import::avanza_commit))
```

### Task 8.2: Fixture + API tests

- [ ] **Step 1: Write the fixture**

`backend/tests/fixtures/avanza_synthetic.csv` (one settled multi-buy instrument, an unsettled EUR buy, a SEK instrument, a dividend, a fractional fund row, and a netted split on an instrument that also has a buy):

```text
Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat
2026-06-02;ISK;Split värdepapper;ServiceNow;-14;;;;;;;US81762P1021;
2026-06-02;ISK;Split värdepapper;ServiceNow;70;;;;;;;US81762P1021;
2026-06-01;ISK;Köp;ServiceNow;10;900,00;-94500,00;SEK;9,00;10,50;USD;US81762P1021;
2026-05-20;ISK;Utdelning;Apple Inc;0;;120,00;SEK;;9,40;USD;US0378331005;
2026-05-15;ISK;Sälj;Apple Inc;-2;210,00;3960,00;SEK;9,00;9,45;USD;US0378331005;200,00
2026-05-10;ISK;Köp;Apple Inc;5;200,00;-9450,00;SEK;9,00;9,45;USD;US0378331005;
2026-05-05;ISK;Köp;Volvo B;3;250,00;-750,00;SEK;0,00;;SEK;SE0000115446;
2026-05-01;ISK;Köp;ASML Holding;1;800,00;-800,00;EUR;;;EUR;NL0010273215;
2026-04-20;ISK;Köp;Avanza Global;1,5;120,00;-180,00;SEK;0,00;;SEK;SE0011527613;
```

> Verify row order keeps each ledger valid. The export is newest-first, so Apple's Sell (2026-05-15) appears *above* its Buy (2026-05-10) in file order; derivation sorts by `(trade_date, id)`, so the buy (05-10) is applied before the sell (05-15) and the position never goes negative (buy 5 → sell 2 → 3 held). The buy date **must** stay earlier than the sell date, or the clean-fixture `errors == 0` assertion fails with `SellExceedsPosition`. ServiceNow's split (2026-06-02) is after its buy (2026-06-01) chronologically — good.

- [ ] **Step 2: API tests**

Add to `backend/tests/import_api.rs`:

```rust
const AVANZA: &[u8] = include_bytes!("fixtures/avanza_synthetic.csv");

#[tokio::test]
async fn avanza_preview_counts_and_groups() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/avanza/preview", AVANZA).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["counts"]["dividends"], 1);
    assert!(body["counts"]["skipped"].as_u64().unwrap() >= 1); // Avanza Global fund row
    // ASML unsettled buy raises a missing_fx warning.
    let warnings = body["warnings"].as_array().expect("warnings");
    assert!(warnings.iter().any(|w| w["code"] == "missing_fx"));
    // No hard errors on the clean fixture.
    assert_eq!(body["counts"]["errors"], 0);
}

#[tokio::test]
async fn avanza_commit_writes_avanza_batch_and_persists_native_source_currency() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(status, StatusCode::OK);
    let batch_id = body["batch_id"].as_i64().expect("batch id");

    let source: String = sqlx::query_scalar("SELECT source FROM import_batches WHERE id = ?")
        .bind(batch_id)
        .fetch_one(&state.pool)
        .await
        .expect("source");
    assert_eq!(source, "AVANZA");

    // Unsettled ASML buy persists source_currency = EUR (native), not SEK.
    let asml = db::instruments::find_by_isin(&state.pool, "NL0010273215")
        .await
        .expect("query")
        .expect("asml instrument");
    let cur: Option<String> = sqlx::query_scalar(
        "SELECT source_currency FROM transactions WHERE instrument_id = ? LIMIT 1",
    )
    .bind(asml.id)
    .fetch_one(&state.pool)
    .await
    .expect("source_currency");
    assert_eq!(cur.as_deref(), Some("EUR"));

    // The instrument symbol is the ISIN.
    assert_eq!(asml.symbol, "NL0010273215");
    assert_eq!(asml.exchange, "AVANZA");
}

#[tokio::test]
async fn avanza_commit_matches_existing_instrument_by_isin() {
    let state = test_state().await;
    // Pre-create the instrument with the ISIN; commit must reuse it.
    let existing = sqlx::query_scalar::<_, i64>(
        "INSERT INTO instruments (symbol, exchange, name, type, currency, isin) \
         VALUES ('US81762P1021','AVANZA','ServiceNow','STOCK','USD','US81762P1021') RETURNING id",
    )
    .fetch_one(&state.pool)
    .await
    .expect("seed");
    let (status, _) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(status, StatusCode::OK);
    let now = db::instruments::find_by_isin(&state.pool, "US81762P1021")
        .await
        .expect("query")
        .expect("servicenow");
    assert_eq!(now.id, existing, "ISIN match must reuse the existing instrument");
}

#[tokio::test]
async fn avanza_rollback_via_shared_route() {
    let state = test_state().await;
    let (_, committed) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let batch_id = committed["batch_id"].as_i64().expect("batch id");
    let (status, _) =
        send_json(&state, "POST", &format!("/api/import/rollback/{batch_id}")).await;
    assert_eq!(status, StatusCode::OK);
}

// Regression for the plan review: a pre-existing symbol-only AVANZA row
// (symbol == ISIN, isin = NULL) is upgraded in place — reused, not duplicated,
// and its isin is backfilled so future find_by_isin matches it.
#[tokio::test]
async fn avanza_backfills_isin_on_symbol_only_existing_instrument() {
    let state = test_state().await;
    let existing = sqlx::query_scalar::<_, i64>(
        "INSERT INTO instruments (symbol, exchange, name, type, currency, isin) \
         VALUES ('US81762P1021','AVANZA','ServiceNow','STOCK','USD',NULL) RETURNING id",
    )
    .fetch_one(&state.pool)
    .await
    .expect("seed symbol-only AVANZA row");

    let (status, _) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(status, StatusCode::OK);

    let matched = db::instruments::find_by_isin(&state.pool, "US81762P1021")
        .await
        .expect("query")
        .expect("isin should now match");
    assert_eq!(matched.id, existing, "must reuse the symbol-only row, not duplicate it");
    assert_eq!(matched.isin.as_deref(), Some("US81762P1021"), "isin must be backfilled");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM instruments WHERE exchange = 'AVANZA' AND symbol = 'US81762P1021'",
    )
    .fetch_one(&state.pool)
    .await
    .expect("count");
    assert_eq!(count, 1, "no duplicate AVANZA instrument was created");
}
```

- [ ] **Step 3: Add a split-without-position regression test**

A fixture-independent CSV with only a split (no buy, no existing position) must surface `split_without_position` and create no AVANZA instrument:

```rust
#[tokio::test]
async fn avanza_split_without_position_is_a_hard_error_not_a_new_instrument() {
    let state = test_state().await;
    let csv = concat!(
        "Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n",
        "2026-06-02;ISK;Split värdepapper;Orphan;5;;;;;;;XS9999999999;\n",
    );
    let (status, body) = send_bytes(&state, "/api/import/avanza/preview", csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["errors"].as_array().unwrap().iter().any(|e| e["code"] == "split_without_position"));

    let (commit_status, _) = send_bytes(&state, "/api/import/avanza/commit", csv.as_bytes()).await;
    assert_eq!(commit_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(db::instruments::find_by_isin(&state.pool, "XS9999999999").await.unwrap().is_none());
}
```

Run: `cargo test --test import_api avanza`
Expected: PASS. (Tune fixture quantities until the clean fixture reports `errors == 0`.)

- [ ] **Step 4: Full backend gate**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add backend/src/api backend/tests
git commit -m "feat(import): Avanza preview/commit routes and fixture tests"
```

Add an `EngineeringDiary.md` entry for the Avanza endpoints.

---

## Phase 9 — Frontend: source toggle, asset checkboxes, exclude, shared rollback, versions

**Outcome:** `ImportView` lets the user pick a source (default Avanza), shows the per-asset list with checkboxes (checked by default), passes the deselected assets as `exclude` on commit, and uses the shared rollback route. Counts show dividends/skipped.

**Files:**
- Modify: `frontend/src/api/types.ts`, `frontend/src/api/queries.ts`, `frontend/src/components/ImportView.tsx`, `frontend/package.json`, `backend/Cargo.toml`

### Task 9.1: Types + query hooks

- [ ] **Step 1: Extend types**

In `frontend/src/api/types.ts`:

```ts
export type ImportSource = "sharesight" | "avanza";

export interface ImportCounts {
  rows: number;
  buys: number;
  sells: number;
  splits: number;
  dividends: number;
  new_instruments: number;
  skipped: number;
  warnings: number;
  errors: number;
}

export interface ImportAssetGroup {
  asset_key: string;
  name: string;
  currency: string;
  buys: number;
  sells: number;
  splits: number;
  dividends: number;
  default_selected: boolean;
  skipped_reason: string | null;
  warnings: ImportRowNote[];
  errors: ImportRowNote[];
  is_new_instrument: boolean;
}

export interface ImportNewInstrument {
  exchange: string;
  symbol: string;
  name: string;
  currency: string;
  isin: string | null;
}

export interface ImportPreview {
  metadata: { title: string; date_from: string; date_to: string } | null;
  counts: ImportCounts;
  assets: ImportAssetGroup[];
  new_instruments: ImportNewInstrument[];
  warnings: ImportRowNote[];
  errors: ImportRowNote[];
  duplicate_of_batch_id: number | null;
}

export interface ImportResult {
  batch_id: number;
  counts: ImportCounts;
  warnings: ImportRowNote[];
}
```

- [ ] **Step 2: Parameterize hooks by source + exclude**

In `frontend/src/api/queries.ts`, change the import hooks to accept a source and exclude list, and point rollback at the shared route:

```ts
export function usePreviewImport() {
  return useMutation({
    mutationFn: ({ source, file }: { source: ImportSource; file: ArrayBuffer }) =>
      apiSendBytes<ImportPreview>("POST", `/api/import/${source}/preview`, file),
  });
}

export function useCommitImport() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      source,
      file,
      allowDuplicate,
      exclude,
    }: {
      source: ImportSource;
      file: ArrayBuffer;
      allowDuplicate: boolean;
      exclude: string[];
    }) => {
      const params = new URLSearchParams();
      if (allowDuplicate) params.set("allow_duplicate", "true");
      if (exclude.length > 0) params.set("exclude", exclude.join(","));
      const qs = params.toString();
      return apiSendBytes<ImportResult>(
        "POST",
        `/api/import/${source}/commit${qs ? `?${qs}` : ""}`,
        file,
      );
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
    },
  });
}

export function useRollbackImport() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (batchId: number) =>
      apiSendBytes<RollbackResult>("POST", `/api/import/rollback/${batchId}`, new ArrayBuffer(0)),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
    },
  });
}
```

Import `ImportSource` from `./types`.

### Task 9.2: ImportView — toggle + asset selection

- [ ] **Step 1: Add source + selection state to the reducer**

Add `source: ImportSource` (default `"avanza"`) and `selected: Record<string, boolean>` (asset_key → checked) to `State`, plus actions `setSource` and `toggleAsset`. On `previewReady`, initialize `selected` from `preview.assets` using each asset's `default_selected`. Compute `exclude` as the asset_keys whose `selected` is false when calling `onCommit`.

> The reducer grows meaningfully here (source toggle, selection map, exclude computation). If `ImportView.tsx` gets unwieldy, extract the reducer + its types into a sibling module (e.g. `ImportView.reducer.ts`) or a custom hook; keep the reducer pure so it stays unit-testable, consistent with the backend's reducer discipline.

- [ ] **Step 2: Render the toggle + asset table**

Add a Sharesight/Avanza segmented control bound to `state.source` (disabled while busy; changing it resets the preview). Replace the eyebrow text "Sharesight" with the active source. Render an asset table after the counts with a checkbox column (checked = selected, disabled when `skipped_reason` is set), name, currency, per-type counts, and an inline badge when `errors.length > 0` so the user can deselect a problematic asset. Show the new `dividends` and `skipped` count chips. On commit, pass `{ source: state.source, file, allowDuplicate, exclude }`.

In the committed-result card, render `result.warnings` (e.g. `unknown_exclude_key`) if the array is non-empty — reuse the existing warnings-table markup so the new `ImportResult.warnings` field is surfaced rather than ignored.

> Follow `docs/VisualDesign.DarkTheme.md` for chip/table/checkbox styling. Reuse existing `table-wrap`, `summary-metrics`, `status-chip`, and `form-*` classes.

- [ ] **Step 3: Version bumps**

`frontend/package.json`: `"version": "0.5.0"`. `backend/Cargo.toml`: `version = "0.4.0"`.

- [ ] **Step 4: Frontend gate**

Run (from `frontend/`): `npm run check && npm run fmt`
Expected: clean (tsc + Biome).

- [ ] **Step 5: Human testing (recommended)**

Build and run the app, then:
1. Import the real Avanza export via the Avanza source; confirm counts (≈239 rows, 99 buys, 103 sells, 35 dividends, 2 split rows) and that ISIN-keyed instruments appear in Holdings.
2. Deselect one asset and commit; confirm only the rest is written and Undo removes the batch.
3. Switch the toggle to Sharesight and re-run an existing Sharesight import to confirm no regression.

- [ ] **Step 6: Commit**

```bash
git add frontend backend/Cargo.toml
git commit -m "feat(import): Avanza source toggle, asset deselect, shared rollback"
```

Add an `EngineeringDiary.md` entry for the frontend import changes (and version bump).

---

## Phase 10 — Documentation: decisions + deferred-dividend TODO

**Outcome:** The durable decisions are recorded and the dividends-deferred TODO lands in the high-level design.

**Files:**
- Modify: `docs/DecisionLog.md`, `docs/Design.HighLevel.md`

- [ ] **Step 1: DecisionLog entry**

Append to `docs/DecisionLog.md` (new entries go to the end), following the template:

```markdown
## 2026-06-16 - Avanza CSV import
Decision: Avanza "AllTradesReport" CSV is a first-class import source alongside Sharesight, sharing a source-neutral import core (per-source parser+mapper → row outcomes → shared planner/writer). Instrument identity is ISIN (nullable on `instruments`, partial-unique); Avanza creates instruments with `exchange = "AVANZA"`, `symbol = ISIN`. Dividends are parsed and counted but not written; fractional/fund quantities are skipped with a warning; unsettled rows are imported (FX absent → `missing_fx`); unknown transaction types are skipped, not fatal. Splits are netted per `(date, ISIN)` and never create an instrument. A whole-asset deselect/exclude path applies before validation so a user can commit the remainder of a file. No cross-source reconciliation; the user converges on one source.
Context: Avanza is the real source of truth; Sharesight data was copied from it.
Consequences: `import_batches.source` allows `'AVANZA'`; `source_currency` is persisted from the row (native for unsettled rows); overlapping settled/unsettled re-imports can double-count — mitigations are per-batch rollback and deselect. Cross-source merge and fractional quantities remain out of scope.
```

- [ ] **Step 2: High-level design TODO for dividends**

Add a short TODO note in `docs/Design.HighLevel.md` that dividend rows are parsed and counted but not yet written, and must eventually feed gains/income.

- [ ] **Step 3: Commit**

```bash
git add docs/DecisionLog.md docs/Design.HighLevel.md
git commit -m "docs: record Avanza import decisions and dividend TODO"
```

> No `EngineeringDiary.md` entry is needed for doc-only meta changes (per its instructions).

---

## Final verification

- [ ] From `backend/`: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
- [ ] From `frontend/`: `npm run check && npm run fmt`
- [ ] Confirm `/api/health` reports backend `0.4.0`; the UI footer shows frontend `0.5.0`.
- [ ] Human end-to-end with the real Avanza export (Phase 9, Step 5).

## Self-review against the spec

Mapping of spec sections → phases: Data model & migrations → P1; shared decimal helper + `InstrumentKey.isin` → P2/P3; generalized `build_plan`/`write_batch`, persist `source_currency` from row, ISIN-aware identity, `instrument_identity_conflict` → P3/P4; Avanza parser → P6; Avanza mapper (buy/sell FX, dividend skip, split netting, fractional skip, instrument synthesis) → P7; unsettled rows → P7 (FX rules) + P8 (persisted native `source_currency` test); deselect-assets flow + `dividends`/`skipped` counts + per-asset warnings/errors → P4/P5/P9; API routes + shared rollback → P4/P8; frontend toggle + checkboxes → P9; DecisionLog + Design.HighLevel TODO + EngineeringDiary + version bumps → P9/P10. Testing matrix from the spec is distributed across the per-phase tests.

Plan-review findings (`docs/reviews/Review.avanza-csv-import-plan.md`) folded in: mapper-stage asset errors are excludable via `asset_key` on `Skip`/`Error` outcomes + a regression test (P3/P5); symbol-only `AVANZA` `isin=NULL` rows are backfilled in place with a guard against a different-ISIN collision + a regression test (P4/P8); all seven `NewInstrument` construction sites are updated (P1); Phases 3–4 are one explicit commit boundary; `ImportResult.warnings` exists from P4; the duplicate-row key includes direction (P4).
