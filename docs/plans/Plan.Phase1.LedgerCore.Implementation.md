# Phase 1 — Ledger Core Implementation Plan
**Goal:** Deliver a manual, no-live-prices portfolio tracker: a SQLite ledger, transaction/instrument CRUD, a derived holdings view (weighted-average cost, per-currency cost basis), and a manual-entry UI.

**Architecture:** Pure `domain/` ledger logic (no axum/sqlx/HTTP) derives positions from `(trade_date, id)`-ordered transactions; `db/` repositories own all SQL against a `SqlitePool`; thin `api/` handlers inject an `AppState { pool }` via axum `State` and reuse the domain for stateful validation; the React frontend keeps input → action → reducer → state → render with side effects (queries/mutations) isolated.

**Tech Stack:** Rust + axum 0.8 + sqlx 0.8 (runtime queries, SQLite) + rust_decimal + chrono; React 19 + TanStack Query + TanStack Table + Biome.

---

## Resolved decisions (read before starting)

These resolve the design's §11 open items and a few implementation choices. They are the contract the tasks below assume.

1. **`transactions.type` CHECK allows `BUY|SELL|SPLIT|DIVIDEND`.** (User decision.) The schema is forward-compatible so no table-rebuild migration is needed later. **However, the API rejects creating a `Dividend` transaction in Phase 1** (no dividend fields/validation exist) with a `dividend_not_supported` validation error. Derivation treats any `Dividend` row as a no-op for position math (defensive; none are created in Phase 1). **This supersedes the design note** in `Design.Phase1.LedgerCore.md` §2.3/§11, which lists `BUY|SELL|SPLIT` only and flagged the choice for confirmation; that older note is reconciled (or archived) so the two documents do not disagree — the CHECK constraint admits `DIVIDEND`.
2. **sqlx via runtime `query_as` + `FromRow`** — no `sqlx-cli`, no `.sqlx/`, no `SQLX_OFFLINE`. Money/decimals are stored as **TEXT** and mapped through `rust_decimal` strings; integers (`quantity`, ids) as INTEGER; dates as ISO-8601 TEXT (`YYYY-MM-DD`). Correctness is covered by DB integration tests that run the real migrations.
3. **Quantity sign convention.** The DB `quantity` column stores the **signed position effect**: Buy `> 0`, Sell `< 0`, Split `= signed delta`. The **API request** carries a **positive magnitude** for Buy/Sell (sign derived from `type`) and a **signed non-zero delta** for Split. The **API response and storage** are the signed effect (matching the frontend's existing `-2` / `+8` display).
4. **Cost-basis model = weighted-average cost.** A Buy adds to `quantity` and `cost_basis_native` (and to `cost_basis_base` when FX is known); a Sell reduces `quantity` and every cost-basis component proportionally at the current average; a Split adds its delta to `quantity` and leaves cost-basis totals unchanged (averages rescale).
5. **Missing-FX contamination.** `cost_basis_base` / `average_cost_base` are available only while **every** Buy contributing to the currently open quantity had a known `fx_rate_to_base`. The first missing-FX Buy makes the position's base cost *unavailable* (with reasons + offending transaction ids) until the position fully closes (`quantity → 0`) and is rebuilt.
6. **Ledger-write validity invariant.** Every write (POST/PUT/DELETE) must leave the affected instrument's ledger derivable — i.e. re-deriving the full `(trade_date, id)`-ordered ledger after the change must succeed (no step drives quantity below zero, no invalid split). Otherwise the write is rejected `422`. This keeps `/api/holdings` always derivable. Missing-FX is **not** an error (it derives `Ok` with an unavailable base). **Concurrency tradeoff (accepted):** validation (`assert_ledger_valid`) and the write run as separate statements, so two simultaneous writers could each validate against the same prior ledger and then commit a jointly-invalid ledger. This is accepted for the single-user, local-only deployment of Phase 1; if multi-writer use is ever added, wrap validate-plus-write in a single SQLite transaction so writers to the same instrument serialize.
7. **Instrument identity = `UNIQUE (exchange, symbol)`.** `POST /api/instruments` is upsert-like: an existing `(exchange, symbol)` returns the existing row (`200`) unchanged; a new pair creates (`201`). The transaction form creates/picks an instrument by calling `POST /api/instruments` first, then `POST /api/transactions` with the returned `instrument_id`.
8. **Wire enum casing = PascalCase.** Transaction `type`: `Buy|Sell|Split|Dividend`. Instrument `type`: `Stock|Etf|Fund`. The DB stores uppercase (`BUY`, `STOCK`); repositories map between the two.
9. **Decimals serialize as JSON strings; `quantity` is a JSON integer. Errors use one shape:** `{ "error": { "code", "message", "details"? } }`. `PUT /api/transactions/:id` is a **full replacement** of editable fields. **Scope of the one error shape:** it covers every error a handler *returns* — `ApiError` from field validation, stateful ledger derivation, currency mismatch, not-found, and repository failures. It does **not** cover request bodies that fail axum's `Json` extractor (malformed JSON or wrong field types), which axum rejects with its own `400` body before the handler runs. The typed frontend client never sends those, and `client.ts` already degrades any non-conforming error body to code `unknown` with a generic message, so the contract holds for all real client traffic without a custom extractor. (If a strict single-shape guarantee for extractor rejections is later required, add a custom `Json` extractor or a `Router` rejection handler that emits `ApiError`.)

10. **Field-range and currency validation.** Beyond presence checks, Buy/Sell require `price > 0`; a present `fx_rate_to_base` must be `> 0`; a present `brokerage` must be `>= 0` (each maps to a stable `422` code in Task 2). Buy/Sell native `currency` must match the instrument's currency (case-insensitive); a mismatch is rejected `422 currency_mismatch` in Task 3. These guard the "missing data is never zero" rule and keep weighted-average cost basis and instrument-labelled native totals coherent.

> **Surface before coding:** decision (1)'s API-rejects-Dividend interpretation, decision (6)'s "DELETE re-validates and may reject" behavior, and decision (10)'s field-range/currency-match rejections are the judgment calls not spelled out verbatim in the design. If the owner disagrees, adjust Tasks 2–3 accordingly.

## Verification workflow (every backend task)

From `backend/`: `cargo test` (task's tests green) → `cargo clippy --all-targets -- -D warnings` → `cargo fmt`.
Every frontend task: from `frontend/` `npm run check` → `npm run fmt`.
Document each completed task in `EngineeringDiary.md` (one entry per logical change; see its "Instructions how to use"). Do **not** reference this plan or "Phase 1" in diary/DecisionLog entries — entries must stand alone.

## File structure

**Backend (new unless noted):**
- `backend/Cargo.toml` *(modify)* — promote `rust_decimal`, `chrono` to runtime deps; add `sqlx`; add `rust_decimal_macros` dev-dep.
- `backend/migrations/0001_create_ledger_core.sql` — `instruments`, `import_batches`, `transactions`.
- `backend/src/db/mod.rs` *(modify; thin)* — declare submodules, re-export `connect`.
- `backend/src/db/pool.rs` — `connect(database_url) -> SqlitePool` + migrate + FK pragma.
- `backend/src/db/instruments.rs` — `InstrumentRow`, find/list/upsert.
- `backend/src/db/transactions.rs` — `TransactionRow`, `NewTransaction`, insert/list/find/replace/delete, `ledger_for_instrument`, `all_for_holdings`.
- `backend/src/db/testing.rs` *(cfg(test))* — `memory_pool()`.
- `backend/src/domain/mod.rs` *(modify; thin)* — re-exports.
- `backend/src/domain/transaction.rs` — `TransactionKind`, `ProposedTransaction`, `LedgerTransaction`, `ValidationError`, `LedgerError`, `validate`.
- `backend/src/domain/position.rs` — `Position`, `BaseCostBasis`, `UnavailableReason`, `derive_position`.
- `backend/src/state.rs` — `AppState { pool }`.
- `backend/src/api/error.rs` — `ApiError` + `IntoResponse` + `From` conversions.
- `backend/src/api/instruments.rs` — `list`, `create` handlers + DTOs.
- `backend/src/api/transactions.rs` — `list`, `create`, `replace`, `remove` handlers + DTOs.
- `backend/src/api/holdings.rs` — `list` handler + DTOs.
- `backend/src/api/mod.rs` *(modify)* — `AppState` routing; refactor `router`/`router_with_static_assets` to take state.
- `backend/src/main.rs` *(modify; thin)* — `mod state;`; build pool; pass state to `serve`.
- `backend/src/app.rs` *(modify)* — `serve(config)` builds pool, threads `AppState`.
- `backend/src/config.rs` *(modify)* — add `database_url` + `TTTB_DATABASE_URL`.
- `backend/src/api/health.rs`, `backend/src/api/sharesight.rs` *(modify tests only)* — pass test `AppState` to `router(...)`.
- `.gitignore` *(modify)* — ignore the dev SQLite file.

**Frontend (new unless noted):**
- `frontend/package.json` *(modify)* — add `@tanstack/react-table`; bump version `0.2.1 → 0.3.0`.
- `frontend/src/api/types.ts` — wire types.
- `frontend/src/api/client.ts` — fetch helpers + error parsing.
- `frontend/src/api/queries.ts` — query/mutation hooks.
- `frontend/src/components/HoldingsTable.tsx` — TanStack Table.
- `frontend/src/components/TransactionsTable.tsx` — TanStack Table + filter.
- `frontend/src/components/AddTransactionForm.tsx` — reducer-driven form.
- `frontend/src/App.tsx` *(modify)* — real data, form wiring, remove mocks.
- `frontend/src/styles.css` *(modify)* — form + table state styles.

**Docs:**
- `docs/DecisionLog.md` *(modify)* — append the durable decisions (Task 6).
- `backend/Cargo.toml` version bump `0.1.1 → 0.2.0` (Task 5).

---

## Task 1 — DB foundation & schema

Adds dependencies, the first migration, the connection/migration runner, `AppState`, and the router refactor. Ends with the backend building and serving exactly as before, plus a migration test proving the schema and its constraints.

**Files:**
- Modify: `backend/Cargo.toml`
- Create: `backend/migrations/0001_create_ledger_core.sql`
- Create: `backend/src/db/pool.rs`, `backend/src/db/testing.rs`
- Modify: `backend/src/db/mod.rs`
- Create: `backend/src/state.rs`
- Modify: `backend/src/config.rs`, `backend/src/app.rs`, `backend/src/main.rs`, `backend/src/api/mod.rs`, `backend/src/api/health.rs`, `backend/src/api/sharesight.rs`
- Modify: `.gitignore`

- [ ] **Step 1: Add dependencies**

Replace the `[dependencies]` and `[dev-dependencies]` sections of `backend/Cargo.toml` with:

```toml
[dependencies]
axum = "0.8"
chrono = { version = "0.4", default-features = false, features = ["std"] }
log = "0.4"
rust_decimal = { version = "1", features = ["serde-with-str"] }
serde = { version = "1", features = ["derive"] }
simplelog = "0.12"
sqlx = { version = "0.8", default-features = false, features = [
    "macros",
    "migrate",
    "runtime-tokio",
    "sqlite",
] }
tokio = { version = "1", features = [
    "fs",
    "macros",
    "net",
    "rt-multi-thread",
    "signal",
] }
tower-http = { version = "0.6", features = ["cors", "fs"] }

[dev-dependencies]
csv = "1"
rust_decimal_macros = "1"
serde_json = "1"
tower = { version = "0.5", features = ["util"] }
```

Notes: `rust_decimal`/`chrono` move to runtime; `serde_json`/`tower` stay dev-only. `sqlx` `macros` feature provides `#[derive(sqlx::FromRow)]` and `migrate!` needs `migrate`; **no** offline metadata or `DATABASE_URL` is required because we use the non-macro `query_as` function and `sqlx::migrate!` only embeds files at compile time.

Run: `cd backend && cargo build`
Expected: compiles (no code uses the new crates yet). Slow first build while sqlx compiles.

- [ ] **Step 2: Write the migration**

Create `backend/migrations/0001_create_ledger_core.sql`:

```sql
-- Ledger core: instruments, import batches, and the transaction ledger.
-- Decimals are stored as TEXT (rust_decimal string round-trip); dates as ISO-8601 TEXT.

CREATE TABLE instruments (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol   TEXT NOT NULL,
    exchange TEXT NOT NULL,
    name     TEXT NOT NULL,
    type     TEXT NOT NULL CHECK (type IN ('STOCK', 'ETF', 'FUND')),
    currency TEXT NOT NULL,
    UNIQUE (exchange, symbol)
);

CREATE TABLE import_batches (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    source        TEXT NOT NULL CHECK (source IN ('SHARESIGHT', 'CSV', 'MANUAL')),
    imported_at   TEXT NOT NULL,
    raw_file_hash TEXT
);

CREATE TABLE transactions (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    instrument_id      INTEGER NOT NULL REFERENCES instruments (id),
    type               TEXT NOT NULL CHECK (type IN ('BUY', 'SELL', 'SPLIT', 'DIVIDEND')),
    trade_date         TEXT NOT NULL,          -- ISO-8601 YYYY-MM-DD
    quantity           INTEGER NOT NULL,       -- signed position effect (Buy>0, Sell<0, Split delta)
    price              TEXT,                   -- native unit price; required Buy/Sell
    currency           TEXT,                   -- native currency; required Buy/Sell
    fx_rate_to_base    TEXT,                   -- SEK per 1 unit native; nullable
    brokerage          TEXT,                   -- SEK fee; nullable
    brokerage_currency TEXT,                   -- 'SEK' in Phase 1; nullable
    source_value       TEXT,                   -- audit; nullable
    source_currency    TEXT,                   -- audit; nullable
    note               TEXT,                   -- nullable
    import_batch_id    INTEGER REFERENCES import_batches (id) -- null for manual entries
);

CREATE INDEX idx_transactions_instrument
    ON transactions (instrument_id, trade_date, id);
```

Delete the placeholder `backend/migrations/.gitkeep` (a real migration now occupies the directory).

- [ ] **Step 3: Add the connection/migration runner**

Create `backend/src/db/pool.rs`:

```rust
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

/// Open the SQLite pool, enabling foreign keys, creating the file if needed,
/// and applying all embedded migrations.
pub async fn connect(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new().connect_with(options).await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}
```

- [ ] **Step 4: Add the in-memory test pool helper**

Create `backend/src/db/testing.rs`:

```rust
//! Test-only database helpers shared across the crate's `#[cfg(test)]` modules.

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

/// A migrated in-memory SQLite pool. Uses a single connection so the in-memory
/// database (which is per-connection) persists for the pool's lifetime.
pub async fn memory_pool() -> SqlitePool {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .expect("in-memory URL is valid")
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("in-memory pool should connect");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations should apply");

    pool
}
```

- [ ] **Step 5: Make `db/mod.rs` a thin wrapper**

Replace the (empty) `backend/src/db/mod.rs` with:

```rust
mod pool;

pub mod instruments;
pub mod transactions;

#[cfg(test)]
pub mod testing;

pub use pool::connect;
```

> The `instruments` and `transactions` modules are created in Task 3. To keep Task 1 compiling on its own, temporarily create empty `backend/src/db/instruments.rs` and `backend/src/db/transactions.rs` files containing only `// Implemented in Task 3.` and remove the `pub mod instruments;` / `pub mod transactions;` lines until Task 3 — **or** implement Task 1 and Task 3's repository skeletons together. Simplest: for Task 1, comment out those two `pub mod` lines and the `pub use` of anything from them; re-enable in Task 3.

For Task 1 specifically, `db/mod.rs` is:

```rust
mod pool;

#[cfg(test)]
pub mod testing;

pub use pool::connect;
```

- [ ] **Step 6: Add `AppState`**

Create `backend/src/state.rs`:

```rust
use sqlx::sqlite::SqlitePool;

/// Shared application state injected into axum handlers via `State`.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[cfg(test)]
impl AppState {
    /// Build state backed by a migrated in-memory database for tests.
    pub async fn for_tests() -> Self {
        Self::new(crate::db::testing::memory_pool().await)
    }
}
```

- [ ] **Step 7: Add `database_url` to config**

In `backend/src/config.rs`, add the constants near the existing ones:

```rust
const DEFAULT_DATABASE_URL: &str = "sqlite://tttb-ledger.sqlite";
const DATABASE_URL_ENV: &str = "TTTB_DATABASE_URL";
```

Add the field to `AppConfig`:

```rust
pub struct AppConfig {
    pub host: IpAddr,
    pub port: u16,
    pub static_assets_dir: PathBuf,
    pub database_url: String,
}
```

In `from_env`, after `static_assets_dir` is computed, add:

```rust
        let database_url = read_optional(DATABASE_URL_ENV)?
            .unwrap_or_else(|| DEFAULT_DATABASE_URL.to_owned());
```

and include `database_url` in the returned struct. Add a `database_url(&self) -> &str` accessor mirroring `static_assets_dir`, and set `database_url: DEFAULT_DATABASE_URL.to_owned()` in the `Default` impl. Update the existing `default_config_*` test to assert `config.database_url == DEFAULT_DATABASE_URL`, and add `(DATABASE_URL_ENV, None)` to each `TestEnv::new(&[...])` array in the config tests so the env is controlled.

- [ ] **Step 8: Refactor the router to take `AppState`**

Replace the top of `backend/src/api/mod.rs` (imports, `router`, `router_with_static_assets`, `api_routes`) with:

```rust
mod cors;
mod health;
mod root;
mod sharesight;

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use std::{path::Path, sync::Arc};
use tower_http::services::ServeDir;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root::handler))
        .nest("/api", api_router())
        .layer(cors::layer())
        .with_state(state)
}

pub fn router_with_static_assets(static_assets_dir: impl AsRef<Path>, state: AppState) -> Router {
    let static_assets_dir = static_assets_dir.as_ref();
    let static_assets = StaticAssets {
        index_path: Arc::from(static_assets_dir.join("index.html").into_boxed_path()),
    };

    Router::new()
        .nest("/api", api_router())
        .fallback_service(
            ServeDir::new(static_assets_dir)
                .fallback(get(static_index).with_state(static_assets)),
        )
        .layer(cors::layer())
        .with_state(state)
}

fn api_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health::handler))
        .route(
            "/import/sharesight/schema-preview",
            get(sharesight::handler),
        )
}
```

`StaticAssets` and `static_index` stay unchanged. Note `static_index` already carries its own `State<StaticAssets>` via `.with_state(static_assets)` on the inner service, so it is independent of `AppState`. (Task 3 adds the ledger routes to `api_router`.)

- [ ] **Step 9: Update the existing router tests to pass state**

These tests currently call `crate::api::router()` / `router_with_static_assets(path)` with no state:
- `backend/src/api/mod.rs` tests (`static_router_*`): change each `router_with_static_assets(fixture.path())` to `router_with_static_assets(fixture.path(), AppState::for_tests().await)`. Add `use crate::state::AppState;` to the test module. Each test fn is already `async`.
- `backend/src/api/health.rs` test: change `crate::api::router()` to `crate::api::router(crate::state::AppState::for_tests().await)`.
- `backend/src/api/sharesight.rs` test: same change.

- [ ] **Step 10: Wire the pool at startup**

In `backend/src/app.rs`, change `serve` to build the pool from config and thread state:

```rust
use crate::config::AppConfig;
use crate::state::AppState;

pub async fn serve(config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let pool = crate::db::connect(config.database_url()).await?;
    crate::engine_info!("database ready at {}", config.database_url());
    let state = AppState::new(pool);

    let address = config.socket_addr();
    let router = if config.static_assets_dir().is_dir() {
        crate::engine_info!(
            "serving frontend assets from {}",
            config.static_assets_dir().display()
        );
        crate::api::router_with_static_assets(config.static_assets_dir(), state)
    } else {
        crate::engine_warn!(
            "frontend assets not found at {}; serving backend routes only",
            config.static_assets_dir().display()
        );
        crate::api::router(state)
    };

    let listener = tokio::net::TcpListener::bind(address).await?;
    let local_address = listener.local_addr()?;
    crate::engine_info!("backend listening on {local_address}");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    crate::engine_info!("backend shutdown complete");
    Ok(())
}
```

Keep `shutdown_signal` unchanged. In `backend/src/main.rs` add `mod state;` to the module list (keep it a thin wrapper otherwise).

- [ ] **Step 11: Ignore the dev database file**

Append to `.gitignore`:

```gitignore
# Local development SQLite database
/backend/tttb-ledger.sqlite
/backend/tttb-ledger.sqlite-shm
/backend/tttb-ledger.sqlite-wal
```

- [ ] **Step 12: Write the migration/constraint test**

Add this test module to `backend/src/db/pool.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::db::testing::memory_pool;
    use sqlx::Row;

    #[tokio::test]
    async fn migrations_create_expected_tables() {
        let pool = memory_pool().await;

        let tables: Vec<String> =
            sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
                .fetch_all(&pool)
                .await
                .expect("query tables")
                .into_iter()
                .map(|row| row.get::<String, _>("name"))
                .collect();

        for expected in ["instruments", "import_batches", "transactions"] {
            assert!(tables.contains(&expected.to_owned()), "missing {expected}");
        }
    }

    #[tokio::test]
    async fn instrument_exchange_symbol_is_unique() {
        let pool = memory_pool().await;
        let insert = "INSERT INTO instruments (symbol, exchange, name, type, currency) \
                      VALUES ('MSFT', 'NASDAQ', 'Microsoft', 'STOCK', 'USD')";

        sqlx::query(insert).execute(&pool).await.expect("first insert");
        let duplicate = sqlx::query(insert).execute(&pool).await;

        assert!(duplicate.is_err(), "duplicate (exchange, symbol) must be rejected");
    }

    #[tokio::test]
    async fn instrument_type_check_rejects_unknown_kind() {
        let pool = memory_pool().await;
        let result = sqlx::query(
            "INSERT INTO instruments (symbol, exchange, name, type, currency) \
             VALUES ('BTC', 'X', 'Bitcoin', 'CRYPTO', 'USD')",
        )
        .execute(&pool)
        .await;

        assert!(result.is_err(), "unknown instrument type must be rejected");
    }

    #[tokio::test]
    async fn transaction_type_check_allows_dividend_but_rejects_unknown() {
        let pool = memory_pool().await;
        sqlx::query(
            "INSERT INTO instruments (id, symbol, exchange, name, type, currency) \
             VALUES (1, 'MSFT', 'NASDAQ', 'Microsoft', 'STOCK', 'USD')",
        )
        .execute(&pool)
        .await
        .expect("instrument insert");

        let dividend = sqlx::query(
            "INSERT INTO transactions (instrument_id, type, trade_date, quantity) \
             VALUES (1, 'DIVIDEND', '2026-06-12', 0)",
        )
        .execute(&pool)
        .await;
        assert!(dividend.is_ok(), "DIVIDEND is permitted by the CHECK constraint");

        let bogus = sqlx::query(
            "INSERT INTO transactions (instrument_id, type, trade_date, quantity) \
             VALUES (1, 'GIFT', '2026-06-12', 0)",
        )
        .execute(&pool)
        .await;
        assert!(bogus.is_err(), "unknown transaction type must be rejected");
    }

    #[tokio::test]
    async fn foreign_keys_are_enforced() {
        let pool = memory_pool().await;
        let orphan = sqlx::query(
            "INSERT INTO transactions (instrument_id, type, trade_date, quantity) \
             VALUES (999, 'BUY', '2026-06-12', 1)",
        )
        .execute(&pool)
        .await;

        assert!(orphan.is_err(), "transaction must reference an existing instrument");
    }
}
```

- [ ] **Step 13: Run, lint, format, commit**

Run: `cd backend && cargo test`
Expected: all tests pass, including the five new `db::pool::tests`.
Run: `cargo clippy --all-targets -- -D warnings` then `cargo fmt`.
Add an `EngineeringDiary.md` entry ("SQLite ledger schema + connection wiring"). Then:

```bash
git add backend/Cargo.toml backend/Cargo.lock backend/migrations backend/src .gitignore EngineeringDiary.md
```

---

## Task 2 — Pure domain ledger

Pure, deterministic, I/O-free types + derivation. No axum/sqlx/HTTP. This is the source of truth for cost-basis math and validation.

**Files:**
- Modify: `backend/src/domain/mod.rs`
- Create: `backend/src/domain/transaction.rs`, `backend/src/domain/position.rs`

- [ ] **Step 1: Define `domain/mod.rs` (thin)**

Replace the (empty) `backend/src/domain/mod.rs` with:

```rust
//! Pure ledger domain: transaction kinds, validation, and position derivation.
//! Contains no axum, sqlx, HTTP, or provider types and performs no I/O.

mod position;
mod transaction;

pub use position::{derive_position, BaseCostBasis, Position, UnavailableReason};
pub use transaction::{
    validate, LedgerError, LedgerTransaction, ProposedTransaction, TransactionKind, ValidationError,
};
```

- [ ] **Step 2: Write failing tests for transaction types + stateless validation**

Create `backend/src/domain/transaction.rs` with the test module first (it will not compile until Step 3 adds the types — that is the failing state):

```rust
#[cfg(test)]
mod tests {
    use super::{validate, ProposedTransaction, TransactionKind, ValidationError};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 12).expect("valid date")
    }

    fn buy() -> ProposedTransaction {
        ProposedTransaction {
            kind: TransactionKind::Buy,
            trade_date: date(),
            quantity: 10,
            price: Some(dec!(12.50)),
            currency: Some("USD".to_owned()),
            fx_rate_to_base: Some(dec!(10.0)),
            brokerage_base: Some(dec!(9.60)),
        }
    }

    #[test]
    fn buy_returns_positive_signed_quantity() {
        assert_eq!(validate(&buy()), Ok(10));
    }

    #[test]
    fn sell_returns_negative_signed_quantity() {
        let sell = ProposedTransaction {
            kind: TransactionKind::Sell,
            ..buy()
        };
        assert_eq!(validate(&sell), Ok(-10));
    }

    #[test]
    fn buy_without_price_is_rejected() {
        let proposed = ProposedTransaction {
            price: None,
            ..buy()
        };
        assert_eq!(validate(&proposed), Err(ValidationError::PriceRequired));
    }

    #[test]
    fn buy_without_currency_is_rejected() {
        let proposed = ProposedTransaction {
            currency: None,
            ..buy()
        };
        assert_eq!(validate(&proposed), Err(ValidationError::CurrencyRequired));
    }

    #[test]
    fn buy_with_non_positive_quantity_is_rejected() {
        let proposed = ProposedTransaction {
            quantity: 0,
            ..buy()
        };
        assert_eq!(
            validate(&proposed),
            Err(ValidationError::QuantityMustBePositive)
        );
    }

    #[test]
    fn buy_with_non_positive_price_is_rejected() {
        let zero = ProposedTransaction {
            price: Some(dec!(0)),
            ..buy()
        };
        assert_eq!(validate(&zero), Err(ValidationError::PriceMustBePositive));

        let negative = ProposedTransaction {
            price: Some(dec!(-1)),
            ..buy()
        };
        assert_eq!(
            validate(&negative),
            Err(ValidationError::PriceMustBePositive)
        );
    }

    #[test]
    fn buy_with_non_positive_fx_is_rejected() {
        let proposed = ProposedTransaction {
            fx_rate_to_base: Some(dec!(0)),
            ..buy()
        };
        assert_eq!(
            validate(&proposed),
            Err(ValidationError::FxRateMustBePositive)
        );
    }

    #[test]
    fn buy_with_negative_brokerage_is_rejected() {
        let proposed = ProposedTransaction {
            brokerage_base: Some(dec!(-0.01)),
            ..buy()
        };
        assert_eq!(
            validate(&proposed),
            Err(ValidationError::BrokerageMustNotBeNegative)
        );
    }

    #[test]
    fn split_returns_signed_delta() {
        let split = ProposedTransaction {
            kind: TransactionKind::Split,
            quantity: 8,
            price: None,
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
            ..buy()
        };
        assert_eq!(validate(&split), Ok(8));
    }

    #[test]
    fn split_with_zero_quantity_is_rejected() {
        let split = ProposedTransaction {
            kind: TransactionKind::Split,
            quantity: 0,
            price: None,
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
            ..buy()
        };
        assert_eq!(
            validate(&split),
            Err(ValidationError::SplitQuantityMustBeNonZero)
        );
    }

    #[test]
    fn split_carrying_cost_inputs_is_rejected() {
        let split = ProposedTransaction {
            kind: TransactionKind::Split,
            quantity: 8,
            price: Some(dec!(1.0)),
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
            ..buy()
        };
        assert_eq!(
            validate(&split),
            Err(ValidationError::SplitMustNotCarryCostInputs)
        );
    }

    #[test]
    fn dividend_is_not_supported() {
        let dividend = ProposedTransaction {
            kind: TransactionKind::Dividend,
            ..buy()
        };
        assert_eq!(validate(&dividend), Err(ValidationError::DividendNotSupported));
    }

    #[test]
    fn db_string_round_trips() {
        for kind in [
            TransactionKind::Buy,
            TransactionKind::Sell,
            TransactionKind::Split,
            TransactionKind::Dividend,
        ] {
            assert_eq!(TransactionKind::from_db_str(kind.as_db_str()), Some(kind));
        }
        assert_eq!(TransactionKind::from_db_str("GIFT"), None);
    }
}
```

- [ ] **Step 3: Run to confirm failure**

Run: `cd backend && cargo test --lib domain::transaction`
Expected: FAIL — `cannot find ... TransactionKind/validate/...` (types not yet defined).

- [ ] **Step 4: Implement the transaction types + validation**

Prepend to `backend/src/domain/transaction.rs` (above the test module):

```rust
use chrono::NaiveDate;
use rust_decimal::Decimal;

/// The kind of a ledger transaction. PascalCase on the wire, UPPERCASE in the DB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionKind {
    Buy,
    Sell,
    Split,
    Dividend,
}

impl TransactionKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
            Self::Split => "SPLIT",
            Self::Dividend => "DIVIDEND",
        }
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "BUY" => Some(Self::Buy),
            "SELL" => Some(Self::Sell),
            "SPLIT" => Some(Self::Split),
            "DIVIDEND" => Some(Self::Dividend),
            _ => None,
        }
    }
}

/// A client-proposed transaction before stateless validation. `quantity` is the
/// positive magnitude for Buy/Sell and a signed non-zero delta for Split.
#[derive(Clone, Debug, PartialEq)]
pub struct ProposedTransaction {
    pub kind: TransactionKind,
    pub trade_date: NaiveDate,
    pub quantity: i64,
    pub price: Option<Decimal>,
    pub currency: Option<String>,
    pub fx_rate_to_base: Option<Decimal>,
    pub brokerage_base: Option<Decimal>,
}

/// A validated ledger row used as pure input to derivation. `quantity` is the
/// signed position effect (Buy > 0, Sell < 0, Split = signed delta).
#[derive(Clone, Debug, PartialEq)]
pub struct LedgerTransaction {
    pub id: i64,
    pub trade_date: NaiveDate,
    pub kind: TransactionKind,
    pub quantity: i64,
    pub price: Option<Decimal>,
    pub fx_rate_to_base: Option<Decimal>,
    pub brokerage_base: Decimal,
}

/// Stateless, field-level validation errors. Each maps to a stable API code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    QuantityMustBePositive,
    PriceRequired,
    PriceMustBePositive,
    CurrencyRequired,
    FxRateMustBePositive,
    BrokerageMustNotBeNegative,
    SplitQuantityMustBeNonZero,
    SplitMustNotCarryCostInputs,
    DividendNotSupported,
}

impl ValidationError {
    pub fn code(self) -> &'static str {
        match self {
            Self::QuantityMustBePositive => "quantity_must_be_positive",
            Self::PriceRequired => "price_required",
            Self::PriceMustBePositive => "price_must_be_positive",
            Self::CurrencyRequired => "currency_required",
            Self::FxRateMustBePositive => "fx_rate_must_be_positive",
            Self::BrokerageMustNotBeNegative => "brokerage_must_not_be_negative",
            Self::SplitQuantityMustBeNonZero => "split_quantity_must_be_non_zero",
            Self::SplitMustNotCarryCostInputs => "split_must_not_carry_cost_inputs",
            Self::DividendNotSupported => "dividend_not_supported",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::QuantityMustBePositive => "Buy and Sell quantity must be a positive integer.",
            Self::PriceRequired => "Buy and Sell require a native price.",
            Self::PriceMustBePositive => "Buy and Sell price must be greater than zero.",
            Self::CurrencyRequired => "Buy and Sell require a native currency.",
            Self::FxRateMustBePositive => "FX rate to base must be greater than zero when present.",
            Self::BrokerageMustNotBeNegative => "Brokerage must not be negative when present.",
            Self::SplitQuantityMustBeNonZero => "Split requires a non-zero quantity delta.",
            Self::SplitMustNotCarryCostInputs => {
                "Split must not carry price, currency, FX, or brokerage."
            }
            Self::DividendNotSupported => "Dividend transactions are not supported yet.",
        }
    }
}

/// Stateful derivation errors (depend on prior ledger state). Each maps to a code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerError {
    SellExceedsPosition {
        transaction_id: i64,
        available: i64,
        requested: i64,
    },
    SplitWithoutPosition {
        transaction_id: i64,
    },
    SplitDrivesNonPositive {
        transaction_id: i64,
        resulting_quantity: i64,
    },
    BuyMissingPrice {
        transaction_id: i64,
    },
}

impl LedgerError {
    pub fn code(self) -> &'static str {
        match self {
            Self::SellExceedsPosition { .. } => "sell_exceeds_position",
            Self::SplitWithoutPosition { .. } => "split_without_position",
            Self::SplitDrivesNonPositive { .. } => "split_drives_non_positive",
            Self::BuyMissingPrice { .. } => "buy_missing_price",
        }
    }

    pub fn transaction_id(self) -> i64 {
        match self {
            Self::SellExceedsPosition { transaction_id, .. }
            | Self::SplitWithoutPosition { transaction_id }
            | Self::SplitDrivesNonPositive { transaction_id, .. }
            | Self::BuyMissingPrice { transaction_id } => transaction_id,
        }
    }
}

/// Validate a proposed transaction's type-specific field rules. On success
/// returns the **signed** position effect for `quantity`.
pub fn validate(proposed: &ProposedTransaction) -> Result<i64, ValidationError> {
    match proposed.kind {
        TransactionKind::Buy | TransactionKind::Sell => {
            if proposed.quantity <= 0 {
                return Err(ValidationError::QuantityMustBePositive);
            }
            match proposed.price {
                None => return Err(ValidationError::PriceRequired),
                Some(price) if price <= Decimal::ZERO => {
                    return Err(ValidationError::PriceMustBePositive);
                }
                Some(_) => {}
            }
            if proposed.currency.as_deref().unwrap_or("").trim().is_empty() {
                return Err(ValidationError::CurrencyRequired);
            }
            if proposed.fx_rate_to_base.is_some_and(|fx| fx <= Decimal::ZERO) {
                return Err(ValidationError::FxRateMustBePositive);
            }
            if proposed.brokerage_base.is_some_and(|fee| fee < Decimal::ZERO) {
                return Err(ValidationError::BrokerageMustNotBeNegative);
            }
            Ok(match proposed.kind {
                TransactionKind::Sell => -proposed.quantity,
                _ => proposed.quantity,
            })
        }
        TransactionKind::Split => {
            if proposed.quantity == 0 {
                return Err(ValidationError::SplitQuantityMustBeNonZero);
            }
            if proposed.price.is_some()
                || proposed.currency.is_some()
                || proposed.fx_rate_to_base.is_some()
                || proposed.brokerage_base.is_some()
            {
                return Err(ValidationError::SplitMustNotCarryCostInputs);
            }
            Ok(proposed.quantity)
        }
        TransactionKind::Dividend => Err(ValidationError::DividendNotSupported),
    }
}
```

- [ ] **Step 5: Run to confirm pass**

Run: `cd backend && cargo test --lib domain::transaction`
Expected: PASS (all Step 2 tests green).

- [ ] **Step 6: Write failing tests for position derivation**

Create `backend/src/domain/position.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::{derive_position, BaseCostBasis, UnavailableReason};
    use crate::domain::transaction::{LedgerError, LedgerTransaction, TransactionKind};
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::str::FromStr;

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    fn buy(id: i64, date: NaiveDate, qty: i64, price: Decimal, fx: Option<Decimal>) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Buy,
            quantity: qty,
            price: Some(price),
            fx_rate_to_base: fx,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn sell(id: i64, date: NaiveDate, qty: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Sell,
            quantity: -qty,
            price: Some(dec!(1)),
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn split(id: i64, date: NaiveDate, delta: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Split,
            quantity: delta,
            price: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    #[test]
    fn single_buy_sets_native_and_base_cost() {
        // Synthetic vector from docs/CurrencyAndFxRules.md.
        let mut tx = buy(1, d(2026, 6, 12), 10, dec!(12.50), Some(dec!(10.0)));
        tx.brokerage_base = dec!(9.60);

        let position = derive_position(&[tx]).expect("derives");

        assert_eq!(position.quantity, 10);
        assert_eq!(position.cost_basis_native, dec!(125.00));
        assert_eq!(position.average_cost_native(), Some(dec!(12.50)));
        match position.base {
            BaseCostBasis::Available {
                cost_basis_base,
                fee_component_base,
            } => {
                assert_eq!(cost_basis_base, dec!(1259.60));
                assert_eq!(fee_component_base, dec!(9.60));
            }
            BaseCostBasis::Unavailable { .. } => panic!("base should be available"),
        }
    }

    #[test]
    fn weighted_average_blends_two_buys() {
        let first = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));
        let second = buy(2, d(2026, 6, 2), 10, dec!(200), Some(dec!(1)));

        let position = derive_position(&[first, second]).expect("derives");

        assert_eq!(position.quantity, 20);
        assert_eq!(position.average_cost_native(), Some(dec!(150)));
    }

    #[test]
    fn sell_keeps_average_and_reduces_components() {
        let first = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(2)));
        let part = sell(2, d(2026, 6, 2), 4);

        let position = derive_position(&[first, part]).expect("derives");

        assert_eq!(position.quantity, 6);
        assert_eq!(position.average_cost_native(), Some(dec!(100)));
        // cost_basis_base = 10*100*2 = 2000; after selling 4/10 -> 1200.
        assert_eq!(position.average_cost_base(), Some(dec!(200)));
    }

    #[test]
    fn same_day_buy_then_sell_orders_by_id() {
        let b = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));
        let s = sell(2, d(2026, 6, 1), 4);

        let position = derive_position(&[b, s]).expect("derives");
        assert_eq!(position.quantity, 6);
    }

    #[test]
    fn same_day_sell_before_buy_is_rejected() {
        // Sell has the lower id, so it is ordered first and has no shares to sell.
        let s = sell(1, d(2026, 6, 1), 4);
        let b = buy(2, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));

        let error = derive_position(&[s, b]).expect_err("sell before any buy fails");
        assert!(matches!(error, LedgerError::SellExceedsPosition { .. }));
    }

    #[test]
    fn sell_below_zero_is_rejected() {
        let b = buy(1, d(2026, 6, 1), 3, dec!(100), Some(dec!(1)));
        let s = sell(2, d(2026, 6, 2), 4);

        let error = derive_position(&[b, s]).expect_err("oversell fails");
        assert_eq!(
            error,
            LedgerError::SellExceedsPosition {
                transaction_id: 2,
                available: 3,
                requested: 4,
            }
        );
    }

    #[test]
    fn missing_fx_makes_base_unavailable_but_native_stays() {
        let known = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));
        let unknown = buy(2, d(2026, 6, 2), 10, dec!(200), None);

        let position = derive_position(&[known, unknown]).expect("derives");

        assert_eq!(position.quantity, 20);
        assert_eq!(position.average_cost_native(), Some(dec!(150)));
        assert_eq!(position.average_cost_base(), None);
        match position.base {
            BaseCostBasis::Unavailable { reasons } => {
                assert_eq!(reasons, vec![UnavailableReason::MissingFx { transaction_id: 2 }]);
            }
            BaseCostBasis::Available { .. } => panic!("base should be unavailable"),
        }
    }

    #[test]
    fn closing_and_reopening_recovers_base_availability() {
        let contaminated = buy(1, d(2026, 6, 1), 10, dec!(100), None);
        let close = sell(2, d(2026, 6, 2), 10);
        let reopen = buy(3, d(2026, 6, 3), 5, dec!(100), Some(dec!(2)));

        let position = derive_position(&[contaminated, close, reopen]).expect("derives");

        assert_eq!(position.quantity, 5);
        assert_eq!(position.average_cost_base(), Some(dec!(200)));
    }

    #[test]
    fn split_rescales_average_without_changing_cost_basis() {
        let b = buy(1, d(2026, 6, 1), 10, dec!(120), Some(dec!(1)));
        let s = split(2, d(2026, 6, 2), 10); // 2-for-1: +10 shares

        let position = derive_position(&[b, s]).expect("derives");

        assert_eq!(position.quantity, 20);
        assert_eq!(position.cost_basis_native, dec!(1200));
        assert_eq!(position.average_cost_native(), Some(dec!(60)));
    }

    #[test]
    fn split_without_position_is_rejected() {
        let s = split(1, d(2026, 6, 1), 10);
        let error = derive_position(&[s]).expect_err("split needs a position");
        assert_eq!(error, LedgerError::SplitWithoutPosition { transaction_id: 1 });
    }

    #[test]
    fn split_driving_non_positive_is_rejected() {
        let b = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));
        let s = split(2, d(2026, 6, 2), -10);
        let error = derive_position(&[b, s]).expect_err("split to zero fails");
        assert_eq!(
            error,
            LedgerError::SplitDrivesNonPositive {
                transaction_id: 2,
                resulting_quantity: 0,
            }
        );
    }

    #[test]
    fn dividend_row_is_a_position_no_op() {
        let b = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));
        let dividend = LedgerTransaction {
            id: 2,
            trade_date: d(2026, 6, 2),
            kind: TransactionKind::Dividend,
            quantity: 0,
            price: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        };

        let position = derive_position(&[b, dividend]).expect("derives");
        assert_eq!(position.quantity, 10);
    }

    #[test]
    fn average_base_handles_repeating_decimal() {
        // Guard against panics from non-terminating division.
        let b = buy(1, d(2026, 6, 1), 3, dec!(10), Some(dec!(1)));
        let position = derive_position(&[b]).expect("derives");
        let avg = position.average_cost_base().expect("available");
        assert!(avg > Decimal::from_str("3.33").unwrap());
    }
}
```

- [ ] **Step 7: Run to confirm failure**

Run: `cd backend && cargo test --lib domain::position`
Expected: FAIL — `derive_position`/`Position`/`BaseCostBasis` not defined.

- [ ] **Step 8: Implement position derivation**

Prepend to `backend/src/domain/position.rs` (above the test module):

```rust
use rust_decimal::Decimal;

use crate::domain::transaction::{LedgerError, LedgerTransaction, TransactionKind};

/// A derived open position for a single instrument.
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    pub quantity: i64,
    /// Σ native gross of the open shares, in the instrument's currency.
    pub cost_basis_native: Decimal,
    pub base: BaseCostBasis,
}

/// SEK cost-basis state. Available only while every contributing buy had FX.
#[derive(Clone, Debug, PartialEq)]
pub enum BaseCostBasis {
    Available {
        cost_basis_base: Decimal,
        fee_component_base: Decimal,
    },
    Unavailable {
        reasons: Vec<UnavailableReason>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    MissingFx { transaction_id: i64 },
}

impl Position {
    fn empty() -> Self {
        Self {
            quantity: 0,
            cost_basis_native: Decimal::ZERO,
            base: BaseCostBasis::Available {
                cost_basis_base: Decimal::ZERO,
                fee_component_base: Decimal::ZERO,
            },
        }
    }

    pub fn average_cost_native(&self) -> Option<Decimal> {
        if self.quantity > 0 {
            Some(self.cost_basis_native / Decimal::from(self.quantity))
        } else {
            None
        }
    }

    pub fn average_cost_base(&self) -> Option<Decimal> {
        match &self.base {
            BaseCostBasis::Available {
                cost_basis_base, ..
            } if self.quantity > 0 => Some(*cost_basis_base / Decimal::from(self.quantity)),
            _ => None,
        }
    }
}

/// Derive a position by folding `(trade_date, id)`-ordered transactions.
/// Callers must pass transactions already sorted by `(trade_date, id)`.
pub fn derive_position(transactions: &[LedgerTransaction]) -> Result<Position, LedgerError> {
    let mut position = Position::empty();
    for tx in transactions {
        apply(&mut position, tx)?;
    }
    Ok(position)
}

fn apply(position: &mut Position, tx: &LedgerTransaction) -> Result<(), LedgerError> {
    match tx.kind {
        TransactionKind::Buy => apply_buy(position, tx),
        TransactionKind::Sell => apply_sell(position, tx),
        TransactionKind::Split => apply_split(position, tx),
        TransactionKind::Dividend => Ok(()),
    }
}

fn apply_buy(position: &mut Position, tx: &LedgerTransaction) -> Result<(), LedgerError> {
    let price = tx.price.ok_or(LedgerError::BuyMissingPrice {
        transaction_id: tx.id,
    })?;
    let native_gross = price * Decimal::from(tx.quantity);
    position.cost_basis_native += native_gross;

    match (&mut position.base, tx.fx_rate_to_base) {
        (
            BaseCostBasis::Available {
                cost_basis_base,
                fee_component_base,
            },
            Some(fx),
        ) => {
            *cost_basis_base += native_gross * fx + tx.brokerage_base;
            *fee_component_base += tx.brokerage_base;
        }
        (BaseCostBasis::Available { .. }, None) => {
            position.base = BaseCostBasis::Unavailable {
                reasons: vec![UnavailableReason::MissingFx {
                    transaction_id: tx.id,
                }],
            };
        }
        (BaseCostBasis::Unavailable { reasons }, fx) => {
            if fx.is_none() {
                reasons.push(UnavailableReason::MissingFx {
                    transaction_id: tx.id,
                });
            }
        }
    }

    position.quantity += tx.quantity;
    Ok(())
}

fn apply_sell(position: &mut Position, tx: &LedgerTransaction) -> Result<(), LedgerError> {
    let sell_qty = -tx.quantity;
    if sell_qty > position.quantity {
        return Err(LedgerError::SellExceedsPosition {
            transaction_id: tx.id,
            available: position.quantity,
            requested: sell_qty,
        });
    }

    let remaining = position.quantity - sell_qty;
    if remaining == 0 {
        *position = Position::empty();
        return Ok(());
    }

    let ratio = Decimal::from(remaining) / Decimal::from(position.quantity);
    position.cost_basis_native *= ratio;
    if let BaseCostBasis::Available {
        cost_basis_base,
        fee_component_base,
    } = &mut position.base
    {
        *cost_basis_base *= ratio;
        *fee_component_base *= ratio;
    }
    position.quantity = remaining;
    Ok(())
}

fn apply_split(position: &mut Position, tx: &LedgerTransaction) -> Result<(), LedgerError> {
    if position.quantity == 0 {
        return Err(LedgerError::SplitWithoutPosition {
            transaction_id: tx.id,
        });
    }
    let resulting = position.quantity + tx.quantity;
    if resulting <= 0 {
        return Err(LedgerError::SplitDrivesNonPositive {
            transaction_id: tx.id,
            resulting_quantity: resulting,
        });
    }
    position.quantity = resulting;
    Ok(())
}
```

- [ ] **Step 9: Run to confirm pass**

Run: `cd backend && cargo test --lib domain`
Expected: PASS (all transaction + position tests).

- [ ] **Step 10: Lint, format, commit**

Run: `cargo clippy --all-targets -- -D warnings` then `cargo fmt`. Add an `EngineeringDiary.md` entry ("pure ledger domain: weighted-average derivation, missing-FX propagation, split-as-delta, field-range validation"). Then:

```bash
git add backend/src/domain EngineeringDiary.md
```

---

## Task 3 — Transaction & instrument CRUD API

Repositories (all SQL lives here), the shared `ApiError`, DTOs, handlers, and route wiring. Stateful validation reuses Task 2's `derive_position` via the ledger-validity invariant (decision 6).

**Files:**
- Modify: `backend/Cargo.toml` (move `serde_json` to runtime), `backend/src/db/mod.rs`, `backend/src/api/mod.rs`
- Create: `backend/src/db/instruments.rs`, `backend/src/db/transactions.rs`
- Create: `backend/src/api/error.rs`, `backend/src/api/instruments.rs`, `backend/src/api/transactions.rs`

- [ ] **Step 1: Make `serde_json` a runtime dependency**

In `backend/Cargo.toml`, add `serde_json = "1"` to `[dependencies]` (keep alphabetical: after `serde`) and remove the `serde_json = "1"` line from `[dev-dependencies]`. Run `cd backend && cargo build` (still compiles).

- [ ] **Step 2: Add `RepoError` and re-enable repo modules in `db/mod.rs`**

Replace `backend/src/db/mod.rs` with:

```rust
mod pool;

pub mod instruments;
pub mod transactions;

#[cfg(test)]
pub mod testing;

pub use pool::connect;

/// Errors a repository can surface: a SQL/driver failure, or a failure to decode
/// stored data back into domain types (an internal invariant violation).
#[derive(Debug)]
pub enum RepoError {
    Sqlx(sqlx::Error),
    Decode(String),
}

impl std::fmt::Display for RepoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlx(error) => write!(f, "database error: {error}"),
            Self::Decode(message) => write!(f, "decode error: {message}"),
        }
    }
}

impl std::error::Error for RepoError {}

impl From<sqlx::Error> for RepoError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sqlx(error)
    }
}
```

- [ ] **Step 3: Implement the instruments repository**

Create `backend/src/db/instruments.rs`:

```rust
use sqlx::sqlite::SqlitePool;

use crate::db::RepoError;

const COLUMNS: &str = "id, symbol, exchange, name, type, currency";

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct InstrumentRow {
    pub id: i64,
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub currency: String,
}

/// Fields for creating an instrument. `kind` is the DB string (e.g. "STOCK").
#[derive(Clone, Debug)]
pub struct NewInstrument {
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    pub kind: String,
    pub currency: String,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<InstrumentRow>, RepoError> {
    let rows = sqlx::query_as::<_, InstrumentRow>(&format!(
        "SELECT {COLUMNS} FROM instruments ORDER BY exchange, symbol"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(&format!(
        "SELECT {COLUMNS} FROM instruments WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn find_by_exchange_symbol(
    pool: &SqlitePool,
    exchange: &str,
    symbol: &str,
) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(&format!(
        "SELECT {COLUMNS} FROM instruments WHERE exchange = ? AND symbol = ?"
    ))
    .bind(exchange)
    .bind(symbol)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Upsert-like on `(exchange, symbol)`: returns the existing row (`created=false`)
/// or inserts a new one (`created=true`). Existing rows are returned unchanged.
pub async fn upsert(
    pool: &SqlitePool,
    new: &NewInstrument,
) -> Result<(InstrumentRow, bool), RepoError> {
    if let Some(existing) = find_by_exchange_symbol(pool, &new.exchange, &new.symbol).await? {
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
    .fetch_one(pool)
    .await;

    match inserted {
        Ok(row) => Ok((row, true)),
        // A concurrent insert won the UNIQUE race; return the now-existing row.
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            let existing = find_by_exchange_symbol(pool, &new.exchange, &new.symbol)
                .await?
                .ok_or_else(|| {
                    RepoError::Decode("instrument vanished after unique violation".to_owned())
                })?;
            Ok((existing, false))
        }
        Err(error) => Err(RepoError::Sqlx(error)),
    }
}
```

- [ ] **Step 4: Implement the transactions repository**

Create `backend/src/db/transactions.rs`:

```rust
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::sqlite::SqlitePool;

use crate::db::RepoError;
use crate::domain::{LedgerTransaction, TransactionKind};

const COLUMNS: &str = "id, instrument_id, type, trade_date, quantity, price, currency, \
    fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, \
    note, import_batch_id";

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct TransactionRow {
    pub id: i64,
    pub instrument_id: i64,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub trade_date: String,
    pub quantity: i64,
    pub price: Option<String>,
    pub currency: Option<String>,
    pub fx_rate_to_base: Option<String>,
    pub brokerage: Option<String>,
    pub brokerage_currency: Option<String>,
    pub source_value: Option<String>,
    pub source_currency: Option<String>,
    pub note: Option<String>,
    pub import_batch_id: Option<i64>,
}

impl TransactionRow {
    /// Convert the stored row into a pure domain transaction for derivation.
    pub fn to_ledger(&self) -> Result<LedgerTransaction, RepoError> {
        let kind = TransactionKind::from_db_str(&self.kind)
            .ok_or_else(|| RepoError::Decode(format!("unknown transaction type {:?}", self.kind)))?;
        let trade_date = NaiveDate::parse_from_str(&self.trade_date, "%Y-%m-%d")
            .map_err(|e| RepoError::Decode(format!("bad trade_date {:?}: {e}", self.trade_date)))?;
        Ok(LedgerTransaction {
            id: self.id,
            trade_date,
            kind,
            quantity: self.quantity,
            price: parse_decimal(self.price.as_deref())?,
            fx_rate_to_base: parse_decimal(self.fx_rate_to_base.as_deref())?,
            brokerage_base: parse_decimal(self.brokerage.as_deref())?.unwrap_or(Decimal::ZERO),
        })
    }
}

fn parse_decimal(value: Option<&str>) -> Result<Option<Decimal>, RepoError> {
    value
        .map(|raw| {
            Decimal::from_str(raw).map_err(|e| RepoError::Decode(format!("bad decimal {raw:?}: {e}")))
        })
        .transpose()
}

/// Persistable transaction fields. `quantity` is the signed position effect.
#[derive(Clone, Debug)]
pub struct NewTransaction {
    pub instrument_id: i64,
    pub kind: TransactionKind,
    pub trade_date: NaiveDate,
    pub quantity: i64,
    pub price: Option<Decimal>,
    pub currency: Option<String>,
    pub fx_rate_to_base: Option<Decimal>,
    pub brokerage: Option<Decimal>,
    pub note: Option<String>,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<TransactionRow>, RepoError> {
    let rows = sqlx::query_as::<_, TransactionRow>(&format!(
        "SELECT {COLUMNS} FROM transactions ORDER BY trade_date DESC, id DESC"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<TransactionRow>, RepoError> {
    let row = sqlx::query_as::<_, TransactionRow>(&format!(
        "SELECT {COLUMNS} FROM transactions WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// All of one instrument's transactions as domain rows, ordered `(trade_date, id)`.
pub async fn ledger_for_instrument(
    pool: &SqlitePool,
    instrument_id: i64,
) -> Result<Vec<LedgerTransaction>, RepoError> {
    let rows = sqlx::query_as::<_, TransactionRow>(&format!(
        "SELECT {COLUMNS} FROM transactions WHERE instrument_id = ? ORDER BY trade_date, id"
    ))
    .bind(instrument_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(TransactionRow::to_ledger).collect()
}

pub async fn insert(pool: &SqlitePool, new: &NewTransaction) -> Result<TransactionRow, RepoError> {
    let row = sqlx::query_as::<_, TransactionRow>(&format!(
        "INSERT INTO transactions \
           (instrument_id, type, trade_date, quantity, price, currency, fx_rate_to_base, \
            brokerage, brokerage_currency, source_value, source_currency, note, import_batch_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?, NULL) RETURNING {COLUMNS}"
    ))
    .bind(new.instrument_id)
    .bind(new.kind.as_db_str())
    .bind(new.trade_date.format("%Y-%m-%d").to_string())
    .bind(new.quantity)
    .bind(new.price.map(|d| d.to_string()))
    .bind(new.currency.clone())
    .bind(new.fx_rate_to_base.map(|d| d.to_string()))
    .bind(new.brokerage.map(|d| d.to_string()))
    .bind(new.brokerage.map(|_| "SEK"))
    .bind(new.note.clone())
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Full replacement of the editable fields. Audit/import columns are left intact.
pub async fn replace(
    pool: &SqlitePool,
    id: i64,
    new: &NewTransaction,
) -> Result<TransactionRow, RepoError> {
    let row = sqlx::query_as::<_, TransactionRow>(&format!(
        "UPDATE transactions SET instrument_id = ?, type = ?, trade_date = ?, quantity = ?, \
           price = ?, currency = ?, fx_rate_to_base = ?, brokerage = ?, brokerage_currency = ?, \
           note = ? WHERE id = ? RETURNING {COLUMNS}"
    ))
    .bind(new.instrument_id)
    .bind(new.kind.as_db_str())
    .bind(new.trade_date.format("%Y-%m-%d").to_string())
    .bind(new.quantity)
    .bind(new.price.map(|d| d.to_string()))
    .bind(new.currency.clone())
    .bind(new.fx_rate_to_base.map(|d| d.to_string()))
    .bind(new.brokerage.map(|d| d.to_string()))
    .bind(new.brokerage.map(|_| "SEK"))
    .bind(new.note.clone())
    .bind(id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<u64, RepoError> {
    let result = sqlx::query("DELETE FROM transactions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 5: Implement the shared `ApiError`**

Create `backend/src/api/error.rs`:

```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::Value;

use crate::db::RepoError;
use crate::domain::{LedgerError, ValidationError};

/// One error shape for the whole API: `{ "error": { code, message, details? } }`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Option<Value>,
}

#[derive(Serialize)]
struct ApiErrorBody {
    error: ApiErrorPayload,
}

#[derive(Serialize)]
struct ApiErrorPayload {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn not_found(resource: &str, id: i64) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("{resource} {id} not found"),
        )
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            message,
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            crate::engine_error!("api error [{}]: {}", self.code, self.message);
        }
        let body = ApiErrorBody {
            error: ApiErrorPayload {
                code: self.code,
                message: self.message,
                details: self.details,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

impl From<ValidationError> for ApiError {
    fn from(error: ValidationError) -> Self {
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.code(), error.message())
    }
}

impl From<LedgerError> for ApiError {
    fn from(error: LedgerError) -> Self {
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.code(), ledger_message(error))
            .with_details(serde_json::json!({ "transaction_id": error.transaction_id() }))
    }
}

impl From<RepoError> for ApiError {
    fn from(error: RepoError) -> Self {
        ApiError::internal(error.to_string())
    }
}

fn ledger_message(error: LedgerError) -> String {
    match error {
        LedgerError::SellExceedsPosition {
            available,
            requested,
            ..
        } => format!("Sell of {requested} exceeds the available position of {available}."),
        LedgerError::SplitWithoutPosition { .. } => {
            "A split requires an existing position.".to_owned()
        }
        LedgerError::SplitDrivesNonPositive {
            resulting_quantity, ..
        } => format!("Split would drive the position to {resulting_quantity} (must stay positive)."),
        LedgerError::BuyMissingPrice { .. } => "A buy requires a native price.".to_owned(),
    }
}
```

- [ ] **Step 6: Implement the instruments handlers + DTOs**

Create `backend/src/api/instruments.rs`:

```rust
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::db::instruments::{self, InstrumentRow, NewInstrument};
use crate::state::AppState;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstrumentKindDto {
    Stock,
    Etf,
    Fund,
}

impl InstrumentKindDto {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Stock => "STOCK",
            Self::Etf => "ETF",
            Self::Fund => "FUND",
        }
    }

    fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "STOCK" => Some(Self::Stock),
            "ETF" => Some(Self::Etf),
            "FUND" => Some(Self::Fund),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateInstrument {
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: InstrumentKindDto,
    pub currency: String,
}

#[derive(Debug, Serialize)]
pub struct InstrumentResponse {
    pub id: i64,
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: InstrumentKindDto,
    pub currency: String,
}

impl InstrumentResponse {
    pub fn from_row(row: &InstrumentRow) -> Result<Self, ApiError> {
        let kind = InstrumentKindDto::from_db_str(&row.kind)
            .ok_or_else(|| ApiError::internal(format!("stored unknown instrument type {:?}", row.kind)))?;
        Ok(Self {
            id: row.id,
            symbol: row.symbol.clone(),
            exchange: row.exchange.clone(),
            name: row.name.clone(),
            kind,
            currency: row.currency.clone(),
        })
    }
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<InstrumentResponse>>, ApiError> {
    let rows = instruments::list(&state.pool).await?;
    let body = rows
        .iter()
        .map(InstrumentResponse::from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(body))
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateInstrument>,
) -> Result<impl IntoResponse, ApiError> {
    let new = NewInstrument {
        symbol: body.symbol.trim().to_owned(),
        exchange: body.exchange.trim().to_owned(),
        name: body.name.trim().to_owned(),
        kind: body.kind.as_db_str().to_owned(),
        currency: body.currency.trim().to_owned(),
    };
    if new.symbol.is_empty() || new.exchange.is_empty() || new.name.is_empty() || new.currency.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_instrument",
            "symbol, exchange, name, and currency are required",
        ));
    }

    let (row, created) = instruments::upsert(&state.pool, &new).await?;
    let status = if created { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, Json(InstrumentResponse::from_row(&row)?)))
}
```

- [ ] **Step 7: Implement the transactions handlers + DTOs**

Create `backend/src/api/transactions.rs`:

```rust
use std::str::FromStr;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;

use crate::api::error::ApiError;
use crate::db::instruments::{self, InstrumentRow};
use crate::db::transactions::{self, NewTransaction, TransactionRow};
use crate::db::RepoError;
use crate::domain::{self, LedgerTransaction, ProposedTransaction, TransactionKind};
use crate::state::AppState;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionKindDto {
    Buy,
    Sell,
    Split,
    Dividend,
}

impl From<TransactionKindDto> for TransactionKind {
    fn from(value: TransactionKindDto) -> Self {
        match value {
            TransactionKindDto::Buy => TransactionKind::Buy,
            TransactionKindDto::Sell => TransactionKind::Sell,
            TransactionKindDto::Split => TransactionKind::Split,
            TransactionKindDto::Dividend => TransactionKind::Dividend,
        }
    }
}

impl From<TransactionKind> for TransactionKindDto {
    fn from(value: TransactionKind) -> Self {
        match value {
            TransactionKind::Buy => TransactionKindDto::Buy,
            TransactionKind::Sell => TransactionKindDto::Sell,
            TransactionKind::Split => TransactionKindDto::Split,
            TransactionKind::Dividend => TransactionKindDto::Dividend,
        }
    }
}

/// Request body for both POST (create) and PUT (full replacement).
#[derive(Debug, Deserialize)]
pub struct TransactionInput {
    pub instrument_id: i64,
    #[serde(rename = "type")]
    pub kind: TransactionKindDto,
    pub trade_date: String,
    pub quantity: i64,
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub fx_rate_to_base: Option<String>,
    #[serde(default)]
    pub brokerage: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl TransactionInput {
    fn proposed(&self) -> Result<ProposedTransaction, ApiError> {
        let trade_date = NaiveDate::parse_from_str(self.trade_date.trim(), "%Y-%m-%d").map_err(|_| {
            ApiError::bad_request(
                "invalid_date",
                format!("trade_date must be YYYY-MM-DD: {:?}", self.trade_date),
            )
        })?;
        Ok(ProposedTransaction {
            kind: self.kind.into(),
            trade_date,
            quantity: self.quantity,
            price: parse_decimal("price", &self.price)?,
            currency: normalize(&self.currency),
            fx_rate_to_base: parse_decimal("fx_rate_to_base", &self.fx_rate_to_base)?,
            brokerage_base: parse_decimal("brokerage", &self.brokerage)?,
        })
    }

    fn note(&self) -> Option<String> {
        normalize(&self.note)
    }
}

fn normalize(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty())
}

fn parse_decimal(label: &str, value: &Option<String>) -> Result<Option<Decimal>, ApiError> {
    match normalize(value) {
        None => Ok(None),
        Some(raw) => Decimal::from_str(&raw).map(Some).map_err(|_| {
            ApiError::bad_request("invalid_decimal", format!("{label} is not a valid decimal: {raw:?}"))
        }),
    }
}

#[derive(Debug, Serialize)]
pub struct TransactionResponse {
    pub id: i64,
    pub instrument_id: i64,
    #[serde(rename = "type")]
    pub kind: TransactionKindDto,
    pub trade_date: String,
    pub quantity: i64,
    pub price: Option<String>,
    pub currency: Option<String>,
    pub fx_rate_to_base: Option<String>,
    pub brokerage: Option<String>,
    pub brokerage_currency: Option<String>,
    pub source_value: Option<String>,
    pub source_currency: Option<String>,
    pub note: Option<String>,
    pub import_batch_id: Option<i64>,
}

impl TransactionResponse {
    pub fn from_row(row: &TransactionRow) -> Result<Self, ApiError> {
        let kind = TransactionKind::from_db_str(&row.kind)
            .ok_or_else(|| ApiError::internal(format!("stored unknown type {:?}", row.kind)))?;
        Ok(Self {
            id: row.id,
            instrument_id: row.instrument_id,
            kind: kind.into(),
            trade_date: row.trade_date.clone(),
            quantity: row.quantity,
            price: row.price.clone(),
            currency: row.currency.clone(),
            fx_rate_to_base: row.fx_rate_to_base.clone(),
            brokerage: row.brokerage.clone(),
            brokerage_currency: row.brokerage_currency.clone(),
            source_value: row.source_value.clone(),
            source_currency: row.source_currency.clone(),
            note: row.note.clone(),
            import_batch_id: row.import_batch_id,
        })
    }
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<TransactionResponse>>, ApiError> {
    let rows = transactions::list(&state.pool).await?;
    let body = rows
        .iter()
        .map(TransactionResponse::from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(body))
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<TransactionInput>,
) -> Result<impl IntoResponse, ApiError> {
    let proposed = body.proposed()?;
    let signed_quantity = domain::validate(&proposed)?;

    let instrument = instruments::find(&state.pool, body.instrument_id)
        .await?
        .ok_or_else(|| ApiError::not_found("instrument", body.instrument_id))?;
    assert_currency_matches(&proposed, &instrument)?;

    let prospective = ledger_transaction(i64::MAX, signed_quantity, &proposed);
    assert_ledger_valid(&state.pool, body.instrument_id, None, Some(prospective)).await?;

    let row = transactions::insert(
        &state.pool,
        &new_transaction(body.instrument_id, signed_quantity, &proposed, body.note()),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(TransactionResponse::from_row(&row)?)))
}

pub async fn replace(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<TransactionInput>,
) -> Result<impl IntoResponse, ApiError> {
    let proposed = body.proposed()?;
    let signed_quantity = domain::validate(&proposed)?;

    let existing = transactions::find(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("transaction", id))?;
    let instrument = instruments::find(&state.pool, body.instrument_id)
        .await?
        .ok_or_else(|| ApiError::not_found("instrument", body.instrument_id))?;
    assert_currency_matches(&proposed, &instrument)?;

    let edited = ledger_transaction(id, signed_quantity, &proposed);
    assert_ledger_valid(&state.pool, body.instrument_id, Some(id), Some(edited)).await?;
    if existing.instrument_id != body.instrument_id {
        assert_ledger_valid(&state.pool, existing.instrument_id, Some(id), None).await?;
    }

    let row = transactions::replace(
        &state.pool,
        id,
        &new_transaction(body.instrument_id, signed_quantity, &proposed, body.note()),
    )
    .await?;

    Ok(Json(TransactionResponse::from_row(&row)?))
}

pub async fn remove(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let existing = transactions::find(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("transaction", id))?;

    assert_ledger_valid(&state.pool, existing.instrument_id, Some(id), None).await?;
    transactions::delete(&state.pool, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

fn ledger_transaction(id: i64, signed_quantity: i64, proposed: &ProposedTransaction) -> LedgerTransaction {
    LedgerTransaction {
        id,
        trade_date: proposed.trade_date,
        kind: proposed.kind,
        quantity: signed_quantity,
        price: proposed.price,
        fx_rate_to_base: proposed.fx_rate_to_base,
        brokerage_base: proposed.brokerage_base.unwrap_or(Decimal::ZERO),
    }
}

fn new_transaction(
    instrument_id: i64,
    signed_quantity: i64,
    proposed: &ProposedTransaction,
    note: Option<String>,
) -> NewTransaction {
    NewTransaction {
        instrument_id,
        kind: proposed.kind,
        trade_date: proposed.trade_date,
        quantity: signed_quantity,
        price: proposed.price,
        currency: proposed.currency.clone(),
        fx_rate_to_base: proposed.fx_rate_to_base,
        brokerage: proposed.brokerage_base,
        note,
    }
}

/// Reject a Buy/Sell whose native currency differs from the instrument's currency.
/// Holdings label native cost basis with the instrument currency, so a mismatch
/// would present mixed-currency totals as if they were all one currency. Split
/// rows carry no currency and are always accepted here.
fn assert_currency_matches(
    proposed: &ProposedTransaction,
    instrument: &InstrumentRow,
) -> Result<(), ApiError> {
    if !matches!(proposed.kind, TransactionKind::Buy | TransactionKind::Sell) {
        return Ok(());
    }
    if let Some(currency) = proposed.currency.as_deref() {
        if !currency.eq_ignore_ascii_case(&instrument.currency) {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "currency_mismatch",
                format!(
                    "transaction currency {currency} does not match instrument currency {}",
                    instrument.currency
                ),
            ));
        }
    }
    Ok(())
}

/// Re-derive the instrument's ledger with the proposed change applied; reject if
/// any step is invalid (decision: every write must leave the ledger derivable).
async fn assert_ledger_valid(
    pool: &SqlitePool,
    instrument_id: i64,
    exclude_id: Option<i64>,
    extra: Option<LedgerTransaction>,
) -> Result<(), ApiError> {
    let mut ledger: Vec<LedgerTransaction> =
        transactions::ledger_for_instrument(pool, instrument_id).await?;
    if let Some(excluded) = exclude_id {
        ledger.retain(|tx| tx.id != excluded);
    }
    if let Some(extra) = extra {
        ledger.push(extra);
    }
    ledger.sort_by(|a, b| (a.trade_date, a.id).cmp(&(b.trade_date, b.id)));
    domain::derive_position(&ledger)?;
    Ok(())
}

// Silence unused-import warnings if RepoError is only referenced via `?` conversions.
#[allow(unused_imports)]
use RepoError as _RepoErrorInUse;
```

> Remove the trailing `#[allow(unused_imports)] use RepoError as _RepoErrorInUse;` if clippy does **not** flag `RepoError` as unused (it is imported only to make the `?`-to-`ApiError` path obvious; if clippy complains it is unused, delete the `RepoError` from the `use crate::db::{instruments, RepoError};` line instead). Verify with clippy in Step 10.

- [ ] **Step 8: Register the routes**

In `backend/src/api/mod.rs`, add the module declarations at the top (with the other `mod` lines):

```rust
mod error;
mod instruments;
mod transactions;
```

Change the routing imports to `use axum::{extract::State, http::StatusCode, response::{Html, IntoResponse}, routing::{get, put}, Router};` and extend `api_router`:

```rust
fn api_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health::handler))
        .route(
            "/import/sharesight/schema-preview",
            get(sharesight::handler),
        )
        .route(
            "/instruments",
            get(instruments::list).post(instruments::create),
        )
        .route(
            "/transactions",
            get(transactions::list).post(transactions::create),
        )
        .route(
            "/transactions/{id}",
            put(transactions::replace).delete(transactions::remove),
        )
}
```

(Note: axum 0.8 uses `{id}` path syntax, not `:id`. The `/holdings` route is added in Task 4.)

- [ ] **Step 9: Write integration tests (failing first if written before Steps 6–8)**

Add this test module to the **bottom** of `backend/src/api/transactions.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::api::router;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn send(state: &AppState, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request builds");
        let response = router(state.clone())
            .oneshot(request)
            .await
            .expect("request completes");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body readable");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json body")
        };
        (status, value)
    }

    async fn create_instrument(state: &AppState) -> i64 {
        let (status, body) = send(
            state,
            "POST",
            "/api/instruments",
            json!({"symbol":"MSFT","exchange":"NASDAQ","name":"Microsoft","type":"Stock","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        body["id"].as_i64().expect("instrument id")
    }

    #[tokio::test]
    async fn duplicate_instrument_returns_existing() {
        let state = AppState::for_tests().await;
        let first = create_instrument(&state).await;
        let (status, body) = send(
            &state,
            "POST",
            "/api/instruments",
            json!({"symbol":"MSFT","exchange":"NASDAQ","name":"Microsoft Corp","type":"Stock","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"].as_i64(), Some(first));
        assert_eq!(body["name"], "Microsoft"); // unchanged
    }

    #[tokio::test]
    async fn buy_round_trips_through_list() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;

        let (status, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-12",
                   "quantity":10,"price":"12.50","currency":"USD","fx_rate_to_base":"10.0","brokerage":"9.60"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created["quantity"], 10);
        assert_eq!(created["price"], "12.50");
        assert_eq!(created["brokerage_currency"], "SEK");

        let (status, list) = send(&state, "GET", "/api/transactions", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list.as_array().expect("array").len(), 1);
    }

    #[tokio::test]
    async fn sell_stores_negative_quantity() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;

        let (status, sold) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":4,"price":"110","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(sold["quantity"], -4);
    }

    #[tokio::test]
    async fn oversell_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":3,"price":"100","currency":"USD"}),
        )
        .await;

        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":4,"price":"110","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "sell_exceeds_position");
    }

    #[tokio::test]
    async fn buy_missing_price_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":3,"currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "price_required");
    }

    #[tokio::test]
    async fn split_without_position_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Split","trade_date":"2026-06-01","quantity":8}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "split_without_position");
    }

    #[tokio::test]
    async fn dividend_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Dividend","trade_date":"2026-06-01","quantity":0}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "dividend_not_supported");
    }

    #[tokio::test]
    async fn put_replaces_fields() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (_, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        let id = created["id"].as_i64().expect("id");

        let (status, replaced) = send(
            &state,
            "PUT",
            &format!("/api/transactions/{id}"),
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":12,"price":"105","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(replaced["quantity"], 12);
        assert_eq!(replaced["price"], "105");
    }

    #[tokio::test]
    async fn delete_that_would_break_ledger_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (_, buy) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        let buy_id = buy["id"].as_i64().expect("id");
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":6,"price":"110","currency":"USD"}),
        )
        .await;

        // Deleting the buy would leave a sell of 6 against no shares.
        let (status, error) =
            send(&state, "DELETE", &format!("/api/transactions/{buy_id}"), Value::Null).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "sell_exceeds_position");
    }

    #[tokio::test]
    async fn transaction_for_unknown_instrument_is_not_found() {
        let state = AppState::for_tests().await;
        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":999,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":1,"price":"100","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error["error"]["code"], "not_found");
    }

    #[tokio::test]
    async fn currency_mismatch_on_create_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await; // USD instrument
        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":1,"price":"100","currency":"EUR"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "currency_mismatch");
    }

    #[tokio::test]
    async fn currency_mismatch_on_replace_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await; // USD instrument
        let (_, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        let id = created["id"].as_i64().expect("id");

        let (status, error) = send(
            &state,
            "PUT",
            &format!("/api/transactions/{id}"),
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"EUR"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "currency_mismatch");
    }
}
```

- [ ] **Step 10: Run, lint, format, commit**

Run: `cd backend && cargo test`
Expected: all new integration tests + prior tests pass.
Run: `cargo clippy --all-targets -- -D warnings` (resolve the `RepoError` import note from Step 7 if flagged) then `cargo fmt`.
Add an `EngineeringDiary.md` entry ("transaction & instrument CRUD API with ledger-validity invariant and currency matching"). Then:

```bash
git add backend/Cargo.toml backend/src EngineeringDiary.md
```

---

## Task 4 — Derived holdings endpoint

`GET /api/holdings` runs Task 2 derivation over Task 3 data and exposes `average_cost_native` / `average_cost_base` with the explicit missing-FX state. Closed positions (`quantity == 0`) are omitted.

**Files:**
- Create: `backend/src/api/holdings.rs`
- Modify: `backend/src/api/mod.rs`

- [ ] **Step 1: Implement the holdings handler + DTOs**

Create `backend/src/api/holdings.rs`:

```rust
use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::api::error::ApiError;
use crate::api::instruments::InstrumentResponse;
use crate::db::instruments::{self, InstrumentRow};
use crate::db::transactions;
use crate::domain::{self, BaseCostBasis, Position, UnavailableReason};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct HoldingResponse {
    pub instrument: InstrumentResponse,
    pub quantity: i64,
    pub cost_basis_native: String,
    pub average_cost_native: String,
    pub base: BaseResponse,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BaseResponse {
    Available {
        cost_basis_base: String,
        average_cost_base: String,
        fee_component_base: String,
    },
    Unavailable {
        reasons: Vec<ReasonResponse>,
    },
}

#[derive(Debug, Serialize)]
pub struct ReasonResponse {
    pub code: &'static str,
    pub transaction_id: i64,
}

impl HoldingResponse {
    fn build(instrument: &InstrumentRow, position: &Position) -> Result<Self, ApiError> {
        let average_cost_native = position
            .average_cost_native()
            .ok_or_else(|| ApiError::internal("holding with non-positive quantity"))?
            .to_string();

        let base = match &position.base {
            BaseCostBasis::Available {
                cost_basis_base,
                fee_component_base,
            } => BaseResponse::Available {
                cost_basis_base: cost_basis_base.to_string(),
                average_cost_base: position
                    .average_cost_base()
                    .ok_or_else(|| ApiError::internal("available base without average"))?
                    .to_string(),
                fee_component_base: fee_component_base.to_string(),
            },
            BaseCostBasis::Unavailable { reasons } => BaseResponse::Unavailable {
                reasons: reasons
                    .iter()
                    .map(|reason| match reason {
                        UnavailableReason::MissingFx { transaction_id } => ReasonResponse {
                            code: "missing_fx",
                            transaction_id: *transaction_id,
                        },
                    })
                    .collect(),
            },
        };

        Ok(Self {
            instrument: InstrumentResponse::from_row(instrument)?,
            quantity: position.quantity,
            cost_basis_native: position.cost_basis_native.to_string(),
            average_cost_native,
            base,
        })
    }
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<HoldingResponse>>, ApiError> {
    let rows = instruments::list(&state.pool).await?;
    let mut holdings = Vec::new();

    for instrument in &rows {
        let ledger = transactions::ledger_for_instrument(&state.pool, instrument.id).await?;
        let position = domain::derive_position(&ledger).map_err(|error| {
            ApiError::internal(format!(
                "inconsistent stored ledger for instrument {}: {error:?}",
                instrument.id
            ))
        })?;
        if position.quantity == 0 {
            continue;
        }
        holdings.push(HoldingResponse::build(instrument, &position)?);
    }

    Ok(Json(holdings))
}
```

- [ ] **Step 2: Register the route**

In `backend/src/api/mod.rs`, add `mod holdings;` to the module declarations, and add to `api_router`:

```rust
        .route("/holdings", get(holdings::list))
```

- [ ] **Step 3: Write integration tests**

Add to the bottom of `backend/src/api/holdings.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::api::router;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn send(state: &AppState, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request builds");
        let response = router(state.clone())
            .oneshot(request)
            .await
            .expect("request completes");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body readable");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json body")
        };
        (status, value)
    }

    async fn instrument(state: &AppState, symbol: &str, exchange: &str, currency: &str) -> i64 {
        let (_, body) = send(
            state,
            "POST",
            "/api/instruments",
            json!({"symbol":symbol,"exchange":exchange,"name":symbol,"type":"Stock","currency":currency}),
        )
        .await;
        body["id"].as_i64().expect("instrument id")
    }

    #[tokio::test]
    async fn holding_reports_weighted_average_and_base_cost() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-12",
                   "quantity":10,"price":"12.50","currency":"USD","fx_rate_to_base":"10.0","brokerage":"9.60"}),
        )
        .await;

        let (status, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let holding = &holdings.as_array().expect("array")[0];
        assert_eq!(holding["quantity"], 10);
        assert_eq!(holding["average_cost_native"], "12.50");
        assert_eq!(holding["base"]["status"], "available");
        assert_eq!(holding["base"]["cost_basis_base"], "1259.60");
    }

    #[tokio::test]
    async fn split_rescales_average_in_holdings() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "NOW", "NYSE", "USD").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"120","currency":"USD","fx_rate_to_base":"1"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Split","trade_date":"2026-06-02","quantity":10}),
        )
        .await;

        let (_, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        let holding = &holdings.as_array().expect("array")[0];
        assert_eq!(holding["quantity"], 20);
        assert_eq!(holding["average_cost_native"], "60");
    }

    #[tokio::test]
    async fn missing_fx_reports_unavailable_base_with_reason() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "ASML", "EURONEXT", "EUR").await;
        let (_, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":5,"price":"600","currency":"EUR"}),
        )
        .await;
        let tx_id = created["id"].as_i64().expect("id");

        let (_, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        let holding = &holdings.as_array().expect("array")[0];
        assert_eq!(holding["average_cost_native"], "600");
        assert_eq!(holding["base"]["status"], "unavailable");
        assert_eq!(holding["base"]["reasons"][0]["code"], "missing_fx");
        assert_eq!(holding["base"]["reasons"][0]["transaction_id"], tx_id);
    }

    #[tokio::test]
    async fn closed_position_is_omitted() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":10,"price":"110","currency":"USD"}),
        )
        .await;

        let (status, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(holdings.as_array().expect("array").len(), 0);
    }
}
```

- [ ] **Step 4: Run, lint, format, commit**

Run: `cd backend && cargo test` (all green) → `cargo clippy --all-targets -- -D warnings` → `cargo fmt`.
Add an `EngineeringDiary.md` entry ("derived holdings endpoint"). Then:

```bash
git add backend/src EngineeringDiary.md
```

---

## Task 5 — Frontend: manual entry + real data

Adds TanStack Table, an API client + query/mutation hooks, the reducer-driven add-transaction form, and sortable/filterable tables wired to real endpoints. Keeps input → action → reducer → state → render with side effects (mutations) isolated and fed back as actions.

**Files:**
- Modify: `frontend/package.json`
- Create: `frontend/src/api/types.ts`, `frontend/src/api/client.ts`, `frontend/src/api/queries.ts`
- Create: `frontend/src/components/HoldingsTable.tsx`, `frontend/src/components/TransactionsTable.tsx`, `frontend/src/components/AddTransactionForm.tsx`
- Modify: `frontend/src/App.tsx`, `frontend/src/styles.css`
- Modify: `backend/Cargo.toml` (version bump)

- [ ] **Step 1: Add TanStack Table and bump the frontend version**

Run: `cd frontend && npm install @tanstack/react-table@^8.20.0`
Then edit `frontend/package.json`: change `"version": "0.2.1"` to `"version": "0.3.0"`. Confirm `@tanstack/react-table` now appears under `dependencies`.

- [ ] **Step 2: Wire types**

Create `frontend/src/api/types.ts`:

```ts
export type TransactionType = "Buy" | "Sell" | "Split" | "Dividend";
export type InstrumentType = "Stock" | "Etf" | "Fund";

export interface Instrument {
  id: number;
  symbol: string;
  exchange: string;
  name: string;
  type: InstrumentType;
  currency: string;
}

export interface Transaction {
  id: number;
  instrument_id: number;
  type: TransactionType;
  trade_date: string;
  quantity: number;
  price: string | null;
  currency: string | null;
  fx_rate_to_base: string | null;
  brokerage: string | null;
  brokerage_currency: string | null;
  source_value: string | null;
  source_currency: string | null;
  note: string | null;
  import_batch_id: number | null;
}

export type HoldingBase =
  | {
      status: "available";
      cost_basis_base: string;
      average_cost_base: string;
      fee_component_base: string;
    }
  | {
      status: "unavailable";
      reasons: { code: string; transaction_id: number }[];
    };

export interface Holding {
  instrument: Instrument;
  quantity: number;
  cost_basis_native: string;
  average_cost_native: string;
  base: HoldingBase;
}

export interface ApiErrorBody {
  error: { code: string; message: string; details?: unknown };
}
```

- [ ] **Step 3: API client**

Create `frontend/src/api/client.ts`:

```ts
import type { ApiErrorBody } from "./types";

export class ApiError extends Error {
  code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.code = code;
  }
}

async function parse<T>(response: Response): Promise<T> {
  if (response.status === 204) {
    return undefined as T;
  }

  const text = await response.text();
  const body = text ? JSON.parse(text) : null;

  if (!response.ok) {
    const error = (body as ApiErrorBody | null)?.error;
    throw new ApiError(
      error?.code ?? "unknown",
      error?.message ?? `Request failed: ${response.status}`,
    );
  }

  return body as T;
}

export async function apiGet<T>(path: string): Promise<T> {
  return parse<T>(await fetch(path));
}

export async function apiSend<T>(
  method: string,
  path: string,
  body: unknown,
): Promise<T> {
  return parse<T>(
    await fetch(path, {
      method,
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    }),
  );
}
```

- [ ] **Step 4: Query and mutation hooks**

Create `frontend/src/api/queries.ts`:

```ts
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiSend } from "./client";
import type { Holding, Instrument, InstrumentType, Transaction, TransactionType } from "./types";

export function useInstruments() {
  return useQuery({
    queryKey: ["instruments"],
    queryFn: () => apiGet<Instrument[]>("/api/instruments"),
  });
}

export function useTransactions() {
  return useQuery({
    queryKey: ["transactions"],
    queryFn: () => apiGet<Transaction[]>("/api/transactions"),
  });
}

export function useHoldings() {
  return useQuery({
    queryKey: ["holdings"],
    queryFn: () => apiGet<Holding[]>("/api/holdings"),
  });
}

export interface NewInstrumentInput {
  symbol: string;
  exchange: string;
  name: string;
  type: InstrumentType;
  currency: string;
}

export interface NewTransactionInput {
  instrument_id: number;
  type: TransactionType;
  trade_date: string;
  quantity: number;
  price?: string;
  currency?: string;
  fx_rate_to_base?: string;
  brokerage?: string;
  note?: string;
}

export function useUpsertInstrument() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: NewInstrumentInput) =>
      apiSend<Instrument>("POST", "/api/instruments", input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
    },
  });
}

export function useCreateTransaction() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: NewTransactionInput) =>
      apiSend<Transaction>("POST", "/api/transactions", input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
    },
  });
}

export function useDeleteTransaction() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => apiSend<void>("DELETE", `/api/transactions/${id}`, undefined),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
    },
  });
}
```

- [ ] **Step 5: Holdings table (TanStack Table)**

Create `frontend/src/components/HoldingsTable.tsx`:

```tsx
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  type SortingState,
  useReactTable,
} from "@tanstack/react-table";
import { ChevronDown, ChevronUp } from "lucide-react";
import { useState } from "react";
import type { Holding } from "../api/types";

const columnHelper = createColumnHelper<Holding>();

const columns = [
  columnHelper.accessor((row) => row.instrument.symbol, {
    id: "instrument",
    header: "Instrument",
    cell: (info) => {
      const { symbol, name, exchange } = info.row.original.instrument;
      return (
        <div className="instrument-cell">
          <strong>{symbol}</strong>
          <span>{name}</span>
          <em>{exchange}</em>
        </div>
      );
    },
  }),
  columnHelper.accessor("quantity", {
    header: "Qty",
    cell: (info) => info.getValue(),
  }),
  columnHelper.accessor("average_cost_native", {
    header: "Avg cost",
    cell: (info) => `${info.row.original.instrument.currency} ${info.getValue()}`,
  }),
  columnHelper.accessor("cost_basis_native", {
    header: "Cost basis",
    cell: (info) => `${info.row.original.instrument.currency} ${info.getValue()}`,
  }),
  columnHelper.display({
    id: "base",
    header: "Avg cost (SEK)",
    cell: (info) => {
      const holding = info.row.original;
      if (holding.base.status === "available") {
        return <span className="number">SEK {holding.base.average_cost_base}</span>;
      }
      return <span className="status-chip warning">FX missing</span>;
    },
  }),
];

const numericColumns = new Set(["quantity", "average_cost_native", "cost_basis_native", "base"]);

export function HoldingsTable({ holdings }: { holdings: Holding[] }) {
  const [sorting, setSorting] = useState<SortingState>([]);
  const table = useReactTable({
    data: holdings,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  });

  return (
    <div className="table-wrap">
      <table>
        <thead>
          {table.getHeaderGroups().map((headerGroup) => (
            <tr key={headerGroup.id}>
              {headerGroup.headers.map((header) => {
                const sorted = header.column.getIsSorted();
                return (
                  <th
                    key={header.id}
                    className={numericColumns.has(header.column.id) ? "sortable number-head" : "sortable"}
                  >
                    <button type="button" className="sort-button" onClick={header.column.getToggleSortingHandler()}>
                      {flexRender(header.column.columnDef.header, header.getContext())}
                      {sorted === "asc" ? (
                        <ChevronUp aria-hidden="true" size={12} />
                      ) : sorted === "desc" ? (
                        <ChevronDown aria-hidden="true" size={12} />
                      ) : null}
                    </button>
                  </th>
                );
              })}
            </tr>
          ))}
        </thead>
        <tbody>
          {table.getRowModel().rows.map((row) => (
            <tr key={row.id}>
              {row.getVisibleCells().map((cell) => (
                <td key={cell.id} className={numericColumns.has(cell.column.id) ? "number" : undefined}>
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
```

- [ ] **Step 6: Transactions table (TanStack Table + filter)**

Create `frontend/src/components/TransactionsTable.tsx`:

```tsx
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getSortedRowModel,
  type SortingState,
  useReactTable,
} from "@tanstack/react-table";
import { ChevronDown, ChevronUp } from "lucide-react";
import { useMemo, useState } from "react";
import type { Instrument, Transaction } from "../api/types";

interface Row {
  transaction: Transaction;
  symbol: string;
  exchange: string;
}

const columnHelper = createColumnHelper<Row>();

const numericColumns = new Set(["trade_date", "quantity", "price"]);

export function TransactionsTable({
  transactions,
  instruments,
  onDelete,
  deletingId,
}: {
  transactions: Transaction[];
  instruments: Instrument[];
  onDelete: (id: number) => void;
  deletingId: number | null;
}) {
  const [sorting, setSorting] = useState<SortingState>([]);
  const [filter, setFilter] = useState("");

  const byId = useMemo(() => {
    const map = new Map<number, Instrument>();
    for (const instrument of instruments) {
      map.set(instrument.id, instrument);
    }
    return map;
  }, [instruments]);

  const rows = useMemo<Row[]>(
    () =>
      transactions.map((transaction) => {
        const instrument = byId.get(transaction.instrument_id);
        return {
          transaction,
          symbol: instrument?.symbol ?? `#${transaction.instrument_id}`,
          exchange: instrument?.exchange ?? "",
        };
      }),
    [transactions, byId],
  );

  const columns = useMemo(
    () => [
      columnHelper.accessor((row) => row.transaction.trade_date, {
        id: "trade_date",
        header: "Date",
        cell: (info) => info.getValue(),
      }),
      columnHelper.accessor((row) => row.transaction.type, {
        id: "type",
        header: "Type",
        cell: (info) => <span className="type-chip">{info.getValue()}</span>,
      }),
      columnHelper.accessor((row) => row.symbol, {
        id: "instrument",
        header: "Instrument",
        cell: (info) => (
          <div className="instrument-cell compact">
            <strong>{info.row.original.symbol}</strong>
            <em>{info.row.original.exchange}</em>
          </div>
        ),
      }),
      columnHelper.accessor((row) => row.transaction.quantity, {
        id: "quantity",
        header: "Qty",
        cell: (info) => info.getValue(),
      }),
      columnHelper.accessor((row) => row.transaction.price ?? "", {
        id: "price",
        header: "Price",
        cell: (info) => {
          const { price, currency } = info.row.original.transaction;
          return price ? `${currency ?? ""} ${price}`.trim() : "-";
        },
      }),
      columnHelper.display({
        id: "actions",
        header: "",
        cell: (info) => {
          const id = info.row.original.transaction.id;
          return (
            <button
              type="button"
              className="button outline danger"
              onClick={() => onDelete(id)}
              disabled={deletingId === id}
            >
              Delete
            </button>
          );
        },
      }),
    ],
    [onDelete, deletingId],
  );

  const table = useReactTable({
    data: rows,
    columns,
    state: { sorting, globalFilter: filter },
    onSortingChange: setSorting,
    onGlobalFilterChange: setFilter,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
  });

  return (
    <>
      <div className="table-toolbar">
        <input
          className="filter-input"
          type="search"
          placeholder="Filter transactions"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
      </div>
      <div className="table-wrap">
        <table>
          <thead>
            {table.getHeaderGroups().map((headerGroup) => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map((header) => (
                  <th
                    key={header.id}
                    className={numericColumns.has(header.column.id) ? "sortable number-head" : "sortable"}
                  >
                    {header.column.id === "actions" ? null : (
                      <button
                        type="button"
                        className="sort-button"
                        onClick={header.column.getToggleSortingHandler()}
                      >
                        {flexRender(header.column.columnDef.header, header.getContext())}
                        {header.column.getIsSorted() === "asc" ? (
                          <ChevronUp aria-hidden="true" size={12} />
                        ) : header.column.getIsSorted() === "desc" ? (
                          <ChevronDown aria-hidden="true" size={12} />
                        ) : null}
                      </button>
                    )}
                  </th>
                ))}
              </tr>
            ))}
          </thead>
          <tbody>
            {table.getRowModel().rows.map((row) => (
              <tr key={row.id}>
                {row.getVisibleCells().map((cell) => (
                  <td key={cell.id} className={numericColumns.has(cell.column.id) ? "number" : undefined}>
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
```

- [ ] **Step 7: Add-transaction form (reducer-driven)**

Create `frontend/src/components/AddTransactionForm.tsx`:

```tsx
import { useReducer } from "react";
import type { Instrument, InstrumentType, TransactionType } from "../api/types";
import {
  type NewTransactionInput,
  useCreateTransaction,
  useUpsertInstrument,
} from "../api/queries";

type InstrumentMode = "existing" | "new";

type TextField =
  | "instrumentId"
  | "symbol"
  | "exchange"
  | "name"
  | "instrumentCurrency"
  | "tradeDate"
  | "quantity"
  | "price"
  | "currency"
  | "fxRate"
  | "brokerage"
  | "note";

interface FormState {
  instrumentMode: InstrumentMode;
  instrumentId: string;
  symbol: string;
  exchange: string;
  name: string;
  instrumentType: InstrumentType;
  instrumentCurrency: string;
  type: TransactionType;
  tradeDate: string;
  quantity: string;
  price: string;
  currency: string;
  fxRate: string;
  brokerage: string;
  note: string;
  error: string | null;
  submitting: boolean;
}

type FormAction =
  | { type: "fieldChanged"; field: TextField; value: string }
  | { type: "instrumentModeChanged"; mode: InstrumentMode }
  | { type: "instrumentTypeChanged"; value: InstrumentType }
  | { type: "transactionTypeChanged"; value: TransactionType }
  | { type: "submitStarted" }
  | { type: "submitFailed"; message: string }
  | { type: "submitSucceeded" };

function initialState(): FormState {
  return {
    instrumentMode: "existing",
    instrumentId: "",
    symbol: "",
    exchange: "",
    name: "",
    instrumentType: "Stock",
    instrumentCurrency: "USD",
    type: "Buy",
    tradeDate: new Date().toISOString().slice(0, 10),
    quantity: "",
    price: "",
    currency: "USD",
    fxRate: "",
    brokerage: "",
    note: "",
    error: null,
    submitting: false,
  };
}

function reducer(state: FormState, action: FormAction): FormState {
  switch (action.type) {
    case "fieldChanged":
      return { ...state, [action.field]: action.value, error: null };
    case "instrumentModeChanged":
      return { ...state, instrumentMode: action.mode, error: null };
    case "instrumentTypeChanged":
      return { ...state, instrumentType: action.value, error: null };
    case "transactionTypeChanged":
      return { ...state, type: action.value, error: null };
    case "submitStarted":
      return { ...state, submitting: true, error: null };
    case "submitFailed":
      return { ...state, submitting: false, error: action.message };
    case "submitSucceeded":
      return { ...initialState(), tradeDate: state.tradeDate };
  }
}

function trimmedOrUndefined(value: string): string | undefined {
  const trimmed = value.trim();
  return trimmed === "" ? undefined : trimmed;
}

export function AddTransactionForm({
  instruments,
  onClose,
}: {
  instruments: Instrument[];
  onClose: () => void;
}) {
  const [state, dispatch] = useReducer(reducer, undefined, initialState);
  const upsertInstrument = useUpsertInstrument();
  const createTransaction = useCreateTransaction();

  const isSplit = state.type === "Split";

  const setField = (field: TextField) => (event: { target: { value: string } }) =>
    dispatch({ type: "fieldChanged", field, value: event.target.value });

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    dispatch({ type: "submitStarted" });
    try {
      let instrumentId: number;
      if (state.instrumentMode === "existing") {
        instrumentId = Number(state.instrumentId);
        if (!instrumentId) {
          throw new Error("Select an instrument.");
        }
      } else {
        const instrument = await upsertInstrument.mutateAsync({
          symbol: state.symbol.trim(),
          exchange: state.exchange.trim(),
          name: state.name.trim(),
          type: state.instrumentType,
          currency: state.instrumentCurrency.trim(),
        });
        instrumentId = instrument.id;
      }

      const input: NewTransactionInput = {
        instrument_id: instrumentId,
        type: state.type,
        trade_date: state.tradeDate,
        quantity: Number(state.quantity),
        price: isSplit ? undefined : trimmedOrUndefined(state.price),
        currency: isSplit ? undefined : trimmedOrUndefined(state.currency),
        fx_rate_to_base: isSplit ? undefined : trimmedOrUndefined(state.fxRate),
        brokerage: isSplit ? undefined : trimmedOrUndefined(state.brokerage),
        note: trimmedOrUndefined(state.note),
      };
      await createTransaction.mutateAsync(input);
      dispatch({ type: "submitSucceeded" });
      onClose();
    } catch (error) {
      dispatch({
        type: "submitFailed",
        message: error instanceof Error ? error.message : "Could not save transaction.",
      });
    }
  }

  return (
    <form className="transaction-form" onSubmit={handleSubmit}>
      <div className="form-row">
        <label className="form-field">
          <span>Instrument source</span>
          <select
            value={state.instrumentMode}
            onChange={(event) =>
              dispatch({
                type: "instrumentModeChanged",
                mode: event.target.value as InstrumentMode,
              })
            }
          >
            <option value="existing">Pick existing</option>
            <option value="new">Create new</option>
          </select>
        </label>

        {state.instrumentMode === "existing" ? (
          <label className="form-field">
            <span>Instrument</span>
            <select value={state.instrumentId} onChange={setField("instrumentId")}>
              <option value="">Select...</option>
              {instruments.map((instrument) => (
                <option key={instrument.id} value={instrument.id}>
                  {instrument.symbol} - {instrument.exchange}
                </option>
              ))}
            </select>
          </label>
        ) : null}
      </div>

      {state.instrumentMode === "new" ? (
        <div className="form-row">
          <label className="form-field">
            <span>Symbol</span>
            <input value={state.symbol} onChange={setField("symbol")} />
          </label>
          <label className="form-field">
            <span>Exchange</span>
            <input value={state.exchange} onChange={setField("exchange")} />
          </label>
          <label className="form-field">
            <span>Name</span>
            <input value={state.name} onChange={setField("name")} />
          </label>
          <label className="form-field">
            <span>Type</span>
            <select
              value={state.instrumentType}
              onChange={(event) =>
                dispatch({
                  type: "instrumentTypeChanged",
                  value: event.target.value as InstrumentType,
                })
              }
            >
              <option value="Stock">Stock</option>
              <option value="Etf">Etf</option>
              <option value="Fund">Fund</option>
            </select>
          </label>
          <label className="form-field">
            <span>Currency</span>
            <input value={state.instrumentCurrency} onChange={setField("instrumentCurrency")} />
          </label>
        </div>
      ) : null}

      <div className="form-row">
        <label className="form-field">
          <span>Type</span>
          <select
            value={state.type}
            onChange={(event) =>
              dispatch({
                type: "transactionTypeChanged",
                value: event.target.value as TransactionType,
              })
            }
          >
            <option value="Buy">Buy</option>
            <option value="Sell">Sell</option>
            <option value="Split">Split</option>
          </select>
        </label>
        <label className="form-field">
          <span>Trade date</span>
          <input type="date" value={state.tradeDate} onChange={setField("tradeDate")} />
        </label>
        <label className="form-field">
          <span>{isSplit ? "Quantity delta" : "Quantity"}</span>
          <input type="number" value={state.quantity} onChange={setField("quantity")} />
        </label>
      </div>

      {!isSplit ? (
        <div className="form-row">
          <label className="form-field">
            <span>Price (native)</span>
            <input value={state.price} onChange={setField("price")} />
          </label>
          <label className="form-field">
            <span>Currency</span>
            <input value={state.currency} onChange={setField("currency")} />
          </label>
          <label className="form-field">
            <span>FX to SEK (optional)</span>
            <input value={state.fxRate} onChange={setField("fxRate")} />
          </label>
          <label className="form-field">
            <span>Brokerage (SEK)</span>
            <input value={state.brokerage} onChange={setField("brokerage")} />
          </label>
        </div>
      ) : null}

      <div className="form-row">
        <label className="form-field grow">
          <span>Note (optional)</span>
          <input value={state.note} onChange={setField("note")} />
        </label>
      </div>

      {state.error ? <p className="form-error">{state.error}</p> : null}

      <div className="form-actions">
        <button type="button" className="button secondary" onClick={onClose}>
          Cancel
        </button>
        <button type="submit" className="button primary" disabled={state.submitting}>
          {state.submitting ? "Saving..." : "Save transaction"}
        </button>
      </div>
    </form>
  );
}
```

- [ ] **Step 8: Rewrite `App.tsx` to use real data + the form**

Replace `frontend/src/App.tsx` with:

```tsx
import { useQuery } from "@tanstack/react-query";
import { Plus, RefreshCw } from "lucide-react";
import { useReducer } from "react";
import packageJson from "../package.json";
import { useDeleteTransaction, useHoldings, useInstruments, useTransactions } from "./api/queries";
import { AddTransactionForm } from "./components/AddTransactionForm";
import { HoldingsTable } from "./components/HoldingsTable";
import { TransactionsTable } from "./components/TransactionsTable";

const frontendVersion = packageJson.version;

type BoardView = "holdings" | "transactions";

interface UiState {
  boardView: BoardView;
  formOpen: boolean;
}

type UiAction =
  | { type: "boardViewSelected"; boardView: BoardView }
  | { type: "formToggled"; open: boolean };

interface HealthResponse {
  status: string;
  version: string;
  build: { package: string; profile: string };
}

function uiReducer(state: UiState, action: UiAction): UiState {
  switch (action.type) {
    case "boardViewSelected":
      return { ...state, boardView: action.boardView };
    case "formToggled":
      return { ...state, formOpen: action.open };
  }
}

async function fetchHealth(): Promise<HealthResponse> {
  const response = await fetch("/api/health");
  if (!response.ok) {
    throw new Error(`Health request failed: ${response.status}`);
  }
  return (await response.json()) as HealthResponse;
}

function healthLabel(healthQuery: ReturnType<typeof useQuery<HealthResponse>>) {
  if (healthQuery.isPending) {
    return "Checking API";
  }
  if (healthQuery.isError) {
    return "API offline";
  }
  return `API ${healthQuery.data.status}`;
}

export function App() {
  const [uiState, dispatch] = useReducer(uiReducer, {
    boardView: "holdings",
    formOpen: false,
  });

  const healthQuery = useQuery({ queryKey: ["health"], queryFn: fetchHealth });
  const instrumentsQuery = useInstruments();
  const transactionsQuery = useTransactions();
  const holdingsQuery = useHoldings();
  const deleteTransaction = useDeleteTransaction();

  const instruments = instrumentsQuery.data ?? [];
  const holdingsCount = holdingsQuery.data?.length ?? 0;
  const transactionsCount = transactionsQuery.data?.length ?? 0;

  return (
    <div className="app-shell">
      <header className="app-bar">
        <a className="brand" href="/" aria-label="TickerTapeTallyBoard home">
          <span className="brand-mark" aria-hidden="true" />
          <span>TickerTapeTallyBoard</span>
        </a>

        <nav className="app-nav" aria-label="Primary">
          <a className="active" href="/">
            Board
          </a>
          <a href="/">Import</a>
          <a href="/">Settings</a>
        </nav>

        <div className="app-actions">
          <button
            className="button secondary"
            type="button"
            onClick={() => {
              void holdingsQuery.refetch();
              void transactionsQuery.refetch();
            }}
            disabled={holdingsQuery.isFetching || transactionsQuery.isFetching}
          >
            <RefreshCw
              aria-hidden="true"
              className={holdingsQuery.isFetching ? "spin" : undefined}
              size={16}
            />
            <span>Refresh</span>
          </button>
          <button
            className="button primary"
            type="button"
            onClick={() => dispatch({ type: "formToggled", open: !uiState.formOpen })}
          >
            <Plus aria-hidden="true" size={16} />
            <span>Add transaction</span>
          </button>
        </div>
      </header>

      <main className="workspace">
        <section className="totals-band" aria-label="Portfolio summary">
          <div>
            <p className="eyebrow">Portfolio</p>
            <strong className="total-value">{holdingsCount} holdings</strong>
          </div>
          <div className="summary-metrics">
            <span>
              Holdings <strong className="number">{holdingsCount}</strong>
            </span>
            <span>
              Transactions <strong className="number">{transactionsCount}</strong>
            </span>
          </div>
        </section>

        <section className="status-strip" aria-label="Development status">
          <span className={healthQuery.isError ? "status-chip warning" : "status-chip"}>
            {healthLabel(healthQuery)}
          </span>
          <span className="status-chip">Manual entry</span>
          <span className="status-chip">SEK base</span>
          <span className="status-chip">UI {frontendVersion}</span>
          <span className="status-chip">
            API{" "}
            {healthQuery.data?.version ??
              (healthQuery.isPending ? "checking" : "unavailable")}
          </span>
        </section>

        {uiState.formOpen ? (
          <section className="panel form-panel" aria-label="Add transaction">
            <div className="panel-header">
              <div>
                <p className="eyebrow">Manual entry</p>
                <h2>Add transaction</h2>
              </div>
            </div>
            <AddTransactionForm
              instruments={instruments}
              onClose={() => dispatch({ type: "formToggled", open: false })}
            />
          </section>
        ) : null}

        <section className="board-grid single">
          <article className="panel ledger-panel">
            <div className="panel-header">
              <div>
                <p className="eyebrow">Workspace</p>
                <h1>Portfolio Board</h1>
              </div>
              <fieldset className="segmented-control">
                <legend className="sr-only">Board view</legend>
                <button
                  className={uiState.boardView === "holdings" ? "active" : undefined}
                  type="button"
                  aria-pressed={uiState.boardView === "holdings"}
                  onClick={() => dispatch({ type: "boardViewSelected", boardView: "holdings" })}
                >
                  Holdings
                </button>
                <button
                  className={uiState.boardView === "transactions" ? "active" : undefined}
                  type="button"
                  aria-pressed={uiState.boardView === "transactions"}
                  onClick={() => dispatch({ type: "boardViewSelected", boardView: "transactions" })}
                >
                  Transactions
                </button>
              </fieldset>
            </div>

            {uiState.boardView === "holdings" ? (
              <BoardSection
                isPending={holdingsQuery.isPending}
                isError={holdingsQuery.isError}
                isEmpty={(holdingsQuery.data?.length ?? 0) === 0}
                onRetry={() => void holdingsQuery.refetch()}
                emptyMessage="No holdings yet. Add a Buy to get started."
              >
                <HoldingsTable holdings={holdingsQuery.data ?? []} />
              </BoardSection>
            ) : (
              <BoardSection
                isPending={transactionsQuery.isPending}
                isError={transactionsQuery.isError}
                isEmpty={(transactionsQuery.data?.length ?? 0) === 0}
                onRetry={() => void transactionsQuery.refetch()}
                emptyMessage="No transactions yet. Add one with the button above."
              >
                <TransactionsTable
                  transactions={transactionsQuery.data ?? []}
                  instruments={instruments}
                  onDelete={(id) => deleteTransaction.mutate(id)}
                  deletingId={deleteTransaction.isPending ? (deleteTransaction.variables ?? null) : null}
                />
              </BoardSection>
            )}
          </article>
        </section>
      </main>
    </div>
  );
}

function BoardSection({
  isPending,
  isError,
  isEmpty,
  onRetry,
  emptyMessage,
  children,
}: {
  isPending: boolean;
  isError: boolean;
  isEmpty: boolean;
  onRetry: () => void;
  emptyMessage: string;
  children: React.ReactNode;
}) {
  if (isPending) {
    return (
      <div className="board-state">
        <div className="skeleton-bar" />
        <div className="skeleton-bar" />
        <div className="skeleton-bar" />
      </div>
    );
  }
  if (isError) {
    return (
      <div className="board-state error">
        <p className="down">Could not load data.</p>
        <button type="button" className="button outline" onClick={onRetry}>
          Retry
        </button>
      </div>
    );
  }
  if (isEmpty) {
    return <div className="board-state muted">{emptyMessage}</div>;
  }
  return <>{children}</>;
}
```

- [ ] **Step 9: Add form/table/state styles**

Append to `frontend/src/styles.css`:

```css
.board-grid.single {
  grid-template-columns: minmax(0, 1fr);
}

.form-panel {
  margin-bottom: var(--space-4);
  padding-bottom: var(--space-4);
}

.transaction-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-4);
}

.form-row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
}

.form-field {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  min-width: 140px;
}

.form-field.grow {
  flex: 1;
}

.form-field span {
  color: var(--text-muted);
  font-size: 0.8125rem;
  font-weight: 600;
}

.form-field input,
.form-field select {
  min-height: 36px;
  padding: 0 var(--space-3);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-md);
  background: var(--surface-2);
  color: var(--text-primary);
}

.form-field input:focus-visible,
.form-field select:focus-visible {
  outline: none;
  border-color: var(--accent);
  box-shadow: var(--focus-ring);
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
}

.form-error {
  margin: 0;
  color: var(--down);
  font-size: 0.8125rem;
}

.button.outline {
  background: transparent;
  border: 1px solid var(--hairline);
}

.button.outline.danger:hover {
  border-color: var(--down);
  color: var(--down);
}

.table-toolbar {
  display: flex;
  padding: var(--space-3) var(--space-4) 0;
}

.filter-input {
  min-height: 32px;
  min-width: 220px;
  padding: 0 var(--space-3);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-md);
  background: var(--surface-2);
  color: var(--text-primary);
}

.sort-button {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  padding: 0;
  background: transparent;
  color: var(--text-muted);
  font: inherit;
  font-size: 0.6875rem;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  cursor: pointer;
}

th.number-head .sort-button {
  margin-left: auto;
}

.board-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  padding: var(--space-8) var(--space-4);
  color: var(--text-muted);
}

.board-state.error {
  color: var(--text-secondary);
}

.skeleton-bar {
  width: 100%;
  height: 20px;
  border-radius: var(--radius-sm);
  background: var(--surface-2);
}
```

- [ ] **Step 10: Bump the backend version**

In `backend/Cargo.toml`, change `version = "0.1.1"` to `version = "0.2.0"` so `/api/health` reports the Phase 1 backend. (No code change; the health handler reads `CARGO_PKG_VERSION`.)

- [ ] **Step 11: Check, format, commit**

Run: `cd frontend && npm run check` then `npm run fmt`.
Expected: `tsc --noEmit` clean, Biome clean.

**Human smoke test (recommended):** from `frontend/` run `npm run build` once to confirm the production bundle compiles. Then in two terminals: `cd backend && cargo run` and `cd frontend && npm run dev`. In the browser (the dev server URL, e.g. `http://127.0.0.1:5173/`):
1. Click **Add transaction**, create a new instrument (e.g. `MSFT / NASDAQ / Microsoft / Stock / USD`), add a **Buy** of `10 @ 12.50`, FX `10.0`, brokerage `9.60`. Save.
2. Confirm the holding shows quantity `10`, avg cost `USD 12.50`, and SEK avg cost `≈ 125.96` (cost basis `1259.60 / 10`).
3. Add a **Buy** of `10 @ 12.50` with **no FX**; confirm the SEK column shows **FX missing**.
4. Add a **Sell** that exceeds the position and confirm the inline error surfaces the API message.
5. Add a **Split** delta and confirm the holding quantity and average rescale.

Bump versions already done (frontend `0.3.0`, backend `0.2.0`); confirm both the `UI 0.3.0` and `API 0.2.0` chips appear in the status strip (frontend from `package.json`, backend from `/api/health`). Then:

```bash
git add frontend backend/Cargo.toml
```

Add an `EngineeringDiary.md` entry ("frontend manual entry + real-data tables; version bump").

---

## Task 6 — Record durable decisions

The §11 decisions are commitments, so they belong in `docs/DecisionLog.md` (new entries go at the end; do not modify committed entries). This task is documentation only.

**Files:**
- Modify: `docs/DecisionLog.md`

- [ ] **Step 1: Append the decision entries**

Append the following to `docs/DecisionLog.md` (keep the existing date-ordered structure):

```markdown
## 2026-06-14: Cost-Basis Accounting Method
Decision: Portfolio cost basis uses weighted-average cost per instrument. A buy adds to quantity and native cost basis; a sell reduces quantity and every cost-basis component proportionally at the current average, leaving average cost per share unchanged. FIFO tax-lots remain out of scope for v1.
Context: `docs/CurrencyAndFxRules.md` deferred "the chosen portfolio accounting method"; this resolves it. The ISK tax scope means cost basis is for analytics and reconciliation only.
Consequences: Sell handling and holdings derivation use a single blended average. Per-lot recovery of historical cost is not possible without a future FIFO model.

## 2026-06-14: Ledger Ordering
Decision: Position derivation processes a single instrument's transactions in `(trade_date, id)` order. `id` is the monotonic insert/import tiebreaker; same-day same-instrument rows stay distinct.
Context: Weighted-average cost is order-sensitive when buys and sells interleave on the same trade date. A deterministic order is required for reproducible holdings.
Consequences: A later explicit import-sequence column can replace the tiebreaker without changing the contract. Writes are validated by re-deriving the full ordered ledger.

## 2026-06-14: Instrument Identity
Decision: Instrument identity is `UNIQUE (exchange, symbol)`. Currency is an attribute, not part of identity. Creating an instrument is upsert-like: an existing `(exchange, symbol)` returns the existing row unchanged rather than creating a duplicate.
Context: Inline instrument creation from the transaction form must not fragment a holding into duplicate instruments.
Consequences: Provider-specific symbols are stored separately when added later. Renames/currency corrections to an existing instrument need an explicit update path (not part of upsert).

## 2026-06-14: Missing-FX Contamination Rule
Decision: A position's SEK cost basis (`cost_basis_base` / `average_cost_base`) is available only while every buy contributing to the currently open quantity had a known trade-date FX rate. The first missing-FX buy folded into the blended average makes the position's base cost unavailable (with reasons and offending transaction ids) until the position fully closes (`quantity → 0`) and is rebuilt from buys that all have FX. Native cost basis is always derivable.
Context: Under one blended weighted-average, shares bought without FX cannot be isolated, so a partial SEK basis would be misleading. Missing data must be explicit, never zero.
Consequences: Holdings expose an explicit unavailable state. Per-lot recovery is deferred with FIFO tax-lots.

## 2026-06-14: Ledger-Write Validity Invariant
Decision: Every transaction write (create, full-replace, delete) must leave the affected instrument's `(trade_date, id)`-ordered ledger derivable — no step may drive quantity below zero and every split must remain valid — otherwise the write is rejected. Missing FX is not a violation (it derives successfully with an unavailable base).
Context: Holdings are derived purely from the ledger and must always be computable. Back-dated edits and deletes can otherwise invalidate later rows.
Consequences: Corrections may need to be applied in dependency order (e.g. remove a dependent sell before the buy it draws from). Holdings derivation can assume a consistent stored ledger.

## 2026-06-14: Transaction Type Constraint And Manual Entry Scope
Decision: The `transactions.type` CHECK constraint allows `BUY|SELL|SPLIT|DIVIDEND`, but manual entry and the API support only Buy, Sell, and Split; a Dividend create request is rejected until dividend fields and validation exist.
Context: Keeping DIVIDEND in the constraint avoids a future table-rebuild migration when dividend support is designed, while Phase-1 behaviour stays limited to the types that have defined fields.
Consequences: The schema is forward-compatible for dividends. Until dividend fields land, no Dividend rows are created, and derivation treats any Dividend row as a position no-op.

## 2026-06-14: Backend Persistence Stack
Decision: The backend persists to SQLite via sqlx using runtime `query_as` and `FromRow` (no compile-time macros, no `.sqlx/` offline metadata, no `SQLX_OFFLINE`). Money, prices, FX, and quantities-as-decimals are stored as TEXT and round-tripped through `rust_decimal` strings; integer share quantities as INTEGER; dates as ISO-8601 TEXT. Schema correctness is covered by DB integration tests that run the real migrations. Migrations are forward-only and additive.
Context: For a single-binary local SQLite app where SQLite stores decimals as TEXT, compile-time query macros add tooling friction for little type-safety gain, and the existing `cargo clippy --all-targets -- -D warnings` workflow must stay free of a live database.
Consequences: All SQL lives in `db/` repositories. Reads decode TEXT decimals/dates into domain types; a decode failure is an internal error. Revisit the macro approach only if a richer database or compile-time guarantees are later justified.
```

- [ ] **Step 2: Commit**

```bash
git add docs/DecisionLog.md
```

(No `EngineeringDiary.md` entry — its instructions say meta-documents like the DecisionLog do not need diary entries.)

---

## Acceptance check (Phase 1)

Confirm against design §10 before archiving:
- [ ] A user can manually enter Buy/Sell/Split trades in the UI in well under 30s each (human smoke test, Task 5 Step 11).
- [ ] Holdings show correct positions, weighted-average cost, and per-currency cost basis derived purely from the ledger (Task 4 tests + smoke test).
- [ ] A split entered as a delta yields the correct post-split position and average cost (`split_rescales_average_in_holdings`).
- [ ] No market value / P&L claims appear in the UI (App.tsx exposes only ledger-derived values).
- [ ] All backend/domain logic is covered by tests; the app builds and lints clean (`cargo test`, `cargo clippy --all-targets -- -D warnings`, `npm run check`).

When all tasks and the acceptance checklist are complete, archive `docs/plans/Design.Phase1.LedgerCore.md` and this plan per the repo's "plans are ephemeral" convention.

---

## Self-review (performed against the design)

**Spec coverage** — every design section maps to a task:
- §5 data model → Task 1 (migration + constraint test).
- §6 API surface + contract → Task 3 (instruments/transactions) + Task 4 (holdings); PascalCase enums, decimal strings, single error shape, PUT = full replacement, upsert-like instrument create all covered.
- §7 domain logic (ordering, separate cost components, weighted-average, split-as-delta, missing-FX propagation, per-currency reporting) → Task 2.
- §8 frontend (TanStack Table, real data, form, sort/filter, dark-theme states) → Task 5.
- §9 incremental steps → Tasks 1–5 (each ends runnable with a verification gate); §11 decisions → Task 6.
- Out-of-scope items (prices, FX fetch, realized P&L, Sharesight wiring, cash ledger, dividend UI) are not implemented, matching §1.

**Placeholder scan** — no `TBD`/`implement later`/"add error handling" placeholders; each code step carries complete code and each command its expected result. Two deliberate in-text notes (the `db/mod.rs` module-staging note in Task 1 Step 5 and the `RepoError` import note in Task 3 Step 7) are clippy-contingent cleanups with explicit instructions, not deferred work.

**Type consistency** — `TransactionKind`/`as_db_str`/`from_db_str`, `ProposedTransaction`, `LedgerTransaction`, `Position`, `BaseCostBasis`, `UnavailableReason`, `validate`, `derive_position`, `RepoError`, `NewInstrument`, `NewTransaction`, `TransactionRow`, `InstrumentRow`, `ApiError`, and the wire DTOs (`TransactionKindDto`, `InstrumentKindDto`, `TransactionInput`, `TransactionResponse`, `InstrumentResponse`, `HoldingResponse`/`BaseResponse`) are defined once and used consistently across tasks. Signed-quantity convention (Buy +, Sell −, Split delta) is uniform from domain → repository → API → frontend display.

---

## Execution handoff

**Plan complete and saved to `docs/plans/Plan.Phase1.LedgerCore.Implementation.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — execute tasks in this session using executing-plans, batch execution with checkpoints.

**Which approach?**






