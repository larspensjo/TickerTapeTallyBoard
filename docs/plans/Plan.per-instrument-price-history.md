# Per-Instrument Price-History API Implementation Plan

> **For agentic workers:** Implement this plan task-by-task following the repo workflow: work one task at a time, run the listed verification commands from `backend/`, stage the changes, and stop for review. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a read-only `GET /api/instruments/{id}/prices` endpoint returning one instrument's daily closes over an optional date window, each converted to SEK with carry-forward FX, missing data represented explicitly.

**Architecture:** Two range repository queries load the full price and FX series once; a pure `build_price_history` function in `domain/valuation.rs` merges them with a single forward pass (two-pointer carry-forward); a thin axum handler maps repository rows into existing `PriceCandidate`/`FxCandidate` domain types, calls the builder, and serializes the result reusing the availability/money helpers already used by holdings and gains.

**Tech Stack:** Rust, axum 0.8, sqlx 0.9 (SQLite), rust_decimal, chrono. Tests use `tokio::test`, `tower::ServiceExt::oneshot`, and `rust_decimal_macros::dec!`.

## Global Constraints

- Build from `backend/` with `cargo build`. On task completion run `cargo clippy --all-targets -- -D warnings` then `cargo fmt` from `backend/`.
- Provider/currency constants are reused verbatim from `api/valuation.rs`: `PRICE_PROVIDER = "YAHOO"`, `FX_PROVIDER = "FRANKFURTER"`, `BASE_CURRENCY = "SEK"`. Do not redefine them.
- Domain code (`domain/`) stays free of axum/sqlx/HTTP and performs no I/O. The builder is pure and unit-testable.
- Keep entry points (`main.rs`, `mod.rs`, `lib.rs`) thin wrappers.
- **FX pair direction:** the DB stores canonical rates as instrument-currency → SEK. FX queries pass `base = instrument.currency`, `quote = BASE_CURRENCY` (e.g. `base="USD", quote="SEK"`). The response field `base_currency` ("convert *into*", SEK) is a different concept from the `fx_rates.base` column ("convert *from*"). Do not conflate them.
- **Decimal serialization differs by field** (`docs/CurrencyAndFxRules.md`): `close_base.value` (SEK money) uses `money_string` (2 dp). Native `close` and `fx.rate` serialize at full stored precision via `Decimal::normalize().to_string()` — never through `money_string`.
- **Mapping gating mirrors valuation:** when the Yahoo price mapping is missing OR disabled, the instrument is treated as unmapped — return `200` with an empty `points` array. Never serve cached price rows while the mapping is missing/disabled.
- Backend version: bump `backend/Cargo.toml` from `0.5.4` to `0.5.5` (final task).

---

### Task 1: Range repository queries (`prices` + `fx_rates`)

Adds the two ascending range loaders the builder depends on. Both return rows ordered by date ascending. `list_for_pair` returns the **full** rate history (no date window) so the first in-range price can carry forward a rate dated before `from`.

**Files:**
- Modify: `backend/src/db/prices.rs` (add SQL const + `list_for_instrument_in_range`; add tests)
- Modify: `backend/src/db/fx_rates.rs` (add SQL const + `list_for_pair`; add tests)

**Interfaces:**
- Consumes: existing `PriceRow`, `FxRateRow`, `RepoError`, and `crate::db::testing::memory_pool`.
- Produces:
  - `prices::list_for_instrument_in_range(pool: &SqlitePool, instrument_id: i64, provider: &str, from: Option<NaiveDate>, to: Option<NaiveDate>) -> Result<Vec<PriceRow>, RepoError>` — ordered by `date ASC, id ASC`, inclusive bounds.
  - `fx_rates::list_for_pair(pool: &SqlitePool, base: &str, quote: &str, provider: &str) -> Result<Vec<FxRateRow>, RepoError>` — full history ordered by `date ASC, id ASC`.

- [ ] **Step 1: Write the failing prices range test**

Add to the `tests` module in `backend/src/db/prices.rs` (it already has `use super::*;` and a `seed_instrument` helper):

```rust
#[tokio::test]
async fn list_for_instrument_in_range_orders_and_bounds_inclusively() {
    let pool = testing::memory_pool().await;
    let instrument_id = seed_instrument(&pool).await;

    for (day, close) in [(10, "10.00"), (11, "11.00"), (12, "12.00")] {
        upsert(
            &pool,
            &NewPrice {
                instrument_id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "MSFT".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, day).expect("valid date"),
                close: Decimal::from_str(close).expect("close"),
                currency: "USD".to_owned(),
                fetched_at: "2026-06-16T08:00:00Z".to_owned(),
            },
        )
        .await
        .expect("price upsert should succeed");
    }

    // Inclusive window 11..=12.
    let windowed = list_for_instrument_in_range(
        &pool,
        instrument_id,
        "YAHOO",
        Some(NaiveDate::from_ymd_opt(2026, 6, 11).unwrap()),
        Some(NaiveDate::from_ymd_opt(2026, 6, 12).unwrap()),
    )
    .await
    .expect("range query should succeed");
    let dates: Vec<&str> = windowed.iter().map(|r| r.date.as_str()).collect();
    assert_eq!(dates, vec!["2026-06-11", "2026-06-12"]);

    // No bounds returns full history ascending.
    let all = list_for_instrument_in_range(&pool, instrument_id, "YAHOO", None, None)
        .await
        .expect("range query should succeed");
    let all_dates: Vec<&str> = all.iter().map(|r| r.date.as_str()).collect();
    assert_eq!(all_dates, vec!["2026-06-10", "2026-06-11", "2026-06-12"]);

    // Provider filtering: wrong provider yields nothing.
    let other = list_for_instrument_in_range(&pool, instrument_id, "OTHER", None, None)
        .await
        .expect("range query should succeed");
    assert!(other.is_empty());

    // Open-ended `from` only.
    let from_only = list_for_instrument_in_range(
        &pool,
        instrument_id,
        "YAHOO",
        Some(NaiveDate::from_ymd_opt(2026, 6, 12).unwrap()),
        None,
    )
    .await
    .expect("range query should succeed");
    assert_eq!(from_only.len(), 1);
    assert_eq!(from_only[0].date, "2026-06-12");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run (from `backend/`): `cargo test db::prices::tests::list_for_instrument_in_range_orders_and_bounds_inclusively`
Expected: FAIL — `cannot find function list_for_instrument_in_range`.

- [ ] **Step 3: Implement `list_for_instrument_in_range`**

Add the SQL const near the other `prices.rs` SQL consts. Optional bounds are handled with `(? IS NULL OR date >= ?)`; the same optional value is bound twice (sqlx binds positional `?` in order, so each `?` gets its own bind):

```rust
const LIST_IN_RANGE_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, date, close, currency, fetched_at \
    FROM prices \
    WHERE instrument_id = ? AND provider = ? \
      AND (? IS NULL OR date >= ?) \
      AND (? IS NULL OR date <= ?) \
    ORDER BY date ASC, id ASC";
```

Add the function (place it after `find_previous_before`):

```rust
pub async fn list_for_instrument_in_range(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: &str,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Result<Vec<PriceRow>, RepoError> {
    let from = from.map(|d| d.format("%Y-%m-%d").to_string());
    let to = to.map(|d| d.format("%Y-%m-%d").to_string());
    let rows = sqlx::query_as::<_, PriceRow>(LIST_IN_RANGE_SQL)
        .bind(instrument_id)
        .bind(provider)
        .bind(&from)
        .bind(&from)
        .bind(&to)
        .bind(&to)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}
```

- [ ] **Step 4: Run it to verify it passes**

Run (from `backend/`): `cargo test db::prices::tests::list_for_instrument_in_range_orders_and_bounds_inclusively`
Expected: PASS.

- [ ] **Step 5: Write the failing fx range test**

Add to the `tests` module in `backend/src/db/fx_rates.rs` (it has `use super::*;` and `use crate::db::testing`):

```rust
#[tokio::test]
async fn list_for_pair_returns_full_history_ascending() {
    let pool = testing::memory_pool().await;

    for (day, rate) in [(8, "9.00"), (10, "10.00"), (12, "11.00")] {
        upsert(
            &pool,
            &NewFxRate {
                base: "USD".to_owned(),
                quote: "SEK".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, day).expect("valid date"),
                rate: Decimal::from_str(rate).expect("rate"),
                provider: "FRANKFURTER".to_owned(),
                fetched_at: "2026-06-16T08:00:00Z".to_owned(),
            },
        )
        .await
        .expect("fx upsert should succeed");
    }

    let rows = list_for_pair(&pool, "USD", "SEK", "FRANKFURTER")
        .await
        .expect("pair query should succeed");
    let dates: Vec<&str> = rows.iter().map(|r| r.date.as_str()).collect();
    assert_eq!(dates, vec!["2026-06-08", "2026-06-10", "2026-06-12"]);

    // Wrong pair direction yields nothing.
    let reversed = list_for_pair(&pool, "SEK", "USD", "FRANKFURTER")
        .await
        .expect("pair query should succeed");
    assert!(reversed.is_empty());
}
```

- [ ] **Step 6: Run it to verify it fails**

Run (from `backend/`): `cargo test db::fx_rates::tests::list_for_pair_returns_full_history_ascending`
Expected: FAIL — `cannot find function list_for_pair`.

- [ ] **Step 7: Implement `list_for_pair`**

Add the SQL const near the other `fx_rates.rs` consts and the function after `find_previous_before`:

```rust
const LIST_FOR_PAIR_SQL: &str = "SELECT id, base, quote, date, rate, provider, fetched_at \
    FROM fx_rates WHERE base = ? AND quote = ? AND provider = ? ORDER BY date ASC, id ASC";
```

```rust
pub async fn list_for_pair(
    pool: &SqlitePool,
    base: &str,
    quote: &str,
    provider: &str,
) -> Result<Vec<FxRateRow>, RepoError> {
    let rows = sqlx::query_as::<_, FxRateRow>(LIST_FOR_PAIR_SQL)
        .bind(base)
        .bind(quote)
        .bind(provider)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}
```

Note: `Decimal::from_str` and `NaiveDate` are already in scope in both test modules via `use super::*;`.

- [ ] **Step 8: Run it to verify it passes**

Run (from `backend/`): `cargo test db::fx_rates::tests::list_for_pair_returns_full_history_ascending`
Expected: PASS.

- [ ] **Step 9: Lint, format, stage**

```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
cd ..
git add backend/src/db/prices.rs backend/src/db/fx_rates.rs
git status --short
```

Stage the changes for review; do not commit. (Per `Agents.md`, plan-driven changes are staged, not committed — any later commit is a separate, user-approved step.)

---

### Task 2: Pure series builder in `domain/valuation.rs`

Adds `FxApplied`, `PricePoint`, and `build_price_history` — a single forward pass over ascending prices that advances an FX index while `fx.date <= point.date`, retaining the last seen rate as the carry-forward value. SEK instruments take the identity path. Wrong-currency rows are dropped silently (the handler logs them in Task 3).

**Files:**
- Modify: `backend/src/domain/valuation.rs` (add types + function + unit tests)
- Modify: `backend/src/domain/mod.rs` (export the new items)

**Interfaces:**
- Consumes: existing `PriceCandidate { date, close, currency }`, `FxCandidate { date, rate, base, quote }`, `Availability<Decimal>`, `ValuationReason::MissingFx` (all already in `domain/valuation.rs`).
- Produces:
  - `pub struct FxApplied { pub rate: Decimal, pub date: NaiveDate }` (derive `Clone, Debug, PartialEq`)
  - `pub struct PricePoint { pub date: NaiveDate, pub close: Decimal, pub close_base: Availability<Decimal>, pub fx: Option<FxApplied> }` (derive `Clone, Debug, PartialEq`)
  - `pub fn build_price_history(native_currency: &str, prices: &[PriceCandidate], fx_rates: &[FxCandidate]) -> Vec<PricePoint>`

- [ ] **Step 1: Write the failing builder tests**

Add to the `tests` module in `backend/src/domain/valuation.rs`. It already has `d`, `price`, and `fx` helpers and imports `Availability, FxCandidate, PriceCandidate, ValuationReason` and `dec!`. Extend the `use super::{...}` line to also import `build_price_history, FxApplied, PricePoint` (add them to the existing import list at the top of the `tests` module).

```rust
#[test]
fn build_price_history_carries_fx_forward() {
    // FX only on the 10th; the 11th (no same-day rate) carries the 10th forward.
    let prices = vec![
        price(d(2026, 6, 10), dec!(100), "USD"),
        price(d(2026, 6, 11), dec!(110), "USD"),
    ];
    let fx_rates = vec![fx(d(2026, 6, 10), dec!(10), "USD", "SEK")];

    let points = build_price_history("USD", &prices, &fx_rates);

    assert_eq!(points.len(), 2);
    assert_eq!(points[0].close_base, Availability::available(dec!(1000)));
    assert_eq!(
        points[0].fx,
        Some(FxApplied { rate: dec!(10), date: d(2026, 6, 10) })
    );
    // Carry-forward: the 11th still applies the 10th's rate and reports the 10th's date.
    assert_eq!(points[1].close_base, Availability::available(dec!(1100)));
    assert_eq!(
        points[1].fx,
        Some(FxApplied { rate: dec!(10), date: d(2026, 6, 10) })
    );
}

#[test]
fn build_price_history_marks_missing_fx_before_any_rate() {
    // Price predates every FX rate: close_base unavailable, native close retained, fx omitted.
    let prices = vec![price(d(2026, 6, 9), dec!(100), "USD")];
    let fx_rates = vec![fx(d(2026, 6, 10), dec!(10), "USD", "SEK")];

    let points = build_price_history("USD", &prices, &fx_rates);

    assert_eq!(points.len(), 1);
    assert_eq!(points[0].close, dec!(100));
    assert_eq!(
        points[0].close_base,
        Availability::unavailable(ValuationReason::MissingFx)
    );
    assert_eq!(points[0].fx, None);
}

#[test]
fn build_price_history_carries_fx_dated_before_the_first_price() {
    // The only applicable rate predates the first price: prove the full FX set was used.
    let prices = vec![price(d(2026, 6, 20), dec!(100), "USD")];
    let fx_rates = vec![fx(d(2026, 6, 1), dec!(10), "USD", "SEK")];

    let points = build_price_history("USD", &prices, &fx_rates);

    assert_eq!(points.len(), 1);
    assert_eq!(points[0].close_base, Availability::available(dec!(1000)));
    assert_eq!(
        points[0].fx,
        Some(FxApplied { rate: dec!(10), date: d(2026, 6, 1) })
    );
}

#[test]
fn build_price_history_sek_uses_identity_and_omits_fx() {
    let prices = vec![price(d(2026, 6, 10), dec!(42), "SEK")];

    let points = build_price_history("SEK", &prices, &[]);

    assert_eq!(points.len(), 1);
    assert_eq!(points[0].close_base, Availability::available(dec!(42)));
    assert_eq!(points[0].fx, None);
}

#[test]
fn build_price_history_drops_wrong_currency_rows() {
    // Instrument is USD; a stray EUR row is an internal data error and is excluded.
    let prices = vec![
        price(d(2026, 6, 10), dec!(100), "USD"),
        price(d(2026, 6, 11), dec!(200), "EUR"),
    ];
    let fx_rates = vec![fx(d(2026, 6, 10), dec!(10), "USD", "SEK")];

    let points = build_price_history("USD", &prices, &fx_rates);

    assert_eq!(points.len(), 1);
    assert_eq!(points[0].date, d(2026, 6, 10));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run (from `backend/`): `cargo test domain::valuation::tests::build_price_history`
Expected: FAIL — `cannot find function build_price_history` / `cannot find type FxApplied`.

- [ ] **Step 3: Implement the types and builder**

Add to `backend/src/domain/valuation.rs` (e.g. after the `FxSnapshot` struct). `NaiveDate`, `Decimal`, `Availability`, `ValuationReason` are already in scope in this module.

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct FxApplied {
    pub rate: Decimal,
    pub date: NaiveDate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PricePoint {
    pub date: NaiveDate,
    pub close: Decimal,
    pub close_base: Availability<Decimal>,
    pub fx: Option<FxApplied>,
}

/// Build a per-instrument daily series converted to SEK.
///
/// `prices` and `fx_rates` must both be sorted by date ascending. A single
/// forward pass advances an FX index while `fx.date <= point.date`, retaining
/// the last seen rate as the carry-forward value. Rows whose currency differs
/// from `native_currency` are dropped (internal data error). SEK instruments
/// take the identity path with `fx` omitted.
pub fn build_price_history(
    native_currency: &str,
    prices: &[PriceCandidate],
    fx_rates: &[FxCandidate],
) -> Vec<PricePoint> {
    let is_base = native_currency.eq_ignore_ascii_case("SEK");
    let mut points = Vec::new();
    let mut fx_idx = 0usize;
    let mut current_fx: Option<&FxCandidate> = None;

    for price in prices {
        if !price.currency.eq_ignore_ascii_case(native_currency) {
            continue;
        }

        if is_base {
            points.push(PricePoint {
                date: price.date,
                close: price.close,
                close_base: Availability::available(price.close),
                fx: None,
            });
            continue;
        }

        while fx_idx < fx_rates.len() && fx_rates[fx_idx].date <= price.date {
            current_fx = Some(&fx_rates[fx_idx]);
            fx_idx += 1;
        }

        match current_fx {
            Some(fx) => points.push(PricePoint {
                date: price.date,
                close: price.close,
                close_base: Availability::available(price.close * fx.rate),
                fx: Some(FxApplied {
                    rate: fx.rate,
                    date: fx.date,
                }),
            }),
            None => points.push(PricePoint {
                date: price.date,
                close: price.close,
                close_base: Availability::unavailable(ValuationReason::MissingFx),
                fx: None,
            }),
        }
    }

    points
}
```

- [ ] **Step 4: Export the new items**

In `backend/src/domain/mod.rs`, extend the `pub use valuation::{...}` block to include `build_price_history, FxApplied, PricePoint`:

```rust
#[allow(unused_imports)]
pub use valuation::{
    build_price_history, summarize_holdings, value_position, Availability, DataFreshness,
    FxApplied, FxCandidate, FxSnapshot, PriceCandidate, PricePoint, PriceSnapshot, ValuationReason,
    ValuationSummary, ValuedHolding,
};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run (from `backend/`): `cargo test domain::valuation::tests::build_price_history`
Expected: PASS (all five tests).

- [ ] **Step 6: Lint, format, stage**

```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
cd ..
git add backend/src/domain/valuation.rs backend/src/domain/mod.rs
git status --short
```

Stage the changes for review; do not commit.

---

### Task 3: API handler & route

Adds the handler in a new focused module, registers the route, and serializes the response. Reuses `money_string`, `serialize_availability`, `AvailabilityResponse`, and the provider/currency constants from `api/valuation.rs` (sibling module — `pub(super)` items are visible).

**Files:**
- Create: `backend/src/api/instrument_prices.rs`
- Modify: `backend/src/api/mod.rs` (declare `mod instrument_prices;` and register the route)

**Interfaces:**
- Consumes: `instruments::find`, `provider_symbols::find_by_instrument_provider`, `prices::list_for_instrument_in_range`, `fx_rates::list_for_pair` (Task 1), `build_price_history`/`PricePoint`/`FxApplied` (Task 2), and from `api::valuation`: `money_string`, `serialize_availability`, `AvailabilityResponse`, `BASE_CURRENCY`, `PRICE_PROVIDER`, `FX_PROVIDER`.
- Produces: `instrument_prices::list` axum handler at `GET /api/instruments/{id}/prices`.

- [ ] **Step 1: Create the handler module skeleton**

Create `backend/src/api/instrument_prices.rs`:

```rust
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::api::valuation::{
    money_string, serialize_availability, AvailabilityResponse, BASE_CURRENCY, FX_PROVIDER,
    PRICE_PROVIDER,
};
use crate::db::{fx_rates, instruments, prices, provider_symbols};
use crate::domain::{build_price_history, FxApplied, FxCandidate, PriceCandidate, PricePoint};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PriceHistoryQuery {
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PriceHistoryResponse {
    instrument_id: i64,
    currency: String,
    base_currency: String,
    points: Vec<PricePointResponse>,
}

#[derive(Debug, Serialize)]
pub struct PricePointResponse {
    date: String,
    close: String,
    close_base: AvailabilityResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    fx: Option<FxAppliedResponse>,
}

#[derive(Debug, Serialize)]
pub struct FxAppliedResponse {
    rate: String,
    date: String,
}

fn parse_date(s: &str, field: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("invalid_date", format!("invalid {field}: {s}")))
}

/// Full stored precision for native price and FX rate (never money_string).
fn precise_string(value: Decimal) -> String {
    value.normalize().to_string()
}

fn point_response(point: &PricePoint) -> PricePointResponse {
    PricePointResponse {
        date: point.date.format("%Y-%m-%d").to_string(),
        close: precise_string(point.close),
        close_base: serialize_availability(&point.close_base, |v| money_string(*v)),
        fx: point.fx.as_ref().map(|FxApplied { rate, date }| FxAppliedResponse {
            rate: precise_string(*rate),
            date: date.format("%Y-%m-%d").to_string(),
        }),
    }
}

pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<PriceHistoryQuery>,
) -> Result<Json<PriceHistoryResponse>, ApiError> {
    let from = query.from.as_deref().map(|s| parse_date(s, "from")).transpose()?;
    let to = query.to.as_deref().map(|s| parse_date(s, "to")).transpose()?;
    if let (Some(from), Some(to)) = (from, to) {
        if from > to {
            return Err(ApiError::bad_request(
                "invalid_date_range",
                "from must not be after to",
            ));
        }
    }

    let instrument = instruments::find(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("instrument", id))?;

    let mapping =
        provider_symbols::find_by_instrument_provider(&state.pool, id, PRICE_PROVIDER).await?;
    let mapping_enabled = mapping.as_ref().is_some_and(|m| m.enabled);

    let points = if mapping_enabled {
        let price_rows =
            prices::list_for_instrument_in_range(&state.pool, id, PRICE_PROVIDER, from, to).await?;
        // Decode failures are internal invariant violations, not missing data:
        // propagate them as `ApiError::internal` instead of silently dropping rows.
        let price_candidates: Vec<PriceCandidate> = price_rows
            .into_iter()
            .map(|row| price_candidate(&instrument, row))
            .collect::<Result<_, _>>()?;

        let is_base = instrument.currency.eq_ignore_ascii_case(BASE_CURRENCY);
        let fx_candidates: Vec<FxCandidate> = if is_base {
            Vec::new()
        } else {
            fx_rates::list_for_pair(&state.pool, &instrument.currency, BASE_CURRENCY, FX_PROVIDER)
                .await?
                .into_iter()
                .map(fx_candidate)
                .collect::<Result<_, _>>()?
        };

        build_price_history(&instrument.currency, &price_candidates, &fx_candidates)
    } else {
        Vec::new()
    };

    Ok(Json(PriceHistoryResponse {
        instrument_id: id,
        currency: instrument.currency.clone(),
        base_currency: BASE_CURRENCY.to_string(),
        points: points.iter().map(point_response).collect(),
    }))
}

/// Map a price row into a `PriceCandidate`. A decode failure (date or close) is
/// an internal invariant violation per `db/mod.rs`, not missing data, so it
/// propagates as `ApiError::internal` with instrument id, row id, and field
/// context rather than producing a `200` with silently missing points. A row
/// whose currency differs from the instrument's is logged here (the
/// repository-mapping boundary where the instrument is known) and kept; the
/// builder drops mismatched rows.
fn price_candidate(
    instrument: &instruments::InstrumentRow,
    row: prices::PriceRow,
) -> Result<PriceCandidate, ApiError> {
    let date = row.date_value().map_err(|e| {
        ApiError::internal(format!(
            "price-history: undecodable date in price row {} for instrument {}: {e}",
            row.id, instrument.id
        ))
    })?;
    let close = row.close_decimal().map_err(|e| {
        ApiError::internal(format!(
            "price-history: undecodable close in price row {} for instrument {}: {e}",
            row.id, instrument.id
        ))
    })?;
    if !row.currency.eq_ignore_ascii_case(&instrument.currency) {
        crate::engine_warn!(
            "price-history currency mismatch for instrument {}: row currency {:?} != instrument {:?}",
            instrument.id,
            row.currency,
            instrument.currency
        );
    }
    Ok(PriceCandidate {
        date,
        close,
        currency: row.currency,
    })
}

fn fx_candidate(row: fx_rates::FxRateRow) -> Result<FxCandidate, ApiError> {
    let date = row.date_value().map_err(|e| {
        ApiError::internal(format!(
            "price-history: undecodable date in fx row {} ({}->{}): {e}",
            row.id, row.base, row.quote
        ))
    })?;
    let rate = row.rate_decimal().map_err(|e| {
        ApiError::internal(format!(
            "price-history: undecodable rate in fx row {} ({}->{}): {e}",
            row.id, row.base, row.quote
        ))
    })?;
    Ok(FxCandidate {
        date,
        rate,
        base: row.base,
        quote: row.quote,
    })
}
```

- [ ] **Step 2: Register the module and route**

In `backend/src/api/mod.rs`, add the module declaration alphabetically among the existing `mod` lines (after `mod import;`, before `mod instruments;`):

```rust
mod instrument_prices;
```

Then add the route inside `api_router()`, alongside the other `/instruments` routes:

```rust
        .route(
            "/instruments/{id}/prices",
            get(instrument_prices::list),
        )
```

- [ ] **Step 3: Build to verify it compiles**

Run (from `backend/`): `cargo build`
Expected: compiles. (`get` is already imported in `api/mod.rs`.)

- [ ] **Step 4: Write the failing handler tests**

Add a `#[cfg(test)] mod tests` to `backend/src/api/instrument_prices.rs`. Mirror the request helper used by `api/prices.rs` and `api/gains.rs` tests.

```rust
#[cfg(test)]
mod tests {
    use crate::api::router;
    use crate::api::valuation::{BASE_CURRENCY, FX_PROVIDER, PRICE_PROVIDER};
    use crate::db::{fx_rates, instruments, prices, provider_symbols};
    use crate::import::now_iso8601;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use serde_json::json;
    use tower::ServiceExt;

    async fn send(state: &AppState, uri: &str) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
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
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json body")
        };
        (status, value)
    }

    async fn instrument(state: &AppState, symbol: &str, currency: &str) -> i64 {
        let (row, _) = instruments::upsert(
            &state.pool,
            &instruments::NewInstrument {
                symbol: symbol.to_owned(),
                exchange: "NASDAQ".to_owned(),
                name: symbol.to_owned(),
                kind: "STOCK".to_owned(),
                currency: currency.to_owned(),
                isin: None,
            },
        )
        .await
        .expect("instrument upsert should succeed");
        row.id
    }

    async fn enable_mapping(state: &AppState, instrument_id: i64, enabled: bool) {
        let now = now_iso8601();
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "SYM".to_owned(),
                currency: None,
                enabled,
                created_at: now.clone(),
                updated_at: now,
            },
        )
        .await
        .expect("mapping upsert should succeed");
    }

    async fn seed_price(state: &AppState, instrument_id: i64, date: NaiveDate, close: rust_decimal::Decimal, currency: &str) {
        prices::upsert(
            &state.pool,
            &prices::NewPrice {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "SYM".to_owned(),
                date,
                close,
                currency: currency.to_owned(),
                fetched_at: now_iso8601(),
            },
        )
        .await
        .expect("price upsert should succeed");
    }

    async fn seed_fx(state: &AppState, date: NaiveDate, rate: rust_decimal::Decimal) {
        fx_rates::upsert(
            &state.pool,
            &fx_rates::NewFxRate {
                base: "USD".to_owned(),
                quote: BASE_CURRENCY.to_owned(),
                date,
                rate,
                provider: FX_PROVIDER.to_owned(),
                fetched_at: now_iso8601(),
            },
        )
        .await
        .expect("fx upsert should succeed");
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
    }

    #[tokio::test]
    async fn non_sek_happy_path_converts_with_full_precision_close() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        enable_mapping(&state, id, true).await;
        seed_fx(&state, d(2026, 6, 10), dec!(10.4731)).await;
        seed_price(&state, id, d(2026, 6, 10), dec!(110.5034), "USD").await;

        let (status, body) = send(&state, &format!("/api/instruments/{id}/prices")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["currency"], "USD");
        assert_eq!(body["base_currency"], "SEK");
        let point = &body["points"][0];
        assert_eq!(point["date"], "2026-06-10");
        assert_eq!(point["close"], "110.5034"); // full precision, not money_string
        assert_eq!(point["close_base"]["status"], "available");
        // 110.5034 * 10.4731 = 1157.31315854, serialized through money_string (2 dp).
        assert_eq!(point["close_base"]["value"], "1157.31");
        assert_eq!(point["fx"]["rate"], "10.4731");
        assert_eq!(point["fx"]["date"], "2026-06-10");
    }

    #[tokio::test]
    async fn sek_instrument_uses_identity_and_omits_fx() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "ERICB", BASE_CURRENCY).await;
        enable_mapping(&state, id, true).await;
        seed_price(&state, id, d(2026, 6, 10), dec!(42.5), BASE_CURRENCY).await;

        let (status, body) = send(&state, &format!("/api/instruments/{id}/prices")).await;
        assert_eq!(status, StatusCode::OK);
        let point = &body["points"][0];
        assert_eq!(point["close_base"]["value"], "42.50");
        assert!(point.get("fx").is_none());
    }

    #[tokio::test]
    async fn unknown_instrument_is_404() {
        let state = AppState::for_tests().await;
        let (status, body) = send(&state, "/api/instruments/999/prices").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
    }

    #[tokio::test]
    async fn from_after_to_is_invalid_date_range() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        let (status, body) = send(
            &state,
            &format!("/api/instruments/{id}/prices?from=2026-06-12&to=2026-06-11"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_date_range");
    }

    #[tokio::test]
    async fn malformed_from_returns_invalid_date() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;

        let (status, body) =
            send(&state, &format!("/api/instruments/{id}/prices?from=not-a-date")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_date");
    }

    #[tokio::test]
    async fn missing_mapping_returns_empty_points() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        // No mapping at all; seed a price that must NOT be served.
        seed_price(&state, id, d(2026, 6, 10), dec!(100), "USD").await;

        let (status, body) = send(&state, &format!("/api/instruments/{id}/prices")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["points"].as_array().expect("points").len(), 0);
    }

    #[tokio::test]
    async fn disabled_mapping_returns_empty_points() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        enable_mapping(&state, id, false).await;
        seed_price(&state, id, d(2026, 6, 10), dec!(100), "USD").await;

        let (status, body) = send(&state, &format!("/api/instruments/{id}/prices")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["points"].as_array().expect("points").len(), 0);
    }

    #[tokio::test]
    async fn carry_forward_when_from_is_after_only_fx_rate() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        enable_mapping(&state, id, true).await;
        // Only FX rate is dated before `from`; the full FX set must still be loaded.
        seed_fx(&state, d(2026, 6, 1), dec!(10)).await;
        seed_price(&state, id, d(2026, 6, 20), dec!(100), "USD").await;

        let (status, body) = send(
            &state,
            &format!("/api/instruments/{id}/prices?from=2026-06-15&to=2026-06-25"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let point = &body["points"][0];
        assert_eq!(point["date"], "2026-06-20");
        assert_eq!(point["close_base"]["value"], "1000.00");
        assert_eq!(point["fx"]["date"], "2026-06-01"); // prior rate's date proves carry-forward
    }
}
```

- [ ] **Step 5: Run the handler tests to verify they fail then pass**

Run (from `backend/`): `cargo test api::instrument_prices`
Expected: with the handler from Steps 1-2 already implemented, these PASS. If any fail, fix the handler (do not change the assertions — the `invalid_date_range` string and full-precision `close` are locked by the spec).

- [ ] **Step 6: Lint, format, stage**

```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
cd ..
git add backend/src/api/instrument_prices.rs backend/src/api/mod.rs
git status --short
```

Stage the changes for review; do not commit.

- [ ] **Step 7: External human testing (recommended)**

Against a backfilled database, run the backend and hit `GET /api/instruments/{id}/prices` for a real non-SEK instrument. Eyeball that the SEK `close_base` series looks right, native `close`/`fx.rate` show full precision, and a date window narrows the points as expected.

---

### Task 4: Documentation & version bump

**Files:**
- Modify: `docs/DecisionLog.md` (append one entry)
- Modify: `backend/Cargo.toml` (version `0.5.4` → `0.5.5`)

- [ ] **Step 1: Append the DecisionLog entry**

Add to the end of `docs/DecisionLog.md`, following the template:

```markdown
## 2026-06-21 - Per-instrument price-history endpoint conventions
Decision: `GET /api/instruments/{id}/prices` returns one point per stored close date (no synthesis), converts to SEK using the latest FX rate on or before each point's date (carry-forward, loading the full FX pair history so a rate dated before the window still applies), and with no `from`/`to` params returns the instrument's full stored history. A missing or disabled Yahoo price mapping yields `200` with an empty `points` array; cached prices are never served while the mapping is missing or disabled. Native `close` and `fx.rate` serialize at full stored precision; `close_base` uses the 2-decimal money format. `from > to` returns `400 invalid_date_range`, sharing the refresh endpoint's error code.
Context: First slice of Phase 4 charts; feeds the reserved price chart on the asset detail page while reusing valuation's mapping-gating and FX conventions.
Consequences: Portfolio value-over-time and position-market-value-over-time series remain separate future specs. The carry-forward-before-window behaviour relies on `fx_rates::list_for_pair` loading the full pair history, not a windowed range.
```

- [ ] **Step 2: Bump the backend version**

In `backend/Cargo.toml` change:

```toml
version = "0.5.4"
```

to:

```toml
version = "0.5.5"
```

- [ ] **Step 3: Verify the build still passes**

Run (from `backend/`): `cargo build`
Expected: compiles (Cargo.lock version updates).

- [ ] **Step 4: Stage**

```bash
git add docs/DecisionLog.md backend/Cargo.toml backend/Cargo.lock
git status --short
```

Stage the changes for review; do not commit.

---

## Notes for the implementer

- **Do not stage a final review commit beyond these tasks.** Per repo workflow, plan changes are staged for review, not auto-merged.
- The spec lists `prices::list_for_instrument_in_range` and `fx_rates::list_for_pair` (Task 1), `build_price_history` (Task 2), and the route registration (Task 3) as the verifiable phase boundaries — they map 1:1 to Tasks 1-3 here. Task 4 covers the spec's "Documentation & housekeeping" section.
- The original spec lives at `docs/plans/Spec.per-instrument-price-history.md`. Per repo convention, the spec and this plan are ephemeral and archived once implemented; never reference them from runtime docs.
```
