# Plan — One valuation clock and a versioned data snapshot

> **For agentic workers:** implement this plan phase by phase, in order. Steps
> use checkbox (`- [ ]`) syntax for tracking. Each phase ends with its own
> verification and commit; do not start a later phase before the previous one is
> green. Read `docs/DecisionLog.md` entries referenced here before changing
> behaviour.

**Goal:** Make it structurally impossible for two panels of the app to hold
numbers from different moments under the same cache key, by giving the backend
one clock and one data version that every answer and every client query names.
A snapshot that changes while requests are in flight is caught by the revision
each response stamps on itself, which makes the client refetch rather than keep
a mixed pair.

**Architecture:** The backend owns "today" in a single `Clock` carried on
application state, and Clippy bans the global now-functions everywhere else, so
no surface can invent its own date. The backend also owns a monotonic data
revision, bumped by one middleware for every successful mutating request and by
the launch refresh job, and exposes it with today's date on
`GET /api/data-version`. The frontend fetches that version and includes it in the
cache key of every data query, so data from different snapshots cannot be held
under the same key, and one version change refetches every panel together
instead of a hand-maintained invalidation list.

**Tech Stack:** Rust (axum 0.8, sqlx 0.9, chrono 0.4, tokio), TypeScript/React 19
with TanStack Query v5, Vitest + Testing Library, Biome.

**Spec:** this document, section *Problem and evidence (verified)*.

## Global Constraints

- Backend commands run from `backend/`: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt`.
- Frontend commands run from `frontend/`: `npx vitest run`, `npm run check`
  (TypeScript + Biome), `npm run fmt`.
- This plan changes *when* and *from which snapshot* numbers are computed, never
  *what* is computed. No valuation, provider, or ledger semantics change.
- Preserve input → action → reducer → state → render. Date resolution and
  version handling live in pure functions, the API layer, or the query layer —
  never in components.
- No SQL migration is added; migration bytes and checksums stay untouched.
- Backend runtime logging goes through `engine_logging` macros with enough
  context to identify the operation.
- API errors keep the single existing `ApiError` shape.
- Version bumps: `backend/Cargo.toml` to `0.17.0` (Phase 2, new endpoint),
  `frontend/package.json` to `0.23.0` (Phase 3). Both are shown in the UI.
- Existing query parameters of `GET /api/gains` keep working unchanged;
  additions are backward compatible.

---

## Problem and evidence (verified)

Two incidents on 2026-09-11, same shape — panels rendered from different moments:

1. The Gains table and its summary strip showed 2026-09-10 while the top cards
   showed 2026-09-11. Period presets ("All", "7D", …) were resolved to concrete
   dates once, in the browser, and frozen in state; a tab left open across
   midnight kept requesting the previous day. `GET /api/gains?end_date=2026-09-10`
   reproduced the table exactly: day change −110,682.13, market value
   6,850,207.05, capital gain 2,754,069.82.
2. After a restart, the top cards were 31,569.64 behind the table in market
   value, day change and capital gain. The backend's launch refresh
   (`spawn_launch_refresh`; run 360, 14:45:58Z → 14:46:08Z) wrote new prices
   between the two fetches. Both responses reported `as_of_date: 2026-09-11`, so
   nothing in the data distinguished them, and the client refetched only after
   its *own* refresh mutation.

Structural causes still in the code today:

- Six backend sites derive "today" independently in two timezones:
  `api/gains.rs:47`, `api/holdings.rs:209`, `api/rebalance.rs:110`,
  `demo/mod.rs:94` use `Local::now()`; `market_data/price_status.rs:38`,
  `market_data/refresh.rs:326,335` use `Utc::now()`. Between local midnight and
  02:00 CEST these disagree by a day.
- The browser derives its own "today" twice, in two timezones:
  `components/DateRangeSelector.tsx` (local) and
  `components/AddTransactionForm.tsx:64` (`toISOString()`, i.e. UTC — a
  transaction entered at 00:30 local pre-fills **yesterday**).
- No response states which price data it was computed from.
- `frontend/src/api/queries.ts` holds four hand-written invalidation lists
  (`invalidatePortfolioData`, `invalidatePriceDerivedData`,
  `invalidateInstrumentData`, plus per-mutation sets). Every new price-dependent
  query must be added to the right lists by hand; forgetting one caused incident 2.

## Settled decisions (implement these; do not re-litigate)

1. **The backend owns "today".** One `Clock` on `AppState`, system local time, no
   new configuration. A fixed-date variant exists for tests.
2. **`chrono::Local::now` and `chrono::Utc::now` are banned** outside
   `backend/src/clock.rs`, enforced by `backend/clippy.toml` under the existing
   `-D warnings` gate. Callers needing an instant use `clock::now_utc()`.
3. **One monotonic data revision per process**, in memory, rendered
   `<session>:<counter>`. Not persisted: a restart produces a new session
   segment, which is itself a change and makes clients refetch. No migration.
4. **The revision is bumped by infrastructure, not by handlers.** One axum
   middleware bumps it after any non-GET/HEAD/OPTIONS `/api/**` request whose
   response is not a client error; the launch refresh job bumps it when it
   finishes, because that write path never passes through HTTP. A handler cannot
   forget, because bumping is not a handler's job. Rejections (400, 403, 404,
   422) leave the revision alone; a server error does not, because a price
   refresh can write and then fail.
5. **`GET /api/data-version` is the single heartbeat**, returning
   `data_revision`, `valuation_date`, `prices_refreshing`.
6. **The snapshot token is `<data_revision>@<valuation_date>`**, part of the
   cache key of every client data query except `useHealth` and `useDataVersion`.
   A day change on the server therefore refetches data with no client clock.
7. **Report periods are resolved server-side** from an intent
   (`period=today|7d|12m|ytd|all|custom`). Explicit `start_date`/`end_date` keep
   working, so the API stays backward compatible.
8. **Client-side date arithmetic is deleted**, including `useLocalDate` and the
   preset→range math added on 2026-09-11; decisions 6 and 7 take over its job.
9. **Price-derived responses carry `data_revision`** — gains, holdings,
   rebalance, portfolio value history — and the client compares that stamp with
   the token it requested. A difference means the snapshot moved mid-flight, and
   the client invalidates the version instead of keeping the mismatched pair.

## Out of scope (do not build here)

- An "updating" state while panels are mid-transition. Recorded as a residual.
- Persisting the revision or multi-process coordination.
- Any change to valuation maths, providers, or the refresh scheduler.
- Timezone configuration: the machine's local timezone is the app's timezone.
- Reworking `usePriceStatus`'s per-instrument detail payload.

---

## Architecture

### The clock

`backend/src/clock.rs` becomes the only module that reads the wall clock:

```rust
use chrono::{DateTime, Local, NaiveDate, Utc};

/// The single source of "today" for every surface that values a portfolio.
#[derive(Clone, Copy, Debug)]
pub enum Clock {
    /// The machine's local calendar date.
    System,
    /// A pinned date, for tests.
    Fixed(NaiveDate),
}

impl Clock {
    pub fn fixed(date: NaiveDate) -> Self {
        Clock::Fixed(date)
    }

    /// Today's date in the app's timezone. Every valuation date, staleness
    /// comparison and refresh window derives from this.
    pub fn today(&self) -> NaiveDate {
        match self {
            Clock::System => {
                #[allow(clippy::disallowed_methods)]
                Local::now().naive_local().date()
            }
            Clock::Fixed(date) => *date,
        }
    }
}

/// The current instant, for timestamps that are points in time rather than
/// calendar dates: log lines, backup file names, refresh run records.
pub fn now_utc() -> DateTime<Utc> {
    #[allow(clippy::disallowed_methods)]
    Utc::now()
}

/// The current instant as an RFC 3339 string, the form stored in the ledger's
/// timestamp columns and in refresh run records.
pub fn now_iso8601() -> String {
    now_utc().to_rfc3339()
}
```

`now_iso8601` moves here from `backend/src/import/mod.rs`, where it reads the
clock through `SystemTime::now()` — a path the Clippy ban cannot see. Eighteen
files import it from `crate::import`, so leave that spelling working with a
re-export in `backend/src/import/mod.rs`:

```rust
pub use crate::clock::now_iso8601;
```

`AppState` carries `clock: Clock`; handlers call `state.clock.today()`; services
take the date as a parameter instead of reading a global.

### The data version

`backend/src/data_revision.rs` holds a cheap, cloneable counter. `session` is
derived once per process from the start instant, so a restart changes the
revision even though the counter restarts at zero.

Bumps happen in exactly two places: the `/api` middleware, and the launch
refresh job. `GET /api/data-version` returns:

```json
{
  "data_revision": "1757682000123:7",
  "valuation_date": "2026-09-12",
  "prices_refreshing": false
}
```

### What the client does

`useDataVersion()` polls `/api/data-version` — every 2s while
`prices_refreshing`, every 15s otherwise, and on window focus. Every other data
query is defined through `useVersionedQuery`, which places the token
`<data_revision>@<valuation_date>` in the cache key and stays disabled until the
token is known. What follows for free:

- A backend refresh, a restart, or the server's midnight changes the token, so
  every panel refetches together.
- A mutation invalidates only `["data-version"]`; the cascade does the rest, and
  the four invalidation lists collapse to one line per mutation.
- A new query hook is covered the moment it is written through
  `useVersionedQuery`, and a test over the module's exports fails if it is not.

### Files

| File | Responsibility |
| --- | --- |
| `backend/src/clock.rs` (new) | The only wall-clock reader: `Clock::today()`, `now_utc()` |
| `backend/clippy.toml` (new) | Bans `chrono::Local::now` / `chrono::Utc::now` elsewhere |
| `backend/src/data_revision.rs` (new) | The `DataRevision` counter type |
| `backend/src/api/data_version.rs` (new) | `GET /api/data-version` handler and response |
| `backend/src/api/gains/period.rs` (new) | Pure preset → date-range resolution |
| `backend/src/state.rs` | Carries `clock` and `revision` |
| `backend/src/api/mod.rs` | Route registration and the revision middleware |
| `backend/src/app.rs` | Builds state; launch refresh bumps the revision |
| `frontend/src/api/dataVersion.ts` (new) | `DataVersion` type and `versionToken` |
| `frontend/src/api/queries.ts` | `useDataVersion`, `useVersionedQuery`, all hooks |
| `frontend/src/components/DateRangeSelector.tsx` | Preset intent only, no date math |

---

## Phases

Every phase leaves the app working and is committed on its own.

### Phase 1 — One clock owns "today"

**Files:**
- Create: `backend/src/clock.rs`, `backend/clippy.toml`
- Modify: `backend/src/lib.rs`, `backend/src/state.rs`, `backend/src/app.rs`,
  `backend/src/api/gains.rs`, `backend/src/api/holdings.rs`,
  `backend/src/api/rebalance.rs`, `backend/src/api/prices.rs`,
  `backend/src/demo/mod.rs`, `backend/src/market_data/refresh.rs`,
  `backend/src/market_data/price_status.rs`, `backend/src/ledger/backup.rs`,
  `backend/src/ledger/open.rs`, `backend/src/ledger/retention.rs`,
  `backend/src/import/mod.rs`, `backend/src/api/test_support.rs`,
  `backend/src/api/gains/tests.rs`
- Test: `backend/src/clock.rs` (unit), `backend/src/api/gains/tests.rs`
  (handler-level, pinned clock)

**Interfaces:**
- Produces: `crate::clock::Clock` with `Clock::System`, `Clock::fixed(NaiveDate)`,
  `Clock::today() -> NaiveDate`; `crate::clock::now_utc() -> DateTime<Utc>`;
  `crate::clock::now_iso8601() -> String` (re-exported as
  `crate::import::now_iso8601`); `AppState.clock: Clock` and
  `AppState::with_clock(Clock) -> AppState`.
- Consumes: nothing from later phases.

- [ ] **Step 1: Write the failing clock test**

Create `backend/src/clock.rs` containing only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn fixed_clock_returns_its_pinned_date() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).expect("valid date");
        assert_eq!(Clock::fixed(date).today(), date);
    }

    #[test]
    fn system_clock_agrees_with_local_calendar_date() {
        let today = Clock::System.today();
        let now = now_utc();
        assert!(
            (now.date_naive() - today).num_days().abs() <= 1,
            "system clock date {today} should be within a day of UTC {}",
            now.date_naive()
        );
    }
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd backend && cargo test clock::`
Expected: FAIL — `Clock` and `now_utc` do not exist.

- [ ] **Step 3: Implement the clock module**

Add the `Clock` enum, `now_utc` and `now_iso8601` exactly as given in
*Architecture → The clock* above, at the top of `backend/src/clock.rs`. Register
the module in `backend/src/lib.rs` next to the other `pub mod` lines:

```rust
pub mod clock;
```

Then delete the body of `now_iso8601` in `backend/src/import/mod.rs` and replace
it with `pub use crate::clock::now_iso8601;`, moving that module's existing
`now_iso8601_looks_like_rfc3339` test into `clock.rs` alongside the clock tests.
The eighteen files that import it from `crate::import` keep compiling unchanged.

- [ ] **Step 4: Run the test again**

Run: `cd backend && cargo test clock::`
Expected: PASS (2 tests).

- [ ] **Step 5: Ban the global clock everywhere else**

Create `backend/clippy.toml`:

```toml
# "Today" and "now" enter the program through crate::clock only, so every
# surface that values a portfolio agrees on the same date. See
# docs/plans/Plan.ConsistentDataSnapshot.md.
#
# std::time::SystemTime::now is deliberately NOT banned: its remaining callers
# build unique temporary file names in tests, which is not a clock reading. The
# one production caller, now_iso8601, moved into crate::clock.
disallowed-methods = [
  { path = "chrono::Local::now", reason = "use AppState's Clock::today(), or crate::clock::now_utc() for instants" },
  { path = "chrono::Utc::now", reason = "use crate::clock::now_utc(), or AppState's Clock::today() for calendar dates" },
]
```

- [ ] **Step 6: Run Clippy to enumerate every offending call site**

Run: `cd backend && cargo clippy --all-targets -- -D warnings`
Expected: FAIL, one error per site. The expected list (verify against the actual
output; fix all of them, not just these):
`api/gains.rs:47`, `api/holdings.rs:209,804`, `api/rebalance.rs:110,787`,
`api/test_support.rs:88`, `api/gains/tests.rs` (11 sites: lines 102, 103, 192,
193, 246, 247, 285, 469, 525, 596, 622),
`demo/mod.rs:94`, `market_data/price_status.rs:38,238`,
`market_data/refresh.rs:326,335` and its `now_iso8601`,
`market_data/refresh/tests.rs:397`, `ledger/backup.rs:100,361`,
`ledger/open.rs:134`, `ledger/retention.rs:343`.

- [ ] **Step 7: Give `AppState` a clock**

In `backend/src/state.rs`, add the field and builder alongside the existing ones:

```rust
use crate::clock::Clock;

pub struct AppState {
    pub pool: SqlitePool,
    pub market_data: Arc<MarketDataService>,
    pub mode: Mode,
    pub ledger_path: Option<PathBuf>,
    pub backup: BackupState,
    pub clock: Clock,
}
```

Initialise `clock: Clock::System` in `AppState::new`, and add:

```rust
pub fn with_clock(mut self, clock: Clock) -> Self {
    self.clock = clock;
    self
}
```

- [ ] **Step 8: Route every date through the clock**

Replace each offending site with the clock:

- `api/gains.rs`: `None => state.clock.today(),`
- `api/holdings.rs`: `let valuation_date = state.clock.today();`
- `api/rebalance.rs`: `let valuation_date = state.clock.today();`
- `demo/mod.rs`: `pub async fn seed(pool: &SqlitePool)` becomes
  `pub async fn seed(pool: &SqlitePool, today: NaiveDate)` and passes `today`
  straight to the existing `seed_for_date`. Its only caller is `build_state` in
  `backend/src/app.rs:169`, which becomes
  `crate::demo::seed(&pool, crate::clock::Clock::System.today())` — state does
  not exist yet at that point, and demo mode always runs on the system clock.
- `market_data/price_status.rs`: `price_status` takes `today: NaiveDate` as its
  second parameter instead of reading `Utc::now()`.
- `market_data/refresh.rs`: `MarketDataService::refresh` and
  `MarketDataService::status` each take `today: NaiveDate`; `refresh_window`
  takes it too. Its `now_iso8601` calls need no change — they already point at
  the re-export, which now goes through `crate::clock`.
- `api/prices.rs`: both handlers pass `state.clock.today()` into the service.
- `app.rs`: the launch refresh passes `state.clock.today()`.
- `ledger/backup.rs`, `ledger/open.rs`, `ledger/retention.rs`:
  `crate::clock::now_utc()`.
- Tests: `Clock::System.today()` where a real date is wanted, or a pinned
  `Clock::fixed(...)` where the test asserts on dates.

- [ ] **Step 9: Write the failing handler test that proves the clock is honoured**

Add to `backend/src/api/gains/tests.rs`:

```rust
#[tokio::test]
async fn gains_as_of_date_comes_from_the_state_clock() {
    let state = AppState::for_tests().await.with_clock(crate::clock::Clock::fixed(
        chrono::NaiveDate::from_ymd_opt(2026, 3, 5).expect("valid date"),
    ));

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["as_of_date"], "2026-03-05");
    assert_eq!(body["report_period"]["end_date"], "2026-03-05");
}
```

`send`, `AppState::for_tests()`, `StatusCode` and `json!` are already in scope in
that file — it is the helper every test there uses, returning
`(StatusCode, serde_json::Value)`.

- [ ] **Step 10: Run the whole backend suite and the lint gate**

Run: `cd backend && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all tests pass; Clippy reports no disallowed-method errors.

- [ ] **Step 11: Commit**

```bash
git add backend/clippy.toml backend/src
git commit -m "Give the backend one clock for today's date"
```

---

### Phase 2 — A data revision the backend stamps and serves

**Files:**
- Create: `backend/src/data_revision.rs`, `backend/src/api/data_version.rs`
- Modify: `backend/src/lib.rs`, `backend/src/state.rs`, `backend/src/api/mod.rs`,
  `backend/src/app.rs`, `backend/src/api/gains/types.rs`,
  `backend/src/api/gains.rs`, `backend/src/api/holdings.rs`,
  `backend/src/api/rebalance.rs`, `backend/src/api/portfolio.rs`,
  `backend/Cargo.toml`
- Test: `backend/src/data_revision.rs` (unit),
  `backend/src/api/data_version.rs` (handler + middleware), `backend/src/app.rs`

**Interfaces:**
- Consumes: `Clock` and `AppState::with_clock` from Phase 1.
- Produces: `crate::data_revision::DataRevision` with `new() -> Self`,
  `current(&self) -> String`, `bump(&self)`; `AppState.revision: DataRevision`;
  `GET /api/data-version` returning
  `{ data_revision: String, valuation_date: String, prices_refreshing: bool }`;
  a `data_revision: String` field on the gains, holdings, rebalance and
  value-history responses.

- [ ] **Step 1: Write the failing revision unit test**

Create `backend/src/data_revision.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_changes_only_when_bumped() {
        let revision = DataRevision::new();
        let first = revision.current();

        assert_eq!(revision.current(), first);

        revision.bump();
        let second = revision.current();

        assert_ne!(second, first);
        assert_eq!(revision.current(), second);
    }

    #[test]
    fn clones_share_one_counter() {
        let revision = DataRevision::new();
        let clone = revision.clone();

        clone.bump();

        assert_eq!(revision.current(), clone.current());
    }
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd backend && cargo test data_revision::`
Expected: FAIL — `DataRevision` does not exist.

- [ ] **Step 3: Implement `DataRevision`**

```rust
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Identifies the state of the stored data. It changes whenever anything a
/// valuation depends on may have changed: a ledger write, an import, an
/// instrument edit, or a market-data refresh. It is process-local: a restart
/// starts a new session, which clients see as a change and refetch.
#[derive(Clone, Debug)]
pub struct DataRevision {
    session: Arc<str>,
    counter: Arc<AtomicU64>,
}

impl DataRevision {
    pub fn new() -> Self {
        let session = crate::clock::now_utc().timestamp_millis().to_string();
        Self {
            session: Arc::from(session.as_str()),
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The current revision, as `<session>:<counter>`.
    pub fn current(&self) -> String {
        format!("{}:{}", self.session, self.counter.load(Ordering::SeqCst))
    }

    /// Record that stored data may have changed.
    pub fn bump(&self) {
        self.counter.fetch_add(1, Ordering::SeqCst);
    }
}

impl Default for DataRevision {
    fn default() -> Self {
        Self::new()
    }
}
```

Register it in `backend/src/lib.rs`:

```rust
pub mod data_revision;
```

- [ ] **Step 4: Run the unit tests**

Run: `cd backend && cargo test data_revision::`
Expected: PASS (2 tests).

- [ ] **Step 5: Put the revision on `AppState`**

In `backend/src/state.rs` add `pub revision: DataRevision` to `AppState`,
initialise it with `DataRevision::new()` in `AppState::new`, and leave it out of
the builder methods — nothing needs to replace it.

- [ ] **Step 6: Write the failing endpoint and middleware tests**

Create `backend/src/api/data_version.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::state::AppState;

    async fn get_data_version(state: &AppState) -> Value {
        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/data-version")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body readable");
        serde_json::from_slice(&body).expect("body is JSON")
    }

    #[tokio::test]
    async fn reports_the_clock_date_and_current_revision() {
        let state = AppState::for_tests().await.with_clock(
            crate::clock::Clock::fixed(
                chrono::NaiveDate::from_ymd_opt(2026, 3, 5).expect("valid date"),
            ),
        );

        let body = get_data_version(&state).await;

        assert_eq!(body["valuation_date"], "2026-03-05");
        assert_eq!(body["data_revision"], state.revision.current());
        assert_eq!(body["prices_refreshing"], false);
    }

    #[tokio::test]
    async fn reads_do_not_change_the_revision() {
        let state = AppState::for_tests().await;
        let before = state.revision.current();

        let _ = get_data_version(&state).await;
        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/holdings")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::OK);

        assert_eq!(state.revision.current(), before);
    }

    #[tokio::test]
    async fn a_successful_mutation_changes_the_revision() {
        let state = AppState::for_tests().await;
        let before = state.revision.current();

        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/instruments")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "symbol": "TEST",
                            "exchange": "STO",
                            "name": "Test",
                            "type": "Stock",
                            "currency": "SEK"
                        })
                        .to_string(),
                    ))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert!(response.status().is_success(), "{}", response.status());

        assert_ne!(state.revision.current(), before);
    }

    #[tokio::test]
    async fn a_rejected_mutation_leaves_the_revision_alone() {
        let state = AppState::for_tests().await;
        let before = state.revision.current();

        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/instruments")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert!(response.status().is_client_error(), "{}", response.status());

        assert_eq!(state.revision.current(), before);
    }
}
```

There is no test for the server-error case: provoking a 5xx from a handler that
has already written would mean faking a failure inside the refresh service. The
rule is stated in the middleware's doc comment instead, and the launch refresh
test in Step 11 covers the same "wrote, then failed" shape.

- [ ] **Step 7: Run them and watch them fail**

Run: `cd backend && cargo test api::data_version::`
Expected: FAIL — the route does not exist (404) and the revision never changes.

- [ ] **Step 8: Implement the endpoint**

At the top of `backend/src/api/data_version.rs`:

```rust
use axum::{extract::State, Json};
use serde::Serialize;

use crate::state::AppState;

/// What snapshot of the data the app is serving, and what day it thinks it is.
/// Clients name this in every data request so panels cannot mix snapshots.
#[derive(Debug, Serialize)]
pub struct DataVersionResponse {
    pub data_revision: String,
    pub valuation_date: String,
    pub prices_refreshing: bool,
}

pub async fn handler(State(state): State<AppState>) -> Json<DataVersionResponse> {
    Json(DataVersionResponse {
        data_revision: state.revision.current(),
        valuation_date: state.clock.today().format("%Y-%m-%d").to_string(),
        prices_refreshing: state.market_data.is_refreshing(),
    })
}
```

Declare the module and route it in `backend/src/api/mod.rs`:

```rust
mod data_version;
```

```rust
        .route("/data-version", get(data_version::handler))
```

- [ ] **Step 9: Implement the bump middleware**

In `backend/src/api/mod.rs`, beside `demo_read_only_layer`:

```rust
/// Records that stored data may have changed. Every mutation reaches the app
/// through a mutating HTTP method, so bumping here means no handler has to
/// remember to do it.
///
/// A client error means the request was rejected before touching anything, so
/// the revision stays put. A server error does not: a refresh can write prices
/// and still fail afterwards, and that data must not stay hidden behind an
/// unchanged revision.
async fn data_revision_layer(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> axum::response::Response {
    let mutating = is_mutating_method(request.method());
    let response = next.run(request).await;

    if mutating && !response.status().is_client_error() {
        state.revision.bump();
    }

    response
}
```

Attach it in `api_mount` so it wraps every `/api` route, outside the demo layer:

```rust
fn api_mount(state: &AppState) -> Router<AppState> {
    let api = api_router()
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            demo_read_only_layer,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            data_revision_layer,
        ));

    Router::new()
        .nest("/api", api)
        .route("/api/", any(error::unmatched_route))
}
```

- [ ] **Step 10: Run the endpoint and middleware tests**

Run: `cd backend && cargo test api::data_version::`
Expected: PASS (4 tests).

- [ ] **Step 11: Make the launch refresh bump the revision**

In `backend/src/app.rs`, inside the task spawned by `spawn_launch_refresh`,
after the refresh call returns (success or failure — prices may have been
written either way):

```rust
    state.revision.bump();
    crate::engine_info!(
        "launch refresh finished; data revision is now {}",
        state.revision.current()
    );
```

Extend the existing `launch_refresh_spawns_background_job` test to assert the
revision changed once the spawned handle has been awaited:

```rust
        let before = state.revision.current();
        handle.await.expect("launch refresh task completes");
        assert_ne!(state.revision.current(), before);
```

- [ ] **Step 12: Stamp price-derived responses**

Add `pub data_revision: String` to `GainsResponse`
(`backend/src/api/gains/types.rs`), the holdings response
(`backend/src/api/holdings.rs`), the rebalance response
(`backend/src/api/rebalance.rs`) and the value-history response
(`backend/src/api/portfolio.rs`). Populate each with `state.revision.current()`
where the handler builds its response. Keep the field last in each struct so the
JSON shape stays stable for existing readers.

- [ ] **Step 13: Assert the stamp in one place**

Add to `backend/src/api/gains/tests.rs`:

```rust
#[tokio::test]
async fn gains_response_names_the_data_revision_it_was_computed_from() {
    let state = AppState::for_tests().await;

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data_revision"], state.revision.current());
}
```

- [ ] **Step 14: Bump the backend version and run the full gate**

Set `version = "0.17.0"` in `backend/Cargo.toml`.

Run: `cd backend && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all green.

- [ ] **Step 15: Commit**

```bash
git add backend
git commit -m "Serve a data revision the client can name in every request"
```

---

### Phase 3 — Every client query names the snapshot it belongs to

**Files:**
- Create: `frontend/src/api/dataVersion.ts`,
  `frontend/src/api/versionedQueries.test.tsx`
- Modify: `frontend/src/api/queries.ts`, `frontend/src/api/types.ts`,
  `frontend/src/components/PortfolioLayout.tsx`,
  `frontend/src/components/AddTransactionForm.tsx`,
  `frontend/package.json`
- Delete: `frontend/src/api/priceRefreshCompletion.ts`,
  `frontend/src/api/priceRefreshCompletion.test.tsx`

**Interfaces:**
- Consumes: `GET /api/data-version` from Phase 2.
- Produces: `DataVersion` type and `versionToken(version): string`;
  `useDataVersion()`; every existing hook re-keyed on the token;
  `AddTransactionForm` seeded with a server-supplied trade date.

- [ ] **Step 1: Write the failing token test**

Create `frontend/src/api/dataVersion.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { type DataVersion, versionToken } from "./dataVersion";

const version: DataVersion = {
  data_revision: "1757682000123:7",
  valuation_date: "2026-09-12",
  prices_refreshing: false,
};

describe("versionToken", () => {
  it("combines the revision and the server's date", () => {
    expect(versionToken(version)).toBe("1757682000123:7@2026-09-12");
  });

  it("changes when either part changes", () => {
    expect(versionToken({ ...version, data_revision: "x:8" })).not.toBe(
      versionToken(version),
    );
    expect(versionToken({ ...version, valuation_date: "2026-09-13" })).not.toBe(
      versionToken(version),
    );
  });
});
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd frontend && npx vitest run src/api/dataVersion.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the module**

Create `frontend/src/api/dataVersion.ts`:

```ts
/**
 * What snapshot of the backend's data a response belongs to, and what day the
 * backend thinks it is. Every data query names this, so two panels cannot show
 * numbers computed from different moments.
 */
export interface DataVersion {
  data_revision: string;
  valuation_date: string;
  prices_refreshing: boolean;
}

export function versionToken(version: DataVersion): string {
  return `${version.data_revision}@${version.valuation_date}`;
}
```

- [ ] **Step 4: Run the test**

Run: `cd frontend && npx vitest run src/api/dataVersion.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Add the heartbeat and the versioned-query helper**

In `frontend/src/api/queries.ts`, above the existing hooks:

```ts
/**
 * The backend's snapshot heartbeat. Polled quickly while a price refresh runs,
 * slowly otherwise, and re-checked when the window regains focus. Everything
 * else keys off it, so this is the only query that decides when the app as a
 * whole moves to newer data.
 */
export function useDataVersion() {
  return useQuery({
    queryKey: ["data-version"],
    queryFn: () => apiGet<DataVersion>("/api/data-version"),
    refetchInterval: (query) =>
      query.state.data?.prices_refreshing ? 2000 : 15_000,
    // A hidden tab stops polling; returning to it re-checks immediately, which
    // is what the focus refetch is for.
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    staleTime: 0,
  });
}

/**
 * Define a data query that belongs to one snapshot. The token is part of the
 * cache key, so results from different snapshots can never share an entry, and
 * a version change refetches every panel together.
 *
 * Previous data is kept while the new snapshot loads, so a panel shows the
 * older numbers briefly rather than emptying out on every refresh, transaction
 * or restart.
 *
 * A response that names a revision other than the one asked for means the
 * snapshot moved while the request was in flight. Rechecking the version is
 * enough: the token changes and everything refetches together.
 */
function useVersionedQuery<T>(
  key: readonly unknown[],
  path: string,
  options: { enabled?: boolean } = {},
) {
  const queryClient = useQueryClient();
  const version = useDataVersion();
  const token = version.data ? versionToken(version.data) : null;
  const requestedRevision = version.data?.data_revision ?? null;

  return useQuery({
    queryKey: [key[0], token, ...key.slice(1)],
    queryFn: async () => {
      const data = await apiGet<T>(path);
      const served = (data as { data_revision?: string } | null)?.data_revision;
      if (served !== undefined && served !== requestedRevision) {
        void queryClient.invalidateQueries({ queryKey: ["data-version"] });
      }
      return data;
    },
    enabled: token !== null && (options.enabled ?? true),
    placeholderData: keepPreviousData,
  });
}
```

The stamp check is what closes the remaining race: the client can read the
version, a refresh can finish, and two requests can then land on either side of
the bump. Both would otherwise be filed under the old token — the exact shape of
incident 2. Responses without a `data_revision` field (instruments,
transactions, price status) skip the check.

Import `DataVersion` and `versionToken` from `./dataVersion` at the top of the
file.

- [ ] **Step 6: Move every data hook onto the helper**

Rewrite each hook to build its path and call `useVersionedQuery`. `useGains`
becomes:

```ts
export function useGains(params: GainsParams = {}) {
  const { includeClosedPositions = false, startDate, endDate, method } = params;
  const search = new URLSearchParams();
  if (includeClosedPositions) search.set("include_closed", "true");
  if (startDate) search.set("start_date", startDate);
  if (endDate) search.set("end_date", endDate);
  if (method) search.set("method", method);
  const qs = search.toString();

  return useVersionedQuery<GainsResponse>(
    ["gains", includeClosedPositions, startDate ?? null, endDate ?? null, method ?? null],
    `/api/gains${qs ? `?${qs}` : ""}`,
  );
}
```

Apply the same treatment to `useInstruments`, `useTransactions`, `useHoldings`,
`usePriceStatus`, `useInstrumentPrices` (keep `enabled: id !== null`),
`usePortfolioValueHistory` and `useRebalancePlan` (keep
`enabled: normalizedAmount !== null`). The explicit `keepPreviousData` lines on
the existing hooks go away: the helper now does it for every versioned query, so
no panel empties out when the snapshot changes. `usePriceStatus` keeps its own
`refetchInterval` while refreshing — write that one with `useQuery` directly,
building its key and `enabled` the same way from `useDataVersion()`, and keep
`placeholderData: keepPreviousData` on it too.

`useHealth` and `useDataVersion` stay unversioned.

- [ ] **Step 7: Delete the invalidation lists**

Remove `invalidatePriceDerivedData`, `invalidatePortfolioData` and
`invalidateInstrumentData`, and make every mutation's `onSuccess` body:

```ts
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["data-version"] });
    },
```

Delete `frontend/src/api/priceRefreshCompletion.ts` and its test, and remove the
completion effect from `usePriceStatus` — the heartbeat now covers refreshes
that the backend starts on its own.

- [ ] **Step 8: Write the failing rule test over the module's exports**

Create `frontend/src/api/versionedQueries.test.tsx`:

```tsx
// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as queries from "./queries";

const VERSION = {
  data_revision: "r:1",
  valuation_date: "2026-09-12",
  prices_refreshing: false,
};

// Hooks that deliberately do not name a snapshot: the heartbeat itself, and
// health, which describes the process rather than the data.
const UNVERSIONED = new Set(["useDataVersion", "useHealth"]);

// Arguments for hooks that fetch nothing until they are given one.
const HOOK_ARGS: Record<string, unknown[]> = {
  useInstrumentPrices: [7],
  useRebalancePlan: ["1000", "sek"],
};

function stubFetch() {
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      const body = url.startsWith("/api/data-version") ? VERSION : { rows: [] };
      return Promise.resolve({
        status: 200,
        ok: true,
        text: () => Promise.resolve(JSON.stringify(body)),
      } as unknown as Response);
    }),
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("every data hook names the snapshot", () => {
  const hookNames = Object.keys(queries).filter(
    (name) =>
      name.startsWith("use") &&
      !name.startsWith("useUpdate") &&
      !name.startsWith("useCreate") &&
      !name.startsWith("useDelete") &&
      !name.startsWith("useUpsert") &&
      !name.startsWith("useRefresh") &&
      !name.startsWith("usePreview") &&
      !name.startsWith("useCommit") &&
      !name.startsWith("useRollback") &&
      !UNVERSIONED.has(name),
  );

  it("covers at least the known data hooks", () => {
    expect(hookNames).toEqual(
      expect.arrayContaining([
        "useGains",
        "useHoldings",
        "useInstruments",
        "useTransactions",
        "usePriceStatus",
        "usePortfolioValueHistory",
      ]),
    );
  });

  it.each(hookNames)("%s puts the version token in its cache key", async (name) => {
    stubFetch();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const hook = (queries as Record<string, (...args: unknown[]) => unknown>)[
      name
    ];

    renderHook(() => hook(...(HOOK_ARGS[name] ?? [])), { wrapper });

    // Active queries only: a versioned hook renders once with a null token
    // before the heartbeat resolves, and that disabled entry stays in the cache
    // until garbage collection.
    await waitFor(() => {
      const keys = queryClient
        .getQueryCache()
        .findAll({ type: "active" })
        .map((query) => query.queryKey)
        .filter((key) => key[0] !== "data-version");
      expect(keys.length).toBeGreaterThan(0);
      for (const key of keys) {
        expect(key[1]).toBe("r:1@2026-09-12");
      }
    });
  });
});
```

A new hook that forgets the helper, or that fetches nothing without arguments,
fails this test until it is either versioned or given arguments in `HOOK_ARGS`.

- [ ] **Step 9: Write the failing behaviour test for a refresh started elsewhere**

Add to the same file:

```tsx
describe("a snapshot change refetches data", () => {
  it("refetches gains when the backend's revision moves", async () => {
    let version = { ...VERSION };
    const gainsRequests: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        const isVersion = url.startsWith("/api/data-version");
        if (!isVersion) gainsRequests.push(url);
        return Promise.resolve({
          status: 200,
          ok: true,
          text: () =>
            Promise.resolve(JSON.stringify(isVersion ? version : { rows: [] })),
        } as unknown as Response);
      }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(() => queries.useGains(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(gainsRequests).toHaveLength(1);

    version = { ...VERSION, data_revision: "r:2" };
    await queryClient.refetchQueries({ queryKey: ["data-version"] });

    await waitFor(() => expect(gainsRequests).toHaveLength(2));
  });

  it("refetches gains when the server's day changes", async () => {
    let version = { ...VERSION };
    const gainsRequests: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        const isVersion = url.startsWith("/api/data-version");
        if (!isVersion) gainsRequests.push(url);
        return Promise.resolve({
          status: 200,
          ok: true,
          text: () =>
            Promise.resolve(JSON.stringify(isVersion ? version : { rows: [] })),
        } as unknown as Response);
      }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(() => queries.useGains(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    version = { ...VERSION, valuation_date: "2026-09-13" };
    await queryClient.refetchQueries({ queryKey: ["data-version"] });

    await waitFor(() => expect(gainsRequests).toHaveLength(2));
  });
});
```

- [ ] **Step 10: Run the frontend suite**

Run: `cd frontend && npx vitest run`
Expected: PASS. Component tests mock `../api/queries` wholesale rather than
stubbing `fetch`, so they should be unaffected; the tests that do change are the
ones Phase 4 touches. Any test that *does* stub `fetch` now needs a
`/api/data-version` answer in the stub.

- [ ] **Step 11: Seed the transaction form from the server's date**

In `frontend/src/components/AddTransactionForm.tsx`, change
`createInitialState(hasInstruments: boolean)` to
`createInitialState(hasInstruments: boolean, tradeDate: string)` and use that
argument instead of `new Date().toISOString().slice(0, 10)`. Give
`AddTransactionForm` a `tradeDate: string` prop and pass it through; in
`PortfolioLayout`, supply `useDataVersion().data?.valuation_date ?? ""`, so the
date field is empty rather than wrong while the version is unknown. In practice
it is never empty: the form mounts only after the user opens it
(`PortfolioLayout.tsx:59`), by which time the heartbeat has answered.

`AddTransactionForm.tsx:140` uses `createInitialState` as `useReducer`'s lazy
initialiser, which passes exactly one argument, so wrap it:

```tsx
  const [state, dispatch] = useReducer(
    addTransactionReducer,
    { hasInstruments: instruments.length > 0, tradeDate },
    (init) => createInitialState(init.hasInstruments, init.tradeDate),
  );
```

Add to the existing `frontend/src/components/AddTransactionForm.reducer.test.ts`
— a pure test at the reducer boundary, which is where this file's other tests
live:

```ts
it("seeds the trade date from the server's valuation date", () => {
  expect(createInitialState(true, "2026-09-12").tradeDate).toBe("2026-09-12");
});

it("leaves the trade date empty when the server date is unknown", () => {
  expect(createInitialState(true, "").tradeDate).toBe("");
});
```

Update the other `createInitialState` calls in that file to pass a date.

- [ ] **Step 12: Bump the frontend version and run the full gate**

Set `"version": "0.23.0"` in `frontend/package.json`.

Run: `cd frontend && npm run fmt && npm run check`
Expected: all green, no Biome warnings.

- [ ] **Step 13: Commit**

```bash
git add frontend
git commit -m "Key every client query on the backend's data snapshot"
```

---

### Phase 4 — The backend resolves report periods

**Files:**
- Create: `backend/src/api/gains/period.rs`
- Modify: `backend/src/api/gains.rs`, `backend/src/api/gains/types.rs`,
  `backend/src/api/gains/tests.rs`, `frontend/src/api/queries.ts`,
  `frontend/src/api/types.ts`, `frontend/src/App.tsx`,
  `frontend/src/components/DateRangeSelector.tsx`,
  `frontend/src/components/DateRangeSelector.test.tsx`,
  `frontend/src/components/Dashboard.tsx`,
  `frontend/src/components/Dashboard.test.tsx` (renders `Dashboard` with a
  `dateRange` prop at line 118, which this phase replaces),
  `frontend/src/components/GainsPage.tsx`,
  `frontend/src/components/GainsTable.tsx`,
  `frontend/src/api/versionedQueries.test.tsx`, `backend/Cargo.toml`
- Delete: `frontend/src/components/useLocalDate.ts` and its tests

**Interfaces:**
- Consumes: `Clock` (Phase 1), the snapshot token (Phase 3).
- Produces: `resolve_period(period: Option<&str>, start: Option<NaiveDate>, end: Option<NaiveDate>, today: NaiveDate) -> Result<ResolvedPeriod, ApiError>`
  where `ResolvedPeriod { start: Option<NaiveDate>, end: NaiveDate }`;
  `GainsParams.period?: DatePreset` on the client.

- [ ] **Step 1: Write the failing period-resolution test**

Create `backend/src/api/gains/period.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    #[test]
    fn presets_resolve_against_today() {
        let today = d(2026, 9, 12);

        for (preset, start, end) in [
            ("today", Some(d(2026, 9, 12)), d(2026, 9, 12)),
            ("7d", Some(d(2026, 9, 5)), d(2026, 9, 12)),
            ("12m", Some(d(2025, 9, 12)), d(2026, 9, 12)),
            ("ytd", Some(d(2026, 1, 1)), d(2026, 9, 12)),
            ("all", None, d(2026, 9, 12)),
        ] {
            let resolved = resolve_period(Some(preset), None, None, today)
                .unwrap_or_else(|_| panic!("{preset} resolves"));
            assert_eq!(resolved.start, start, "{preset} start");
            assert_eq!(resolved.end, end, "{preset} end");
        }
    }

    #[test]
    fn twelve_months_back_from_a_leap_day_starts_on_first_march() {
        let leap_day = d(2028, 2, 29);

        let resolved = resolve_period(Some("12m"), None, None, leap_day).expect("resolves");

        assert_eq!(resolved.start, Some(d(2027, 3, 1)));
        assert_eq!(resolved.end, leap_day);
    }

    #[test]
    fn explicit_dates_win_and_default_their_end_to_today() {
        let today = d(2026, 9, 12);

        let resolved = resolve_period(None, Some(d(2026, 2, 1)), Some(d(2026, 6, 29)), today)
            .expect("explicit range resolves");
        assert_eq!(resolved.start, Some(d(2026, 2, 1)));
        assert_eq!(resolved.end, d(2026, 6, 29));

        let open_ended = resolve_period(Some("custom"), Some(d(2026, 2, 1)), None, today)
            .expect("open-ended custom range resolves");
        assert_eq!(open_ended.end, today);
    }

    #[test]
    fn an_unknown_preset_is_a_bad_request() {
        let error = resolve_period(Some("last-tuesday"), None, None, d(2026, 9, 12))
            .expect_err("unknown preset is rejected");
        assert_eq!(error.code(), "invalid_period");
    }

    #[test]
    fn a_start_after_the_end_is_a_bad_request() {
        let error = resolve_period(None, Some(d(2026, 9, 13)), Some(d(2026, 9, 12)), d(2026, 9, 12))
            .expect_err("inverted range is rejected");
        assert_eq!(error.code(), "start_date_after_end_date");
    }
}
```

`ApiError` keeps its `code` field private and has no accessor today, so add one
next to the constructors in `backend/src/api/error.rs` rather than asserting on
a serialized body in a unit test:

```rust
    pub fn code(&self) -> &'static str {
        self.code
    }
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd backend && cargo test api::gains::period::`
Expected: FAIL — `resolve_period` does not exist.

- [ ] **Step 3: Implement the resolver**

```rust
use chrono::{Datelike, Duration, NaiveDate};

use crate::api::error::ApiError;

/// A report period the backend has resolved. `start: None` means "since the
/// first transaction", which the caller derives from the ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedPeriod {
    pub start: Option<NaiveDate>,
    pub end: NaiveDate,
}

/// Resolve the period a report covers. Clients send an intent — the preset a
/// person picked — and the backend turns it into dates with its own clock, so a
/// long-open page cannot keep asking for a day that has passed.
pub fn resolve_period(
    period: Option<&str>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    today: NaiveDate,
) -> Result<ResolvedPeriod, ApiError> {
    let resolved = match period {
        None | Some("custom") => ResolvedPeriod {
            start: start_date,
            end: end_date.unwrap_or(today),
        },
        Some("today") => ResolvedPeriod {
            start: Some(today),
            end: today,
        },
        Some("7d") => ResolvedPeriod {
            start: Some(today - Duration::days(7)),
            end: today,
        },
        Some("12m") => ResolvedPeriod {
            // 29 February has no counterpart a year earlier; the browser's
            // Date arithmetic rolled it to 1 March, so match that rather than
            // changing what the report covers.
            start: Some(today.with_year(today.year() - 1).unwrap_or_else(|| {
                NaiveDate::from_ymd_opt(today.year() - 1, 3, 1)
                    .expect("1 March exists in every year")
            })),
            end: today,
        },
        Some("ytd") => ResolvedPeriod {
            start: NaiveDate::from_ymd_opt(today.year(), 1, 1),
            end: today,
        },
        Some("all") => ResolvedPeriod {
            start: None,
            end: today,
        },
        Some(other) => {
            return Err(ApiError::bad_request(
                "invalid_period",
                format!("invalid period: {other}"),
            ))
        }
    };

    if let Some(start) = resolved.start {
        if start > resolved.end {
            return Err(ApiError::bad_request(
                "start_date_after_end_date",
                "start_date must not be after end_date",
            ));
        }
    }

    Ok(resolved)
}
```

Note on `12m`: `with_year` returns `None` only for 29 February. Falling back to
1 March reproduces what the browser's `setFullYear` did, so the period a person
sees does not change with this refactor.

- [ ] **Step 4: Run the resolver tests**

Run: `cd backend && cargo test api::gains::period::`
Expected: PASS (4 tests).

- [ ] **Step 5: Wire it into the gains handler**

Add `pub(super) period: Option<String>` to `GainsQuery`, declare `mod period;`
in `backend/src/api/gains.rs`, and replace the current `end_date`/`start_date`
block with:

```rust
    let resolved = period::resolve_period(
        query.period.as_deref(),
        query
            .start_date
            .as_deref()
            .map(|s| parse_date(s, "start_date"))
            .transpose()?,
        query
            .end_date
            .as_deref()
            .map(|s| parse_date(s, "end_date"))
            .transpose()?,
        state.clock.today(),
    )?;
    let end_date = resolved.end;
    let start_date = resolved.start;
```

Everything downstream keeps using `start_date` and `end_date` unchanged.

- [ ] **Step 6: Write the failing handler test for the preset**

Add to `backend/src/api/gains/tests.rs`:

```rust
#[tokio::test]
async fn a_period_preset_is_resolved_with_the_state_clock() {
    let state = AppState::for_tests().await.with_clock(crate::clock::Clock::fixed(
        chrono::NaiveDate::from_ymd_opt(2026, 3, 5).expect("valid date"),
    ));

    let (status, body) = send(&state, "GET", "/api/gains?period=ytd", json!({})).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["report_period"]["start_date"], "2026-01-01");
    assert_eq!(body["report_period"]["end_date"], "2026-03-05");
    assert_eq!(body["as_of_date"], "2026-03-05");
}

#[tokio::test]
async fn an_unknown_period_is_rejected() {
    let state = AppState::for_tests().await;

    let (status, body) = send(&state, "GET", "/api/gains?period=last-tuesday", json!({})).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_period");
}
```

- [ ] **Step 7: Run the backend gate**

Run: `cd backend && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all green.

- [ ] **Step 8: Send intent, not dates, from the client**

In `frontend/src/api/queries.ts`, extend `GainsParams` with
`period?: DatePreset` and add `if (period) search.set("period", period);` plus
`period ?? null` to the cache key.

In `frontend/src/components/DateRangeSelector.tsx`:

- Delete `presetToRange`, `activeDateRange` and `localDateFromString`.
- Keep `DatePreset`, the persistence helpers and the component.
- Change the persisted shape to `{ datePreset, customRange }`, and make
  `loadDateRangeSelection` accept the previous `{ datePreset, dateRange }` shape:
  when `datePreset` is `"custom"`, read `customRange` and fall back to
  `dateRange`; otherwise return the default empty custom range.
- Give the component a `valuationDate: string | null` prop, used as the
  placeholder value of the custom end-date input when it is empty.

In `frontend/src/App.tsx`, drop `useLocalDate` and `activeDateRange`; pass
`datePreset` and `customRange` to the pages, plus
`valuationDate={dataVersion.data?.valuation_date ?? null}`.

In `Dashboard.tsx` and `GainsPage.tsx`, call
`useGains({ period: datePreset, startDate: customRange.startDate, endDate: customRange.endDate, ... })`,
sending `startDate`/`endDate` only when `datePreset === "custom"`.

In `Dashboard.tsx`, filter the value-history chart with the resolved period from
the response instead of a client-computed range:

```tsx
  const reportPeriod = gainsQuery.data?.report_period;
  const filteredHistory = useMemo(
    () =>
      filterValueHistoryPoints(history ?? [], {
        startDate: reportPeriod?.start_date ?? null,
        endDate: reportPeriod?.end_date ?? null,
      }),
    [history, reportPeriod],
  );
```

In the same edit, `Dashboard.tsx:242` passes the old range to the chart; change
it to the resolved period so the chart's left edge matches the numbers:

```tsx
        visibleStart={
          reportPeriod?.start_date ?? query.data?.start_date ?? undefined
        }
```

- [ ] **Step 9: Delete the client's clock**

Delete `frontend/src/components/useLocalDate.ts` and the `useLocalDate` and
`activeDateRange` blocks from
`frontend/src/components/DateRangeSelector.test.tsx`. Do not replace them with a
page-level test: the component tests in this repo mock `../api/queries`
wholesale, so a page would never issue a request to observe. Assert the
behaviour at the effect-layer boundary instead, in the Phase 3 file
`frontend/src/api/versionedQueries.test.tsx`, which already stubs `fetch`:

```tsx
it("sends the chosen preset as the period", async () => {
  const requests: string[] = [];
  stubFetchRecording(requests);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  renderHook(() => queries.useGains({ period: "ytd" }), { wrapper });

  await waitFor(() =>
    expect(requests.some((url) => url.includes("period=ytd"))).toBe(true),
  );
});
```

`stubFetchRecording` is the recording stub already written for that file's
"a snapshot change refetches data" tests; extract it into a local helper there
when adding this test.

- [ ] **Step 10: Bump the backend version for the contract change**

`GET /api/gains` gained a query parameter after Phase 2 set `0.17.0`, so set
`version = "0.17.1"` in `backend/Cargo.toml` and confirm the footer shows it.

- [ ] **Step 11: Run both gates**

Run: `cd backend && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Run: `cd frontend && npx vitest run && npm run check && npm run fmt`
Expected: all green.

- [ ] **Step 12: Commit**

```bash
git add backend frontend
git commit -m "Resolve report periods on the backend from the client's intent"
```

---

### Phase 5 — Documentation and decision log

**Files:**
- Modify: `README.md`, `docs/DecisionLog.md`

- [ ] **Step 1: Document the endpoint and the timezone rule in `README.md`**

`README.md` has no API or configuration reference section. Add this to "Running
the application", after the paragraph about the Vite proxy and `/api/health`
(around line 40):

```markdown
- `GET /api/data-version` reports the data revision the backend is serving, the
  date it considers today, and whether a price refresh is running. The frontend
  names that revision in every data request, so all panels show one snapshot.
  "Today" is the backend machine's local date; the browser never decides it.
```

- [ ] **Step 2: Append the decision log entries**

Append to `docs/DecisionLog.md`, at the end of the file:

```markdown
## 2026-09-12 - One Clock Owns Today
Decision: The backend is the only component that decides what day it is. A single clock on application state answers "today" for every valuation, staleness comparison and refresh window, using the machine's local timezone, and the global now-functions are rejected by the lint gate everywhere else. Instants used for timestamps enter through the same module. Clients never compute a date for a request; they send the period they want and the backend resolves it, returning the resolved period in the response.
Context: Six backend sites derived today independently in two timezones, and the browser derived it twice more, so surfaces disagreed by a day around local midnight and a page left open across midnight kept asking for a day that had passed.
Consequences: Adding a surface that needs today's date means reading the state clock; tests pin the clock instead of depending on the machine's date. Report period presets are part of the API contract, and adding one is a backend change. The app's timezone is the backend machine's timezone, which is a deliberate limitation for a single-user deployment.

## 2026-09-12 - Data Is Served And Requested By Revision
Decision: The backend keeps a process-local data revision that changes whenever stored data may have changed, bumped by the middleware that sees every mutating request and by the launch refresh job, never by individual handlers. A rejected request leaves the revision alone; a request that fails after writing does not. It is published together with today's date and the refresh state on a single endpoint, and price-derived responses carry the revision they were computed from. Every client data query includes the revision and date in its cache key, so results from different snapshots cannot share a cache entry and one version change refetches every panel; mutations invalidate only the version. A response whose revision differs from the one requested makes the client recheck the version rather than keep the mismatched pair.
Context: Two panels showed numbers computed before and after a background price refresh, with nothing in either response to tell them apart, because keeping views in step depended on hand-maintained invalidation lists that a new query could silently fall outside of.
Consequences: Cache invalidation is no longer a per-feature decision. A restart or a day change is a revision change and refetches everything, at the cost of one extra round trip when a page first loads. The revision is not persisted and describes one process, so sharing a ledger between processes would need a persisted counter. Every mutating request refetches every panel, including import previews that write nothing, which is accepted rather than maintaining a list of exceptions. Panels keep their previous numbers while refetching and transition one query at a time, so the screen can briefly show one older panel beside a newer one; a shared "updating" state is future work.
```

- [ ] **Step 3: Verify the docs build and commit**

Run: `cd frontend && npm run fmt`
Expected: no changes to documentation files (Biome does not format Markdown; the
command is only to confirm the tree is clean).

```bash
git add README.md docs/DecisionLog.md
git commit -m "Record the snapshot consistency decisions"
```

---

## Verification summary

| Phase | Automated | Manual |
| --- | --- | --- |
| 1 | `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt` | none |
| 2 | as above, plus the four data-version tests | `curl` the endpoint; confirm the revision changes after adding a transaction |
| 3 | `npx vitest run`, `npm run check` | human gate below, items 1–3 |
| 4 | both suites | human gate below, items 4–5 |
| 5 | — | read back the decision log entries |

### Human gate (run once, after Phase 4)

1. Start the app and open the dashboard. The top card's day change and the
   table's TOTAL "Today" column agree, in both amount and percent.
2. Click Refresh prices. Both update together; neither lags behind.
3. Restart the backend with the page left open. Within about fifteen seconds the
   page moves to the new prices, and every panel moves at once.
4. Open Add transaction. The trade date is today's local date. Repeat between
   00:00 and 02:00 local time, or with the machine clock set into that window.
5. With the page open, set the machine clock past midnight. Within about fifteen
   seconds the "All" period follows the new day without a reload, and the
   summary strip and table agree.

Record the outcome of this gate in this file, following the pattern of the
"Gate record" section in `docs/plans/Plan.ProductionSetupHardening.md`.

#### Gate record — 2026-09-12

**Run by the maintainer (pass).** The five checklist items were exercised by
hand against a running app after report-period resolution moved to the backend,
and reported as behaving as described: the dashboard's day change agrees with
the table's "Today" total, a manual refresh and a backend restart move every
panel together, the transaction form's trade date comes from the server, and a
machine-clock rollover past midnight is followed without a reload. Per-item
evidence was not captured, so this record reflects the maintainer's overall
judgement rather than five separately logged observations.

The one behaviour worth re-checking if it becomes annoying is the chart's
one-round-trip lag behind the period buttons: the chart's bounds now come from
the response's resolved period, so a preset click leaves the previous bounds in
place until gains answers, and first paint briefly shows the whole history. This
is the "mixed state during a transition" residual below, not a defect.

## Risks and residuals

- **Mixed state during a transition.** Panels refetch independently and keep
  their previous data while doing so, so for the duration of a refetch one panel
  shows the older snapshot beside a newer one. The window is short and each
  panel is internally consistent, but the screen as a whole is briefly mixed.
  A shared "updating" state is the follow-up; the numbers are never silently
  wrong, only briefly old.
- **A response stamps the revision, not the valuation date.** If the server's
  day rolls over without a mutation, a panel refetching under the previous
  token can receive next-day numbers whose revision still matches. Panels can
  then disagree by a day until the heartbeat catches up, bounded at fifteen
  seconds. This is accepted for now; Phase 4 rewrites these response contracts
  while resolving report periods and closes the gap as one coordinated change.
- **In-flight snapshot changes are healed, not prevented.** A refresh can finish
  between the version fetch and a data request. The response's revision stamp
  makes the client notice and refetch, so the mismatch lasts one round trip
  rather than being filed permanently under the old token.
- **Import previews bump the revision.** `POST /api/import/*/preview` writes
  nothing but uses a mutating method, so each preview refetches every panel.
  Accepted as harmless noise rather than special-casing paths in the middleware,
  which would reintroduce a list to maintain.
- **One extra round trip at first paint.** Data queries wait for the version.
  Locally this is a few milliseconds; on a slow link it delays first data.
- **Coarse revision.** Any successful mutation refetches every data query, not
  just the affected one. With this data volume that is cheap and the
  simplification is the point.
- **Heartbeat latency.** A refresh the backend starts on its own becomes visible
  within about fifteen seconds, or two while `prices_refreshing` is true.
- **The Clippy ban needs a home for new time uses.** A legitimate new instant
  must go through `clock::now_utc()`; adding `#[allow]` elsewhere defeats the
  guarantee and should be rejected in review.
- **Rejected mutations that still wrote.** The middleware skips the bump only
  for client errors, which are rejections that never reached the data. A
  handler that validated, wrote, and then returned 400 would hide its write;
  none does today, and the rule is stated where the middleware is defined.
