# Plan — Nasdaq Nordic as a second automatic price provider

## Summary

Add Nasdaq Nordic (`https://api.nasdaq.com/api/nordic`) as a second automatic
price source so Nordic-listed holdings Yahoo cannot price get valued without any
hand entry, and rebuild the price read path around *several* sources resolved
per date instead of one hardcoded `"YAHOO"` string.

Done means: instrument 32 "AVA SAMSUNG TRACKER" (ISIN `JE00BJ7HNC92`, SEK,
exchange `AVANZA`), its five sibling AVA trackers (`JE00BLH0QR80`,
`JE00BJ7H8575`, `JE00BM8DHK30`, `JE00BL4PPM48`, `JE00BLH0LQ29`) and instrument 38
AstraZeneca (`GB0009895292`, SEK, whose `AZN.L` Yahoo mapping the refresh
auto-disabled on a GBP/SEK currency mismatch) all show real market values,
gains and price charts; every holding that works today keeps working; and the
portfolio-level movement caused by six instruments entering valuation is
measured and explained rather than assumed away.

**Out of scope:** hand-entered prices (`docs/plans/Plan.HandEnteredPrices.md`
stays shelved and unbuilt), any third provider, the day-change adjacency/FX
pairing defect (see *Deliberately out of scope*), and an ambiguity review queue
in the UI.

## Why now, and what this replaces

Yahoo returns zero quotes for `JE00BJ7HNC92`, so `seed_provider_symbols` never
creates an `instrument_provider_symbols` row, `load_valuation_inputs`
(`backend/src/api/valuation.rs`) sets `price_mapping_enabled = false`,
`backend/src/api/valued_holdings.rs` sets `valuation: None`, the
`#[serde(skip_serializing_if)]` on the `valuation` field in
`backend/src/api/holdings.rs:26-27` drops it from the response, and the UI shows
"Valuation missing" and sorts the row last. Every refresh for weeks has logged
`[WARN] market data symbol search returned no supported match instrument_id=32
isin=JE00BJ7HNC92`.

`docs/plans/Plan.HandEnteredPrices.md` was written for exactly this instrument.
Its motivating premise — that no provider carries these securities — is false:
Nasdaq Nordic carries all six. That plan is not deleted and not built; it is
used here as the **design document for the multi-source refactor**, which this
work needs regardless of whether prices arrive from a feed or a keyboard.

## Verified provider facts (probed live)

These are the facts the implementation may rely on. Raw captures now live in
`backend/tests/fixtures/market_data/`, and `scripts/probe-nasdaq-nordic.ps1`
re-checks every fact here against the live endpoint.

- `GET /search?searchText=<ISIN>` →
  `{"data":[{"group":"Warrants","instruments":[{"orderbookId":"TX2997672","fullName":"AVA SAMSUNG TRACKER","isin":"JE00BJ7HNC92","symbol":"AVA SAMSUNG TRACKER","assetClass":"TRACKER_CERTIFICATES","currency":"SEK"}]}],"messages":null,"status":{…}}`
- `GET /instruments/{orderbookId}/price-history?assetClass=<assetClass>&fromDate=YYYY-MM-DD&toDate=YYYY-MM-DD`
  → `data.priceHistory.headers` (a label map, not data) and
  `data.priceHistory.rows[]`, each row
  `{date, high, low, close, atap, totalVolume, turnover, duration, yield, ycp, priceAPA, volumeAPA, turnoverAPA}`.
  309 rows for a ~15-month window in one 64 KB response. No pagination.
- **The price-history payload carries no currency.** Currency is only available
  from `search`. This is load-bearing — see *Where Nasdaq's quote currency comes
  from* below.
- **A search that matches nothing is a success, not an error.** `GET
  /search?searchText=<unknown>` →  HTTP 200 with
  `{"data":null,"messages":{"code":"NO_INST_FOUND","message":"No instruments found"},"status":{"rCode":200,"bCodeMessage":null,…}}`.
  Both `rCode` and the HTTP status are 200 and `bCodeMessage` is null; the only
  marker is `data: null` plus a `messages` **object** (on a search that does
  match, `messages` is null and `data` is the group array). The search client
  therefore returns an empty match list here, never a provider error. This is
  load-bearing for the phases that branch on "no candidates" versus "the
  provider failed" — auto-connect leaves such an instrument unmapped rather
  than reporting a failure, and the add-instrument lookup owes the user
  `no_match` rather than `provider_unavailable`. Verified live against three
  unrelated nonsense queries; pinned by
  `nasdaq_search_no_match.json` and by a probe check.
- All numbers are strings with comma thousands separators (`"1,609.00"`), and
  absent fields are empty strings (`""`).
- Requires only a non-default `User-Agent`. No key, auth, cookies, Origin or
  Referer. Six rapid sequential calls all returned 200 in 1–3 s.
- Unknown instrument → typed envelope
  `{"data":null,"status":{"rCode":400,"bCodeMessage":[{"code":10003,"errorMessage":"Instrument not found"}]}}`.
- **History silently clamps to ~10 years.** `fromDate=2010-01-01` for
  AstraZeneca returned rows starting 2016-08-29 with no error and no warning.
- **ISIN → instrument is not unique.** Ericsson B (`SE0000108656`) returns
  `TX50143` (SHARES, EUR, Helsinki) and `TX69` (SHARES, SEK, Stockholm).
- **Nasdaq price history is split-adjusted on the same basis as Yahoo.** Probed
  Investor B (`TX76`) across its May 2021 4:1 split: no discontinuity, and all
  128 overlapping trading days agree with Yahoo's adjusted `INVE-B.ST` series
  within 0.1%; one day differs by 0.026%. This matters because the 2026-06-19
  split-adjusted-quantities decision normalizes ledger quantities on the stated
  rationale that provider closes are split-adjusted; had Nasdaq been unadjusted,
  every Nasdaq-priced instrument with a split would have been silently
  mis-valued in money of record. It is
  not. No design work follows, but the evidence is pinned by a fixture-backed
  test and re-checked live by the probe script.
- `GET /instruments/{orderbookId}/info?assetClass=…` returns an intraday last
  sale, bid/ask and ranges. **Deliberately unused** — it is not an end-of-day
  close, and mixing an intraday number into a daily close series would corrupt
  both the chart and the staleness tri-state.

## Settled decisions (implement these; do not re-litigate)

1. **Build the automatic provider; hand entry stays shelved.** No typing, no
   upkeep, and it fixes six-plus instruments rather than one.
2. **Adopt the shelved plan's design foundation now**: a pure
   `backend/src/domain/price_resolution.rs`; a domain-level `ProviderCode`
   provenance type so `backend/src/domain/` stays free of provider types;
   **per-date** precedence resolution; the rule that stored rows behind a
   *disabled* mapping are never promoted into valuation; provider constants
   consolidated in `providers/mod.rs`; refresh reporting that keeps PARTIAL
   meaning "action needed"; and a frontend `priceSourceLabel` replacing the
   bare-symbol render at `frontend/src/components/AssetView.tsx:805`. Building a
   per-instrument rule now would mean ripping it out later from the valuation
   read path, where mistakes appear as wrong money rather than as errors.
3. **That plan's `FEED_PRICE_PROVIDER: MarketDataProvider = Yahoo` singleton is
   invalidated and must not be carried over.** It is replaced by an ordered
   precedence list (below).
4. **Yahoo keeps precedence; Nasdaq fills gaps, resolved per date.** For each
   date the highest-precedence source with a row wins; lower-precedence sources
   fill dates the winner does not cover. Holdings priced correctly today behave
   identically.
5. **Auto-connect only when unambiguous.** Narrow `search` candidates to those
   quoted in the instrument's own currency; connect only if exactly one
   survives. Otherwise the instrument stays unmapped and keeps its "no price
   source" state. Always-connect-with-best-guess was rejected (a bad guess
   silently values a holding off the wrong exchange with nothing marking it a
   guess); connect-only-on-confirmation was rejected (every new Nordic holding
   needs a decision); a UI review queue was deferred.

   **Be precise about who can see an ambiguity.** The `Ambiguous` refresh-item
   status, the `Unavailable` status and the per-provider split in the run
   message are **API- and log-level signals only**. Nothing in the frontend
   renders refresh items or the run message today: the items array exists only
   as a TypeScript type (`frontend/src/api/types.ts:389-396`), the run message
   is read by no component, and the manual-refresh response — the one place
   `Ambiguous` items appear — is discarded by the UI. The BackfillPanel
   (`ImportView.tsx:1036-1055`) interpolates run status and counters, not items.
   So for a non-API user the **entire** UI signal for an ambiguous instrument is
   the asset page's "No price source" chip, with nothing saying that a hand
   mapping is needed or where to apply it. That is not a regression —
   item-level refresh detail has never been rendered — but it is the honest
   description, and it is the state this plan ships. Surfacing per-item states
   in the BackfillPanel, or carrying an "ambiguous — needs hand mapping" state
   on `/api/prices/status`, is real scope and is deferred with the review queue.
6. **Hand mapping is the workaround for an ambiguous instrument, and it is an
   API-level workflow.** `PUT /api/instruments/{id}/provider-symbols/{provider}`
   already validates via `MarketDataProvider::from_db_str`, so adding the enum
   variant opens this path, and it already accepts `currency`; it is extended
   here to carry `asset_class`. There is no UI affordance for it and none is
   added: it is performed with curl or any HTTP client. Because that makes it
   undiscoverable otherwise, the workflow — endpoint, body fields, where to get
   the orderbook id and asset class from a `search` response, and how to tell
   an instrument needs it — is written into the market-data section of
   `docs/Design.HighLevel.md` as part of the documentation phase. Without that
   entry an ambiguous instrument has no user-reachable route at all.
7. **Nasdaq is auto-connected only for instruments with no *enabled* Yahoo
   mapping.** That covers instrument 32 (no mapping) and instrument 38 (disabled
   mapping). Per-date resolution still governs reads, so an instrument that
   later gains both mappings by hand behaves correctly with no code change.
8. **One table rebuild removes the database's duplicate provider list.** The
   `CHECK (provider IN (…))` constraints on `prices`,
   `instrument_provider_symbols` and `fx_rates` duplicate the
   `MarketDataProvider` / `FxProvider` enums. Rebuild once, drop the CHECKs, and
   make the Rust enums the single source of truth. Widening the CHECK list again
   was rejected (keeps the list in four places and repeats the rebuild over
   price data every time); storing Nasdaq rows under an already-permitted code
   was rejected (misreports provenance).
9. **Dropping the CHECK is only safe if the write and read paths become
   enum-typed, so that typing work is part of this plan, not optional.**
   `provider` is passed as `&str` literals at `backend/src/db/prices.rs:249`,
   `backend/src/api/valuation.rs:11` and ~20 other sites. Every repository
   function that takes a provider takes `MarketDataProvider` / `FxProvider`
   after this work.
10. **Nasdaq's asset class lives in its own nullable column** on
    `instrument_provider_symbols`. The identifier stays clean where it is
    displayed and hand-typed; a nullable column is genuinely additive; and the
    table is being rebuilt anyway. A composite `TX271|SHARES` string was
    rejected (invented format leaking into every surface); re-resolving the
    asset class on every call was rejected (extra network call per holding per
    refresh, plus a failure mode unrelated to prices).
11. **The UI names the effective price source, and the misleading label is
    fixed.** `assetViewModel.ts` currently says "Mapping disabled" both when a
    mapping exists but is off *and* when none was ever created; these become
    distinct messages. One truthful line per holding names the source actually
    used and its identifier. A full per-source breakdown of every source tried
    and why was considered and deferred.
12. **Backfill runs to the portfolio's earliest transaction date, and portfolio
    aggregates will move.** Six instruments gaining usable valuations enters
    them into the conviction target pool (2026-07-06 / 2026-07-07), changing
    every other holding's relative target weight and the rebalance ladder.
    Backfilled history rewrites `/api/portfolio/value-history` retroactively,
    moving the dashboard value line, the net-invested-capital gap, XIRR /
    Modified Dietz totals, top movers, treemap and the portfolio waterfall.
    **This is a correction, not a regression** — those figures have been quietly
    excluding real money. Pricing forward-only was rejected (the past stays
    permanently wrong and the numbers move whenever you eventually backfill);
    a reviewable capture-compare-undo mechanism was rejected as extra machinery.
13. **Day change is unavailable when the latest and previous close come from
    different sources.** Cheap, structural, and it prevents a wrong money number
    on any instrument that ends up with two enabled mappings. No instrument has
    two mappings today, so nothing currently visible changes.
14. **Nasdaq is registered by default; there is no feature flag.**
    `MarketDataService::live()` registers it unconditionally. Only the base URL
    is overridable (`TTTB_NASDAQ_BASE_URL`), defaulting to the live endpoint, so
    the default configuration exercises the new code path.
15. **Decision-log entries are proposals in this plan and are written during
    implementation, not now.** Several committed decisions are falsified by this
    work and the log's own rules require reversal entries referencing the
    originals. See *Decision-log proposals*.

## Architecture

### One ordered precedence list, one source of truth

`"YAHOO"` is currently hardcoded in `api/valuation.rs` (`PRICE_PROVIDER`),
`market_data/refresh.rs` (`YAHOO_PROVIDER`), `demo/mod.rs` (`PRICE_PROVIDER`),
`app.rs:189`, `api/instruments.rs` (~524, 538, 563, 571), `db/prices.rs` and
`db/provider_symbols.rs` tests, and `"FRANKFURTER"` in several of the same
files. All of it collapses to `providers/mod.rs`:

```rust
pub const PRICE_PROVIDER_PRECEDENCE: &[MarketDataProvider] =
    &[MarketDataProvider::Yahoo, MarketDataProvider::NasdaqNordic];
pub const BASE_FX_PROVIDER: FxProvider = FxProvider::Frankfurter;
```

Earlier in the slice wins a date tie. `MarketDataProvider` gains
`NasdaqNordic` (`"NASDAQ_NORDIC"`). When hand entry eventually lands it appends
`Manual` to this list and nothing else moves. The three `BASE_CURRENCY`/`"SEK"`
copies are *not* folded in — base currency is a domain concept and untangling it
is an unrelated refactor.

### Provider identity is enum-typed end to end

Dropping the DB CHECK removes an invariant, so the invariant moves into Rust.
Every repository entry point that takes a provider changes from `&str` to the
enum: `db::prices::{find_by_key, find_latest_on_or_before, find_previous_before,
list_for_instrument_in_range, upsert}`, `db::provider_symbols::{find_by_instrument_provider,
list_by_provider_symbol, upsert}`, `db::fx_rates::{…}`. `NewPrice.provider`,
`NewProviderSymbol.provider` and `NewFxRate.provider` become enum-typed too; the
repository converts with `as_str()` at the bind site and decodes with
`from_db_str` at the read site, treating an unknown stored code as
`RepoError::Decode` with instrument id and row id in the message.

`db/` may name `providers::MarketDataProvider`: the boundary `Agents.md` commits
to is that `domain/` stays free of provider types, and these enums carry no
HTTP, axum or sqlx. The dependency direction stays acyclic (`providers` knows
nothing of `db`).

### Provenance without provider types in the domain

```rust
// backend/src/domain/price_resolution.rs — pure; no axum/sqlx/provider types
pub struct ProviderCode(String);   // opaque to the domain; as_str() only
```

`PriceCandidate`, `PriceSnapshot` and `PricePoint` (all in
`domain/valuation.rs`) gain `source: ProviderCode`, and `build_price_history`
carries the candidate's source through to the point it emits. The domain never
enumerates providers and never compares against `"YAHOO"`; precedence is
expressed by the *order of the slices* the IO layer hands it:

```rust
/// `series_by_precedence` is ordered highest precedence first; each slice is
/// date-ascending. For a shared date the earlier slice wins.
pub fn resolve_price_series(series_by_precedence: &[&[PriceCandidate]]) -> Vec<PriceCandidate>;
pub fn pick_latest_on_or_before(candidates_by_precedence: &[Option<PriceCandidate>]) -> Option<PriceCandidate>;
pub fn pick_previous_before(candidates_by_precedence: &[Option<PriceCandidate>]) -> Option<PriceCandidate>;
```

This deliberately differs from the shelved plan's
`enum PriceSource { Fetched { provider }, Manual }`: with hand entry unbuilt,
`Manual` would be an uninhabited variant that no code constructs, and a
structural check that can only ever answer one way is worse documentation than
none. When hand entry lands it either appends `Manual` to the precedence list
(no type change) or promotes `ProviderCode` into that enum in one compiler-guided
change. The API still serializes the flat code (`"YAHOO"`, `"NASDAQ_NORDIC"`,
later `"MANUAL"`), which is what the shelved plan actually requires.

`build_value_history` deliberately does **not** gain provenance: a portfolio
value point aggregates many instruments, and a mixed source set there would not
mean anything useful. Recorded so a later reader does not think it was forgotten.

### Price resolution: pure rule + one IO wrapper

`backend/src/market_data/effective_prices.rs` becomes the only module that knows
a disabled mapping means "no source":

```rust
pub async fn enabled_price_sources(pool, instrument_id) -> Result<Vec<PriceSourceMapping>, RepoError>;
pub async fn has_price_coverage(pool, instrument_id) -> Result<bool, RepoError>;
pub async fn effective_series(pool, instrument_id, from, to) -> Result<Vec<PriceCandidate>, RepoError>;
pub async fn effective_latest_on_or_before(pool, instrument_id, date) -> Result<Option<PriceCandidate>, RepoError>;
pub async fn effective_previous_before(pool, instrument_id, before_date) -> Result<Option<PriceCandidate>, RepoError>;
```

Rows are read for a provider only when that provider has an **enabled** mapping
for the instrument, iterating `PRICE_PROVIDER_PRECEDENCE`. Row decode failures
are internal invariant violations: log through `engine_error!` with instrument
id, row id and field, then return `RepoError::Decode`. This replaces the silent
`.and_then(price_candidate)` drop currently in `api/valuation.rs` and matches
what `api/instrument_prices.rs` and `api/portfolio.rs` already do.

All four gating sites route through this module: `api/valuation.rs`
(`load_valuation_inputs`, `load_period_inputs`), `api/instrument_prices.rs`,
`api/portfolio.rs::value_history`. `ValuationInputs.price_mapping_enabled` is
renamed `has_price_coverage`, and `api/valued_holdings.rs:91` reads the new
name — that single flag is the direct cause of "Valuation missing" on
instrument 32.

Cost: the valuation hot path grows from one mapping lookup plus two indexed
price lookups per instrument to one lookup per provider (two today). At
single-portfolio scale that is acceptable; revisit only if the sub-second LAN
load target is threatened.

### The provider trait grows a request struct

`PriceProvider::daily_history(&self, symbol: &str, start, end)` cannot express
Nasdaq, which needs `orderbookId` **and** `assetClass` **and** a quote currency.
The existing `PriceHistoryRequest` (already in `providers/mod.rs`, used by
`FakePriceProvider` to record calls) becomes the parameter, so there is one
struct rather than two:

```rust
pub struct PriceHistoryRequest {
    pub symbol: String,
    pub asset_class: Option<String>,
    pub quote_currency: Option<String>,
    pub start: NaiveDate,
    pub end: NaiveDate,
}

#[async_trait]
pub trait PriceProvider: Send + Sync {
    async fn daily_history(&self, request: &PriceHistoryRequest) -> ProviderResult<Vec<DailyClose>>;
}
```

Yahoo ignores `asset_class` and `quote_currency` (it reports its own currency in
the payload). The trait stays object-safe, so the 2026-06-16 dispatch decision
(`Arc<dyn PriceProvider + Send + Sync>`, `async-trait`) is unchanged.

### Where Nasdaq's quote currency comes from

Nasdaq's price-history payload has no currency field, so `DailyClose.currency`
for Nasdaq rows is the currency **recorded on the mapping** — populated from the
`search` response at auto-connect time, or supplied by the user on the hand-map
PUT. Consequences, stated plainly:

- A Nasdaq mapping with a null `currency` is never fetched; the refresh reports
  the item with reason `missing_source_currency`. This cannot happen for
  auto-connected mappings (the currency is what the narrowing matched on) and is
  a validation error on the hand-map path.
- The existing currency-mismatch auto-disable still runs, but for Nasdaq it
  compares *mapping* currency against instrument currency rather than *payload*
  currency against instrument currency. For auto-connected mappings that check
  is a tautology by construction; for hand-typed mappings it is a real guard.
  Yahoo's payload-level check is unchanged.
- Periodically re-verifying a Nasdaq mapping's currency through `search` or
  `info` was considered and deferred: it doubles the per-refresh call count for
  a class of error only reachable by hand-typing a wrong currency for a
  correctly-typed orderbook id.

### Multi-provider service

`MarketDataServiceInner` stops holding one price provider and one search
provider:

```rust
// both ordered by PRICE_PROVIDER_PRECEDENCE; lookup is a linear scan
price_providers: Vec<(MarketDataProvider, Arc<dyn PriceProvider + Send + Sync>)>,
symbol_search_providers: Vec<(MarketDataProvider, Arc<dyn SymbolSearchProvider + Send + Sync>)>,
fx_provider: Arc<dyn FxRateProvider + Send + Sync>,
```

**Both registries are precedence-ordered `Vec`s, not a `BTreeMap`.**
`MarketDataProvider` derives only `Clone, Copy, Debug, Eq, PartialEq, Serialize,
Deserialize` (`providers/mod.rs:18-24`), so a `BTreeMap` key would require
adding `PartialOrd`/`Ord`. Deriving them is harmless but wrong in substance: it
would make precedence incidental to the order the variants happen to be declared
in, and it would imply a global ordering on a type that has none. A `Vec` keeps
`PRICE_PROVIDER_PRECEDENCE` the single source of truth for order, makes the
order explicit at the registration site, and costs nothing to scan at two
entries. The builder sorts registered providers into `PRICE_PROVIDER_PRECEDENCE`
order and rejects (debug-asserts) a provider absent from that constant, so the
registry and the read-path precedence can never disagree.

`live()` registers Yahoo chart + Frankfurter + Yahoo search + Nasdaq (as both a
price provider and a search provider). The existing test constructors
`with_providers` / `with_symbol_search_providers` are kept as thin wrappers that
register a single Yahoo-keyed provider, so the ~20 existing tests that use them
compile unchanged; a new `with_provider_registry` builder serves the
multi-provider tests. `AppState` and the 2026-06-16 injection decision are
untouched.

### Symbol matching becomes per-provider and pure

`is_supported_yahoo_quote` is Yahoo-shaped (EQUITY/ETF/MUTUALFUND) and would
reject `TRACKER_CERTIFICATES` even with Nasdaq wired in. It moves into a pure,
unit-tested `market_data/symbol_matching.rs`:

```rust
pub fn is_supported_quote(item: &SymbolSearchMatch) -> bool;      // dispatches on item.provider
pub fn unique_currency_match(instrument_currency: &str, matches: Vec<SymbolSearchMatch>)
    -> CurrencyMatch;   // Unique(match) | None | Ambiguous(Vec<match>)
```

Yahoo keeps its quote-type allow-list. Nasdaq accepts every returned instrument:
a `search` by ISIN returns listings of that one security, and the narrowing that
matters (currency + uniqueness) is `unique_currency_match`.

`SymbolSearchMatch` gains `asset_class: Option<String>` and
`currency: Option<String>`. Yahoo leaves both `None` (its search response does
carry a currency, but parsing it is not needed here and is left alone). Nasdaq
sets `exchange` from the group label (`"Shares Main Market"`, `"Warrants"`),
`name` from `fullName`, `asset_class` and `currency` from the instrument entry,
and leaves `quote_type` `None`.

### Shared HTTP client construction

`build_client()` is currently duplicated verbatim in `providers/yahoo.rs:430`
and `providers/frankfurter.rs:196`; a third copy would be the point where
`Agents.md`'s DRY rule stops being a preference. It moves to
`providers/http.rs` as one `pub(crate) fn build_client() -> reqwest::Client`
with the existing timeouts and the existing
`concat!(CARGO_PKG_NAME, "/", CARGO_PKG_VERSION)` user agent — which is exactly
the non-default UA Nasdaq requires.

### `/api/prices/status` contract

The response is shaped around one mapping per instrument (`mapping_enabled` and
`provider_symbol` as scalars in `MarketDataService::status`,
`refresh.rs:277-287`, mirrored in `frontend/src/api/types.ts`
`PriceStatusInstrument`). It becomes source-plural:

```
PriceStatusInstrument {
  instrument_id, exchange, symbol, currency, open_quantity,
  price_sources: [ { provider, provider_symbol, asset_class, currency, enabled } ],
  effective_price_source: string | null,   // provider code behind latest_price
  latest_price: PriceSnapshotState,        // resolved across enabled sources
  latest_fx:   PriceSnapshotState,
}
```

`mapping_enabled` and `provider_symbol` are **removed**, not kept as derived
scalars: they have no honest single-valued meaning once two sources exist, and
leaving them would guarantee a future reader trusts them. Frontend and backend
ship together, so this is a coordinated change, not a compatibility problem.
`PriceSnapshotState.status = "unmapped"` keeps its wire value and now means "no
enabled source at all"; the UI renders it as "No price source".
`latest_price_snapshot` stops returning `unmapped` for a *disabled* mapping
(`refresh.rs:1562-1564`) and instead reports whatever the effective resolution
produced, so the `/api/prices/status` consumers — `AssetView.tsx:75` via
`usePriceStatus`, `AddInstrumentDialog.tsx:207`, and the `refreshing` boolean in
`PortfolioLayout.tsx:19-22` — agree with valuation.

### Refresh reporting

`RefreshItem` gains `provider: Option<String>` so a failure names the feed that
failed. `RefreshItemStatus` gains:

- `Ambiguous` — search returned more than one same-currency candidate; the
  instrument stays unmapped and needs a hand mapping.
- `Unavailable` — the provider could not be reached at all (transport failure or
  a 5xx), as distinct from `Failed`, which means it answered badly. This is the
  named state that makes a Nasdaq outage explicit instead of a silent staleness
  slide.

The refresh loop restructures from "one mapping per target" to "the target's
enabled mappings, in precedence order". `unmapped_instruments` counts
instruments with **no** usable source after every provider has been tried, so an
instrument priced only by Nasdaq no longer inflates that counter and a run that
finishes PARTIAL still means "action needed". `Ambiguous` items count as
unmapped (action *is* needed). No new run counters and therefore no migration to
`market_data_refresh_runs`.

Clamp detection: after a Nasdaq fetch in `Backfill` mode, if the earliest
returned row is more than 5 calendar days after the requested window start, the
item is reported `Fetched` with reason `history_clamped` and an `engine_warn!`
naming instrument id, orderbook id, requested start and actual start. Backfill
cannot otherwise distinguish "no data" from "older than the provider's window".

**Visibility, stated once so no later section overstates it:** everything in
this subsection is API and log output. No frontend surface renders refresh items
or the run message, and this plan does not add one (decision 5). An implementer
should therefore treat `engine_info!` / `engine_warn!` context here as the
primary diagnostic channel and include enough of it — instrument id, ISIN,
provider, orderbook id, asset class, currency, candidate list — to diagnose an
ambiguity or an outage from `engine.log` alone.

### Frontend

Unidirectional flow is preserved and nothing new is derived inside a component.
`components/priceSourceViewModel.ts` (pure, Vitest-covered) holds:

- `priceSourceLabel(code)` — `YAHOO` → "Yahoo", `NASDAQ_NORDIC` → "Nasdaq
  Nordic", `MANUAL` → "Hand-entered", anything else → the prettified code.
- `priceSourceRows(priceStatus)` — one row per mapping: label, identifier,
  asset class, enabled/disabled chip, and which row is effective.
- `priceAvailabilityLabel(priceStatus, gain)` — the fix for the conflation:
  no mappings at all → "No price source"; mappings exist but all disabled →
  "Price sources disabled"; effective source present → the source's name.

`AssetView.tsx`'s `providerContent` becomes a presentational render of those
rows. `assetViewModel.ts`'s "Mapping disabled" branch is replaced by
`priceAvailabilityLabel`. `AddInstrumentDialog.tsx` (which reads
`mapping_enabled` / `provider_symbol`) reads `price_sources` instead. The
holdings availability sort needs no change — those rows simply stop returning
`Number.NEGATIVE_INFINITY` once they have valuations.

Styling reuses the existing `status-chip` and `data-value` classes already in
`frontend/src/styles.css` (`:355`, `:2127`) under the chip and table-density
rules `docs/VisualDesign.DarkTheme.md` states as prose; no new CSS variables or
design tokens are expected.

---

## Phases

Plans are ephemeral. Durable documents and code must name the behavior, never
these phase numbers.

Verification commands, everywhere: from `backend/` run `cargo build`, then
`cargo test`, then `cargo clippy --all-targets -- -D warnings`, then
`cargo fmt`. From `frontend/` run `npm run check` (which covers `tsc --noEmit`,
Biome and `vitest run`), then `npm run fmt`. When launching npm through
`Start-Process`, use `npm.cmd` explicitly.

---

### Phase 1 — One resolved price source (refactor; no user-visible change)

**Status: DONE** — commit `f2c9f5b`.

No Nasdaq rows exist yet, so this phase must be behavior-preserving apart from
one additive response field.

1. **Migration `0007_provider_codes_and_asset_class.sql`** — one rebuild,
   following the `0004_widen_import_batch_source.sql` mechanics: leading
   `-- no-transaction` directive, `PRAGMA foreign_keys=OFF`, create-new /
   INSERT-SELECT / DROP / RENAME per table, then `PRAGMA foreign_keys=ON`.
   Recreating the indexes is **not** demonstrated by 0004 (`import_batches` has
   none) but is required here and easy to forget: a `DROP TABLE` takes its
   indexes with it, and all five indexes below must be recreated by name.
   Rebuild:
   - `instrument_provider_symbols` — drop the provider CHECK, add
     `asset_class TEXT` (nullable), keep `UNIQUE (instrument_id, provider)` and
     both indexes.
   - `prices` — drop the provider CHECK, keep
     `UNIQUE (instrument_id, provider, date)` and both indexes.
   - `fx_rates` — drop the provider CHECK, keep `UNIQUE (base, quote, provider,
     date)` and its index.

   A header comment states why this is not additive and what replaces the
   invariant (the enum-typed write path in step 3).
2. `providers/mod.rs`: add `MarketDataProvider::NasdaqNordic` (`"NASDAQ_NORDIC"`
   in `as_str` and `from_db_str`), `PRICE_PROVIDER_PRECEDENCE`,
   `BASE_FX_PROVIDER`. Delete `api/valuation.rs::PRICE_PROVIDER`/`FX_PROVIDER`,
   `refresh.rs::YAHOO_PROVIDER`/`FRANKFURTER_PROVIDER`,
   `demo/mod.rs::PRICE_PROVIDER`/`FX_PROVIDER`, and repoint every reader.
   Known sites: `api/test_support.rs`, `api/gains/tests.rs`, `api/holdings.rs`,
   `api/rebalance.rs`, `api/instrument_prices.rs`, `api/portfolio.rs`,
   `api/instruments.rs`, `api/mod.rs:197`, `api/provider_symbols.rs:177,183,188,226`,
   `api/prices.rs:182`, `app.rs:189`, and the `db/prices.rs`,
   `db/provider_symbols.rs` and `db/fx_rates.rs` tests. Treat that as a
   starting point, not a checklist — the enum-typing in step 3 makes the
   compiler enumerate the rest.
3. Enum-type the repositories (`db/prices.rs`, `db/provider_symbols.rs`,
   `db/fx_rates.rs`) as described in *Provider identity is enum-typed end to
   end*. `NewProviderSymbol` gains `asset_class: Option<String>`, threaded
   through `UPSERT_SQL`'s column list, `RETURNING` list and bindings, and
   through `ProviderSymbolRow`, `LIST_SQL`, `FIND_SQL`,
   `FIND_BY_INSTRUMENT_PROVIDER_SQL` and `LIST_BY_PROVIDER_SYMBOL_SQL`.
4. Add `domain/price_resolution.rs` (`ProviderCode`, `resolve_price_series`,
   `pick_latest_on_or_before`, `pick_previous_before`) and export it from
   `domain/mod.rs`, which stays a thin wrapper.
5. Add `source: ProviderCode` to `PriceCandidate`, `PriceSnapshot` and
   `PricePoint`, and carry it through `build_price_history`. The compiler finds
   every construction site.
6. Add `market_data/effective_prices.rs` and rewire `api/valuation.rs`,
   `api/instrument_prices.rs` and `api/portfolio.rs` through it. Rename
   `price_mapping_enabled` → `has_price_coverage`; update
   `api/valued_holdings.rs`.
7. Implement decision 13 in `domain/valuation.rs::value_position`: when the
   previous close's `source` differs from the latest close's `source`, previous
   price availability becomes `Unavailable` with a new
   `ValuationReason::PreviousCloseSourceMismatch` (serialized
   `previous_close_source_mismatch`), so day change is explicitly unavailable
   rather than computed across two feeds. Two verified implementation notes:
   `ValuationReason` has **no serde derive** — it is serialized by the
   hand-written `serialize_valuation_reason` (`api/valuation.rs:195-227`), which
   must gain the new arm; and `value_position` (`domain/valuation.rs:606-645`)
   is the **only** site that computes day change — the portfolio-level figures
   are sums of per-holding `day_change_base` (`:701-771`) — so this single guard
   covers every day-change surface.
8. Serialize provenance additively: `source` on `PriceSnapshotResponse` and on
   the price-history point response.

Tests:
- `price_resolution` unit tests: higher-precedence series wins a shared date;
  a lower-precedence series fills a gap; single-series input; empty input;
  interleaved dates stay ascending; `pick_*` prefer precedence on a tie and the
  newer date otherwise.
- `build_price_history` unit test: a point's source equals its candidate's
  source, including a mixed two-source series.
- `effective_prices` integration tests on the in-memory pool: a disabled mapping
  yields no rows for that provider; a missing mapping yields no rows; a
  decode-broken row surfaces as an error rather than a silently missing point.
- `value_position` unit test: mixed-source latest/previous yields
  `previous_close_source_mismatch` while market value stays available;
  same-source pairs are unaffected.
- Repository test asserting an unknown stored provider code decodes as
  `RepoError::Decode` (the invariant that replaces the dropped CHECK). The
  existing `provider_symbol_constraints_are_enforced` test's CHECK assertion is
  replaced by this; its FOREIGN KEY assertion stays.
- Every pre-existing gains / holdings / price-history / value-history test
  passes unchanged, plus assertions that `source` is `"YAHOO"` on those paths.

Verify:
- Backend command sequence; every pre-existing test green.
- No frontend change; no UI change.
- **External human testing recommended (migration safety):** copy the real
  SQLite file, point the backend at the copy via `TTTB_DATABASE_URL`, start it
  once so migration 0007 runs, and confirm with `sqlite3` that
  `SELECT COUNT(*) FROM prices`, `… FROM instrument_provider_symbols` and
  `… FROM fx_rates` match the pre-migration counts and that the app loads with
  identical holdings values. This is the one irreversible step in the plan.

---

### Phase 2 — Nasdaq Nordic provider client (parsing only; not yet reachable)

**Status: DONE.** Backend sequence green (430 unit + 53 integration tests,
clippy clean under `-D warnings`, `cargo fmt --check` clean). The external
network check passed: `scripts/probe-nasdaq-nordic.ps1`, 51 checks, 0 failures,
including the live Investor B split cross-check at 128 overlapping days and 0
mismatches. Reviewed by a Claude implementation reviewer; eight findings
applied, four advisories deliberately skipped.

Five things differ from this section as originally written, all recorded where
they belong rather than only here:

- The split-adjustment guard asserts a **0.1% relative tolerance**, not exact
  equality. A live re-probe disproved the original "matches to 0.000" claim:
  2021-06-08 differs by ~0.026% (Nasdaq 194.90 vs Yahoo 194.949996948242), an
  ordinary closing-price convention difference. The *Verified provider facts*
  bullet and step 5 below were corrected to match. A 4× unadjusted divergence
  still fails this tolerance by three orders of magnitude.
- **A no-match search returns an empty list, not an error** — see the
  `NO_INST_FOUND` bullet under *Verified provider facts*. The first
  implementation mapped it to a provider error; the live probe caught it. This
  is the one behavior here that later phases branch on.
- `nasdaq_price_history_astrazeneca.json` was added beyond the original fixture
  list because it is the **only real capture containing comma-thousands
  closes** (AVA trades near 123 SEK, Investor B near 180). It is also the
  second instrument this feature exists to fix.
- `nasdaq_price_history_synthetic_gaps.json` is **hand-authored, not a
  capture** — no real response carries an empty close, so the skip and
  `NoDataInRange` paths had no live data to test against.
- Provider clients still **cannot distinguish a transport failure from a
  malformed response**; both surface as `ProviderMissingReason::ProviderError`
  with no HTTP status. Deferred deliberately, and recorded as a prerequisite at
  the top of the multi-provider refresh section below, which owns the
  `Unavailable` status that consumes the distinction.

1. `providers/http.rs` — extract the duplicated `build_client()` from
   `yahoo.rs` and `frankfurter.rs`; both call the shared one.
2. `providers/nasdaq_nordic.rs` — `NasdaqNordicClient` with
   `new()` / `with_base_url()` / `with_client()` mirroring `YahooChartClient`,
   reading `TTTB_NASDAQ_BASE_URL` in `new()` with the live URL as the default.
   Implements:
   - `PriceProvider::daily_history` — GET
     `/instruments/{symbol}/price-history?assetClass={asset_class}&fromDate&toDate`.
     Missing `asset_class` → `ProviderError` with reason `provider_error` and a
     message naming the symbol. Rows with an empty `close` are skipped; if no
     row has a close, return `NoDataInRange`. `DailyClose.currency` is
     `request.quote_currency`; absent → `ProviderError` naming
     `missing_source_currency`. `provider_symbol` is the orderbook id. Rows are
     sorted ascending by date.
   - `SymbolSearchProvider::search` — GET `/search?searchText={query}`,
     flattening `data[].instruments[]` into `SymbolSearchMatch` (with the new
     `asset_class` / `currency` fields).
   - Typed envelope handling: `status.rCode >= 400` or a non-empty
     `status.bCodeMessage` maps to a `ProviderError` carrying the Nasdaq error
     code and message; `"Instrument not found"` maps to
     `ProviderMissingReason::NotListed`. Transport failures and malformed
     responses currently both map to `ProviderMissingReason::ProviderError`
     without an HTTP status; the reporting distinction is deferred to the
     multi-provider refresh work described below.
   - Number parsing: one helper stripping `,`, ASCII space and U+00A0 before
     `Decimal::from_str`; failures produce a `ProviderError` naming the field
     and the row date, never a silent skip.
   - Failure logging through `engine_error!` / `engine_warn!` including
     provider, orderbook id, asset class, date range, reason code and message,
     per the logging rules.
3. `SymbolSearchMatch` gains `asset_class` and `currency`; every construction
   site (Yahoo, the fakes, the tests) is updated.
4. Fixtures into `backend/tests/fixtures/market_data/`:
   `nasdaq_search_ava_samsung.json`, `nasdaq_search_ericsson_ambiguous.json`,
   `nasdaq_price_history_ava_samsung.json`,
   `nasdaq_price_history_astrazeneca.json`, `nasdaq_search_no_match.json`,
   `nasdaq_error_not_found.json`,
   `nasdaq_price_history_investor_b_split.json`,
   `nasdaq_price_history_synthetic_gaps.json`, `yahoo_inve_b_adjusted.json`.
   All are raw live captures except `nasdaq_price_history_synthetic_gaps.json`,
   which is hand-authored; the captures are byte-preserved and must not be
   reformatted.
5. **Split-adjustment guard test** — parse the Investor B Nasdaq fixture and the
   Yahoo `INVE-B.ST` fixture, intersect their dates across the May 2021 4:1
   split, and assert every overlapping close agrees within a 0.1% relative
   tolerance. If Nasdaq ever ships unadjusted history, this test is where the
   parsing side of the change is caught; the live side is the probe script.
6. `scripts/probe-nasdaq-nordic.ps1`, following the
   `scripts/probe-connectivity.ps1` precedent (comment-based help, params,
   `$ErrorActionPreference = "Stop"`, output under `.local/logs/`): hits
   `search`, a no-match search, `price-history` and the not-found case for pinned
   ISINs, asserts the expected JSON paths and field names still exist, re-runs
   the Investor B vs Yahoo split cross-check live, and prints a pass/fail
   summary. This is the only thing that detects endpoint *shape drift*, which
   fixtures cannot.

Tests:
- Fixture parse tests: row count, first and last date, exact decimal for the
  AstraZeneca comma-thousands value (`"1,369.00"` → `1369.00`), empty-close
  rows skipped, currency taken from the request.
- Ambiguous-search fixture yields two matches with distinct currencies and
  orderbook ids.
- Not-found fixture maps to `NotListed`, not a parse failure.
- The three search outcomes stay distinguishable: a search with matches, a
  no-match search (empty list, no error) and a genuine error envelope.
- The split-adjustment guard test above.
- A URL-construction test pinning the query parameter names and date format.

Verify:
- Backend command sequence. Nothing is wired into the app yet, so the running
  application is unchanged.
- **External human testing recommended (network):** run
  `pwsh -File scripts/probe-nasdaq-nordic.ps1` and confirm every check passes
  against the live endpoint.

---

### Phase 3 — Multi-provider refresh: dispatch, auto-connect, honest reporting

This is where the feature lands on the backend: after this phase the six
instruments have stored prices and are valued.

Reporting prerequisite: provider clients currently cannot distinguish a
transport failure from a malformed response; both surface as
`ProviderMissingReason::ProviderError` with no HTTP status. Nasdaq and Yahoo
must gain a distinguishable marker at the same time the `Unavailable`
refresh-item status is introduced. Without that marker, `Unavailable` would
also fire on a provider schema change and misreport a format change as an
outage.

1. `MarketDataService`: the provider registry and ordered search-provider list;
   `live()` registers Yahoo chart, Frankfurter, Yahoo search and Nasdaq (price +
   search). `with_providers` / `with_symbol_search_providers` keep working as
   Yahoo-keyed wrappers; add `with_provider_registry` for multi-provider tests.
2. `market_data/symbol_matching.rs`: move `is_supported_yahoo_quote` in as
   `is_supported_quote` with per-provider dispatch, and add
   `unique_currency_match`. `best_yahoo_search_match`,
   `is_plausible_symbol_recovery`, `is_us_exchange`, `normalized_security_name`
   and `is_isin_like` move with it; `refresh.rs` shrinks accordingly.
3. Seeding (`seed_provider_symbols`): Yahoo seeding behavior is unchanged. After
   it, when the instrument has **no enabled Yahoo mapping** and an ISIN-like
   identifier (`instruments.isin`, else `symbol` for Avanza rows), search
   Nasdaq, run `unique_currency_match` against the instrument currency, and:
   - `Unique` → upsert an enabled `NASDAQ_NORDIC` mapping with
     `provider_symbol = orderbookId`, `asset_class`, `currency`; log
     `engine_info!` with instrument id, ISIN, orderbook id, asset class and
     currency.
   - `Ambiguous` → write nothing; `engine_warn!` listing every candidate's
     orderbook id, currency and asset class; the refresh item is `Ambiguous`.
   - `None` → write nothing; the instrument stays unmapped exactly as today.
4. Refresh loop: resolve each target's enabled mappings in precedence order and
   fetch each. Per source, on success write rows under that provider code; the
   currency-mismatch auto-disable disables **that** mapping only, leaving other
   sources intact. `unmapped_instruments` counts instruments with no usable
   source after all providers are tried.
5. `RefreshItem.provider`; `RefreshItemStatus::{Ambiguous, Unavailable}`;
   `history_clamped` reason on Backfill (see *Refresh reporting*). Include the
   per-provider split in the run-finished `engine_info!` line and the run
   `message`.
6. `MarketDataService::status`: the `price_sources` / `effective_price_source`
   shape; `latest_price_snapshot` resolves through `effective_prices` instead of
   gating on one Yahoo mapping.
7. `api/provider_symbols.rs::update`: accept and persist `asset_class`; require
   `asset_class` and `currency` when the provider is `NASDAQ_NORDIC`
   (`400 missing_asset_class` / `400 missing_source_currency`); return
   `asset_class` in the response. `reject_demo_mutation` is already called.
8. `demo/mod.rs`: seed one SEK Nordic-style demo instrument priced under
   `NASDAQ_NORDIC` with a matching enabled mapping (orderbook-id-shaped symbol,
   asset class `SHARES`), so demo exercises multi-source resolution and the
   price-source label rather than only the Yahoo path.

Tests (backend, in-memory pool + injected fakes):
- **Regression test for the reported defect:** an instrument with no Yahoo
  mapping whose Nasdaq search returns one SEK candidate gets an enabled
  `NASDAQ_NORDIC` mapping, prices are written, and `/api/holdings` returns a
  `valuation` object with an available `market_value_base` — no "Valuation
  missing".
- **Disabled-Yahoo repair path (instrument 38 shape):** a disabled `YAHOO`
  mapping with stored GBP rows plus a Nasdaq mapping values from Nasdaq; the
  stored Yahoo rows are never promoted into valuation, and the disabled mapping
  is not re-enabled.
- **Ambiguity:** two same-currency candidates → no mapping written, item status
  `Ambiguous`, instrument still counted unmapped, run finishes PARTIAL.
- **Currency narrowing:** the Ericsson-shaped two-candidate case (EUR + SEK)
  against a SEK instrument connects the SEK orderbook id.
- **Per-date precedence end to end:** Yahoo and Nasdaq rows on the same date →
  Yahoo wins and `source` serializes `"YAHOO"`; a date only Nasdaq covers →
  Nasdaq is used and `source` serializes `"NASDAQ_NORDIC"`;
  latest-on-or-before across mixed sources picks the newest date.
- **PARTIAL still means action needed:** a portfolio whose only non-Yahoo
  instrument is Nasdaq-mapped finishes SUCCEEDED; add one genuinely unmapped
  instrument and it finishes PARTIAL.
- **Outage:** a Nasdaq provider returning a transport error yields item status
  `Unavailable` with the provider named, prices from other sources still get
  written, and valuation falls back to stored Nasdaq rows with normal staleness
  rather than disappearing.
- **Clamp:** a Backfill whose returned rows start well after the requested start
  reports `history_clamped`.
- `/api/prices/status` contract test: `price_sources` lists both mappings with
  `asset_class`, and `effective_price_source` names the winner.
- Hand-map contract tests: `PUT …/provider-symbols/NASDAQ_NORDIC` round-trips
  `asset_class`; missing asset class or currency is rejected before write; demo
  mode still rejects it with `demo_read_only`.
- Demo smoke test: the seeded Nasdaq demo instrument is valued.

Verify:
- Backend command sequence; all tests green.
- **External human testing REQUIRED, against a *copy* of the real database**
  (`TTTB_DATABASE_URL` pointed at the copy — this phase performs live network
  calls and writes prices): trigger a manual refresh, then check that
  instrument 32 and its five siblings have `NASDAQ_NORDIC` mappings with
  plausible orderbook ids and `TRACKER_CERTIFICATES` asset class; instrument 38
  is mapped to `TX271`/`SHARES`/SEK with its `AZN.L` mapping still disabled;
  `/api/holdings` values all seven; the run message names the per-provider
  split; and no previously-working instrument changed its value. Spot-check one
  close against the ledger (BUY @123.30 on 2025-06-16 should match the stored
  close exactly).
- Also run `scripts/start.ps1 -Demo` and confirm demo still loads with the new
  demo instrument valued.
- **Expected intermediate breakage, do not report it as a defect:** this phase
  removes `mapping_enabled` / `provider_symbol` from `/api/prices/status` while
  the frontend still reads them (`AssetView.tsx:810,815,825`,
  `assetViewModel.ts:322`, `AddInstrumentDialog.tsx:143-144`). The wire change
  is invisible to `tsc`, so `npm run check` still passes, but the asset page's
  Data & mapping panel and the add-instrument dialog's mapping note **will
  misrender** (undefined fields producing wrong chips) until the frontend lands
  in the next phase. Holdings values, gains and charts are unaffected. The
  feature ships as one release, so no intermediate state is user-facing.

---

### Phase 4 — Frontend: name the effective source, fix the misleading label

1. `api/types.ts`: `PriceStatusInstrument` loses `mapping_enabled` and
   `provider_symbol` and gains `price_sources` and `effective_price_source`;
   `PriceSnapshot` and `PriceHistoryPoint` gain `source: string`. **Also update
   the refresh mirrors even though nothing consumes them**: the
   `RefreshItemStatus` union (`types.ts:381`) gains `"ambiguous"` and
   `"unavailable"`, and the `RefreshItem` interface (`types.ts:389-396`) gains
   `provider: string | null`. Because no component reads the items array, `tsc`
   cannot catch a stale mirror — it would simply keep lying about the wire
   shape.
2. New pure `components/priceSourceViewModel.ts` with `priceSourceLabel`,
   `priceSourceRows` and `priceAvailabilityLabel` (see *Frontend*).
3. `valuationDisplay.tsx`: add a `reasonLabel` case for
   `previous_close_source_mismatch` → "Day change unavailable across sources".
   The existing default would render it as the raw lower-cased code on every
   surface that shows reasons (`assetViewModel.ts:336`, `GainsTable.tsx`,
   `HoldingsTable.tsx`, `AssetView.tsx:378-386`).
4. `AssetView.tsx`: `providerContent` renders `priceSourceRows` — one line per
   source with its label, identifier, asset class when present, and a
   disabled chip — plus a single "Price source" line naming the source behind
   the displayed latest price.
5. `assetViewModel.ts`: replace the "Mapping disabled" branch with
   `priceAvailabilityLabel`, so "No price source" and "Price sources disabled"
   are distinct.
6. `AddInstrumentDialog.tsx`: read `price_sources` instead of the removed
   scalars for its price-mapping note.
7. Update the fixtures in `assetViewModel.test.ts` and
   `AddInstrumentDialog.reducer.test.ts` to the new shape.

Tests (Vitest, pure logic only — no snapshots, no DOM-structure assertions):
- `priceSourceLabel` for `YAHOO`, `NASDAQ_NORDIC`, `MANUAL` and an unknown code.
- `priceAvailabilityLabel` for: no sources; one disabled source; one enabled
  source; two sources where the lower-precedence one is effective.
- `priceSourceRows` marks exactly one row effective and carries asset class only
  when present.
- `reasonLabel("previous_close_source_mismatch")` returns the polished label,
  not the default prettified code.

Verify:
- Frontend command sequence.
- **External human testing REQUIRED** (still against the database copy from
  Phase 3): on instrument 32's asset page the Valuation-missing chip is gone,
  market value / unrealized gain / portfolio weight / conviction target all
  populate, the price chart draws the Nasdaq series (and is no longer carried by
  `instrumentChartViewModel.ts`'s transaction-price anchors alone), and Data &
  mapping names "Nasdaq Nordic" with the orderbook id and asset class. On
  instrument 38 both sources are listed, Yahoo shown disabled, Nasdaq effective.
  On a Yahoo-priced holding nothing changed except the wording. Check one
  never-mapped instrument reads "No price source", not "Mapping disabled".

---

### Phase 5 — Add-instrument lookup consults every provider

Without this, adding a seventh AVA tracker by ISIN still shows "No suitable
provider match was found for this instrument", which is now untrue.

1. `MarketDataService::lookup_symbol_search` queries every configured search
   provider in precedence order, filters each result set through
   `is_supported_quote`, and merges (deduplicating on
   `(provider, provider_symbol)`). Status rules: `matches` when any provider
   yields a supported match; `no_match` when every reachable provider yields
   none; `provider_unavailable` only when **no** provider was reachable.
2. `SymbolSearchLookupMatch` gains `asset_class` and `currency`; the lookup
   response carries them so the dialog can show "Nasdaq Nordic · TX2997672 ·
   TRACKER_CERTIFICATES · SEK".
3. Frontend: render the extra fields through `priceSourceLabel`; keep the
   existing `provider_unavailable`-allows-creation-with-a-warning behavior
   (2026-07-08).

Tests:
- Yahoo has no match, Nasdaq does → `matches`, with the Nasdaq match present and
  its asset class populated.
- Both providers have matches → both are listed, Yahoo first.
- Yahoo errors, Nasdaq answers with no match → `no_match` (not
  `provider_unavailable`).
- Both providers error → `provider_unavailable`.
- A `TRACKER_CERTIFICATES` Nasdaq match survives the supported-quote filter that
  Yahoo's allow-list would have rejected.
- Demo mode still short-circuits to `provider_unavailable`.

Verify:
- Both command sequences.
- **External human testing recommended:** open Add instrument and paste
  `JE00BLH0QR80`; the dialog should report a Nasdaq Nordic match instead of "No
  suitable provider match was found". The user's existing trackers arrived via
  CSV import, so the import path is unaffected either way.

---

### Phase 6 — Backfill the real history and prove nothing else moved

No test in the repository pins `/api/gains` or `/api/portfolio/value-history`
aggregates, so "nothing else changed" must be checked, not asserted.

1. `scripts/capture-aggregates.ps1` (checked in, following the existing script
   conventions): dumps `/api/gains?include_closed=true`,
   `/api/portfolio/value-history` and `/api/holdings` to timestamped files under
   `.local/aggregates/`, and given two capture directories prints a per-row diff
   of `market_value_base`, `total_return_base` and the value-history points.
   This is the tool the verification obligation needs, and it stays useful for
   every future change to the valuation read path.
2. Capture **before**: run it against the real database prior to any Nasdaq
   backfill.
3. Run a `Backfill` refresh against the real database. Depth is the portfolio's
   earliest transaction date via the existing `refresh_window` /
   `earliest_transaction_date` path — unchanged behavior. Where that predates
   Nasdaq's ~10-year clamp, the instrument gets history from the clamp forward
   and the run reports `history_clamped`; earlier dates keep the existing
   partial-data behavior (`/api/portfolio/value-history` points marked
   `incomplete` with `excluded_count`), which is honest and already designed for.

   Testing note: `refresh_window` returns `400 missing_start_date` when the
   portfolio has no transactions at all (`refresh.rs:1366-1384`). Irrelevant to
   the real database, but it will bite anyone rehearsing this step against a
   fresh or demo database — pass an explicit `start_date` there.
4. Capture **after** and diff.

Expected and acceptable movement, to be confirmed rather than assumed:
- The six instruments gain market value, unrealized gain and portfolio weight.
- Every other holding's *portfolio weight* and *conviction target* changes,
  because the denominator and the target pool grew.
- The dashboard value line, the net-invested-capital gap, XIRR / Modified Dietz
  totals, treemap and the portfolio waterfall all move, including retroactively.
- Top movers move too, but note they are a frontend selector over `/api/gains`
  rows (`dashboardSelectors.ts:16`), not a backend aggregate — so they change as
  a consequence of the gains rows, and the capture diff covers them indirectly.

Unacceptable and a stop condition:
- Any currently-working instrument's `market_value_base`, `cost_basis_base`,
  `unrealized_gain_base` or `total_return_base` changes.
- Any value-history point *before* the earliest newly-priced instrument's first
  transaction changes.

Verify:
- **External human testing REQUIRED.** Take a database backup first. Run the
  before/after capture and read the diff against the two lists above. Then walk
  the UI: Dashboard, Holdings, Gains, Rebalance and two asset pages. Confirm the
  rebalance ladder now includes the newly-eligible instruments and that its
  warning-stale notice behaves sensibly.

---

### Phase 7 — Documents, decision-log entries, version bumps, final sweep

1. `docs/Design.HighLevel.md`:
   - Correct `prices  instrument_id, date, close, currency  -- EOD cache` to note
     that rows are keyed by provider and that several providers can cover one
     instrument.
   - Market-data section: name Nasdaq Nordic as the second automatic provider,
     state per-date precedence with Yahoo first, state that a disabled mapping
     counts as no source, and record that Nasdaq is an undocumented internal API
     treated as best-effort.
   - **The hand-mapping workflow, in the same market-data section** (decision 6,
     and the *only* place it will be discoverable): when it is needed (an
     instrument showing "No price source" whose refresh item is `Ambiguous`),
     `PUT /api/instruments/{id}/provider-symbols/NASDAQ_NORDIC` with
     `{ provider_symbol, asset_class, currency, enabled }`, where to read the
     orderbook id / asset class / currency out of a `search` response, and the
     fact that ambiguity is visible only in `engine.log` or the raw refresh
     response because no UI renders refresh items.
   - Risk row 2 ("unofficial APIs breaking") gains the probe script as its
     concrete mitigation.
2. `docs/CurrencyAndFxRules.md`: one line recording that a Nasdaq price row's
   currency comes from the mapping (the provider's price payload carries none),
   and that a Nasdaq mapping without a recorded currency is never fetched.
3. `docs/DecisionLog.md`: the entries listed below.
4. Version bumps (one release for the whole feature; intermediate phases are not
   released): `backend/Cargo.toml` `0.14.2 → 0.15.0`,
   `frontend/package.json` `0.22.11 → 0.23.0` plus the matching
   `frontend/package-lock.json` version fields.
5. Full verification sweep on both stacks.

Verify:
- Backend and frontend command sequences.
- **External human testing REQUIRED:** launch the app and confirm the two
  version values in the UI match the bumped manifests, then re-walk the Phase 4
  and Phase 6 checklists once against the real database.

---

## Decision-log proposals

> **The decision log is deliberately not written during planning.** `Agents.md`
> notes decisions are *sometimes* recorded during planning; this plan takes the
> other option on purpose. The entries below are **proposals**. Do not append
> them to `docs/DecisionLog.md` until this plan is being implemented, and
> re-check each against what was actually built first — a plan can still change,
> and the log records settled commitments only. Their absence today is
> intentional. Entries name behaviors, never this plan's phase numbers.

**New entries**

1. **Nasdaq Nordic is the second automatic price provider; price sources resolve
   per date.** Yahoo has precedence, Nasdaq fills dates Yahoo does not cover, a
   disabled mapping counts as no source and its stored rows are never promoted
   into valuation, auto-connection happens only when exactly one search
   candidate is quoted in the instrument's currency, ambiguous instruments stay
   unmapped and are hand-mapped through the provider-symbol endpoint, a Nasdaq
   mapping carries an asset class and a quote currency because the provider's
   price payload carries neither, the API is undocumented and treated as
   best-effort with an explicit unavailable state, and its ~10-year history
   clamp is reported rather than mistaken for missing data. Record plainly that
   the ambiguity and outage states are API- and log-level, with "No price
   source" the only UI signal. Reference **2026-06-13 "Primary Market Data
   Sources"** as the commitment this fulfils — that is the primary-sources
   decision (2026-06-12 is only the spike *scope*), it is where the
   "Twelve Data is the keyed fallback" and "hide provider-specific response
   shapes behind a provider boundary / recorded fixtures / symbol mappings per
   provider" clauses live, and it is where the "re-check terms and usage limits
   before any hosted or distributed deployment" clause appears verbatim
   (`docs/DecisionLog.md:79-82`). That clause applies with extra force to an
   undocumented internal API.
2. **Provider identity is enum-typed end to end and the database no longer
   duplicates the provider list.** The `MarketDataProvider` / `FxProvider` enums
   are the single source of truth; repositories take and return them; an unknown
   stored code is a decode error. **This refines the 2026-06-14 "migrations are
   forward-only and additive" commitment**: a rebuild is permitted when it
   *removes* a duplicated invariant, provided the invariant is simultaneously
   re-established in typed Rust — never merely to widen a list.
3. **Backfilled Nordic history restates portfolio aggregates, and that is a
   correction.** Instruments that gain a usable valuation enter the conviction
   target pool and the value-history spine retroactively; the reported figures
   were previously excluding real money. Records the before/after capture as the
   expected practice whenever the valuation read path changes, since no
   automated test pins those aggregates.
4. **Day change is unavailable when the latest and previous close come from
   different price sources.** Prevents a cross-feed comparison being reported as
   a day's price move.

**Reversal / refinement entries (each references its original)**

5. **Reverses 2026-06-21's** "a missing or disabled Yahoo price mapping yields
   200 with an empty points array" for `GET /api/instruments/{id}/prices`: the
   endpoint now serves every enabled source resolved per date, and each point
   names its source. The rest of that entry (one point per stored close date,
   carry-forward FX, precision rules, `400 invalid_date_range`) stands.
6. **Reverses 2026-06-22's** "cached prices are used only behind an enabled
   Yahoo mapping" for `GET /api/portfolio/value-history`: the date spine and the
   per-instrument series are built from every enabled source. The rest of that
   entry stands.
7. **Refines 2026-07-02's** demo-mode statement that "demo seed prices use
   provider YAHOO so existing read paths work without changing provider
   dispatch": dispatch is now provider-plural, and the demo seed deliberately
   includes a Nasdaq-priced instrument so demo exercises multi-source
   resolution. Demo remains ephemeral, read-only and offline.

## Documents to update

| Document | Change | Phase |
|---|---|---|
| `docs/Design.HighLevel.md` | `prices` is provider-keyed and multi-source; market-data section names Nasdaq Nordic, per-date precedence, best-effort status; risk mitigation names the probe script | 7 |
| `docs/Design.HighLevel.md` (market-data section) | **the hand-mapping workflow for ambiguous instruments** — endpoint, body fields, where to read the orderbook id and asset class, and the fact that ambiguity surfaces only in the log or the raw refresh response. This is the "documented workaround" decision 6 depends on; without it the workflow is curl-only and undiscoverable | 7 |
| `docs/CurrencyAndFxRules.md` | a Nasdaq price row's currency comes from its mapping | 7 |
| `docs/DecisionLog.md` | seven entries (see above) — **proposals only; deliberately not written until implementation** | 7 |
| `backend/Cargo.toml`, `frontend/package.json` (+ lock) | version bumps | 7 |

No `docs/VisualDesign.DarkTheme.md` change is expected — the price-source rows
reuse the existing `status-chip` and `data-value` classes under the chip and
table-density rules that document already states. If implementation finds it
needs a new CSS variable or design token, that is a document change to add here,
not a silent addition.

## Deliberately out of scope (recorded, not overlooked)

- **The day-change adjacency and FX-pairing defect.** The shelved hand-entry
  plan documents it fully: day change is currently computed across
  non-adjacent closes, and the previous FX rate is anchored to `latest_fx.date`
  rather than to the previous close's own date. A second feed with different
  trading-day coverage makes it more visible, which argues for fixing it soon —
  but not here. It is an independent defect whose fix deliberately *blanks* day
  change on existing rows, and mixing that user-visible change into the same
  release as the aggregate restatement would make the before/after comparison
  unreadable. Recommend scheduling it as the next plan. The cross-source guard
  (decision 13) is included here because it is structural, one condition, and
  prevents a wrong number rather than changing an existing one.
- **Hand-entered prices** (`docs/plans/Plan.HandEnteredPrices.md`), any third
  provider, bulk price entry.
- **Any UI surface for ambiguous instruments or per-item refresh detail.** No
  review queue, no per-item list in the BackfillPanel (the only refresh UI), and
  no "ambiguous — needs hand mapping" state carried on `/api/prices/status`.
  What ships instead: the `Ambiguous` refresh item and the log line for
  diagnosis, the "No price source" chip as the sole UI signal, and the
  hand-mapping workflow written into `docs/Design.HighLevel.md` for API use.
  Both alternatives are real scope; the second in particular needs a new
  per-instrument state on the status contract.
- **Nasdaq's `info` endpoint** and any intraday price surface.
- **A tested database backup/restore procedure.** The migration in Phase 1 is
  irreversible in the sense that the CHECK constraints do not come back, and the
  human step there is a copy-first drill — but a real backup/retention/restore
  plan is whole-database work that exists independently of this feature.

## Risks and accepted consequences

- **Undocumented internal API, no SLA, can change without notice.** Mitigated
  three ways: recorded fixtures make parsing deterministic; the `Unavailable`
  refresh state makes an outage explicit instead of a silent staleness slide;
  and `scripts/probe-nasdaq-nordic.ps1` is the only thing that can catch
  endpoint *shape* drift, so it must be run when something looks wrong.
- **The ~10-year history clamp** means the oldest part of some holdings' history
  is unavailable and cannot be distinguished from "no data" by the provider.
  Reported as `history_clamped`; the affected value-history points stay marked
  `incomplete`.
- **`history_clamped` is a heuristic with benign false positives.** A
  recently-listed instrument (listed after the requested window start) or a
  holiday stretch longer than the 5-day tolerance trips it with no clamp
  involved. It is warning-only and errs in the safe direction, but the reason
  string must not be read as proof that a clamp occurred.
- **ISIN → instrument is not unique.** Handled by currency narrowing plus the
  exactly-one rule; the residual case is a genuinely ambiguous same-currency
  pair, which stays unmapped and needs a hand mapping.
- **The Nasdaq currency guard is mapping-level, not payload-level** (the price
  payload has no currency), so it is a tautology for auto-connected mappings.
  Accepted; the auto-connect narrowing is what establishes the currency in the
  first place.
- **Portfolio aggregates move**, including retroactively. Deliberate; measured
  in Phase 6 rather than asserted.
- **Newly-valued instruments enter the conviction target pool and the rebalance
  ladder**, changing every other holding's target weight (2026-07-06 /
  2026-07-07). Intended.
- **Dropping the CHECK constraints removes a database-level invariant.** It is
  replaced by enum-typed repository signatures plus a decode error on unknown
  stored codes. If the typing work is descoped, the invariant is simply gone —
  which is why decision 9 makes it non-optional.
- **Extra queries per instrument on the valuation path** (one mapping lookup and
  up to two price lookups per provider). Acceptable at single-portfolio scale.
- **Nasdaq search runs once per unmapped instrument per refresh.** Bounded by
  the number of instruments Yahoo cannot price, which is the set already failing
  today; six rapid probe calls showed no throttling.
- **A dual-source instrument is fetched twice on every refresh, indefinitely.**
  The refresh loop fetches every enabled mapping, so an instrument hand-mapped
  to both Yahoo and Nasdaq costs two provider calls per run forever. Auto-connect
  never creates that state (decision 7), so it only arises from a deliberate
  hand mapping — but the cost is permanent, not one-off, and is worth knowing
  before hand-mapping a second source onto an instrument that already works.
- **The ambiguity and outage states are invisible in the UI** (decision 5). An
  instrument that needs a hand mapping looks identical to one no provider
  carries. Accepted for this release; the only mitigations are the log and the
  documented workflow.

## Open Questions

Surface these before resolving them silently during implementation.

1. **Should Nasdaq also be auto-connected for instruments that already have a
   working Yahoo mapping**, so per-date resolution can fill Yahoo's gaps? The
   plan says no (decision 7), following the brief's "Nasdaq is consulted where
   there is no working mapping", so mixed-source series arise only from a
   deliberate hand mapping. Confirm this is the intent — the machinery supports
   the other choice with no code change beyond the seeding condition.
2. **Should AstraZeneca's disabled `AZN.L` Yahoo mapping be kept or deleted?**
   The plan keeps it (disabled rows are never promoted, and the record of why it
   was disabled is useful), which means the Data & mapping panel will list two
   sources for that instrument, one greyed out. Deleting it would be tidier but
   would lose that history and let a future refresh re-seed it.
3. **How should an instrument whose transactions predate Nasdaq's ~10-year clamp
   be presented?** The plan reports `history_clamped` on the refresh item and
   lets the existing `incomplete` / `excluded_count` machinery handle the
   value-history points. An explicit stored "history starts here" marker per
   instrument-source would let the UI say so directly, but that is new schema
   and new UI. Is the reported-and-incomplete behavior enough?
4. **Is adding a Nasdaq-priced instrument to the demo seed acceptable?** Demo is
   a presentation surface, so its data is user-visible. The plan adds one so the
   default configuration exercises multi-source resolution; the alternative is
   leaving demo Yahoo-only and testing the path only in automated tests.
5. **Is API/log-level reporting of `Ambiguous` the intended final state, with
   the hand-map workflow documented for API use — or should the one existing
   refresh UI (the BackfillPanel) surface per-item states?** The plan takes the
   former (decision 5), which is consistent with the deliberate deferral of the
   ambiguity review queue, but that is a provisional answer and the framing has
   not been confirmed. The latter is real scope and, in the richer variant,
   needs a new per-instrument state on `/api/prices/status`.

**Resolved during review, recorded so it is not reopened:** whether removing
`mapping_enabled` / `provider_symbol` from `/api/prices/status` is safe.
Verification enumerated the complete consumer list for those two scalars —
`AssetView.tsx:810,815,825`, `assetViewModel.ts:322` and
`AddInstrumentDialog.tsx:143-144` — all inside this repository and all inside
the frontend phase's scope. No external consumer exists. Removal is safe; the
fields are removed outright rather than retained as documented Yahoo-only
fields.
