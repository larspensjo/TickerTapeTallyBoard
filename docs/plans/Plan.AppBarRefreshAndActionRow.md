# Plan — App-bar Refresh, Transactions-only manual entry, and removal of the portfolio action row

## Summary

On every portfolio tab (Dashboard, Holdings, Rebalance, Gains, Transactions) a
separate action row (`.portfolio-actions` in `PortfolioLayout.tsx`) sits between
the app bar and the summary cards, holding "Refresh prices" and "+ Add
transaction". It costs one full button row of vertical space (by the current CSS:
a 16px top padding + 36px button + the 16px layout gap = **~68px**) on every
portfolio tab.

This work:

1. Moves price refresh into a right-side **app-bar actions cluster**, present on
   every route (including `/import` and `/asset/:id`), as a compact outline
   icon+label "Refresh" button. Hidden in demo mode, as today.
2. Moves "Add transaction" to the Transactions page's panel header (mirroring
   "Add instrument" on Holdings); the form opens inside that panel above the list.
3. Deletes the `.portfolio-actions` row.
4. Replaces HTTP-error-only refresh feedback with a warning derived from the
   **last finished refresh run** (any trigger, any process) plus an unresolved
   failure of the user's own refresh request, shown next to the header Refresh
   button and in the PRICES card, persisting until a later run finishes cleanly.
   A run raises the warning only when a price or FX source actually failed, or
   the run failed outright (error, interruption); a run whose only problem is
   that some holding has no price source does not, whether the backend recorded
   it as partial or, having written nothing, as failed.
5. Gives header Refresh and the Import page's Backfill one shared "a refresh is
   running" signal, so each disables while any run is in progress, and makes
   Backfill honest when its request merged into an existing run.
6. Turns empty-state copy that pointed at the removed buttons into links to the
   Transactions page and Import (omitted in demo mode).

Done = the row is gone and summary cards start higher on the portfolio tabs,
proven by the before/after measurement recorded in this plan, with the app bar
not taller at full width or at ~800px.

One small, additive backend change is part of the work: the heartbeat
(`GET /api/data-version`) publishes the last finished run's outcome as a new
`last_refresh` field. It is isolated in its own phase, with a backend version bump
and widened workspace clippy.

## Settled decisions (from the brief and the user's answers — do not re-open)

- **Refresh lives in the app bar on every route**, including `/import` and
  `/asset/:id`; hidden when `appMode.canMutate` is false (demo, and while health
  is still loading — unchanged gating). Spinner icon while refreshing stays.
- **Style:** compact outline button, icon + visible label "Refresh"; accessible
  name "Refresh prices". Not the accent pill, not icon-only.
- **Actions cluster:** a deliberately placed right-side grid area in the app bar,
  built so the future global Undo and History controls
  (`docs/Design.CommandUndo.md` §14) can join it without layout rework. Those
  controls are **not** implemented here. At ≤900px the cluster stays on the nav's
  line rather than becoming its own row.
- **"Add transaction" only on the Transactions tab**, top-right in its panel
  header. `AddTransactionForm` itself is unchanged. `formOpen` stays view-local
  (DecisionLog 2026-06-19). An asset-detail variant with a pre-filled instrument is
  deferred, not rejected.
- **The `.portfolio-actions` row is removed entirely.**
- **Failure feedback reflects the server's last finished refresh run**, including
  launch-refresh runs and runs started by another process, plus transport/HTTP
  errors from the user's own refresh click; shown as a small warning indicator
  next to header Refresh and in the PRICES card; persists until a successful
  refresh; derived by pure, tested selectors and a pure reducer. Noise from a
  routinely failing instrument is accepted.
- **A run needs attention only when a price source actually failed** (or the run
  failed outright). A run whose only problem is unmapped holdings
  (`failed_items === 0`, `unmapped_instruments > 0`) does **not** raise the
  warning — neither as `partial` nor as the `failed` status the backend assigns
  when such a run wrote nothing. Unmapped holdings keep only their per-asset "No
  price source" label (consistent with the 2026-08-29 Nasdaq Nordic decision).
  This deliberately narrows the brief's original "failed or partial". The exact
  predicate is `runNeedsAttention` in the selector section.
- **The backend change is accepted:** `GET /api/data-version` gains an additive
  `last_refresh` field; the workspace version is bumped and workspace-wide clippy
  runs for that phase. There is no frontend-only fallback.
- **Empty states become links** to the Transactions page and/or Import; on the
  Transactions tab the copy refers to the button in its own header; in demo mode
  the invitation to add is omitted.
- **One shared "refresh running" signal** for header Refresh and Backfill; Backfill
  reports honestly when its request merged into a running run.
- **Measure the goal** at full width and ~800px; demo mode is expected to gain
  nothing from the row removal.
- **Tests:** selector and reducer tests first; component tests by role/text using
  the `MemoryRouter` + `vi.mock("../api/queries")` / `useAppMode` pattern from
  `HoldingsPage.test.tsx`. No snapshots, no DOM-structure assertions.

## Planner decisions (routine, reversible — recorded so review can see them)

- **Source of the last-run outcome: the heartbeat (`GET /api/data-version`).**
  The only place a run outcome is exposed today is `GET /api/prices/status`, whose
  payload is per-instrument and costs roughly five small queries per instrument.
  Mounting `usePriceStatus()` app-wide would refetch that whole payload on every
  revision change on every route. The heartbeat is already polled app-wide (2s
  while a refresh runs, 15s otherwise, re-checked on focus), already publishes
  `prices_refreshing`, and is unversioned, so adding the last finished run costs one
  indexed single-row query per heartbeat.
- **`last_refresh` is tri-state, so "unknown" never reads as "fine".** The field is
  a tagged object: `{"state":"none"}` (no run has ever finished on this ledger),
  `{"state":"finished","run":{…RefreshRunSummary…}}`, or
  `{"state":"unavailable"}` (the summary could not be read this time; logged on
  the backend). The heartbeat still answers 200 with its other fields, because
  every data query depends on it. The frontend keeps the last known outcome
  through `unavailable` responses, so a temporary read failure cannot erase a
  displayed warning.
- **"Last finished run", not "latest run".** The heartbeat publishes the
  most-recently-claimed run whose status is not `running` (ordered by run id,
  which the lease assigns in claim order, rather than by the server clock). While
  a new run is in progress the previous outcome therefore stays visible beside the
  spinning button — which is what "persists until a successful refresh" requires —
  and the published run changes identity only when another run finishes.
- **Refresh feedback is an app-level state slice.** What must be remembered
  across observations — the last known run outcome (kept through `unavailable`)
  and an unresolved request failure — lives in a pure reducer
  (`priceRefreshFeedbackReducer`) owned by one composite hook (`usePriceRefresh`)
  that `App` calls once and whose results it passes down as props, the same way it
  already owns and passes the date-range selection. Inputs reach the reducer as
  actions: heartbeat observations (dispatched from an effect when the heartbeat
  query's `dataUpdatedAt` changes, following the `RebalancePage` `planChanged`
  precedent) and the header request's settle callbacks. View derivation stays in
  pure selectors. Nothing reads TanStack's per-mutation `status`/`error` for the
  warning, so a retry (which resets the mutation to `pending` with no error)
  cannot clear it.
- **An unresolved request failure is cleared only by a qualifying finished
  outcome.** Two things qualify:
  1. A later refresh request from the header that returns a **finished** run
     (`succeeded`, `partial` or `failed`). Header requests are serialized — the
     controller ignores a request while anything is running — so such a run was
     claimed after the failure. A response with `status: "running"` (the request
     merely joined a run already in progress) and a pending retry do not qualify.
  2. A heartbeat reporting a **different** last finished run than the first
     heartbeat that resolved after the failure. That first post-failure
     observation is the failure's *baseline*: it describes everything that had
     already finished by then, including runs that finished before the click but
     had not yet been seen by the 15s idle poll. Only a run that finishes after the
     baseline was observed can supersede the failure. `unavailable` observations
     neither establish a baseline nor supersede.

  The baseline rule needs to know which heartbeat resolved after the failure. It
  compares the heartbeat query's `dataUpdatedAt` with the failure's own
  `Date.now()` — two readings of the **browser** clock ordering two browser-side
  events, never a browser timestamp against a backend one. The mutation's settle
  invalidation cancels any in-flight heartbeat fetch (TanStack's
  `invalidateQueries` defaults to `cancelRefetch: true`), so data that resolves
  after the failure comes from a fetch started after the request settled. The
  rule is conservative: if the backend was unreachable during the click and a
  launch refresh finished before the frontend reconnected, that run becomes the
  baseline and the request failure stays until the next run or click, rather than
  being cleared by a run the frontend cannot place relative to the failure.
- **Shared running signal** = heartbeat `prices_refreshing` OR any in-flight
  refresh request (TanStack `useIsMutating` over a shared mutation key, so the
  Backfill panel's own mutation instance counts too). The request half covers the
  gap between a click and the next heartbeat; the heartbeat half covers launch
  refresh, backfill, and other processes (the per-ledger lease, DecisionLog
  2026-09-18). This replaces the header's current
  `priceStatusQuery.data?.refreshing`, which only polls after it has already seen
  `refreshing: true` and whose cache key excludes refresh state.
- **PRICES card shows the warning chip alongside the freshness chip**, not instead
  of it. Today "Refresh failed" replaces the freshness chip, which was tolerable
  because the error vanished on the next click; now that the warning persists
  until a success, replacing would hide freshness indefinitely. "Refreshing" keeps
  its current priority over both.
- **Indicator placement:** immediately left of the Refresh button inside the
  cluster, so the button's right edge stays put when the indicator appears and any
  shift goes into the nav's slack.
- **Copy names both import sources** ("import from Avanza or Sharesight"), since
  Avanza is a first-class import source (DecisionLog 2026-06-16 Avanza CSV import).
- **Heartbeat cache token is unchanged.** `versionToken()` keeps using
  `data_revision@valuation_date`; the new field is not part of it. A finished run
  already bumps the revision (DecisionLog 2026-09-18), so data refetch behaviour
  does not change.
- **`PortfolioLayout` stops calling `usePriceStatus()`.** Its only uses were the
  refreshing flag (now from the shared signal) and an `isPending` contribution to
  "Checking" (now `gainsQuery.isFetching` alone). This removes the per-instrument
  payload from every portfolio tab; `AssetView` keeps its own use.
- **Mutation invalidates the heartbeat on settle, not only on success**, so a
  click that ends in HTTP 500 (the run row is finished as failed) or a transport
  error triggers an immediate heartbeat refetch, which also establishes the
  failure's baseline promptly.

## Not in scope

- Global Undo / History controls (only room for them is reserved).
- An "Add transaction" entry point on the asset detail page.
- Rendering per-item refresh details (which instrument failed, ambiguity
  candidates). The indicator is a summary; details remain API/log-level.
- Any change to the refresh algorithm, its status rules, or provider behaviour.
- Surfacing Backfill request failures in the header: the Backfill panel keeps
  showing its own request error, and its run's outcome reaches the header through
  the heartbeat like any other run.
- The mobile shell design (`docs/plans/Design.mobile-view-design.md`) — it has no
  desktop app bar and is unaffected.

## Code map (verified starting points)

| Area | File | Notes |
|---|---|---|
| App shell / header | `frontend/src/App.tsx` | Header is inline today; extract to `AppBar.tsx` to keep `App` thin. Already owns one app-level reducer (date-range selection) and passes its results down as props — the precedent for `usePriceRefresh`. |
| Action row (to delete) | `frontend/src/components/PortfolioLayout.tsx` | Holds Refresh, Add transaction, `formOpen`, `usePriceStatus`. |
| Summary cards | `frontend/src/components/PortfolioSummary.tsx` | Props `isRefreshingPrices`, `refreshError`. PRICES card chip priority: Refreshing > Refresh failed > freshness > Checking > No data. |
| Transactions page | `frontend/src/components/TransactionsPage.tsx` | Empty copy "Add one with the button above." |
| Backfill | `frontend/src/components/ImportView.tsx` (`BackfillPanel`, ~line 1001) | Own `useRefreshPrices()`; prints "Backfill {status}: wrote 0 …" on a merged run. |
| Empty states | `GainsPage.tsx:77`, `HoldingsPage.tsx:126`, `Dashboard.tsx` (~385, ~503) | Dashboard has no first-run state for an empty ledger. `RebalancePage`'s `empty_pool` and `AssetView`'s "No transactions recorded for this asset." do not point at a button; reviewed, unchanged. |
| Query layer | `frontend/src/api/queries.ts` | `useDataVersion` (heartbeat), `usePriceStatus`, `useRefreshPrices`. `versionedQueries.test.tsx` requires every exported `use*` hook to be versioned or a mutation — new composite hooks therefore live in `components/`, not `queries.ts`. |
| Query defaults | `frontend/src/main.tsx` | Queries retry 3 times with backoff; relevant to how quickly a heartbeat failure surfaces, not to the reducer. |
| Types | `frontend/src/api/dataVersion.ts`, `frontend/src/api/types.ts` | `DataVersion`; `RefreshRunSummary`, `RefreshPricesResult`. |
| Styles | `frontend/src/styles.css` | `.app-bar` grid (~107), `.portfolio-actions` (~280), `@media (max-width: 900px)` (~1686), `@media (max-width: 640px)` (~1707). |
| Heartbeat (backend) | `backend/src/api/data_version.rs` | Publishes `data_revision`, `valuation_date`, `prices_refreshing`; `prices_refreshing` already degrades to `false` with an `engine_warn!` on read failure. |
| Run records (backend) | `backend/src/db/market_data_runs.rs`, `backend/src/market_data/refresh.rs` (`run_summary`, `running_response`), `refresh_contract.rs` (`RefreshRunSummary`), `refresh_execution.rs` (~229 status rule) | Status is `partial` whenever `unmapped_instruments > 0` or `failed_items > 0` and something was written, and `failed` when nothing was written. Other `failed` runs: an execution error (counts left at zero, message is the error), `lease_lost` (counts as accumulated), and `abandoned` / `cancelled` (counts zero). Provider errors, including Nasdaq `Unavailable`, are counted in `failed_items` (`price_source_refresh.rs` returns `SourceRefreshOutcome::failed` for them). Run records are never deleted. |

## Layout design for the app bar

Markup (in a new `AppBar` component):

```
header.app-bar
  div.app-bar-identity      brand link + DEMO badge (badge no longer a grid column)
  nav.app-nav               unchanged items
  AppBarActions             div.app-bar-actions, role="group", aria-label="App actions"
```

Grid, placed by named areas rather than auto-placement:

- **Desktop (>900px):** `grid-template-columns: minmax(0, 1fr) auto auto;`
  `grid-template-areas: "identity nav actions";`. Bar `min-height: 56px` is
  unchanged; the compact button is 32px tall, so the bar does not grow.
- **≤900px:** `grid-template-columns: minmax(0, 1fr) auto;`
  `grid-template-areas: "identity identity" "nav actions";`. The cluster sits at
  the right end of the nav row (nav row `min-height: 40px` > 32px button), so the
  bar does not grow. Side effect: in demo mode the DEMO badge currently takes its
  own row at ≤900px (three auto-placed children in one column); inside the
  identity wrapper it joins the brand row, so the demo bar gets shorter there.
- **≤640px:** same areas; the nav may wrap inside its cell (it already has
  `flex-wrap: wrap`). The label stays visible; human testing decides whether a
  narrower treatment is ever needed.
- The cluster is a flex row with `gap: var(--space-2)`, so later Undo/History
  icon buttons slot in without new grid work. When the cluster renders nothing
  (demo), the empty `auto` track collapses.

New button size: `.button.compact { min-height: 32px; padding: 0 var(--space-3); }`
used with `.button.outline`. Warning indicator: a lucide warning-triangle icon in
`--warning` (semantic as text colour, per the dark theme), exposed as
`role="img"` with an accessible name carrying the label and detail, and a `title`
tooltip with the same text.

## Refresh feedback: state slice, selectors, and controller

### Heartbeat contract (frontend type)

```ts
// frontend/src/api/dataVersion.ts
export type LastRefresh =
  | { state: "none" }
  | { state: "finished"; run: RefreshRunSummary }
  | { state: "unavailable" };

export interface DataVersion {
  data_revision: string;
  valuation_date: string;
  prices_refreshing: boolean;
  last_refresh: LastRefresh;
}
```

### State slice — `frontend/src/components/priceRefreshFeedback.ts` (pure)

```ts
type RunObservation = RefreshRunSummary | null;   // null = no run has finished yet

interface PriceRefreshFeedbackState {
  lastRun: { known: false } | { known: true; run: RunObservation };
  requestFailure: null | {
    message: string;
    failedAt: number;                              // browser Date.now() at the failure
    baseline: { established: false } | { established: true; runId: number | null };
  };
}

type PriceRefreshFeedbackAction =
  | { type: "heartbeatObserved"; lastRefresh: LastRefresh; observedAt: number } // observedAt = dataUpdatedAt
  | { type: "requestFailed"; message: string; failedAt: number }
  | { type: "requestSucceeded"; result: RefreshPricesResult };

export const initialPriceRefreshFeedback: PriceRefreshFeedbackState;
export function priceRefreshFeedbackReducer(state, action): PriceRefreshFeedbackState;
```

Reducer rules:

- `heartbeatObserved`, `unavailable` → state unchanged (last known outcome kept;
  no baseline established; nothing superseded).
- `heartbeatObserved`, `none` / `finished` → `lastRun` becomes that observation
  (`null` for `none`). Then, if a `requestFailure` exists and
  `observedAt > failedAt`:
  - baseline not yet established → establish it with this observation's run id
    (`null` for `none`); the failure stays;
  - baseline established and this observation's run id differs from it → clear
    `requestFailure`.
  Observations with `observedAt <= failedAt` resolved before the failure and are
  ignored for the failure (they still update `lastRun`).
- `requestFailed` → `requestFailure` = the new message and `failedAt`, baseline
  not established (a newer failure replaces an older one).
- `requestSucceeded` with `result.status === "running"` (merged into a run in
  progress) → state unchanged.
- `requestSucceeded` with a finished status → clear `requestFailure`; `lastRun`
  becomes the result's run summary (the result minus `items`), so the header does
  not briefly show an older run's outcome before the settle refetch lands. The
  next heartbeat observation replaces it with the server's view.

### Selectors — `frontend/src/components/priceRefreshViewModel.ts` (pure)

```ts
interface PriceRefreshInputs {
  serverRefreshing: boolean;                // heartbeat prices_refreshing
  requestInFlight: boolean;                 // any refresh mutation pending (shared key)
  feedback: PriceRefreshFeedbackState;
}

interface PriceRefreshStatus {
  running: boolean;
  warning: null | {
    kind: "run_failed" | "run_incomplete" | "request_failed";
    label: string;   // short: "Refresh failed" | "Refresh incomplete"
    detail: string;  // plain-language sentence for tooltip / accessible name
  };
}

export function runNeedsAttention(run: RefreshRunSummary): boolean;
export function priceRefreshStatus(inputs: PriceRefreshInputs): PriceRefreshStatus;
export function backfillOutcome(result: RefreshPricesResult): { tone: "info" | "warning"; message: string };
```

Rules:

- `running = serverRefreshing || requestInFlight`.
- `runNeedsAttention(run) = run.failed_items > 0 || (run.status === "failed" && run.unmapped_instruments === 0)`.
  Derived from how the backend finishes runs (`refresh_execution.rs` status rule,
  `RefreshOutcome::lease_lost`, the reclaim/cancel updates in
  `market_data_runs.rs`, and the error path in `refresh.rs`):
  - `failed_items > 0` (any status) → a price or FX source failed → warn.
  - `failed` with zero counts → the run errored (`MarketDataError`, counts never
    populated, message is the error), was abandoned and reclaimed (`abandoned`),
    cancelled (`cancelled`), or lost its lease before counting anything → warn.
  - `failed` with `failed_items === 0` and `unmapped_instruments > 0` → the status
    rule's "nothing written" case whose only problem is unmapped instruments → no
    warning.
  - `partial` with `failed_items === 0` (necessarily `unmapped_instruments > 0`)
    → no warning. `succeeded` → no warning.
- Warning precedence: an unresolved `requestFailure` → `request_failed`
  ("Refresh failed", detail from the error message). Otherwise, a known last run
  that needs attention → `run_failed` for `failed`, `run_incomplete` for
  `partial`. Otherwise `null` — including a `succeeded` run, an unmapped-only
  `partial` or `failed` run, no run yet, and an outcome not yet known.
- Detail text names the trigger in plain words ("at launch", "manual",
  "backfill") and the counts that matter ("3 price sources failed"; when a
  warning run also has unmapped instruments, "1 holding has no price source" is
  appended), never the raw `message` field, except for a failed run with zero
  counts, whose message is the error (or a plain rewording of the `abandoned`,
  `cancelled` and `lease_lost` codes).
- `backfillOutcome`: `status === "running"` means the request merged into a run
  already in progress (a freshly claimed run only returns once finished) →
  warning message "Another price refresh was already running, so the backfill
  did not start. Try again when it finishes." Otherwise the existing
  "Backfill {status}: wrote N price rows and M FX rates." with `partial`/`failed`
  phrased as such and toned as warning.

Why the predicate looks at counts rather than status alone: the backend records a
run as `failed` when it wrote nothing at all, even if its only problem was
instruments without a price source (e.g. a ledger whose only holdings are
unmapped). Warning on every `failed` run would keep the warning lit permanently
for such a ledger, contrary to "warn only when a price source actually failed".
The predicate does not read the free-text `message`. One accepted corner follows:
a run that lost its lease mid-way after counting only unmapped instruments
(`failed`, `failed_items === 0`, `unmapped_instruments > 0`, message `lease_lost`)
does not warn; the process that took the lease over finishes its own run, which
then becomes the reported outcome. The selector tests pin both the unmapped-only
`failed` case and this corner so any later change is deliberate.

### Controller — `frontend/src/components/usePriceRefresh.ts` (effect boundary)

Called once, in `App`. It:

- owns `useReducer(priceRefreshFeedbackReducer, initialPriceRefreshFeedback)`;
- reads `useDataVersion()` and dispatches `heartbeatObserved` from an effect keyed
  on the query's `dataUpdatedAt` (so every successful fetch is observed, even when
  the data is structurally unchanged), with `observedAt = dataUpdatedAt`;
- reads `useIsMutating({ mutationKey: PRICE_REFRESH_MUTATION_KEY })` for
  `requestInFlight`;
- owns the header's `useRefreshPrices()` instance and exposes
  `refresh(): void`, which does nothing while `status.running` is true and
  otherwise calls `mutate({ mode: "latest" }, { onSuccess, onError })`, the
  per-call callbacks dispatching `requestSucceeded` / `requestFailed` (with
  `failedAt: Date.now()` taken in the callback, after the hook-level settle
  invalidation has run);
- returns `{ status: priceRefreshStatus(...), refresh }`.

`App` passes `priceRefresh` to `AppBar`, `priceRefresh.status` to
`PortfolioLayout` (→ `PortfolioSummary`), and `priceRefresh.status.running` to
`ImportView` (→ `BackfillPanel`). It lives in `components/` (like `useAppMode.ts`)
because `versionedQueries.test.tsx` enumerates every `use*` export of
`queries.ts`. `queries.ts` exports `PRICE_REFRESH_MUTATION_KEY` (a non-hook
constant), and `useRefreshPrices` gains `mutationKey` and `onSettled` heartbeat
invalidation. The Backfill panel keeps its own `useRefreshPrices()` instance for
its result text and error; the shared key makes it count toward `running`.

## Phases

Verification commands (from `Agents.md`):

- Frontend, from `frontend/`: targeted tests with `npm run test -- <paths>`, then
  `npm run check` (tsc + Biome + full Vitest) and `npm run fmt`. Use `npm.cmd` if
  launching through PowerShell `Start-Process`.
- Backend, from the repository root: `cargo build -p ticker-tape-tally-board-backend`,
  `cargo test -p ticker-tape-tally-board-backend`, then
  `cargo clippy -p ticker-tape-tally-board-backend --all-targets -- -D warnings`
  and `cargo fmt`. The backend phase touches the workspace manifest (version
  bump), so it widens clippy to `cargo clippy --workspace --all-targets -- -D warnings`.
- Running the app for visual checks: `.\scripts\start.ps1 -Dev` (dev ledger;
  create once with `-InitLedger` if missing), `.\scripts\start.ps1 -Demo`, and the
  desktop window (default Tauri size, ~800px wide).

### Phase 0 — Baseline measurement (no code)

Record the "before" numbers in the Measurement record below, on the current
`main`, before any change.

Procedure (agent if it can drive a browser; otherwise **human**):

1. Start the app (`.\scripts\start.ps1 -Dev`; an empty dev ledger is fine — the
   summary cards render regardless of data). Also run once with `-Demo`.
2. Open Dashboard and Holdings, scrolled to the top. In DevTools responsive mode
   set the viewport to 1440×900, then 800×600. In the console run:

   ```js
   const r = (s) => document.querySelector(s)?.getBoundingClientRect();
   ({ appBar: r(".app-bar")?.height, summaryTop: r('[aria-label="Portfolio summary"]')?.top })
   ```

3. Repeat in the desktop window at its default size if available.

Verification: the table has before values for every row. No builds needed.

### Phase 1 — "Add transaction" moves to the Transactions page

Changes:

- `TransactionsPage.tsx`: view-local `formOpen` state; a secondary "Add
  transaction" button (Plus icon) top-right in the panel header, shown only when
  `canMutate`, toggling the form, with `aria-expanded`. When open, render the
  unchanged `AddTransactionForm` inside the ledger panel between the header and
  the table, wrapped in a `section` with `aria-label="Add transaction"` and an
  `h2` (no nested panel chrome). Inputs: `useInstruments()` (already present) and
  `useDataVersion().data?.valuation_date` for `tradeDate`, exactly as
  `PortfolioLayout` passes them today.
- Transactions empty-state copy refers to the button in its own header ("No
  transactions yet. Use Add transaction above." — the Import link is added with
  the empty-state links). Demo: "No transactions yet."
- `PortfolioLayout.tsx`: remove the Add transaction button, `formOpen`, the form
  panel, and the now-unused `useInstruments`/`useDataVersion` imports. The row now
  holds only Refresh (removed with the app-bar move).
- Styles: add any small spacing rule needed for the in-panel form section.

Tests (new `frontend/src/components/TransactionsPage.test.tsx`, jsdom pragma,
`vi.mock("../api/queries")` + `vi.mock("./useAppMode")` as in
`HoldingsPage.test.tsx`):

- With `canMutate`, a button named "Add transaction" exists; clicking it reveals
  the region named "Add transaction" (by role); the form's Cancel hides it.
- In demo (`canMutate: false`) no "Add transaction" button.
- Empty ledger copy refers to the header button when mutable; plain sentence in
  demo.

Verify: `npm run test -- src/components/TransactionsPage.test.tsx`, then
`npm run check`, `npm run fmt` (from `frontend/`).

Human check (optional at this point): add a transaction from the Transactions
page in the dev ledger; the list updates.

### Phase 2 — Empty states link to where the actions now live

Changes:

- New presentational component `frontend/src/components/LedgerEntryInvitation.tsx`
  rendering the invitation sentence with router `Link`s (`--accent-link` text):
  - `variant="elsewhere"`: "Add one on the [Transactions page] or [import from
    Avanza or Sharesight]."
  - `variant="transactions"`: "Use Add transaction above, or [import from Avanza
    or Sharesight]."
  - Renders nothing when `canMutate` is false (Import is redirected away in demo,
    and nothing can be added).
- Apply it (the lead sentence stays page-specific, passed as `AsyncBoundary`'s
  `emptyMessage`, which already accepts `ReactNode`):
  - `TransactionsPage`: "No transactions yet." + `transactions` variant.
  - `HoldingsPage`: "No holdings yet." + `elsewhere` variant (replaces "Add a Buy
    to get started.").
  - `GainsPage`: "No valued holdings yet." + `elsewhere` variant, followed by
    "Then refresh prices." (replaces "Add a Buy and refresh prices.").
  - `Dashboard`: new first-run state. When the value-history response has
    `start_date === null` (the backend sets it only from the earliest
    transaction, so null means the ledger has no transactions), the chart panel
    band shows "No transactions yet." + `elsewhere` variant instead of "No
    portfolio history in this interval". The emptiness test is a small pure
    selector in `portfolioValueViewModel.ts` (`ledgerHasNoTransactions`). The
    waterfall panel's "No valued holdings in this interval." stays.
- Update `docs/VisualDesign.DarkTheme.md` → **States → Empty**: centered
  `--text-muted` message; when the action lives on another page, inline
  `--accent-link` links to it instead of a CTA button; the invitation is omitted
  where data cannot be changed (demo).

Tests:

- `LedgerEntryInvitation.test.tsx`: links named "Transactions page" and "import
  from Avanza or Sharesight" with the right `href`s when mutable; nothing in demo.
- `TransactionsPage.test.tsx`: empty ledger shows the Import link and refers to
  "Add transaction"; demo shows neither.
- `HoldingsPage.test.tsx`: add an empty-holdings case asserting the Transactions
  link (and its absence in demo — this requires the existing `useAppMode` mock).
- `Dashboard.test.tsx`: add `useAppMode` mock; with `start_date: null`, the
  Transactions and Import links appear; with a non-null start date, they do not.
- `portfolioValueViewModel.test.ts`: `ledgerHasNoTransactions` on null vs a date,
  and on missing data (pending/error → false, so no flash of the invitation).

Verify: targeted tests above, then `npm run check`, `npm run fmt`.

### Phase 3 — Refresh moves to the app-bar actions cluster; the row is removed

This phase builds the shared running signal and the request-failure half of the
feedback slice. The run-outcome half arrives once the heartbeat publishes it.

Changes:

- `queries.ts`: export `PRICE_REFRESH_MUTATION_KEY`; `useRefreshPrices` uses it
  and invalidates `["data-version"]` in `onSettled` (success and error).
- New `priceRefreshFeedback.ts` with `requestFailure` (holding only `message` at
  this stage) and the `requestFailed` / `requestSucceeded` actions: a failure is
  retained until a later request returns a finished run; a merged (`running`)
  response leaves it in place.
- New `priceRefreshViewModel.ts` with `priceRefreshStatus` (running rule and the
  `request_failed` warning) and `backfillOutcome`, as specified above.
- New `usePriceRefresh.ts` controller (reducer, `useIsMutating`, heartbeat
  `prices_refreshing`, the header mutation instance with its per-call callbacks,
  and the "ignore while running" guard).
- New `AppBar.tsx` (identity, nav, actions) and `AppBarActions.tsx` containing the
  Refresh control: `button outline compact`, `RefreshCw` icon spinning while
  `running`, visible label "Refresh", `aria-label="Refresh prices"`, disabled
  while `running`, `onClick` → `priceRefresh.refresh()`; the warning indicator to
  its left when `warning` is non-null. Renders the Refresh control only when
  `canMutate`.
- `App.tsx` calls `usePriceRefresh()` once, renders `<AppBar priceRefresh={…} />`
  in place of the inline header, and passes `refreshStatus` to `PortfolioLayout`
  and `priceRefreshRunning` to `ImportView`; nothing else moves.
- `PortfolioLayout.tsx`: delete the action row, `useRefreshPrices`,
  `usePriceStatus`; accept `refreshStatus: PriceRefreshStatus` and pass it to
  `PortfolioSummary`; `isCheckingPrices` becomes `gainsQuery.isFetching`.
- `PortfolioSummary.tsx`: replace `isRefreshingPrices`/`refreshError` props with
  `refreshStatus: PriceRefreshStatus`. Chip rules: running → "Refreshing" (as
  today); otherwise warning chip (`status-chip warning`, text = `label`, `title` =
  `detail`) **plus** the freshness / Checking / No data chip.
- `ImportView.tsx`: accepts `priceRefreshRunning` and hands it to `BackfillPanel`,
  which is disabled while it is true, shows a muted note "A price refresh is
  running. Backfill is available when it finishes." when disabled by a run it did
  not start (running and its own request not pending), renders result text from
  `backfillOutcome(result)`, keeps its own request-error text, and spins while its
  own request is pending.
- `styles.css`: `.app-bar` grid areas and the ≤900px/≤640px rules from the
  Layout design section; `.app-bar-identity`, `.app-bar-actions`,
  `.button.compact`, indicator styles; delete `.portfolio-actions`.
- `docs/VisualDesign.DarkTheme.md` → **Components → App bar**: replace "right
  side: outline 'Refresh' + primary 'Add transaction'" with the actions cluster
  (compact outline Refresh with icon, warning indicator beside it, reserved for
  Undo/History, stays on the nav row at ≤900px); note that the summary band
  beneath belongs to the portfolio tabs; mention "Add transaction" is a secondary
  button in the Transactions panel header. **Buttons**: add the compact size
  (~32px) for app-bar use.
- `docs/Design.HighLevel.md` (~line 92): "The ordinary **Refresh prices** button"
  → the app-bar Refresh button (accessible name "Refresh prices"); still sends
  `mode: "latest"`.
- Append DecisionLog entry A (draft below).

Tests:

- `priceRefreshFeedback.test.ts` (reducer): `requestFailed` records the failure;
  a later `requestSucceeded` with a finished status clears it; one with
  `status: "running"` (merged) does not; a second `requestFailed` replaces the
  message.
- `priceRefreshViewModel.test.ts`: running from heartbeat only, from request only,
  neither; an unresolved failure → `request_failed` with the error text;
  `backfillOutcome` for `running` (merged), `succeeded`, `partial`, `failed`.
- `usePriceRefresh.test.tsx` (effect boundary, real `QueryClient`, stubbed
  `fetch` as in `versionedQueries.test.tsx`; drive heartbeat refetches explicitly
  rather than waiting on the poll interval):
  - a pending POST started through the controller reports `running`; a pending
    POST started by a separately rendered `useRefreshPrices()` (the Backfill
    instance) also reports `running`; resolving clears it; a heartbeat with
    `prices_refreshing: true` and no request reports `running`;
  - a 500 response yields `request_failed` and triggers a heartbeat refetch;
  - **retry pending:** after a failure, a second `refresh()` whose POST is still
    pending keeps `request_failed`;
  - **merged response:** after a failure, a retry answered 200 with
    `status: "running"` keeps `request_failed`;
  - a retry answered with a finished `succeeded` run clears the warning;
  - `refresh()` while `running` sends no POST.
- New `frontend/src/App.test.tsx` (jsdom; `MemoryRouter initialEntries`; mock
  `../api/queries`, `./components/useAppMode`, `./components/usePriceRefresh`,
  and stub heavy route modules — `Dashboard`, `HoldingsPage`, `RebalancePage`,
  `GainsPage`, `ImportView`, `AssetView`, `AppFooter` — keeping `AppBar`,
  `PortfolioLayout`, `PortfolioSummary`, and `TransactionsPage` real; use
  `findBy*` for the lazy routes):
  - Button "Refresh prices" present on `/`, `/import`, `/asset/7`; absent in demo.
  - Clicking it calls the controller's `refresh`.
  - "Add transaction" button present only on `/transactions`; absent on `/`,
    `/holdings`, `/gains`, `/import`.
  - With a mocked `request_failed` status, the warning (by role `img` and name)
    is visible on `/import` and `/asset/7`.
  - With `running: true`, "Refresh prices" is disabled.
- Backfill test (new `ImportView.backfill.test.tsx`, mocking the import
  mutations and `useRefreshPrices`): with `priceRefreshRunning` true the
  "Backfill full price history" button is disabled and the note is shown; a mocked
  result with `status: "running"` renders the merged message, not "wrote 0".
- `PortfolioSummary.test.tsx` (new): warning chip and freshness chip render
  together; "Refreshing" replaces both while running.

Verify: targeted tests, `npm run check`, `npm run fmt` (from `frontend/`).

Measurement: fill the "after" column for all rows (same procedure as Phase 0).
Expected: `summaryTop` ~68px smaller in non-demo at both widths; `appBar` height
unchanged at 1440 and at 800 (demo at 800 may be shorter, see Layout design);
demo `summaryTop` unchanged at 1440. Any growth of `appBar` is a defect to fix in
this phase.

**Human testing recommended here:** header at full width and ~800px (browser and
desktop window), and ≤640px; demo mode (no Refresh, DEMO badge beside the brand,
nothing in the cluster); start a Refresh, open `/import` — Backfill is disabled
with the note; start a Backfill, go to a portfolio tab — header Refresh is disabled
and spinning; Add transaction appears only on the Transactions page; stop the
backend and click Refresh — the warning appears and survives a second, also
failing, click.

### Phase 4 — Backend: the heartbeat publishes the last finished run

Changes:

- `backend/src/db/market_data_runs.rs`: `latest_finished(pool)` — the run with
  the highest id whose status is not `RUNNING` (`ORDER BY id DESC LIMIT 1`). Ids
  are assigned in claim order under the lease and run records are never deleted,
  so this is the most recently claimed finished run, independent of the server
  clock.
- `MarketDataService::last_finished_run(&pool) -> Result<Option<RefreshRunSummary>, MarketDataError>`
  using the existing `run_summary` mapping, so the API layer does not reach into
  `db/`.
- `backend/src/api/data_version.rs`: a serialized enum
  `#[serde(tag = "state", rename_all = "snake_case")] enum LastRefresh { None, Finished { run: RefreshRunSummary }, Unavailable }`,
  and `DataVersionResponse` gains `last_refresh: LastRefresh` (additive; no
  existing field changes; the run object has the same shape as
  `PriceStatusResponse.latest_run`). `Ok(None)` → `none`; `Ok(Some(run))` →
  `finished`; `Err` → `unavailable` with
  `crate::engine_warn!("could not read last finished market-data refresh run operation=data_version error={error}")`.
  The heartbeat keeps answering 200, as it already does when the refresh claim
  cannot be read.
- Bump `[workspace.package].version` in the root `Cargo.toml` (0.19.0 → 0.20.0)
  for the additive API field.
- `README.md`: the `GET /api/data-version` bullet also mentions the last
  finished refresh run's outcome and that it reports `unavailable` rather than
  failing when it cannot be read.

Tests (in `data_version.rs` / `market_data_runs.rs` / the refresh tests):

- No runs → `last_refresh` is `{"state":"none"}`.
- A finished `partial` run with `failed_items` and `unmapped_instruments` is
  published as `finished` with those counts and `trigger`.
- While a run holds the lease, `prices_refreshing` is `true` and `last_refresh`
  is the **previous finished** run, not the running one.
- A summary read failure (e.g. the runs table renamed on the test pool) still
  answers 200 with `data_revision` and `valuation_date`, and `last_refresh` is
  `{"state":"unavailable"}` — not `none`.
- Cross-process: a run finished through one `AppState` is visible on another
  `AppState`'s heartbeat over the same file (extend the existing separate-pools
  test rather than adding a new fixture).
- A provider outage item (`Unavailable`) is counted in `failed_items` and makes
  the run `partial`/`failed` — the warning rule depends on it. The code already
  behaves this way; add a refresh test pinning it (the existing item-failure test
  covers only `currency_mismatch`).

Verify (repository root): `cargo build -p ticker-tape-tally-board-backend`,
`cargo test -p ticker-tape-tally-board-backend`,
`cargo clippy --workspace --all-targets -- -D warnings` (widened because the
workspace manifest changed), `cargo fmt`. No desktop code changes; the desktop
shell inherits the field through the shared router.

### Phase 5 — The warning reflects the last run's outcome

Changes:

- `frontend/src/api/dataVersion.ts`: `LastRefresh` type; `DataVersion` gains
  `last_refresh: LastRefresh`. `versionToken` unchanged.
- `priceRefreshFeedback.ts`: full state and rules from the contract —
  `lastRun` (kept through `unavailable`), `heartbeatObserved`, `failedAt`, the
  post-failure baseline and supersession, and `requestSucceeded` folding a
  finished run into `lastRun`.
- `priceRefreshViewModel.ts`: `runNeedsAttention`
  (`failed_items > 0 || (status === "failed" && unmapped_instruments === 0)`), the
  run warnings, precedence, and plain-language detail text.
- `usePriceRefresh.ts`: the `dataUpdatedAt`-keyed effect dispatching
  `heartbeatObserved`; `requestFailed` carries `failedAt`.
- No component changes beyond what the app-bar move built: the header indicator
  and PRICES card already render `warning`.
- `docs/VisualDesign.DarkTheme.md` → **Semantics**: add the refresh warning
  indicator to `--warning`'s roles.
- Append DecisionLog entry B (draft below).

Tests:

- `priceRefreshViewModel.test.ts`:
  - `failed` with `failed_items > 0` → `run_failed`; `partial` with
    `failed_items > 0` → `run_incomplete`; `succeeded` → none;
  - **unmapped-only partial:** `partial`, `failed_items: 0`,
    `unmapped_instruments > 0` → no warning;
  - **unmapped-only failed:** `failed`, `failed_items: 0`,
    `unmapped_instruments > 0`, nothing written → no warning;
  - `failed` with all counts zero (error message; and each of `abandoned`,
    `cancelled`, `lease_lost`) → `run_failed`;
  - the accepted corner: `failed`, `failed_items: 0`, `unmapped_instruments > 0`,
    message `lease_lost` → no warning;
  - `failed_items > 0` together with `unmapped_instruments > 0` → warning, detail
    mentions both counts;
  - launch/backfill/manual triggers named in the detail; a failed run with zero
    counts surfaces its message;
  - an unresolved request failure outranks a run warning; outcome not yet known →
    no warning.
- `priceRefreshFeedback.test.ts` (reducer):
  - **unavailable keeps the last known outcome:** observe a failed run, then
    `unavailable`, then a succeeded run → `lastRun` is failed after the second
    step and succeeded after the third (selector: `run_failed`, `run_failed`, none);
  - `unavailable` before anything is known leaves the outcome unknown;
  - **stale history does not supersede a failure:** `lastRun` = run 10;
    `requestFailed` at t; an observation of run 11 with `observedAt > t`
    establishes the baseline (failure kept); a later observation of run 11 keeps
    it; an observation of run 12 clears it;
  - an observation with `observedAt <= failedAt` neither establishes the baseline
    nor clears the failure;
  - `unavailable` after a failure establishes nothing; the next definite
    observation does;
  - a `none` baseline is superseded by the first finished run;
  - a finished `requestSucceeded` clears the failure and sets `lastRun` to its
    run; a merged one changes nothing.
- `usePriceRefresh.test.tsx` (effect boundary) — **the reviewer's scenario:**
  heartbeat fixture reports run 10 (succeeded) and is cached; the server's next
  heartbeat reports run 11 (succeeded, i.e. finished before the click without
  being seen); `refresh()`'s POST fails with a network error; the settle refetch
  returns run 11 → status stays `request_failed`; a further heartbeat reporting
  run 12 succeeded → no warning (variant: run 12 failed → `run_failed`). Also:
  failed run → `unavailable` heartbeat → succeeded run, observed through the real
  query, shows `run_failed`, `run_failed`, none.
- `dataVersion.test.ts`: the token ignores `last_refresh`.
- `versionedQueries.test.tsx`: add `last_refresh: { state: "none" }` to the
  `VERSION` fixture.
- `App.test.tsx`: with a mocked status derived from a failed last run, the
  indicator is visible on `/import` and `/asset/7` (no click involved — this is the
  launch-refresh case).

Verify: targeted tests, `npm run check`, `npm run fmt` (from `frontend/`).

**Human testing recommended here (forced failures):**

1. Launch failure: disconnect the network, start the app (non-demo). After the
   launch refresh finishes, the warning appears beside Refresh on every route and
   in the PRICES card, without any click.
2. Click failure with HTTP 200: still offline, click Refresh — the warning stays
   ("Refresh failed"), its tooltip names a manual refresh.
3. Recovery: reconnect, click Refresh — the warning clears when the run succeeds,
   and also clears when the run's only problem is that some holding has no price
   source (that holding keeps its "No price source" label).
4. Unmapped-only ledger: in a scratch dev ledger whose only holding has no price
   source (e.g. an instrument with no enabled mapping), click Refresh — the run
   writes nothing and is recorded as failed, yet no warning appears; the asset
   shows "No price source".
5. Transport failure: with the app open, stop the backend, click Refresh — the
   warning shows the request error and stays through a second failing click.
   Restart the backend: if the frontend sees the launch refresh running, the
   warning switches to that run's outcome when it finishes; if the launch refresh
   had already finished before the frontend reconnected, the request error stays
   — a click on Refresh then resolves it.
6. Two shells: with the desktop window and the browser on the same ledger, a
   refresh started in one shows spinning/disabled Refresh in the other within one
   heartbeat interval, and its outcome appears in both.

### Phase 6 — Versions, documents, and final verification

- Bump `frontend/package.json` version (0.26.0 → 0.27.0).
- Review that every document in the list below is updated and that no durable
  document or code comment refers to this plan's phases.
- Run the full frontend gate (`npm run check`, `npm run fmt` in `frontend/`) and
  the backend gate from the repository root as in the backend phase (build, test,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt`).
- Confirm the Measurement record is complete and matches expectations.

## Measurement record

| Shell / mode | Viewport | Page | appBar before | appBar after | summaryTop before | summaryTop after |
|---|---|---|---|---|---|---|
| Browser, non-demo | 1440×900 | Dashboard | 56 | 56 | 144 | 76 |
| Browser, non-demo | 1440×900 | Holdings | 56 | 56 | 144 | 76 |
| Browser, non-demo | 800×600 | Dashboard | 102 | 102 | 190 | 122 |
| Browser, non-demo | 800×600 | Holdings | 102 | 102 | 190 | 122 |
| Browser, demo | 1440×900 | Dashboard | 56 | 56 | 76 | 76 |
| Browser, demo | 800×600 | Dashboard | 140.5 | 103.5 | 160.5 | 123.5 |
| Desktop window, non-demo | default (1258×1138 CSS px at 150% scaling) | Dashboard | 56 | 56 | 144 | 76 |

Before values taken 2026-10-05 at commit `c08de50` (the Transactions-page entry
and empty-state links change neither measured element), on an empty dev ledger,
in headless Chrome (`Emulation.setDeviceMetricsOverride`, scale 1) and, for the
desktop row, inside the debug desktop window over the WebView2 debugging port.
The desktop window's default viewport is not ~800px wide as assumed above; its
800px behaviour is covered by the browser 800×600 rows.

After values taken 2026-10-05 on the uncommitted app-bar change (on top of
`c71dbd1`), same ledger and method; the desktop window was rebuilt first. Every
non-demo row lost exactly 68px; no app bar grew; the demo bar at 800px lost its
DEMO-badge row (37px). An extra 600×800 non-demo check: app bar 102, summary
top 114 (nav still on one line).

Expected: non-demo `summaryTop` falls by ~68px at both widths; `appBar` does not
grow anywhere; demo `summaryTop` unchanged at full width (the row was already
hidden there) and possibly smaller at 800px because the DEMO badge no longer takes
its own app-bar row.

## Documents to update

| Document | Change | Phase |
|---|---|---|
| `docs/VisualDesign.DarkTheme.md` | States → Empty (links instead of CTA where the action lives elsewhere; omitted in demo) | 2 |
| `docs/VisualDesign.DarkTheme.md` | Components → App bar (actions cluster, compact outline Refresh, warning indicator, reserved for Undo/History, nav-row placement at ≤900px; summary band on portfolio tabs only; Add transaction on the Transactions panel); Buttons (compact size) | 3 |
| `docs/VisualDesign.DarkTheme.md` | Semantics → `--warning` role includes the refresh warning indicator | 5 |
| `docs/Design.HighLevel.md` | Line ~92 "The ordinary **Refresh prices** button" → the app-bar Refresh button (accessible name "Refresh prices"); still sends `mode: "latest"` | 3 |
| `docs/DecisionLog.md` | Entry A | 3 |
| `docs/DecisionLog.md` | Entry B | 5 |
| `README.md` | `GET /api/data-version` bullet mentions the last finished refresh outcome and its `unavailable` state | 4 |
| Root `Cargo.toml` | `[workspace.package].version` 0.19.0 → 0.20.0 | 4 |
| `frontend/package.json` | version 0.26.0 → 0.27.0 | 6 |

`docs/Design.CommandUndo.md` needs no change: it already places Undo/History in
the app bar; the cluster is where they will go.

### Decision log drafts

Append at the end of `docs/DecisionLog.md`, dated on the day each is recorded.
Wording must name behaviours, not this plan.

**Entry A — App-wide actions live in the app bar; manual entry lives on the
Transactions page**

```
## YYYY-MM-DD - App-Wide Actions Live In The App Bar; Manual Entry Lives On The Transactions Page
Decision: The app bar carries an app-wide actions cluster at its right edge on every route. It holds the price refresh action, shown wherever the data can be changed and hidden in demo, and it is the reserved home for the global Undo and History controls. Portfolio pages carry no page-level action row. Manual transaction entry is offered only on the Transactions page, from that page's panel header, and its open state stays local to that page. Empty states that used to point at a page-level button link instead to where the action now lives — the Transactions page and Import — and omit the invitation where data cannot be changed.
Context: A row of two buttons sat between the app bar and the summary cards on every portfolio page, costing a full row of vertical space for one app-wide action and one rarely used one, while the refresh action was missing from the import and asset pages.
Consequences: Refines the 2026-06-19 Client-Side Routing And Asset Detail Page entry, which scoped the app-bar actions to the board route and kept the transaction form's open state board-local; both are now app-wide and Transactions-page-local respectively. Follows the vertical-space precedent of the 2026-07-14 Gains page header compaction. At narrow widths the cluster stays on the navigation row, so future app-wide controls join the cluster rather than adding a row. Adding a transaction from another page costs one navigation; an asset-page entry point with the instrument pre-filled is deferred, not rejected.
```

**Entry B — Refresh feedback reports the last finished run from any trigger**

```
## YYYY-MM-DD - Refresh Feedback Reports The Last Finished Run, And All Refresh Triggers Share One Running Signal
Decision: The interface's refresh warning reflects the outcome of the most recent finished market-data refresh run on the ledger, whatever started it — launch, the refresh action, a backfill, or another process — and persists until a later run finishes without needing attention. A run needs attention when at least one price or exchange-rate source failed, or when the run failed for any reason other than instruments lacking a price source; a run whose only problem is instruments without a price source does not raise the warning — whether it is recorded as partial or, having written nothing, as failed — and that state keeps its per-asset signal. A failed refresh request — transport or HTTP error — is shown until the heartbeat reports a different last finished run than the first one it reported after the failure, or until a later refresh request returns a finished run; a pending retry, or a request that joined a run already in progress, does not clear it. The data-version heartbeat publishes the last finished run's summary as one of three states — no run yet, a finished run, or outcome unavailable — and an unavailable outcome leaves the last known one displayed; the summary is not part of the cache version token. The interface keeps this feedback in one app-level state slice fed by heartbeat observations and refresh-request outcomes. Every refresh trigger in the interface derives "a refresh is running" from one shared signal — the heartbeat's flag combined with any in-flight refresh request — so no trigger can start while a run is in progress, and a request that joined a run already in progress is reported as having started nothing.
Context: Provider outages finish as failed or partial runs behind a successful HTTP response, so feedback limited to HTTP errors stayed silent through real outages, launch refreshes and runs started elsewhere were invisible on idle pages, and a backfill pressed during another refresh silently did nothing. A failure indicator tied to the latest request's state vanished on retry, and comparing a cached run against one discovered after the click could mistake an earlier run for recovery.
Consequences: Refines the 2026-09-12 Data Is Served And Requested By Revision entry (the heartbeat now carries the last run's outcome; one more small query per heartbeat, on top of the cost noted in the 2026-09-18 per-ledger lease entry) and fulfils the last-refresh status surfacing promised by the 2026-06-16 Market Data Refresh Triggering entry. Refines the 2026-08-29 Nasdaq Nordic entry's statement that the per-asset source labels are the only UI signal: a summary-level warning now also reports provider failure, while per-item details stay API- and log-level and an instrument without a price source is still signalled only per asset. The last finished run is the most recently claimed one, so its identity changes only when another run finishes; supersession relies on that and orders only browser-side events, never browser against backend clocks. The interface tells these cases apart by the run's failed-item and unmapped-instrument counts, never by its free-text message, so a run interrupted after counting only unmapped instruments does not warn; the run that took over reports instead. A request failure made while the backend was unreachable can outlast a launch refresh that finished before the interface reconnected; the next refresh clears it. A routinely failing instrument keeps the warning lit; narrowing further is a possible follow-up.
```

## Open Questions

None. Both earlier questions are settled (see Settled decisions): a run warns only
when a price source actually failed, and the heartbeat carries the additive
`last_refresh` field.
