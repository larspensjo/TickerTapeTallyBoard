# Sharesight Import Implementation Plan

**Goal:** Load a Sharesight All Trades CSV into the ledger through a dry-run preview, an atomic commit, and a per-batch rollback, with a reducer-driven Import view.

**Architecture:** Promote the working spike parser into pure, unit-testable `parser` / `mapper` / `plan` modules behind a new `src/lib.rs`. Two stateless endpoints (`preview`, `commit`) take raw CSV bytes; the frontend holds the file between them. Commit writes one atomic `import_batches` + transactions batch via executor-generic db functions; rollback deletes a batch and re-validates remaining ledgers.

**Tech Stack:** Rust (axum 0.8, sqlx 0.8 SQLite, rust_decimal, chrono, csv, sha2), React 19 + TanStack Query/Table, Vite, Biome.

---

## Working agreement (read first)

- **No commits between phases.** After each phase, run the phase's verification commands, then **stage** the changes with `git add` and stop for the user to review `git diff --staged`. Do **not** run `git commit`. The user commits manually after review.
- Backend checks (run from `backend/`): `cargo build`, then `cargo clippy --all-targets -- -D warnings`, then `cargo fmt`.
- Frontend checks (run from `frontend/`): `npm run check`, then `npm run fmt`.
- Each phase is independently testable and leaves the tree green.
- `EngineeringDiary.md` entries are added at the end of each phase that changes the application (not for this plan document itself). Entries must stand on their own and must not reference "the plan" or phase numbers (per `Agents.md`).
- This plan is the implementation of `docs/plans/Design.sharesight-import-design.md`. That design is the source of truth for decisions; this plan is how to build it.

## Assumptions / resolved open questions

These were resolved from the codebase and `docs/DecisionLog.md`. Flag during review if any is wrong:

1. **No schema migration is needed.** `import_batches` and the `transactions` audit columns (`source_value`, `source_currency`, `brokerage_currency`, `import_batch_id`) already exist in `migrations/0001_create_ledger_core.sql`. `raw_file_hash` is a nullable column; duplicate detection is a query, not a DB constraint.
2. **Persistence: a dedicated `NewImportTransaction` DTO + executor-generic `*_in_tx` functions.** The existing pool-based `insert`/`replace` and `NewTransaction` are left untouched (lower risk than widening the manual-entry path). Manual entry keeps hardcoding the audit columns to `NULL`.
3. **Parser keeps every CSV column it already parses** (including the all-zero `Cost base per share (SEK)` and the unnamed source column) so the maintained example keeps its diagnostics; the mapper ignores audit-only columns. The per-row `raw_signature` is dropped from the shared parser (per-row dedupe is a non-goal); the example computes its own signature locally.
4. **`exchange_rate` is `Option<Decimal>` in the parser** (blank → `None`; present-but-not-a-decimal → parse error). The mapper inverts only positive rates.
5. **Synthetic CSV fixtures contain no private data** and are committed: small inline `const`s for unit tests, plus `backend/tests/fixtures/sharesight_synthetic.csv` for the integration suite.
6. **Preview always returns `200` with a plan-shaped body** (errors live in the `errors` array, including a parse failure rendered as one error entry). **Commit uses distinct `ApiError` statuses** and persists nothing on any hard error: `400 bad_request` for a parse failure (malformed CSV), `409 duplicate_import` when the file's `raw_file_hash` already exists and `allow_duplicate` is not set, and `422 Unprocessable Entity` for mapping/ledger hard errors (e.g. `non_sek_brokerage`, `sell_exceeds_position`). The frontend must treat all three as commit failures and surface the `error.code`/`error.message`; only `409` offers the "Import anyway" override.
7. **Diary entries are added per phase** (Agents.md requires every code change to be reflected in the diary); the design's "EngineeringDiary entries" phase is folded into per-phase staging, leaving only the spec-archival + human-acceptance work for the final phase.

---

## File structure

**New (backend):**
- `backend/src/lib.rs` — exposes modules so `examples/` and `tests/` can reuse production code.
- `backend/src/import/sharesight/mod.rs` — re-exports `parser`, `mapper`, `plan`.
- `backend/src/import/sharesight/parser.rs` — `parse_report(&[u8]) -> Result<ParsedReport, ParseError>`.
- `backend/src/import/sharesight/mapper.rs` — `map_row(&ParsedRow) -> Result<MappedRow, MapError>`.
- `backend/src/import/sharesight/plan.rs` — `build_plan(&ParsedReport, &PlanContext) -> ImportPlan` (pure).
- `backend/src/db/import_batches.rs` — batch insert/find/delete + `max_transaction_id`.
- `backend/src/api/import.rs` — `preview`, `commit`, `rollback` handlers + service-layer duplicate annotation.
- `backend/tests/fixtures/sharesight_synthetic.csv` — synthetic All Trades fixture.
- `backend/tests/import_api.rs` — integration suite against real migrations.

**Modified (backend):**
- `backend/Cargo.toml` — `csv` to `[dependencies]`, add `sha2`; bump version (Phase 3).
- `backend/src/main.rs` — thin wrapper over the lib.
- `backend/src/import/mod.rs` — `pub mod sharesight;`.
- `backend/src/db/mod.rs` — `pub mod import_batches;` + executor-generic insert/upsert helpers.
- `backend/src/db/transactions.rs` — add `NewImportTransaction`, `insert_in_tx`, `ledger_for_instrument_in_tx`.
- `backend/src/db/instruments.rs` — add `upsert_in_tx`.
- `backend/src/api/mod.rs` — route the new endpoints; remove the `schema-preview` route.
- `backend/src/api/sharesight.rs` — **deleted** (placeholder handler + test removed).
- `backend/examples/sharesight_import_spike.rs` — call the shared parser.

**New (frontend):**
- `frontend/src/components/ImportView.tsx` — reducer-driven import UI.

**Modified (frontend):**
- `frontend/src/api/types.ts` — import DTO types.
- `frontend/src/api/client.ts` — `apiSendBytes` helper.
- `frontend/src/api/queries.ts` — `usePreviewImport`, `useCommitImport`, `useRollbackImport`.
- `frontend/src/App.tsx` — top-level Board/Import view switch wired to the nav.
- `frontend/package.json` — bump version (Phase 5).

---

# Phase 1 — Pure modules, `lib.rs`, dependencies, example refactor

**Outcome:** `parser`, `mapper`, `plan` exist as pure modules with unit tests; the crate has a library target; the example shares the parser. No HTTP, no DB writes. `cargo test` green.

## Task 1.1: Add the library target and thin `main.rs`

**Files:**
- Create: `backend/src/lib.rs`
- Modify: `backend/src/main.rs`

- [ ] **Step 1: Create `src/lib.rs` exposing the existing modules**

```rust
//! Library surface for the TickerTapeTallyBoard backend.
//!
//! `main.rs` is a thin wrapper over this crate; `examples/` and `tests/`
//! reuse these modules instead of duplicating logic.

pub mod api;
pub mod app;
pub mod config;
pub mod db;
pub mod domain;
pub mod engine_logging;
pub mod import;
pub mod providers;
pub mod state;
```

- [ ] **Step 2: Reduce `main.rs` to a thin wrapper**

```rust
use ticker_tape_tally_board_backend::{app, config, engine_logging};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    engine_logging::initialize();

    app::serve(config::AppConfig::from_env()?).await
}
```

Note: the `engine_*!` macros are `#[macro_export]`, so `crate::engine_info!` calls inside the library keep working unchanged.

- [ ] **Step 3: Build to verify the lib + bin split compiles**

Run: `cargo build`
Expected: builds; both the lib (`ticker_tape_tally_board_backend`) and the bin compile.

- [ ] **Step 4: Run the existing test suite (no behavior change)**

Run: `cargo test`
Expected: all existing tests pass.

## Task 1.2: Promote dependencies

**Files:**
- Modify: `backend/Cargo.toml`

- [ ] **Step 1: Move `csv` and `rust_decimal_macros` to `[dependencies]` and add `sha2`**

In `[dependencies]` add:

```toml
csv = "1"
rust_decimal_macros = "1"
sha2 = "0.10"
```

Remove `csv = "1"` and `rust_decimal_macros = "1"` from `[dev-dependencies]` (keep `tower` there). The `plan` module uses the `dec!` macro in production constants, so `rust_decimal_macros` must be a normal dependency rather than dev-only.

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: resolves `sha2`, builds.

## Task 1.3: Sharesight `parser` module (promoted from the spike)

**Files:**
- Modify: `backend/src/import/mod.rs`
- Create: `backend/src/import/sharesight/mod.rs`
- Create: `backend/src/import/sharesight/parser.rs`

The parser is the spike's parsing core, moved verbatim **except** for the changes below. Keep all helper fns (`parse_metadata`, `is_header_record`, `record_is_empty`, `require_header`, `field`, `parse_decimal`, `normalize_decimal`, `normalize_text`, `sanitize_report_title`, `HeaderIndexes::parse`) as private fns in this module.

**Changes from the spike:**
1. `parse_report` takes `&[u8]` and uses `ReaderBuilder::...from_reader(bytes)` instead of `from_path`.
2. The public row type is `ParsedRow` (no `raw_signature`); `exchange_rate` becomes `Option<Decimal>`.
3. Errors are a `ParseError { row: Option<usize>, code: &'static str, message: String }` instead of `Box<dyn Error>` / `String`.
4. Transaction type is the public `ParsedKind` enum (`Buy`/`Sell`/`Split`).

- [ ] **Step 1: Wire the module tree**

`backend/src/import/mod.rs`:

```rust
pub mod sharesight;
```

`backend/src/import/sharesight/mod.rs`:

```rust
pub mod mapper;
pub mod parser;
pub mod plan;
```

> **Compilation order:** Rust requires every declared module file to exist. Before declaring `mapper` and `plan` here, create empty placeholder files `backend/src/import/sharesight/mapper.rs` and `backend/src/import/sharesight/plan.rs` (Tasks 1.4 and 1.5 fill them in). Otherwise the parser test command in Task 1.3 Step 6 fails to compile because the declared modules are missing. (Alternatively, declare only `pub mod parser;` here and add `pub mod mapper;` / `pub mod plan;` in Tasks 1.4 and 1.5.)

- [ ] **Step 2: Define the public types and `ParseError` in `parser.rs`**

```rust
use chrono::NaiveDate;
use rust_decimal::Decimal;

/// A parsed All Trades report: metadata plus one row per data line.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedReport {
    pub metadata: ReportMetadata,
    pub header_row_number: usize,
    pub rows: Vec<ParsedRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportMetadata {
    pub title: String,
    pub date_from: NaiveDate,
    pub date_to: NaiveDate,
}

/// One faithfully parsed CSV data row. Audit-only columns
/// (`cost_base_per_share_sek`, `source_column`) are retained for diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedRow {
    pub source_row_number: usize,
    pub market: String,
    pub code: String,
    pub name: String,
    pub kind: ParsedKind,
    pub trade_date: NaiveDate,
    pub quantity: Decimal,
    pub price: Decimal,
    pub instrument_currency: String,
    pub cost_base_per_share_sek: Decimal,
    pub brokerage: Decimal,
    pub brokerage_currency: String,
    /// `None` when the Exchange Rate cell is blank; `Some` for any parsed decimal.
    pub exchange_rate: Option<Decimal>,
    pub value: Decimal,
    pub source_column: String,
    pub comments: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsedKind {
    Buy,
    Sell,
    Split,
}

impl ParsedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "Buy",
            Self::Sell => "Sell",
            Self::Split => "Split",
        }
    }
}

/// A parse-stage failure with optional row context and a stable code.
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

- [ ] **Step 3: Implement `parse_report(&[u8])`**

```rust
use csv::{ReaderBuilder, StringRecord};
use std::str::FromStr;

const HEADER_MARKER: &str = "Market";
const REPORT_TITLE_MARKER: &str = "All Trades Report between";

pub fn parse_report(bytes: &[u8]) -> Result<ParsedReport, ParseError> {
    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes);

    let mut metadata = None;
    let mut header: Option<(usize, HeaderIndexes)> = None;
    let mut data: Vec<(usize, StringRecord)> = Vec::new();

    for (zero_based, record) in reader.records().enumerate() {
        let row_number = zero_based + 1;
        let record = record
            .map_err(|e| ParseError::row(row_number, "csv_read", format!("CSV read error: {e}")))?;

        if metadata.is_none() {
            metadata = parse_metadata(&record)?;
        }
        if header.is_none() && is_header_record(&record) {
            header = Some((row_number, HeaderIndexes::parse(&record)?));
            continue;
        }
        if header.is_some() && !record_is_empty(&record) {
            data.push((row_number, record));
        }
    }

    let metadata = metadata.ok_or_else(|| ParseError::header("report metadata line was not found"))?;
    let (header_row_number, header) =
        header.ok_or_else(|| ParseError::header("header row was not found"))?;

    let rows = data
        .iter()
        .map(|(row_number, record)| parse_row(*row_number, record, &header))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ParsedReport { metadata, header_row_number, rows })
}
```

- [ ] **Step 4: Port the helpers with the two semantic changes**

Move `HeaderIndexes` (drop the `raw_signature` machinery), `parse_metadata`, `is_header_record`, `record_is_empty`, `require_header`, `field`, `parse_decimal`, `normalize_decimal`, `normalize_text`, and `sanitize_report_title` from the spike. Replace the spike's `String`/`Box<dyn Error>` results with `ParseError`. Implement `parse_row` and the new `ParsedKind::parse` and an optional-decimal helper:

```rust
fn parse_row(
    row_number: usize,
    record: &StringRecord,
    header: &HeaderIndexes,
) -> Result<ParsedRow, ParseError> {
    Ok(ParsedRow {
        source_row_number: row_number,
        market: field(record, header.market, "Market", row_number)?.to_string(),
        code: field(record, header.code, "Code", row_number)?.to_string(),
        name: field(record, header.name, "Name", row_number)?.to_string(),
        kind: parse_kind(field(record, header.kind, "Type", row_number)?, row_number)?,
        trade_date: NaiveDate::parse_from_str(
            field(record, header.trade_date, "Date", row_number)?,
            "%d/%m/%Y",
        )
        .map_err(|e| ParseError::row(row_number, "invalid_date", format!("invalid Date: {e}")))?,
        quantity: decimal_field(record, header.quantity, "Quantity", row_number)?,
        price: decimal_field(record, header.price, "Price", row_number)?,
        instrument_currency: field(record, header.instrument_currency, "Instrument Currency", row_number)?.to_string(),
        cost_base_per_share_sek: decimal_field(record, header.cost_base_per_share_sek, "Cost base per share (SEK)", row_number)?,
        brokerage: decimal_field(record, header.brokerage, "Brokerage", row_number)?,
        brokerage_currency: field(record, header.brokerage_currency, "Brokerage Currency", row_number)?.to_string(),
        exchange_rate: optional_decimal_field(record, header.exchange_rate, "Exchange Rate", row_number)?,
        value: decimal_field(record, header.value, "Value", row_number)?,
        source_column: field(record, header.source_column, "source column", row_number)?.to_string(),
        comments: field(record, header.comments, "Comments", row_number)?.to_string(),
    })
}

fn parse_kind(value: &str, row_number: usize) -> Result<ParsedKind, ParseError> {
    match value.trim() {
        "Buy" => Ok(ParsedKind::Buy),
        "Sell" => Ok(ParsedKind::Sell),
        "Split" => Ok(ParsedKind::Split),
        other => Err(ParseError::row(row_number, "unknown_type", format!("unknown transaction type {other:?}"))),
    }
}

fn decimal_field(record: &StringRecord, index: usize, label: &str, row_number: usize) -> Result<Decimal, ParseError> {
    let raw = field(record, index, label, row_number)?;
    let normalized = normalize_decimal(raw);
    if normalized.is_empty() {
        return Err(ParseError::row(row_number, "invalid_decimal", format!("empty {label}")));
    }
    Decimal::from_str(&normalized)
        .map_err(|_| ParseError::row(row_number, "invalid_decimal", format!("invalid {label}: {raw:?}")))
}

/// Blank cell -> None; a present cell that is not a decimal -> parse error.
fn optional_decimal_field(record: &StringRecord, index: usize, label: &str, row_number: usize) -> Result<Option<Decimal>, ParseError> {
    let raw = field(record, index, label, row_number)?;
    let normalized = normalize_decimal(raw);
    if normalized.is_empty() {
        return Ok(None);
    }
    Decimal::from_str(&normalized)
        .map(Some)
        .map_err(|_| ParseError::row(row_number, "invalid_exchange_rate", format!("invalid {label}: {raw:?}")))
}
```

`HeaderIndexes` keeps the same fields as the spike but renames `transaction_type` to `kind`. `field`, `require_header`, `parse_metadata`, `is_header_record`, `record_is_empty`, `normalize_decimal`, `normalize_text`, `sanitize_report_title` are copied from the spike with `String`→`ParseError` substitutions (use `ParseError::header(...)` for metadata/header failures and `ParseError::row(...)` for field failures).

- [ ] **Step 5: Write parser unit tests with a synthetic CSV**

Add `#[cfg(test)] mod tests` at the bottom of `parser.rs`. The const uses explicit `\u{00A0}` (NBSP thousands) and `\u{2212}` (Unicode minus); comma-decimal numeric cells are quoted:

```rust
#[cfg(test)]
mod tests {
    use super::{parse_report, ParsedKind};
    use rust_decimal_macros::dec;

    const SYNTHETIC: &str = concat!(
        "Synthetic Portfolio - All Trades Report between 2025-06-12 and 2026-06-12\n",
        "\n",
        "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
        "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"9,60\",SEK,\"0,100000\",\"1\u{00A0}259,60\",All Trades,First buy\n",
        "NASDAQ,MSFT,Microsoft,Sell,13/06/2026,\u{2212}5,\"12,60\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"\u{2212}629,80\",All Trades,\n",
        "XETR,ASML,ASML Holding,Buy,14/06/2026,3,\"600,00\",EUR,\"0,00\",\"0,00\",SEK,,\"0,00\",All Trades,Missing FX\n",
        "NASDAQ,MSFT,Microsoft,Split,15/06/2026,10,\"0,00\",USD,\"0,00\",\"0,00\",SEK,,\"0,00\",All Trades,Ten for one\n",
    );

    #[test]
    fn parses_metadata_header_and_rows() {
        let report = parse_report(SYNTHETIC.as_bytes()).expect("parses");
        assert_eq!(report.metadata.date_from.to_string(), "2025-06-12");
        assert_eq!(report.metadata.date_to.to_string(), "2026-06-12");
        assert_eq!(report.rows.len(), 4);
    }

    #[test]
    fn parses_comma_decimals_nbsp_thousands_and_unicode_minus() {
        let report = parse_report(SYNTHETIC.as_bytes()).expect("parses");
        let buy = &report.rows[0];
        assert_eq!(buy.kind, ParsedKind::Buy);
        assert_eq!(buy.price, dec!(12.50));
        assert_eq!(buy.value, dec!(1259.60));
        let sell = &report.rows[1];
        assert_eq!(sell.kind, ParsedKind::Sell);
        assert_eq!(sell.quantity, dec!(-5));
        assert_eq!(sell.value, dec!(-629.80));
    }

    #[test]
    fn blank_exchange_rate_parses_as_none() {
        let report = parse_report(SYNTHETIC.as_bytes()).expect("parses");
        assert_eq!(report.rows[2].exchange_rate, None);
        assert_eq!(report.rows[0].exchange_rate, Some(dec!(0.100000)));
    }

    #[test]
    fn parses_dd_mm_yyyy_dates_and_split_row() {
        let report = parse_report(SYNTHETIC.as_bytes()).expect("parses");
        assert_eq!(report.rows[3].kind, ParsedKind::Split);
        assert_eq!(report.rows[3].trade_date.to_string(), "2026-06-15");
    }

    #[test]
    fn missing_header_is_an_error() {
        let bad = "Synthetic Portfolio - All Trades Report between 2025-06-12 and 2026-06-12\n\nno,header,here\n";
        let error = parse_report(bad.as_bytes()).expect_err("no header");
        assert_eq!(error.code, "header_not_found");
    }

    #[test]
    fn non_decimal_exchange_rate_is_a_parse_error() {
        let bad = SYNTHETIC.replace("\"0,100000\"", "\"abc\"");
        let error = parse_report(bad.as_bytes()).expect_err("bad rate");
        assert_eq!(error.code, "invalid_exchange_rate");
    }
}
```

- [ ] **Step 6: Run the parser tests**

Run: `cargo test --lib import::sharesight::parser`
Expected: all pass.

## Task 1.4: Sharesight `mapper` module

**Files:**
- Create: `backend/src/import/sharesight/mapper.rs`

The mapper turns a `ParsedRow` into an `InstrumentKey` + a domain `ProposedTransaction` (positive magnitude for Buy/Sell, signed delta for Split), preserving `source_value` and flagging missing FX.

- [ ] **Step 1: Write failing mapper tests**

```rust
#[cfg(test)]
mod tests {
    use super::{map_row, MapError};
    use crate::domain::TransactionKind;
    use crate::import::sharesight::parser::{ParsedKind, ParsedRow};
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn row(kind: ParsedKind) -> ParsedRow {
        ParsedRow {
            source_row_number: 1,
            market: "NASDAQ".into(),
            code: "MSFT".into(),
            name: "Microsoft".into(),
            kind,
            trade_date: NaiveDate::from_ymd_opt(2026, 6, 12).unwrap(),
            quantity: dec!(10),
            price: dec!(12.50),
            instrument_currency: "USD".into(),
            cost_base_per_share_sek: dec!(0),
            brokerage: dec!(9.60),
            brokerage_currency: "SEK".into(),
            exchange_rate: Some(dec!(0.100000)),
            value: dec!(1259.60),
            source_column: "All Trades".into(),
            comments: String::new(),
        }
    }

    #[test]
    fn buy_inverts_fx_keeps_sek_brokerage_and_source_value() {
        let mapped = map_row(&row(ParsedKind::Buy)).expect("maps");
        assert_eq!(mapped.instrument.exchange, "NASDAQ");
        assert_eq!(mapped.instrument.symbol, "MSFT");
        assert_eq!(mapped.instrument.currency, "USD");
        assert_eq!(mapped.proposed.kind, TransactionKind::Buy);
        assert_eq!(mapped.proposed.quantity, 10); // positive magnitude
        assert_eq!(mapped.proposed.price, Some(dec!(12.50)));
        assert_eq!(mapped.proposed.fx_rate_to_base, Some(dec!(10)));
        assert_eq!(mapped.proposed.brokerage_base, Some(dec!(9.60)));
        assert_eq!(mapped.source_value, dec!(1259.60));
        assert!(!mapped.fx_warning);
    }

    #[test]
    fn sell_passes_absolute_magnitude() {
        let mut r = row(ParsedKind::Sell);
        r.quantity = dec!(-5);
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.kind, TransactionKind::Sell);
        assert_eq!(mapped.proposed.quantity, 5); // magnitude; validate() re-signs
    }

    #[test]
    fn blank_or_zero_fx_maps_to_none_with_warning() {
        let mut r = row(ParsedKind::Buy);
        r.exchange_rate = None;
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.fx_rate_to_base, None);
        assert!(mapped.fx_warning);

        r.exchange_rate = Some(dec!(0));
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.fx_rate_to_base, None);
        assert!(mapped.fx_warning);
    }

    #[test]
    fn zero_brokerage_stores_no_fee_and_ignores_currency_label() {
        let mut r = row(ParsedKind::Buy);
        r.brokerage = dec!(0);
        r.brokerage_currency = "USD".into();
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.brokerage_base, None);
    }

    #[test]
    fn non_zero_non_sek_brokerage_is_a_hard_error() {
        let mut r = row(ParsedKind::Buy);
        r.brokerage_currency = "USD".into();
        assert_eq!(map_row(&r).unwrap_err().code, "non_sek_brokerage");
    }

    #[test]
    fn non_integer_quantity_is_a_hard_error() {
        let mut r = row(ParsedKind::Buy);
        r.quantity = dec!(1.5);
        assert_eq!(map_row(&r).unwrap_err().code, "non_integer_quantity");
    }

    #[test]
    fn split_carries_only_quantity_delta() {
        let r = row(ParsedKind::Split);
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.kind, TransactionKind::Split);
        assert_eq!(mapped.proposed.quantity, 10);
        assert_eq!(mapped.proposed.price, None);
        assert_eq!(mapped.proposed.currency, None);
        assert_eq!(mapped.proposed.fx_rate_to_base, None);
        assert_eq!(mapped.proposed.brokerage_base, None);
        let _ = MapError { row: 1, code: "x", message: String::new() }; // type is constructible
        let _ = Decimal::ZERO;
    }

    #[test]
    fn reverse_split_preserves_negative_delta() {
        let mut r = row(ParsedKind::Split);
        r.quantity = dec!(-9); // reverse split removes shares
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.kind, TransactionKind::Split);
        assert_eq!(mapped.proposed.quantity, -9); // sign preserved, not abs()
    }
}
```

- [ ] **Step 2: Run tests to verify they fail to compile (module not implemented)**

Run: `cargo test --lib import::sharesight::mapper`
Expected: FAIL (unresolved `map_row` / `MapError` / `MappedRow`).

- [ ] **Step 3: Implement the mapper**

```rust
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::domain::{ProposedTransaction, TransactionKind};
use crate::import::sharesight::parser::{ParsedKind, ParsedRow};

/// Instrument identity + display fields from one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstrumentKey {
    pub exchange: String,
    pub symbol: String,
    pub name: String,
    pub currency: String,
}

/// A row mapped to a proposed ledger transaction plus audit/warning context.
#[derive(Clone, Debug, PartialEq)]
pub struct MappedRow {
    pub source_row_number: usize,
    pub kind: ParsedKind,
    pub instrument: InstrumentKey,
    pub proposed: ProposedTransaction,
    pub source_value: Decimal,
    /// True when a Buy/Sell had a blank or non-positive Exchange Rate.
    pub fx_warning: bool,
}

/// A mapping-stage failure (parse-level errors are handled by the parser).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapError {
    pub row: usize,
    pub code: &'static str,
    pub message: String,
}

pub fn map_row(row: &ParsedRow) -> Result<MappedRow, MapError> {
    let instrument = InstrumentKey {
        exchange: row.market.trim().to_string(),
        symbol: row.code.trim().to_string(),
        name: row.name.trim().to_string(),
        currency: row.instrument_currency.trim().to_string(),
    };

    let kind = match row.kind {
        ParsedKind::Buy => TransactionKind::Buy,
        ParsedKind::Sell => TransactionKind::Sell,
        ParsedKind::Split => TransactionKind::Split,
    };

    let proposed = match row.kind {
        ParsedKind::Buy | ParsedKind::Sell => {
            let magnitude = integral_magnitude(row)?;
            let fx_rate_to_base = invert_fx(row.exchange_rate);
            let fx_warning = fx_rate_to_base.is_none();
            let brokerage_base = sek_brokerage(row)?;
            return Ok(MappedRow {
                source_row_number: row.source_row_number,
                kind: row.kind,
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
                source_value: row.value,
                fx_warning,
            });
        }
        ParsedKind::Split => ProposedTransaction {
            kind,
            trade_date: row.trade_date,
            quantity: integral_signed(row)?, // signed split delta (preserve reverse-split sign)
            price: None,
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
        },
    };

    Ok(MappedRow {
        source_row_number: row.source_row_number,
        kind: row.kind,
        instrument,
        proposed,
        source_value: row.value,
        fx_warning: false,
    })
}

/// Quantity must be integral; the magnitude (absolute value) is passed to the
/// domain validator for Buy/Sell, which re-signs it from the row's kind.
fn integral_magnitude(row: &ParsedRow) -> Result<i64, MapError> {
    Ok(integral_signed(row)?.abs())
}

/// Quantity must be integral; returns the signed value. Splits keep the sign so
/// a reverse-split (negative delta) is preserved; Buy/Sell take `.abs()` of this.
fn integral_signed(row: &ParsedRow) -> Result<i64, MapError> {
    if !row.quantity.fract().is_zero() {
        return Err(MapError {
            row: row.source_row_number,
            code: "non_integer_quantity",
            message: format!("quantity {} is not an integer", row.quantity),
        });
    }
    row.quantity.to_i64().ok_or(MapError {
        row: row.source_row_number,
        code: "non_integer_quantity",
        message: format!("quantity {} does not fit in i64", row.quantity),
    })
}

/// `fx_rate_to_base = 1 / Exchange Rate`, only for a present positive rate.
fn invert_fx(exchange_rate: Option<Decimal>) -> Option<Decimal> {
    match exchange_rate {
        Some(rate) if rate > Decimal::ZERO => Some(Decimal::ONE / rate),
        _ => None,
    }
}

/// SEK brokerage as an optional fee; non-zero non-SEK brokerage is a hard error.
fn sek_brokerage(row: &ParsedRow) -> Result<Option<Decimal>, MapError> {
    if row.brokerage.is_zero() {
        return Ok(None);
    }
    if !row.brokerage_currency.trim().eq_ignore_ascii_case("SEK") {
        return Err(MapError {
            row: row.source_row_number,
            code: "non_sek_brokerage",
            message: format!(
                "non-zero brokerage in {} is not SEK",
                row.brokerage_currency.trim()
            ),
        });
    }
    Ok(Some(row.brokerage))
}
```

- [ ] **Step 4: Run the mapper tests**

Run: `cargo test --lib import::sharesight::mapper`
Expected: all pass.

## Task 1.5: Sharesight `plan` module (pure planner)

**Files:**
- Create: `backend/src/import/sharesight/plan.rs`

The planner consumes a `ParsedReport` plus DB-derived context (passed in to stay pure) and produces an `ImportPlan` of counts, new instruments, warnings, and hard errors. It assigns provisional ids `max_existing_id + 1 + i` (global 0-based row index `i`) so preview and commit derive an identical `(trade_date, id)` order.

- [ ] **Step 1: Define the public types**

```rust
use std::collections::BTreeMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::domain::{self, LedgerTransaction};
use crate::import::sharesight::mapper::{map_row, InstrumentKey};
use crate::import::sharesight::parser::{ParsedKind, ParsedReport};

/// Reconciliation tolerance: warn when residual exceeds max(floor, rate * |value|).
const RECONCILIATION_FLOOR_SEK: Decimal = dec!(300);
const RECONCILIATION_RATE: Decimal = dec!(0.01);

/// Context the pure planner needs but cannot read itself (DB-derived).
#[derive(Clone, Debug, Default)]
pub struct PlanContext {
    /// Existing instruments keyed by `(exchange, symbol)` lowercased.
    pub existing_instruments: Vec<ExistingInstrument>,
    /// Stored ledger per existing instrument id.
    pub existing_ledgers: BTreeMap<i64, Vec<LedgerTransaction>>,
    /// Current `MAX(transactions.id)`, or 0 when the table is empty.
    pub max_existing_id: i64,
}

#[derive(Clone, Debug)]
pub struct ExistingInstrument {
    pub id: i64,
    pub exchange: String,
    pub symbol: String,
    pub currency: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPlan {
    pub counts: PlanCounts,
    pub new_instruments: Vec<InstrumentKey>,
    pub warnings: Vec<RowNote>,
    pub errors: Vec<RowNote>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanCounts {
    pub rows: usize,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub new_instruments: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowNote {
    pub row: Option<usize>,
    pub code: &'static str,
    pub message: String,
}
```

- [ ] **Step 2: Implement `build_plan`**

```rust
pub fn build_plan(report: &ParsedReport, ctx: &PlanContext) -> ImportPlan {
    let mut warnings: Vec<RowNote> = Vec::new();
    let mut errors: Vec<RowNote> = Vec::new();
    let mut new_instruments: Vec<InstrumentKey> = Vec::new();

    // Per-instrument simulated ledgers, seeded from existing storage.
    // Key: lowercased (exchange, symbol).
    let mut ledgers: BTreeMap<(String, String), Vec<LedgerTransaction>> = BTreeMap::new();
    let mut seeded: std::collections::BTreeSet<(String, String)> = Default::default();

    let mut counts = PlanCounts { rows: report.rows.len(), ..Default::default() };

    for (i, parsed) in report.rows.iter().enumerate() {
        match parsed.kind {
            ParsedKind::Buy => counts.buys += 1,
            ParsedKind::Sell => counts.sells += 1,
            ParsedKind::Split => counts.splits += 1,
        }

        let mapped = match map_row(parsed) {
            Ok(mapped) => mapped,
            Err(err) => {
                errors.push(RowNote { row: Some(err.row), code: err.code, message: err.message });
                continue;
            }
        };

        let key = (
            mapped.instrument.exchange.to_lowercase(),
            mapped.instrument.symbol.to_lowercase(),
        );

        // Resolve instrument: existing (currency check) or new.
        if let Some(existing) = ctx
            .existing_instruments
            .iter()
            .find(|e| e.exchange.eq_ignore_ascii_case(&mapped.instrument.exchange)
                && e.symbol.eq_ignore_ascii_case(&mapped.instrument.symbol))
        {
            if !existing.currency.eq_ignore_ascii_case(&mapped.instrument.currency) {
                errors.push(RowNote {
                    row: Some(parsed.source_row_number),
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
                    ctx.existing_ledgers.get(&existing.id).cloned().unwrap_or_default(),
                );
            }
        } else {
            seeded.insert(key.clone());
            ledgers.entry(key.clone()).or_default();
            // New instruments are surfaced via `new_instruments` (the M count), not
            // as warning rows — the design counts them separately from warnings.
            if !new_instruments.contains(&mapped.instrument) {
                new_instruments.push(mapped.instrument.clone());
            }
        }

        // Field-level validation -> signed quantity; build provisional ledger row.
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
            }
            Err(validation) => errors.push(RowNote {
                row: Some(parsed.source_row_number),
                code: validation.code(),
                message: validation.message().to_string(),
            }),
        }

        if mapped.fx_warning {
            warnings.push(RowNote {
                row: Some(parsed.source_row_number),
                code: "missing_fx",
                message: "Exchange Rate blank or non-positive; SEK base unavailable".to_string(),
            });
        }

        // Reconciliation (Buy/Sell with FX only).
        if matches!(mapped.kind, ParsedKind::Buy | ParsedKind::Sell) {
            if let (Some(fx), Some(price)) = (mapped.proposed.fx_rate_to_base, mapped.proposed.price) {
                if let Ok(signed) = domain::validate(&mapped.proposed) {
                    let signed_native_gross = Decimal::from(signed) * price;
                    let brokerage = mapped.proposed.brokerage_base.unwrap_or(Decimal::ZERO);
                    let derived = signed_native_gross * fx + brokerage;
                    let residual = (mapped.source_value - derived).abs();
                    let threshold = reconciliation_threshold(mapped.source_value);
                    if residual > threshold {
                        warnings.push(RowNote {
                            row: Some(parsed.source_row_number),
                            code: "reconciliation_residual",
                            message: format!("derived SEK off by {} (> {})", residual.round_dp(2), threshold.round_dp(2)),
                        });
                    }
                }
            }
        }
    }

    // Same-day, same-instrument, same-type informational warnings.
    let mut groups: BTreeMap<(String, String, String, &'static str), Vec<usize>> = BTreeMap::new();
    for parsed in &report.rows {
        groups
            .entry((
                parsed.market.to_lowercase(),
                parsed.code.to_lowercase(),
                parsed.trade_date.to_string(),
                parsed.kind.as_str(),
            ))
            .or_default()
            .push(parsed.source_row_number);
    }
    for ((_, _, _, _), rows) in groups.iter().filter(|(_, rows)| rows.len() > 1) {
        warnings.push(RowNote {
            row: rows.first().copied(),
            code: "same_day_multiple",
            message: format!("{} same-day same-type rows kept distinct", rows.len()),
        });
    }

    // Re-derive each affected instrument ledger; surface invariant violations.
    for (_, ledger) in ledgers.iter_mut() {
        ledger.sort_by_key(|tx| (tx.trade_date, tx.id));
        if let Err(ledger_error) = domain::derive_position(ledger) {
            let id = ledger_error.transaction_id();
            let row = if id > ctx.max_existing_id {
                report
                    .rows
                    .get((id - ctx.max_existing_id - 1) as usize)
                    .map(|r| r.source_row_number)
            } else {
                None
            };
            errors.push(RowNote { row, code: ledger_error.code(), message: ledger_message(ledger_error) });
        }
    }

    counts.new_instruments = new_instruments.len();
    counts.warnings = warnings.len();
    counts.errors = errors.len();

    ImportPlan { counts, new_instruments, warnings, errors }
}

fn reconciliation_threshold(source_value: Decimal) -> Decimal {
    let proportional = RECONCILIATION_RATE * source_value.abs();
    if proportional > RECONCILIATION_FLOOR_SEK {
        proportional
    } else {
        RECONCILIATION_FLOOR_SEK
    }
}

fn ledger_message(error: crate::domain::LedgerError) -> String {
    use crate::domain::LedgerError::*;
    match error {
        SellExceedsPosition { available, requested, .. } =>
            format!("Sell of {requested} exceeds available position of {available}."),
        SplitWithoutPosition { .. } => "A split requires an existing position.".to_string(),
        SplitDrivesNonPositive { resulting_quantity, .. } =>
            format!("Split would drive the position to {resulting_quantity}."),
        BuyMissingPrice { .. } => "A buy requires a native price.".to_string(),
    }
}
```

Note: `LedgerError`, `LedgerTransaction`, `validate`, `derive_position` are already re-exported from `crate::domain`. If `LedgerError` is not yet in the `domain` re-export list, add it to `backend/src/domain/mod.rs`'s `pub use transaction::{...}` (it currently exports `LedgerError`).

- [ ] **Step 2b: Write planner unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::{build_plan, ExistingInstrument, PlanContext};
    use crate::domain::{LedgerTransaction, TransactionKind};
    use crate::import::sharesight::parser::parse_report;
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use std::collections::BTreeMap;

    const FRESH: &str = concat!(
        "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
        "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
        "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"9,60\",SEK,\"0,100000\",\"1259,60\",All Trades,\n",
        "NASDAQ,MSFT,Microsoft,Sell,13/06/2026,\u{2212}4,\"12,60\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"\u{2212}504,00\",All Trades,\n",
    );

    fn plan_for(csv: &str, ctx: PlanContext) -> super::ImportPlan {
        build_plan(&parse_report(csv.as_bytes()).expect("parses"), &ctx)
    }

    #[test]
    fn fresh_portfolio_counts_new_instrument_and_no_errors() {
        let plan = plan_for(FRESH, PlanContext::default());
        assert_eq!(plan.counts.rows, 2);
        assert_eq!(plan.counts.buys, 1);
        assert_eq!(plan.counts.sells, 1);
        assert_eq!(plan.counts.new_instruments, 1);
        assert_eq!(plan.counts.errors, 0);
    }

    #[test]
    fn oversell_is_a_hard_error() {
        let oversell = FRESH.replace("Buy,12/06/2026,10", "Buy,12/06/2026,2");
        let plan = plan_for(&oversell, PlanContext::default());
        assert!(plan.errors.iter().any(|e| e.code == "sell_exceeds_position"));
    }

    #[test]
    fn currency_mismatch_against_existing_instrument_is_an_error() {
        let ctx = PlanContext {
            existing_instruments: vec![ExistingInstrument {
                id: 1,
                exchange: "NASDAQ".into(),
                symbol: "MSFT".into(),
                currency: "EUR".into(),
            }],
            existing_ledgers: BTreeMap::new(),
            max_existing_id: 0,
        };
        let plan = plan_for(FRESH, ctx);
        assert!(plan.errors.iter().any(|e| e.code == "currency_mismatch"));
        assert_eq!(plan.counts.new_instruments, 0);
    }

    #[test]
    fn provisional_ids_sort_imported_rows_after_existing_same_day_rows() {
        // Existing same-day buy of 4 (id 5); imported same-day sell of 4 must be valid.
        let existing = LedgerTransaction {
            id: 5,
            trade_date: NaiveDate::from_ymd_opt(2026, 6, 12).unwrap(),
            kind: TransactionKind::Buy,
            quantity: 4,
            price: Some(dec!(10)),
            fx_rate_to_base: Some(dec!(1)),
            brokerage_base: dec!(0),
        };
        let mut ledgers = BTreeMap::new();
        ledgers.insert(1, vec![existing]);
        let csv = concat!(
            "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
            "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
            "NASDAQ,MSFT,Microsoft,Sell,12/06/2026,\u{2212}4,\"10,00\",USD,\"0,00\",\"0,00\",SEK,\"1,000000\",\"\u{2212}40,00\",All Trades,\n",
        );
        let ctx = PlanContext {
            existing_instruments: vec![ExistingInstrument {
                id: 1, exchange: "NASDAQ".into(), symbol: "MSFT".into(), currency: "USD".into(),
            }],
            existing_ledgers: ledgers,
            max_existing_id: 5,
        };
        let plan = plan_for(csv, ctx);
        assert_eq!(plan.counts.errors, 0, "imported sell after existing buy must validate");
    }

    #[test]
    fn reconciliation_residual_over_threshold_warns() {
        let off = FRESH.replace("\"1259,60\"", "\"9999,00\"");
        let plan = plan_for(&off, PlanContext::default());
        assert!(plan.warnings.iter().any(|w| w.code == "reconciliation_residual"));
    }
}
```

- [ ] **Step 3: Run planner tests**

Run: `cargo test --lib import::sharesight::plan`
Expected: all pass.

## Task 1.6: Refactor the example to share the parser

**Files:**
- Modify: `backend/examples/sharesight_import_spike.rs`

- [ ] **Step 1: Replace the example's private parser with the shared one**

Delete the example's local parsing types/fns (`Report`, `ReportMetadata`, `Trade`, `TransactionType`, `HeaderIndexes`, `parse_report`, `parse_metadata`, `parse_trade`, `is_header_record`, `record_is_empty`, `require_header`, `field`, `parse_decimal_field`, `parse_decimal`, `normalize_decimal`, `normalize_text`, `DecimalParseError`). Keep `Args`, `summarize`, and all diagnostic helpers (`fx_model_summary`, `split_summary`, `partial_fill_summary`, etc.). Read the file with `std::fs` and call the shared parser:

```rust
use std::fs;
use ticker_tape_tally_board_backend::import::sharesight::parser::{parse_report, ParsedKind, ParsedReport, ParsedRow};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse(std::env::args().skip(1))?;
    let bytes = fs::read(&args.csv_path)?;
    let report = parse_report(&bytes)?;
    let summary = summarize(&report, args.split_current_position);
    print!("{summary}");
    Ok(())
}
```

- [ ] **Step 2: Adapt diagnostics to the shared types**

Update `summarize` and helpers to use `ParsedReport`/`ParsedRow`/`ParsedKind`. Two adaptations:
- `exchange_rate` is now `Option<Decimal>`: in `fx_model_summary`, filter `row.exchange_rate.is_some_and(|r| !r.is_zero())` and unwrap inside the loop.
- `raw_signature` no longer exists: build it locally where `duplicate_count` needs it, e.g. `format!("{}|{}|{}|{}", row.market, row.code, row.kind.as_str(), row.value)` (a coarse signature is fine for the spike's duplicate metric), or drop the duplicate metric line.

- [ ] **Step 3: Build and run the example against the synthetic fixture**

Run: `cargo run --example sharesight_import_spike -- --csv ../backend/tests/fixtures/sharesight_synthetic.csv`

(Defer this run until Task 5 of Phase 2 creates the fixture, or point `--csv` at any local file. The example must at least compile now.)

Run: `cargo build --examples`
Expected: builds.

## Task 1.7: Phase 1 verification and staging

- [ ] **Step 1: Full backend checks**

Run (from `backend/`): `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all green; `cargo fmt` makes no further changes after a second run.

- [ ] **Step 2: Add an EngineeringDiary entry**

Append to `EngineeringDiary.md` (stand-alone, no plan/phase references):

```markdown
## 2026-06-15 - Sharesight parser promoted to a library module
Promoted the All Trades CSV parser from the example into reusable library modules.
What changed:
- Added `src/lib.rs` and reduced `src/main.rs` to a thin wrapper.
- Added pure `import::sharesight` `parser`, `mapper`, and `plan` modules with unit tests.
- Moved `csv` to a normal dependency and added `sha2`.
- Refactored the import spike example to call the shared parser.
Observed:
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` pass.
Open question:
- None.
Refs: `backend/src/lib.rs`, `backend/src/import/sharesight/`, `backend/examples/sharesight_import_spike.rs`; implements: Sharesight CSV Import Conventions.
```

- [ ] **Step 3: Stage for review (no commit)**

Run: `git add backend/ EngineeringDiary.md`
Then stop and tell the user: "Phase 1 staged — review `git diff --staged`."

---

# Phase 2 — `preview` endpoint (read-only) + integration test

**Outcome:** `POST /api/import/sharesight/preview` parses raw CSV bytes, builds the plan from live DB context, annotates duplicate-file detection, and returns `ImportPreview`. No writes. The `schema-preview` placeholder is removed.

## Task 2.1: DB read helpers for plan context and duplicate detection

**Files:**
- Create: `backend/src/db/import_batches.rs`
- Modify: `backend/src/db/mod.rs`
- Modify: `backend/src/db/transactions.rs`

- [ ] **Step 1: Add `max_transaction_id` to `db/transactions.rs`**

```rust
/// Current maximum transaction id, or 0 when the table is empty.
pub async fn max_id(pool: &SqlitePool) -> Result<i64, RepoError> {
    let max: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM transactions")
        .fetch_one(pool)
        .await?;
    Ok(max)
}
```

- [ ] **Step 2: Add `db/import_batches.rs` with a hash lookup**

```rust
use sqlx::sqlite::SqlitePool;

use crate::db::RepoError;

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ImportBatchRow {
    pub id: i64,
    pub source: String,
    pub imported_at: String,
    pub raw_file_hash: Option<String>,
}

const COLUMNS: &str = "id, source, imported_at, raw_file_hash";

/// First existing batch whose `raw_file_hash` matches, if any.
pub async fn find_by_hash(
    pool: &SqlitePool,
    hash: &str,
) -> Result<Option<ImportBatchRow>, RepoError> {
    let row = sqlx::query_as::<_, ImportBatchRow>(&format!(
        "SELECT {COLUMNS} FROM import_batches WHERE raw_file_hash = ? ORDER BY id LIMIT 1"
    ))
    .bind(hash)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
```

- [ ] **Step 3: Register the module**

In `backend/src/db/mod.rs` add:

```rust
pub mod import_batches;
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: builds.

## Task 2.2: A `sha2` hashing helper

**Files:**
- Modify: `backend/src/import/sharesight/mod.rs` (or a small `import::hashing` helper)

- [ ] **Step 1: Add a hash function**

In `backend/src/import/mod.rs`:

```rust
pub mod sharesight;

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of the raw file bytes, used as `raw_file_hash`.
pub fn raw_file_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}
```

- [ ] **Step 2: Unit-test the hash is stable and hex**

```rust
#[cfg(test)]
mod tests {
    use super::raw_file_hash;

    #[test]
    fn hash_is_stable_64_char_hex() {
        let a = raw_file_hash(b"hello");
        assert_eq!(a.len(), 64);
        assert_eq!(a, raw_file_hash(b"hello"));
        assert_ne!(a, raw_file_hash(b"world"));
    }
}
```

Run: `cargo test --lib import::tests`
Expected: pass.

## Task 2.3: The `preview` handler

**Files:**
- Create: `backend/src/api/import.rs`
- Modify: `backend/src/api/mod.rs`
- Delete: `backend/src/api/sharesight.rs`

- [ ] **Step 1: Define the response DTOs and the handler**

```rust
use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::api::error::ApiError;
use crate::db::{import_batches, instruments, transactions};
use crate::import::raw_file_hash;
use crate::import::sharesight::mapper::InstrumentKey;
use crate::import::sharesight::parser::{parse_report, ParseError};
use crate::import::sharesight::plan::{build_plan, ExistingInstrument, ImportPlan, PlanContext, RowNote};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub metadata: Option<PreviewMetadata>,
    pub counts: PreviewCounts,
    pub new_instruments: Vec<NewInstrumentDto>,
    pub warnings: Vec<RowNoteDto>,
    pub errors: Vec<RowNoteDto>,
    pub duplicate_of_batch_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PreviewMetadata {
    pub title: String,
    pub date_from: String,
    pub date_to: String,
}

#[derive(Debug, Serialize, Default)]
pub struct PreviewCounts {
    pub rows: usize,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub new_instruments: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Serialize)]
pub struct NewInstrumentDto {
    pub exchange: String,
    pub symbol: String,
    pub name: String,
    pub currency: String,
}

#[derive(Debug, Serialize)]
pub struct RowNoteDto {
    pub row: Option<usize>,
    pub code: &'static str,
    pub message: String,
}

pub async fn preview(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<Json<ImportPreview>, ApiError> {
    let hash = raw_file_hash(&bytes);
    let duplicate_of_batch_id = import_batches::find_by_hash(&state.pool, &hash)
        .await?
        .map(|batch| batch.id);

    let report = match parse_report(&bytes) {
        Ok(report) => report,
        Err(error) => return Ok(Json(parse_error_preview(error, duplicate_of_batch_id))),
    };

    let ctx = load_plan_context(&state).await?;
    let plan = build_plan(&report, &ctx);

    Ok(Json(ImportPreview {
        metadata: Some(PreviewMetadata {
            title: report.metadata.title.clone(),
            date_from: report.metadata.date_from.to_string(),
            date_to: report.metadata.date_to.to_string(),
        }),
        counts: counts_dto(&plan),
        new_instruments: plan.new_instruments.iter().map(new_instrument_dto).collect(),
        warnings: plan.warnings.iter().map(row_note_dto).collect(),
        errors: plan.errors.iter().map(row_note_dto).collect(),
        duplicate_of_batch_id,
    }))
}

fn parse_error_preview(error: ParseError, duplicate_of_batch_id: Option<i64>) -> ImportPreview {
    ImportPreview {
        metadata: None,
        counts: PreviewCounts { errors: 1, ..Default::default() },
        new_instruments: Vec::new(),
        warnings: Vec::new(),
        errors: vec![RowNoteDto { row: error.row, code: error.code, message: error.message }],
        duplicate_of_batch_id,
    }
}

pub(crate) async fn load_plan_context(state: &AppState) -> Result<PlanContext, ApiError> {
    let instrument_rows = instruments::list(&state.pool).await?;
    let mut existing_ledgers = BTreeMap::new();
    let mut existing_instruments = Vec::new();
    for row in &instrument_rows {
        existing_instruments.push(ExistingInstrument {
            id: row.id,
            exchange: row.exchange.clone(),
            symbol: row.symbol.clone(),
            currency: row.currency.clone(),
        });
        existing_ledgers.insert(
            row.id,
            transactions::ledger_for_instrument(&state.pool, row.id).await?,
        );
    }
    Ok(PlanContext {
        existing_instruments,
        existing_ledgers,
        max_existing_id: transactions::max_id(&state.pool).await?,
    })
}

fn counts_dto(plan: &ImportPlan) -> PreviewCounts {
    PreviewCounts {
        rows: plan.counts.rows,
        buys: plan.counts.buys,
        sells: plan.counts.sells,
        splits: plan.counts.splits,
        new_instruments: plan.counts.new_instruments,
        warnings: plan.counts.warnings,
        errors: plan.counts.errors,
    }
}

fn new_instrument_dto(key: &InstrumentKey) -> NewInstrumentDto {
    NewInstrumentDto {
        exchange: key.exchange.clone(),
        symbol: key.symbol.clone(),
        name: key.name.clone(),
        currency: key.currency.clone(),
    }
}

fn row_note_dto(note: &RowNote) -> RowNoteDto {
    RowNoteDto { row: note.row, code: note.code, message: note.message.clone() }
}
```

- [ ] **Step 2: Route the endpoint and remove the placeholder**

In `backend/src/api/mod.rs`:
- Replace `mod sharesight;` with `mod import;`.
- In `api_router()`, remove the `/import/sharesight/schema-preview` route and add:

```rust
.route("/import/sharesight/preview", post(import::preview))
```

- Add `post` to the `axum::routing` import line (`use axum::{..., routing::{get, post, put}, ...}`).
- Delete `backend/src/api/sharesight.rs`.

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: builds; the `schema-preview` test is gone with its file.

## Task 2.4: Create the synthetic integration fixture

**Files:**
- Create: `backend/tests/fixtures/sharesight_synthetic.csv`

- [ ] **Step 1: Write the fixture (no private data)**

Content (note: the Value cell of the first buy uses a literal non-breaking space U+00A0 as the thousands separator, and the sell quantity/value use a literal Unicode minus U+2212 — preserve these exact bytes):

```
Synthetic Portfolio - All Trades Report between 2025-06-12 and 2026-06-12

Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments
NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,"12,50",USD,"0,00","9,60",SEK,"0,100000","1 259,60",All Trades,First buy
NASDAQ,MSFT,Microsoft,Sell,13/06/2026,−4,"12,60",USD,"0,00","0,00",SEK,"0,100000","−504,00",All Trades,
XETR,ASML,ASML Holding,Buy,14/06/2026,3,"600,00",EUR,"0,00","0,00",SEK,,"0,00",All Trades,Missing FX
NASDAQ,MSFT,Microsoft,Split,15/06/2026,10,"0,00",USD,"0,00","0,00",SEK,,"0,00",All Trades,Ten for one
```

This yields: 3 instruments referenced (MSFT exists once as new, ASML new) → 2 new instruments; 2 buys, 1 sell, 1 split; one missing-FX warning (ASML buy). MSFT ends at quantity (10 − 4) then split +10 = 16.

## Task 2.5: Preview integration test

**Files:**
- Create: `backend/tests/import_api.rs`

Integration tests use the public crate. `AppState::for_tests()` is `#[cfg(test)]`-only inside the crate, so the integration test builds its own migrated pool and state via the public API. Add a small public test helper or reuse `db::connect` against `sqlite::memory:`.

- [ ] **Step 1: Expose a way to build state for integration tests**

Add to `backend/src/state.rs` a non-test constructor path the integration test can use — it already has `AppState::new(pool)`. The integration test creates a pool with `db::connect("sqlite::memory:")`? `connect` runs migrations but the in-memory DB needs a single pinned connection. Add a public helper in `db` for a migrated in-memory pool:

In `backend/src/db/mod.rs`:

```rust
/// A migrated single-connection in-memory pool, for examples and integration tests.
pub async fn memory_pool() -> Result<sqlx::sqlite::SqlitePool, sqlx::Error> {
    use std::str::FromStr;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
```

(The existing `#[cfg(test)] db::testing::memory_pool` stays for in-crate unit tests; this public one serves `tests/` and the example. Refactor `testing::memory_pool` to delegate to this public helper (or have both call one shared private fn) so the two cannot drift in migration setup.)

- [ ] **Step 2: Write the preview integration test**

```rust
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use ticker_tape_tally_board_backend::{api, db, state::AppState};
use tower::ServiceExt;

const SYNTHETIC: &[u8] = include_bytes!("fixtures/sharesight_synthetic.csv");

async fn test_state() -> AppState {
    AppState::new(db::memory_pool().await.expect("memory pool"))
}

async fn send_bytes(state: &AppState, uri: &str, body: &[u8]) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "text/csv")
        .body(Body::from(body.to_vec()))
        .expect("request builds");
    let response = api::router(state.clone()).oneshot(request).await.expect("completes");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("body");
    let value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).expect("json") };
    (status, value)
}

#[tokio::test]
async fn preview_returns_counts_and_writes_nothing() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/sharesight/preview", SYNTHETIC).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["counts"]["rows"], 4);
    assert_eq!(body["counts"]["buys"], 2);
    assert_eq!(body["counts"]["sells"], 1);
    assert_eq!(body["counts"]["splits"], 1);
    assert_eq!(body["counts"]["new_instruments"], 2);
    assert_eq!(body["counts"]["errors"], 0);
    assert_eq!(body["duplicate_of_batch_id"], Value::Null);

    // No writes: instruments and transactions are still empty.
    let instruments: Vec<Value> = ticker_tape_tally_board_backend::db::instruments::list(&state.pool)
        .await
        .expect("list");
    assert!(instruments.is_empty());
}
```

(`db::instruments::list` returns `Vec<InstrumentRow>`, not `Vec<Value>` — adjust the type to `Vec<_>` and assert `.is_empty()`.)

- [ ] **Step 3: Run the integration test**

Run: `cargo test --test import_api`
Expected: pass.

## Task 2.6: Phase 2 verification and staging

- [ ] **Step 1: Full backend checks**

Run (from `backend/`): `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all green.

- [ ] **Step 2: EngineeringDiary entry**

```markdown
## 2026-06-15 - Sharesight import preview endpoint
Added a read-only dry-run preview for Sharesight CSV import.
What changed:
- Added `POST /api/import/sharesight/preview` returning counts, new instruments, warnings, errors, and a duplicate-file annotation.
- Added `db::import_batches` hash lookup, `transactions::max_id`, a public migrated in-memory pool helper, and a SHA-256 file-hash helper.
- Removed the Phase 0 `schema-preview` placeholder endpoint.
Observed:
- Preview writes nothing; integration test confirms counts and an empty database.
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` pass.
Refs: `backend/src/api/import.rs`, `backend/src/db/import_batches.rs`, `backend/tests/import_api.rs`; implements: Sharesight CSV Import Conventions.
```

- [ ] **Step 3: Stage for review**

Run: `git add backend/`  and  `git add EngineeringDiary.md`
Stop: "Phase 2 staged — review `git diff --staged`."

---

# Phase 3 — `commit` endpoint (atomic batch + duplicate guard) + integration test

**Outcome:** `POST /api/import/sharesight/commit?allow_duplicate={bool}` writes one atomic batch (import_batches row, upserted instruments, transactions in CSV order), guarded by the duplicate-hash check; hard errors roll everything back. Backend version bumped. Decision Log entry recorded.

## Task 3.1: Executor-generic db write functions

**Files:**
- Modify: `backend/src/db/instruments.rs`
- Modify: `backend/src/db/transactions.rs`
- Modify: `backend/src/db/import_batches.rs`

All `*_in_tx` functions take `&mut sqlx::SqliteConnection` so a `&mut *tx` from a `sqlx::Transaction` can drive them. The existing pool-based functions are unchanged.

- [ ] **Step 1: `instruments::upsert_in_tx`**

```rust
use sqlx::SqliteConnection;

/// Upsert on `(exchange, symbol)` inside a caller-managed transaction.
pub async fn upsert_in_tx(
    conn: &mut SqliteConnection,
    new: &NewInstrument,
) -> Result<(InstrumentRow, bool), RepoError> {
    if let Some(existing) = sqlx::query_as::<_, InstrumentRow>(&format!(
        "SELECT {COLUMNS} FROM instruments WHERE exchange = ? AND symbol = ?"
    ))
    .bind(&new.exchange)
    .bind(&new.symbol)
    .fetch_optional(&mut *conn)
    .await?
    {
        return Ok((existing, false));
    }

    let inserted = sqlx::query_as::<_, InstrumentRow>(&format!(
        "INSERT INTO instruments (symbol, exchange, name, type, currency) \
         VALUES (?, ?, ?, ?, ?) RETURNING {COLUMNS}"
    ))
    .bind(&new.symbol)
    .bind(&new.exchange)
    .bind(&new.name)
    .bind(&new.kind)
    .bind(&new.currency)
    .fetch_one(&mut *conn)
    .await?;
    Ok((inserted, true))
}
```

- [ ] **Step 2: `transactions::NewImportTransaction` + `insert_in_tx` + `ledger_for_instrument_in_tx`**

```rust
use sqlx::SqliteConnection;

/// Import insert payload: editable fields plus the audit/batch columns the
/// manual path leaves NULL. `quantity` is the signed position effect.
#[derive(Clone, Debug)]
pub struct NewImportTransaction {
    pub instrument_id: i64,
    pub kind: TransactionKind,
    pub trade_date: NaiveDate,
    pub quantity: i64,
    pub price: Option<Decimal>,
    pub currency: Option<String>,
    pub fx_rate_to_base: Option<Decimal>,
    pub brokerage: Option<Decimal>,
    pub brokerage_currency: Option<String>,
    pub source_value: Option<Decimal>,
    pub source_currency: Option<String>,
    pub note: Option<String>,
    pub import_batch_id: i64,
}

pub async fn insert_in_tx(
    conn: &mut SqliteConnection,
    new: &NewImportTransaction,
) -> Result<TransactionRow, RepoError> {
    let row = sqlx::query_as::<_, TransactionRow>(&format!(
        "INSERT INTO transactions \
           (instrument_id, type, trade_date, quantity, price, currency, fx_rate_to_base, \
            brokerage, brokerage_currency, source_value, source_currency, note, import_batch_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING {COLUMNS}"
    ))
    .bind(new.instrument_id)
    .bind(new.kind.as_db_str())
    .bind(new.trade_date.format("%Y-%m-%d").to_string())
    .bind(new.quantity)
    .bind(new.price.map(|d| d.to_string()))
    .bind(new.currency.clone())
    .bind(new.fx_rate_to_base.map(|d| d.to_string()))
    .bind(new.brokerage.map(|d| d.to_string()))
    .bind(new.brokerage_currency.clone())
    .bind(new.source_value.map(|d| d.to_string()))
    .bind(new.source_currency.clone())
    .bind(new.note.clone())
    .bind(new.import_batch_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}

/// One instrument's stored ledger inside a transaction, ordered `(trade_date, id)`.
pub async fn ledger_for_instrument_in_tx(
    conn: &mut SqliteConnection,
    instrument_id: i64,
) -> Result<Vec<LedgerTransaction>, RepoError> {
    let rows = sqlx::query_as::<_, TransactionRow>(&format!(
        "SELECT {COLUMNS} FROM transactions WHERE instrument_id = ? ORDER BY trade_date, id"
    ))
    .bind(instrument_id)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(TransactionRow::to_ledger).collect()
}
```

- [ ] **Step 3: `import_batches::insert_in_tx`**

```rust
use sqlx::SqliteConnection;

/// Insert a batch row inside a transaction; returns the new batch id.
pub async fn insert_in_tx(
    conn: &mut SqliteConnection,
    source: &str,
    imported_at: &str,
    raw_file_hash: &str,
) -> Result<i64, RepoError> {
    let row = sqlx::query_as::<_, (i64,)>(
        "INSERT INTO import_batches (source, imported_at, raw_file_hash) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(source)
    .bind(imported_at)
    .bind(raw_file_hash)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.0)
}
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: builds.

## Task 3.2: `imported_at` timestamp helper

**Files:**
- Modify: `backend/src/import/mod.rs`

- [ ] **Step 1: Add an ISO-8601 timestamp from `SystemTime`**

chrono is built without the `clock` feature, so `Utc::now()` is unavailable. Convert from `SystemTime`:

```rust
use chrono::{DateTime, Utc};
use std::time::SystemTime;

/// Current instant as an ISO-8601 / RFC-3339 UTC string for `imported_at`.
pub fn now_iso8601() -> String {
    let now: DateTime<Utc> = SystemTime::now().into();
    now.to_rfc3339()
}
```

If the `From<SystemTime>` conversion is unavailable in the pinned chrono build, fall back to computing from `duration_since(UNIX_EPOCH)` and `DateTime::from_timestamp(secs, nanos)`.

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: builds.

## Task 3.3: The `commit` handler

**Files:**
- Modify: `backend/src/api/import.rs`
- Modify: `backend/src/api/mod.rs`

- [ ] **Step 1: Add commit DTOs and the handler**

```rust
use axum::extract::Query;
use serde::Deserialize;

use crate::db::instruments::NewInstrument;
use crate::db::transactions::NewImportTransaction;
use crate::domain::{self, TransactionKind};
use crate::import::{now_iso8601, sharesight::mapper::map_row};

#[derive(Debug, Deserialize)]
pub struct CommitParams {
    #[serde(default)]
    pub allow_duplicate: bool,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub batch_id: i64,
    pub counts: PreviewCounts,
}

pub async fn commit(
    State(state): State<AppState>,
    Query(params): Query<CommitParams>,
    bytes: Bytes,
) -> Result<Json<ImportResult>, ApiError> {
    let hash = raw_file_hash(&bytes);

    // Re-parse and re-plan to reject hard errors before any write.
    let report = parse_report(&bytes)
        .map_err(|e| ApiError::bad_request(e.code, e.message))?;
    let ctx = load_plan_context(&state).await?;
    let plan = build_plan(&report, &ctx);
    if !plan.errors.is_empty() {
        let first = &plan.errors[0];
        return Err(ApiError::new(
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            first.code,
            first.message.clone(),
        )
        .with_details(serde_json::json!({
            "errors": plan.errors.iter().map(|e| serde_json::json!({
                "row": e.row, "code": e.code, "message": e.message
            })).collect::<Vec<_>>()
        })));
    }

    // Duplicate-file guard.
    if let Some(existing) = import_batches::find_by_hash(&state.pool, &hash).await? {
        if !params.allow_duplicate {
            return Err(ApiError::new(
                axum::http::StatusCode::CONFLICT,
                "duplicate_import",
                format!("file already imported as batch {}", existing.id),
            )
            .with_details(serde_json::json!({ "duplicate_of_batch_id": existing.id })));
        }
    }

    let counts = counts_dto(&plan);
    let batch_id = write_batch(&state, &report, &hash).await?;
    Ok(Json(ImportResult { batch_id, counts }))
}

/// One sqlx transaction: batch row, upserted instruments, transactions in CSV
/// order, then an in-transaction re-derivation. Any error rolls everything back.
async fn write_batch(
    state: &AppState,
    report: &crate::import::sharesight::parser::ParsedReport,
    hash: &str,
) -> Result<i64, ApiError> {
    let mut tx = state.pool.begin().await.map_err(|e| ApiError::internal(e.to_string()))?;

    let batch_id =
        import_batches::insert_in_tx(&mut *tx, "SHARESIGHT", &now_iso8601(), hash).await?;

    // Upsert instruments (first-seen order) and remember their ids.
    let mut instrument_ids: std::collections::BTreeMap<(String, String), i64> = Default::default();
    let mut affected: std::collections::BTreeSet<i64> = Default::default();

    for parsed in &report.rows {
        let mapped = map_row(parsed).map_err(|e| ApiError::new(
            axum::http::StatusCode::UNPROCESSABLE_ENTITY, e.code, e.message))?;
        let key = (
            mapped.instrument.exchange.to_lowercase(),
            mapped.instrument.symbol.to_lowercase(),
        );
        let instrument_id = match instrument_ids.get(&key) {
            Some(id) => *id,
            None => {
                let (row, _created) = instruments::upsert_in_tx(
                    &mut *tx,
                    &NewInstrument {
                        symbol: mapped.instrument.symbol.clone(),
                        exchange: mapped.instrument.exchange.clone(),
                        name: mapped.instrument.name.clone(),
                        kind: "STOCK".to_string(),
                        currency: mapped.instrument.currency.clone(),
                    },
                )
                .await?;
                instrument_ids.insert(key.clone(), row.id);
                row.id
            }
        };
        affected.insert(instrument_id);

        let signed = domain::validate(&mapped.proposed)
            .map_err(ApiError::from)?;
        let brokerage_currency = mapped
            .proposed
            .brokerage_base
            .map(|_| "SEK".to_string());
        let source_currency = matches!(mapped.kind,
            crate::import::sharesight::parser::ParsedKind::Buy
            | crate::import::sharesight::parser::ParsedKind::Sell)
            .then(|| "SEK".to_string());

        transactions::insert_in_tx(
            &mut *tx,
            &NewImportTransaction {
                instrument_id,
                kind: mapped.proposed.kind,
                trade_date: mapped.proposed.trade_date,
                quantity: signed,
                price: mapped.proposed.price,
                currency: mapped.proposed.currency.clone(),
                fx_rate_to_base: mapped.proposed.fx_rate_to_base,
                brokerage: mapped.proposed.brokerage_base,
                brokerage_currency,
                source_value: Some(mapped.source_value),
                source_currency,
                note: non_empty(&parsed.comments),
                import_batch_id: batch_id,
            },
        )
        .await?;
    }

    // In-transaction revalidation: every affected ledger must still derive.
    for instrument_id in affected {
        let ledger = transactions::ledger_for_instrument_in_tx(&mut *tx, instrument_id).await?;
        domain::derive_position(&ledger).map_err(ApiError::from)?;
    }

    tx.commit().await.map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(batch_id)
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
```

Note: `TransactionKind` is already imported in some branches; ensure imports compile (remove unused). `_ = TransactionKind;` is not needed — drop the unused import if clippy flags it.

- [ ] **Step 2: Route the endpoint**

In `backend/src/api/mod.rs`, add to `api_router()`:

```rust
.route("/import/sharesight/commit", post(import::commit))
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: builds.

## Task 3.4: Commit integration tests

**Files:**
- Modify: `backend/tests/import_api.rs`

- [ ] **Step 1: Add commit tests**

```rust
#[tokio::test]
async fn commit_writes_one_atomic_batch() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    assert_eq!(status, StatusCode::OK);
    let batch_id = body["batch_id"].as_i64().expect("batch id");
    assert!(batch_id >= 1);
    assert_eq!(body["counts"]["rows"], 4);

    // MSFT holding: 10 buy - 4 sell + 10 split = 16; ASML buy of 3.
    let (status, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert_eq!(status, StatusCode::OK);
    let holdings = holdings.as_array().expect("array");
    assert_eq!(holdings.len(), 2);
}

#[tokio::test]
async fn second_commit_of_same_file_is_rejected_unless_allowed() {
    let state = test_state().await;
    let (first, _) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    assert_eq!(first, StatusCode::OK);

    let (dup, body) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    assert_eq!(dup, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "duplicate_import");

    let (allowed, _) =
        send_bytes(&state, "/api/import/sharesight/commit?allow_duplicate=true", SYNTHETIC).await;
    assert_eq!(allowed, StatusCode::OK);
}

#[tokio::test]
async fn preview_reports_duplicate_after_commit() {
    let state = test_state().await;
    send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    let (status, body) = send_bytes(&state, "/api/import/sharesight/preview", SYNTHETIC).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["duplicate_of_batch_id"].as_i64().is_some());
}

#[tokio::test]
async fn hard_error_is_rejected_before_any_write() {
    let state = test_state().await;
    // A sell with no prior buy is caught during pre-commit planning, so commit
    // returns 422 and persists nothing. (This proves "no writes on a hard
    // error", not in-transaction rollback — the sqlx transaction is never
    // opened. The in-transaction revalidation/rollback path is exercised by the
    // rollback tests below, which delete rows inside a transaction and re-derive.)
    let bad = concat!(
        "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
        "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
        "NASDAQ,MSFT,Microsoft,Sell,12/06/2026,−4,\"10,00\",USD,\"0,00\",\"0,00\",SEK,\"1,000000\",\"−40,00\",All Trades,\n",
    );
    let (status, body) = send_bytes(&state, "/api/import/sharesight/commit", bad.as_bytes()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "sell_exceeds_position");

    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert_eq!(holdings.as_array().expect("array").len(), 0);
}
```

Add a `send_json` helper next to `send_bytes` for GET requests returning JSON (mirror the pattern used in `api/holdings.rs` tests).

- [ ] **Step 2: Run the integration tests**

Run: `cargo test --test import_api`
Expected: all pass.

## Task 3.5: Decision Log entry + version bump

**Files:**
- Modify: `docs/DecisionLog.md`
- Modify: `backend/Cargo.toml`

- [ ] **Step 1: Append a Decision Log entry**

```markdown
## 2026-06-15 - Sharesight Import Endpoints And Atomicity
Decision: Sharesight CSV import uses two stateless endpoints — a read-only `preview` and an atomic `commit` — that both take raw CSV bytes; the frontend holds the file between them. A commit is one sqlx transaction writing an `import_batches` row, upserted `STOCK` instruments, and transactions in CSV row order, with an in-transaction re-derivation that rolls the whole batch back on any ledger-invariant violation. Re-importing a file whose `raw_file_hash` already exists is rejected with `duplicate_import` unless `allow_duplicate=true`. Preview and commit derive validity from the same provisional id order (`max_existing_id + 1 + row_index`).
Context: The import must be previewable without writes and safe to retry, and same-day ordering must agree between preview and commit.
Consequences: New instruments default to `type = STOCK` (corrections deferred). Imports assume a fresh portfolio for same-day ordering against existing manual rows; a later import-sequence column can replace the id tiebreaker. Persisting the source CSV row number is deferred.
```

- [ ] **Step 2: Bump the backend version**

In `backend/Cargo.toml` change `version = "0.2.0"` to `version = "0.3.0"` (aligns backend with the import API; the UI surfaces it via `/api/health`).

- [ ] **Step 3: Rebuild and re-run the health test**

Run: `cargo test --lib api::health`
Expected: pass (reads `CARGO_PKG_VERSION`).

## Task 3.6: Phase 3 verification and staging

- [ ] **Step 1: Full backend checks**

Run (from `backend/`): `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all green.

- [ ] **Step 2: EngineeringDiary entry**

```markdown
## 2026-06-15 - Sharesight import commit and atomic batch write
Added atomic commit of a Sharesight CSV import with a duplicate-file guard.
What changed:
- Added `POST /api/import/sharesight/commit?allow_duplicate={bool}` writing an import batch, instruments, and transactions in one transaction.
- Added executor-generic `upsert_in_tx`, `insert_in_tx`, `ledger_for_instrument_in_tx`, and a batch insert.
- Added a `NewImportTransaction` DTO carrying the audit/batch columns.
- Bumped the backend package version.
Observed:
- A hard error rolls the commit back; a repeat file is rejected unless explicitly allowed.
- Checks pass.
Refs: `backend/src/api/import.rs`, `backend/src/db/`, `backend/Cargo.toml`; implements: Sharesight Import Endpoints And Atomicity.
```

- [ ] **Step 3: Stage for review**

Run: `git add backend/ docs/DecisionLog.md EngineeringDiary.md`
Stop: "Phase 3 staged — review `git diff --staged`."

---

# Phase 4 — `rollback` endpoint + integration test

**Outcome:** `POST /api/import/sharesight/rollback/{batch_id}` atomically deletes a batch's transactions and re-validates remaining ledgers, rejecting when a dependent manual sell would break. Decision Log entry recorded.

## Task 4.1: Rollback db helpers

**Files:**
- Modify: `backend/src/db/transactions.rs`
- Modify: `backend/src/db/import_batches.rs`

- [ ] **Step 1: Affected-instrument lookup and batch delete (in tx)**

In `db/transactions.rs`:

```rust
/// Distinct instrument ids touched by a batch.
pub async fn instrument_ids_for_batch(
    conn: &mut SqliteConnection,
    batch_id: i64,
) -> Result<Vec<i64>, RepoError> {
    let ids: Vec<(i64,)> = sqlx::query_as(
        "SELECT DISTINCT instrument_id FROM transactions WHERE import_batch_id = ?",
    )
    .bind(batch_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(ids.into_iter().map(|(id,)| id).collect())
}

/// Delete every transaction in a batch; returns the count removed.
pub async fn delete_batch_in_tx(
    conn: &mut SqliteConnection,
    batch_id: i64,
) -> Result<u64, RepoError> {
    let result = sqlx::query("DELETE FROM transactions WHERE import_batch_id = ?")
        .bind(batch_id)
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
}
```

In `db/import_batches.rs`:

```rust
pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<ImportBatchRow>, RepoError> {
    let row = sqlx::query_as::<_, ImportBatchRow>(&format!(
        "SELECT {COLUMNS} FROM import_batches WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Delete the batch row itself inside a transaction; returns the count removed.
/// Called after a batch's transactions are deleted so re-importing the same
/// file is not reported as a duplicate.
pub async fn delete_in_tx(
    conn: &mut SqliteConnection,
    id: i64,
) -> Result<u64, RepoError> {
    let result = sqlx::query("DELETE FROM import_batches WHERE id = ?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: builds.

## Task 4.2: The `rollback` handler

**Files:**
- Modify: `backend/src/api/import.rs`
- Modify: `backend/src/api/mod.rs`

- [ ] **Step 1: Add the handler**

```rust
use axum::extract::Path;
use axum::http::StatusCode;

#[derive(Debug, Serialize)]
pub struct RollbackResult {
    pub batch_id: i64,
    pub removed: u64,
}

pub async fn rollback(
    State(state): State<AppState>,
    Path(batch_id): Path<i64>,
) -> Result<Json<RollbackResult>, ApiError> {
    if import_batches::find(&state.pool, batch_id).await?.is_none() {
        return Err(ApiError::not_found("import batch", batch_id));
    }

    let mut tx = state.pool.begin().await.map_err(|e| ApiError::internal(e.to_string()))?;

    let affected = transactions::instrument_ids_for_batch(&mut *tx, batch_id).await?;
    let removed = transactions::delete_batch_in_tx(&mut *tx, batch_id).await?;

    // Every remaining ledger must still derive; otherwise reject and roll back.
    for instrument_id in affected {
        let ledger = transactions::ledger_for_instrument_in_tx(&mut *tx, instrument_id).await?;
        domain::derive_position(&ledger).map_err(ApiError::from)?;
    }

    // Remove the now-empty batch row so the same file can be re-imported without
    // tripping the duplicate-hash guard.
    import_batches::delete_in_tx(&mut *tx, batch_id).await?;

    tx.commit().await.map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(RollbackResult { batch_id, removed }))
}
```

(Instruments created by the batch are intentionally left in place — harmless empty instruments. The `from_ledger_error` `ApiError` already carries the offending `transaction_id` in details.)

- [ ] **Step 2: Route it**

In `backend/src/api/mod.rs` add:

```rust
.route("/import/sharesight/rollback/{batch_id}", post(import::rollback))
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: builds.

## Task 4.3: Rollback integration tests

**Files:**
- Modify: `backend/tests/import_api.rs`

- [ ] **Step 1: Add rollback tests**

```rust
#[tokio::test]
async fn rollback_removes_a_batch() {
    let state = test_state().await;
    let (_, committed) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    let batch_id = committed["batch_id"].as_i64().expect("batch id");

    let (status, body) =
        send_json(&state, "POST", &format!("/api/import/sharesight/rollback/{batch_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["removed"].as_u64().expect("removed") >= 4);

    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert_eq!(holdings.as_array().expect("array").len(), 0);
}

#[tokio::test]
async fn rollback_is_rejected_when_a_dependent_manual_sell_exists() {
    let state = test_state().await;
    let (_, committed) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    let batch_id = committed["batch_id"].as_i64().expect("batch id");

    // Find the ASML instrument id and add a manual sell that depends on the imported buy.
    let instruments = ticker_tape_tally_board_backend::db::instruments::list(&state.pool)
        .await
        .expect("list");
    let asml = instruments.iter().find(|i| i.symbol == "ASML").expect("asml");

    let (sell_status, _) = send_json_body(
        &state,
        "POST",
        "/api/transactions",
        serde_json::json!({
            "instrument_id": asml.id, "type": "Sell", "trade_date": "2026-06-20",
            "quantity": 3, "price": "650", "currency": "EUR"
        }),
    )
    .await;
    assert_eq!(sell_status, StatusCode::CREATED);

    let (status, body) =
        send_json(&state, "POST", &format!("/api/import/sharesight/rollback/{batch_id}")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "sell_exceeds_position");

    // Nothing was removed: holdings still present.
    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert!(!holdings.as_array().expect("array").is_empty());
}

#[tokio::test]
async fn rollback_unknown_batch_is_not_found() {
    let state = test_state().await;
    let (status, body) = send_json(&state, "POST", "/api/import/sharesight/rollback/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn same_file_can_be_reimported_after_rollback() {
    let state = test_state().await;
    let (_, committed) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    let batch_id = committed["batch_id"].as_i64().expect("batch id");

    let (rolled, _) =
        send_json(&state, "POST", &format!("/api/import/sharesight/rollback/{batch_id}")).await;
    assert_eq!(rolled, StatusCode::OK);

    // Preview no longer reports a duplicate, and a fresh commit succeeds without
    // `allow_duplicate` because the batch row (and its hash) was removed.
    let (status, preview) = send_bytes(&state, "/api/import/sharesight/preview", SYNTHETIC).await;
    assert_eq!(status, StatusCode::OK);
    assert!(preview["duplicate_of_batch_id"].is_null());

    let (recommit, _) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    assert_eq!(recommit, StatusCode::OK);
}
```

Add a `send_json_body` helper (POST/PUT JSON body → status + JSON), mirroring the `send` helper in `api/transactions.rs` tests.

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test import_api`
Expected: all pass.

## Task 4.4: Decision Log entry for rollback semantics

**Files:**
- Modify: `docs/DecisionLog.md`

- [ ] **Step 1: Append the entry**

```markdown
## 2026-06-15 - Import Batch Rollback Semantics
Decision: Rolling back an import batch deletes every transaction tagged with that `import_batch_id` in one transaction, then re-derives each affected instrument's remaining ledger. If any remaining row is no longer derivable (a later manual sell now oversells, or a split becomes invalid), the rollback is rejected with the offending transaction id and nothing is removed. When re-derivation passes, the now-empty `import_batches` row (carrying `raw_file_hash`) is deleted in the same transaction, so the same file can be re-imported afterwards without tripping the duplicate guard. Instruments created by the batch are left in place as harmless empty instruments.
Context: Imports must be reversible, but a back-dated import can become load-bearing for later manual edits. A rolled-back file should be importable again without forcing `allow_duplicate`. Cleanup of empty instruments is not worth a dependency-tracking pass in v1.
Consequences: A user must remove dependent manual transactions before rolling back an import they depend on. The batch id is not reusable after rollback. Empty-instrument cleanup is deferred.
```

## Task 4.5: Phase 4 verification and staging

- [ ] **Step 1: Full backend checks**

Run (from `backend/`): `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all green.

- [ ] **Step 2: EngineeringDiary entry**

```markdown
## 2026-06-15 - Sharesight import rollback
Added per-batch rollback for Sharesight imports.
What changed:
- Added `POST /api/import/sharesight/rollback/{batch_id}` deleting a batch and re-validating remaining ledgers.
- Added batch lookup, affected-instrument query, and batch-delete db helpers.
Observed:
- Rollback is rejected when a dependent manual sell exists; an unknown batch is 404.
- Checks pass.
Refs: `backend/src/api/import.rs`, `backend/src/db/`; implements: Import Batch Rollback Semantics.
```

- [ ] **Step 3: Stage for review**

Run: `git add backend/ docs/DecisionLog.md EngineeringDiary.md`
Stop: "Phase 4 staged — review `git diff --staged`."

---

# Phase 5 — Frontend Import view

**Outcome:** The inert **Import** nav link opens a reducer-driven view: choose a CSV, preview, see a summary card + warnings/new-instruments, commit (with duplicate "Import anyway"), and undo. Queries invalidated on success. Frontend version bumped.

## Task 5.1: Types and client helper

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/client.ts`

- [ ] **Step 1: Add import DTO types**

Append to `types.ts`:

```ts
export interface ImportRowNote {
  row: number | null;
  code: string;
  message: string;
}

export interface ImportCounts {
  rows: number;
  buys: number;
  sells: number;
  splits: number;
  new_instruments: number;
  warnings: number;
  errors: number;
}

export interface ImportNewInstrument {
  exchange: string;
  symbol: string;
  name: string;
  currency: string;
}

export interface ImportPreview {
  metadata: { title: string; date_from: string; date_to: string } | null;
  counts: ImportCounts;
  new_instruments: ImportNewInstrument[];
  warnings: ImportRowNote[];
  errors: ImportRowNote[];
  duplicate_of_batch_id: number | null;
}

export interface ImportResult {
  batch_id: number;
  counts: ImportCounts;
}

export interface RollbackResult {
  batch_id: number;
  removed: number;
}
```

- [ ] **Step 2: Add a raw-bytes sender to `client.ts`**

```ts
export async function apiSendBytes<T>(
  method: string,
  path: string,
  body: ArrayBuffer,
): Promise<T> {
  return parse<T>(
    await fetch(path, {
      method,
      headers: { "content-type": "text/csv" },
      body,
    }),
  );
}
```

- [ ] **Step 3: Frontend type check**

Run (from `frontend/`): `npm run check`
Expected: passes.

## Task 5.2: Query hooks

**Files:**
- Modify: `frontend/src/api/queries.ts`

- [ ] **Step 1: Add the import hooks**

```ts
import type { ImportPreview, ImportResult, RollbackResult } from "./types";
import { apiSendBytes } from "./client";

export function usePreviewImport() {
  return useMutation({
    mutationFn: (file: ArrayBuffer) =>
      apiSendBytes<ImportPreview>("POST", "/api/import/sharesight/preview", file),
  });
}

export function useCommitImport() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ file, allowDuplicate }: { file: ArrayBuffer; allowDuplicate: boolean }) =>
      apiSendBytes<ImportResult>(
        "POST",
        `/api/import/sharesight/commit${allowDuplicate ? "?allow_duplicate=true" : ""}`,
        file,
      ),
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
      apiSendBytes<RollbackResult>(
        "POST",
        `/api/import/sharesight/rollback/${batchId}`,
        new ArrayBuffer(0),
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
    },
  });
}
```

(Note: `apiSendBytes` and the `apiGet`/`apiSend` imports — keep the existing import line and add `apiSendBytes`.)

- [ ] **Step 2: Type check**

Run: `npm run check`
Expected: passes.

## Task 5.3: The Import view component

**Files:**
- Create: `frontend/src/components/ImportView.tsx`

- [ ] **Step 1: Implement a reducer-driven view**

Build `ImportView` following the `AddTransactionForm` reducer pattern and the design's state machine (`idle → fileSelected → previewing → previewReady → committing → committed | error`). Reuse existing CSS classes (`panel`, `panel-header`, `eyebrow`, `button primary/secondary/outline`, `table-wrap`, `form-error`, `status-chip`). Key behaviors:

```tsx
import { useReducer, useState } from "react";
import { useCommitImport, usePreviewImport, useRollbackImport } from "../api/queries";
import type { ImportPreview, ImportResult } from "../api/types";

type Phase = "idle" | "previewReady" | "committed";

interface State {
  phase: Phase;
  fileName: string | null;
  preview: ImportPreview | null;
  result: ImportResult | null;
  /// True once the user has explicitly chosen to override a duplicate file.
  confirmingDuplicate: boolean;
  error: string | null;
}

type Action =
  | { type: "fileCleared" }
  | { type: "previewReady"; preview: ImportPreview; fileName: string }
  | { type: "confirmDuplicate" }
  | { type: "cancelDuplicate" }
  | { type: "committed"; result: ImportResult }
  | { type: "failed"; message: string }
  | { type: "reset" };

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case "fileCleared":
      return { ...state, preview: null, result: null, error: null, fileName: null, confirmingDuplicate: false, phase: "idle" };
    case "previewReady":
      return { ...state, phase: "previewReady", preview: action.preview, fileName: action.fileName, confirmingDuplicate: false, error: null };
    case "confirmDuplicate":
      return { ...state, confirmingDuplicate: true, error: null };
    case "cancelDuplicate":
      return { ...state, confirmingDuplicate: false };
    case "committed":
      return { ...state, phase: "committed", result: action.result, confirmingDuplicate: false, error: null };
    case "failed":
      return { ...state, error: action.message };
    case "reset":
      return { phase: "idle", fileName: null, preview: null, result: null, confirmingDuplicate: false, error: null };
  }
}

export function ImportView() {
  const [state, dispatch] = useReducer(reducer, {
    phase: "idle", fileName: null, preview: null, result: null, confirmingDuplicate: false, error: null,
  });
  const [fileBytes, setFileBytes] = useState<ArrayBuffer | null>(null);
  const previewImport = usePreviewImport();
  const commitImport = useCommitImport();
  const rollbackImport = useRollbackImport();

  async function onFile(file: File) {
    const bytes = await file.arrayBuffer();
    setFileBytes(bytes);
    try {
      const preview = await previewImport.mutateAsync(bytes.slice(0));
      dispatch({ type: "previewReady", preview, fileName: file.name });
    } catch (error) {
      dispatch({ type: "failed", message: error instanceof Error ? error.message : "Preview failed." });
    }
  }

  async function onCommit(allowDuplicate: boolean) {
    if (!fileBytes) return;
    try {
      const result = await commitImport.mutateAsync({ file: fileBytes.slice(0), allowDuplicate });
      dispatch({ type: "committed", result });
    } catch (error) {
      dispatch({ type: "failed", message: error instanceof Error ? error.message : "Commit failed." });
    }
  }

  async function onRollback(batchId: number) {
    try {
      await rollbackImport.mutateAsync(batchId);
      dispatch({ type: "reset" });
      setFileBytes(null);
    } catch (error) {
      dispatch({ type: "failed", message: error instanceof Error ? error.message : "Undo failed." });
    }
  }

  const preview = state.preview;
  const hasErrors = (preview?.counts.errors ?? 0) > 0;
  const isDuplicate = preview?.duplicate_of_batch_id != null;

  return (
    <section className="panel">
      <div className="panel-header">
        <div>
          <p className="eyebrow">Sharesight</p>
          <h1>Import All Trades CSV</h1>
        </div>
      </div>

      {state.phase === "idle" ? (
        <div className="board-state muted">
          <label className="button primary">
            Choose CSV…
            <input
              type="file"
              accept=".csv,text/csv"
              style={{ display: "none" }}
              onChange={(e) => {
                const file = e.target.files?.[0];
                if (file) void onFile(file);
              }}
            />
          </label>
        </div>
      ) : null}

      {preview && state.phase === "previewReady" ? (
        <>
          <p className="total-value">
            {preview.counts.rows} trades — {preview.counts.buys} buys / {preview.counts.sells} sells /{" "}
            {preview.counts.splits} splits, {preview.counts.new_instruments} new instruments,{" "}
            {preview.counts.warnings} warnings
          </p>

          {isDuplicate ? (
            <p className="status-chip warning">
              Already imported as batch {preview.duplicate_of_batch_id}
            </p>
          ) : null}

          {preview.errors.length > 0 ? (
            <div className="table-wrap">
              <table>
                <thead><tr><th>Row</th><th>Code</th><th>Message</th></tr></thead>
                <tbody>
                  {preview.errors.map((e, i) => (
                    <tr key={`${e.code}-${i}`}><td>{e.row ?? "-"}</td><td>{e.code}</td><td>{e.message}</td></tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : null}

          {/* New-instruments list and warnings table follow the same table pattern. */}

          {/* Duplicate override is a deliberate two-step confirmation, not a
              single-click bypass: the first click only arms the override. */}
          {isDuplicate && state.confirmingDuplicate ? (
            <p className="form-error">
              This file was already imported as batch {preview.duplicate_of_batch_id}. Importing again
              will create a second, duplicate batch. Click “Import anyway” to confirm.
            </p>
          ) : null}

          <div className="form-actions">
            <button type="button" className="button secondary" onClick={() => { dispatch({ type: "reset" }); setFileBytes(null); }}>
              Cancel
            </button>
            {isDuplicate && !state.confirmingDuplicate ? (
              <button
                type="button"
                className="button outline danger"
                disabled={hasErrors || commitImport.isPending}
                onClick={() => dispatch({ type: "confirmDuplicate" })}
              >
                Import anyway…
              </button>
            ) : (
              <button
                type="button"
                className="button primary"
                disabled={hasErrors || commitImport.isPending}
                onClick={() => void onCommit(isDuplicate)}
              >
                {isDuplicate ? "Import anyway" : "Commit import"}
              </button>
            )}
          </div>
        </>
      ) : null}

      {state.phase === "committed" && state.result ? (
        <div className="board-state">
          <p className="total-value">Imported batch {state.result.batch_id} — {state.result.counts.rows} trades.</p>
          <button type="button" className="button outline danger" onClick={() => void onRollback(state.result!.batch_id)}>
            Undo this import
          </button>
        </div>
      ) : null}

      {state.error ? <p className="form-error">{state.error}</p> : null}
    </section>
  );
}
```

Notes: pass fresh `ArrayBuffer` copies (`.slice(0)`) to each mutation since `fetch` may detach the buffer. Render the new-instruments list and warnings table with the same `table-wrap`/`table` markup shown for errors.

- [ ] **Step 2: Type check**

Run: `npm run check`
Expected: passes.

## Task 5.4: Wire the nav link

**Files:**
- Modify: `frontend/src/App.tsx`

- [ ] **Step 1: Add a top-level view switch**

Extend `UiState` with `appView: "board" | "import"` and a `UiAction` `{ type: "appViewSelected"; appView: "board" | "import" }`. Make the **Board** and **Import** nav anchors buttons-as-links that dispatch the switch and toggle the `active` class. Render `<ImportView />` when `appView === "import"`, otherwise the existing board `<main>` content. Keep the header/totals/status strip shared.

```tsx
// nav
<a
  className={uiState.appView === "board" ? "active" : undefined}
  href="/"
  onClick={(e) => { e.preventDefault(); dispatch({ type: "appViewSelected", appView: "board" }); }}
>
  Board
</a>
<a
  className={uiState.appView === "import" ? "active" : undefined}
  href="/"
  onClick={(e) => { e.preventDefault(); dispatch({ type: "appViewSelected", appView: "import" }); }}
>
  Import
</a>
```

Add the reducer case:

```tsx
case "appViewSelected":
  return { ...state, appView: action.appView };
```

Initial state gains `appView: "board"`. In the workspace, conditionally render:

```tsx
{uiState.appView === "import" ? <ImportView /> : (/* existing board sections */)}
```

- [ ] **Step 2: Type check and run the dev server for a manual smoke**

Run: `npm run check`
Expected: passes.

> **Human testing recommended:** `npm run dev` (frontend) + the backend running; click **Import**, choose the synthetic CSV, confirm the summary card, commit, and undo. Confirm the board's holdings/transactions update after commit and after undo.

## Task 5.5: Version bump

**Files:**
- Modify: `frontend/package.json`

- [ ] **Step 1: Bump the frontend version**

Change `"version": "0.3.0"` to `"version": "0.4.0"`.

## Task 5.6: Phase 5 verification and staging

- [ ] **Step 1: Frontend checks**

Run (from `frontend/`): `npm run check && npm run fmt`
Expected: both pass.

- [ ] **Step 2: EngineeringDiary entry**

```markdown
## 2026-06-15 - Sharesight import UI
Wired the Import nav link to a dry-run/commit/undo view.
What changed:
- Added an `ImportView` with file selection, preview summary, warnings and new-instruments lists, commit (with a duplicate "Import anyway" override), and undo.
- Added `usePreviewImport`, `useCommitImport`, `useRollbackImport`, a raw-bytes client sender, and import DTO types.
- Added a top-level Board/Import view switch and bumped the frontend version.
Observed:
- `npm run check` and `npm run fmt` pass; committing invalidates instruments, transactions, and holdings.
Refs: `frontend/src/components/ImportView.tsx`, `frontend/src/api/`, `frontend/src/App.tsx`, `frontend/package.json`.
```

- [ ] **Step 3: Stage for review**

Run: `git add frontend/ EngineeringDiary.md`
Stop: "Phase 5 staged — review `git diff --staged`."

---

# Phase 6 — Acceptance and spec archival

**Outcome:** Human acceptance against the real export; the design spec archived per the project convention (plans are ephemeral and archived when implemented).

## Task 6.1: Human acceptance with the real export

- [ ] **Step 1: Run the full stack and import the real CSV**

Run the backend (from `backend/`: `cargo run`) and frontend (`npm run dev`). In the Import view, choose the real private `docs/AllTradesReport_2026-06-12.csv` (gitignored — never commit it). Verify:
- Preview counts match expectations (≈189 rows, the right buy/sell/split split, ~30 new instruments).
- Warnings are reconciliation residuals within tolerance and the expected missing-FX/partial-fill notes — no hard errors.
- Commit succeeds; spot-check a few positions and the portfolio holdings against Sharesight.
- "Undo this import" removes the batch and the board returns to empty.

> **Human testing required.** This step exercises private data and cannot be automated in the repo.

- [ ] **Step 2: Record acceptance in the EngineeringDiary**

```markdown
## 2026-06-15 - Sharesight import accepted against the real export
Imported the full private All Trades export end to end.
Observed:
- Preview/commit/undo worked; reconciliation residuals stayed within tolerance.
- Spot-checked positions and total matched Sharesight.
Refs: import endpoints and UI.
```

## Task 6.2: Archive the design spec

**Files:**
- Move: `docs/plans/Design.sharesight-import-design.md` → `docs/plans/archive/Design.sharesight-import-design.md` (or the repo's established archive location — check how the prior "Ledger Core" plan was archived in git history and mirror it).
- Move: this plan `docs/plans/Plan.sharesight-import.md` → the same archive location.

- [ ] **Step 1: Archive both the design and this plan**

Follow the same archival convention used for the Ledger Core plan (see commit `56da881 Ledger Core task 6: Record decisions and archive the plan`). Do not leave repository documents referencing the archived plan.

## Task 6.3: Phase 6 verification and staging

- [ ] **Step 1: Final full checks**

Run (from `backend/`): `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Run (from `frontend/`): `npm run check && npm run fmt`
Expected: all green.

- [ ] **Step 2: Stage for review**

Run: `git add -A`
Stop: "Phase 6 staged — review `git diff --staged`. Feature complete pending your commit."

---

## Self-review checklist (run by the implementer before starting)

- **Spec coverage:** parse (§1 goals) → P1; preview + duplicate annotation (§2.2, §3) → P2; atomic commit + duplicate guard + executor-generic db + provisional-id contract (§2.3–§2.5) → P3; rollback (§2.5) → P4; frontend (§5) → P5; tests (§6) across phases; acceptance + archival (§7.6) → P6.
- **Reconciliation (§2.4):** implemented in `plan.rs` with the `max(300, 1%)` threshold.
- **Validation matrix (§3):** non-integer quantity, non-SEK brokerage, invalid exchange rate, currency mismatch, ledger invariants, duplicate import — each has a test.
- **Ordering caveat (§2.5):** provisional-id test in P1; same-day-against-existing covered by the integration commit path.
- **No migration needed:** confirmed against `0001_create_ledger_core.sql`.
- **No commits between phases:** every phase ends with `git add` + stop.
