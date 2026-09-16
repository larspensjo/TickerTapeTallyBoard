# Plan — TickerTapeTallyBoard as a native Windows window

## Summary

Give the app its own Windows application window (Tauri v2 + WebView2) in which the
UI reaches Rust **in-process** — no TCP listener, no port, no firewall prompt —
while the existing web-server deployment keeps working from the same source
under its deliberate production and development run models.

Done means: launching the desktop app shows the real portfolio in a native
window; every page and every `/api/*` route behaves identically to the browser
build, proven by a probe that runs **inside the real WebView2 window** and by
adapter-level Rust tests underneath it (not by a route count);
`scripts/start.ps1` still runs the web app exactly as it does today; the ledger
the window opens is named and visible in the UI, and the app refuses to start
rather than silently creating an empty one; and a market-data refresh and the
data revision are coordinated across every process on that ledger.

**Out of scope:** MSI/installer, icons, branding, code signing; tray icon, native
menus, notifications, file-drop import, single-instance UI; multi-portfolio
support and a ledger picker. See *Deliberately out of scope*.

## Landing order — read this before editing anything

The production-setup hardening has landed before this plan. It delivered the
loopback-only web server, app-data ledger, refuse-to-create behavior, launch-time
backups, bounded app-data logs, pinned port `8480`, and the new launcher flag
surface. The current versions are backend `0.18.0` and frontend `0.24.0`.

This plan was written expecting `docs/plans/Plan.NasdaqNordicPriceProvider.md` to
land first. That provider work has landed too, including migration `0007`.

What the Nasdaq work actually does that matters here: it makes provider identity
enum-typed through `db/`, adds a migration, and bumps both manifests to `0.15.0`
/ `0.23.0`. **It does not restructure `backend/src/app.rs`** — an earlier draft of
this plan said it did, which was wrong. Its only `app.rs` touch is enum-typing one
hardcoded provider string. The composition-root split is entirely this plan's own
work whichever order the two land in.

Consequences that are binding on the implementer:

- Every reference below to `app.rs`, `config.rs`, `refresh.rs` or `db/` describes
  **behavior**, never a line number. Those lines may have moved.
- **Phase 1 opens with an explicit re-verification step.** Do not start editing
  until the assumptions listed there have been checked against the then-current
  tree.
- Version bumps are stated relative to the current versions when this plan lands;
  after the production hardening release they are backend `0.18.0` and frontend
  `0.24.0`. The desktop release should make its own minor bump from those real
  versions when it lands.
- **Migration numbering, and a collision to avoid.** This plan adds one additive
  migration, referred to by name as `add_refresh_run_claim.sql`. It takes the next
  free number: `0007` if this plan lands first, `0008` if the Nasdaq work lands
  first. `Plan.NasdaqNordicPriceProvider.md` names its own migration
  `0007_provider_codes_and_asset_class.sql`, so **if this plan lands first, that
  one must be renumbered when it lands.** Noted here only; do not edit the Nasdaq
  plan, which is a separate ephemeral document under its own review.

`docs/plans/Plan.HandEnteredPrices.md` is **postponed indefinitely and must not
be assumed to have landed.** Context only, not work for this plan: that plan and
the Nasdaq plan share an identical "Phase 1 — One resolved price source" refactor
and both bump to `0.15.0`. If hand entry is ever revived, that overlap needs
reconciling first; nothing in this plan depends on it either way.

## Settled decisions (implement these; do not re-litigate)

1. **Single origin, one custom URI scheme.** The window opens on a registered
   custom scheme (`WebviewUrl::CustomProtocol`), and that one scheme's
   *asynchronous* protocol handler serves both the SPA and `/api/*` by feeding
   the request into the existing `crate::api::router_with_static_assets(...)`
   through `tower`'s `oneshot`.
2. **The frontend transport is untouched.** `frontend/src/api/client.ts` is not
   modified. No transport abstraction, no base-URL selector, no `isTauri()`
   branch, and no `@tauri-apps/api` dependency in `frontend/package.json`.
   Relative `fetch("/api/...")` works because page and API share one origin. If
   implementation finds a frontend transport change unavoidable, **stop and raise
   it as an open question** rather than adding one.
3. **`backend/src/api/cors.rs` is untouched and must not be widened.** It is
   currently `allow_origin(Any).allow_methods([Method::GET]).allow_headers(Any)`
   and it guards the web server that `docs/plans/Design.mobile-view-design.md`
   wants exposed on the LAN. Because page and API share an origin, writes from
   the desktop window are same-origin and never reach a CORS check.
   **Rejected:** two origins (Tauri asset protocol for the page, a second scheme
   for the API) plus widened CORS.
4. **Rejected: ~23 typed Tauri commands** over an extracted transport-neutral
   service layer. It pays off only if HTTP goes away, which it does not, and it
   doubles per-endpoint maintenance forever.
5. **Walking skeleton first, and it must run inside the window.** The single
   biggest unknown is whether WebView2's custom-protocol handling carries the
   awkward cases. Adapter tests that call the bridge function directly do **not**
   prove that — they prove the axum side. The gate is an in-window probe; see
   *The three cases* and *The in-window probe*.
6. **Assets are served from `frontend/dist` on disk, not embedded.** The user is
   not distributing the app; embedding would make every CSS tweak require a Rust
   rebuild.
7. **One launch mode, always the real in-process path.** No Vite dev server
   inside the window, no dev/prod window branching, no HMR. `scripts/start.ps1`
   keeps its Vite dev workflow for the web app; the desktop window simply does
   not use it.
8. **One shared ledger.** The desktop app opens the same production ledger as the
   web server, resolved by the backend configuration at
   `%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio.sqlite`. The desktop shell
   does not define a competing build-tree ledger path.
9. **That file is renamed** away from a name containing "test" (it holds the real
   portfolio and is at risk from any future cleanup of test artifacts), with a
   safe migration for the `-wal`/`-shm` sidecars. The landed location is the
   per-user app-data path above.
10. **A missing ledger is a hard startup failure, not an empty new file — in
    *both* entry points.** Creating one requires an explicit opt-in
    (`TTTB_CREATE_LEDGER_IF_MISSING=1`, surfaced as `scripts/start.ps1
    -InitLedger`). A fresh clone needs one `-InitLedger` run before the web app
    starts; the retired `-ProductionDb` flag is not a second path or an alternate
    initialization flow.
11. **The resolved ledger path is observable** through `/api/health` and rendered
    in the app footer. This deliberately touches the frontend; it is a separate
    feature, not a transport change. **No shell/`DESKTOP` badge is added** —
    a native window is self-evidently the desktop app, and a third badge would
    crowd the one piece of information that is genuinely not otherwise visible.
    `/api/health` therefore gains `mode`, `ledger` and `backup`, and loses the
    retired `demo` field; which shell is running is recorded in the log banner,
    where it is actually needed.
12. **The repository becomes a Cargo workspace** with `backend/` and the new
    `desktop/` crate as members, sharing one target directory.
13. **Nothing in the desktop process is located relative to the working
    directory.** The ledger and runtime log use the app-data convention; only
    the desktop static-assets directory is anchored to the build tree, so a
    shortcut's arbitrary working directory cannot change runtime identity.
14. **No console window** (`windows_subsystem = "windows"`), file logging at one
    fixed known location, failure to open the log never prevents startup, and a
    startup failure produces a **native dialog** rather than a window that never
    appears.
15. **Cross-process refresh coordination is committed work, and it includes the
    data revision.** It is the last phase because it is independent of the
    window, not because it is optional. The closing documentation and
    verification assume it landed. Sharing a ledger means sharing *two* pieces of
    state, not one: the refresh claim **and** the data revision that clients use
    to invalidate their caches. The 2026-09-12 *Data Is Served And Requested By
    Revision* decision already names a persisted counter as the requirement for a
    shared ledger; this plan delivers it. Refresh-derived writes are fenced
    against lease loss inside the database, not only by a cancellation flag.
16. **Decision-log entries are written as each phase lands, not batched at the
    end**, and are rendered in the log's own `Decision`/`Context`/`Consequences`
    template. See *Decision-log entries*.
17. **The renamed ledger is the app-data production ledger.**
    `%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio.sqlite` is deliberately used
    instead of a repository-local or working-directory-relative name.
18. **The desktop app launches through `scripts/start.ps1 -Desktop`, not a second
    script.** One launch script owns ledger resolution, the legacy-name guard,
    environment save/restore and build orchestration for both shells.
    **`scripts/Common.ps1` is not created:** the only reason to extract it was to
    share those pieces between two launch scripts, and with one script the DRY
    pressure disappears. Extracting helpers for hypothetical future consumers
    (`probe-connectivity.ps1`, `project-stats.ps1`) would be speculative and is
    not this plan's work.
19. **Demo mode is reachable from both shells.** `TTTB_MODE=demo`, surfaced by
    `scripts/start.ps1 -Demo`, reaches both shells; `scripts/start.ps1 -Desktop -Demo`
    opens the demo in the native window. In demo mode no ledger path is resolved,
    no file is opened, and the refuse-to-create rule is structurally unreachable —
    the config resolves to an in-memory ledger before any filesystem work happens.
20. **Request body limits are explicit, and the import limit is raised.** The API
    router carries an explicit 1 MiB limit; the four import routes carry an
    explicit 32 MiB limit; both come from one shared constant definition.
    **Every route returns the standard `ApiError` envelope for an oversized *or*
    malformed body** — never axum's bare plain-text rejection. That guarantee is
    unconditional, which is why it needs an `ApiJson` extractor across all seven
    `Json<T>` handler signatures as well as an `ImportBody` one; an import-only
    version was considered and rejected because it would leave the guarantee true
    of four routes and false of the rest (see *Request body limits are explicit*).
21. **The backend configuration is the one definition of the default ledger
    path.** `scripts/start.ps1` leaves the default unset and the desktop crate
    consumes the same configuration; no second machine-readable path file is
    created or read.
22. **Static assets follow one policy matrix everywhere.** The production web
    server requires them (landed: `Mode::asset_policy()` returns `Required` for
    production and startup fails otherwise); the development and demo web
    servers keep their current optional policy; **every desktop mode, demo
    included, requires them.** The requirement is the landed nonempty
    `index.html` check, not mere file or directory existence. No text in this
    plan may describe the web server as universally optional; that description
    predates the production hardening and is wrong.
23. **The desktop response path emits its own Content-Security-Policy header**,
    and `tauri.conf.json` sets `csp: null`, so there is exactly one policy source
    and it is on the path that actually serves the HTML.
24. **The policy value is settled, directive by directive, on evidence from the
    built bundle** — not left to be discovered at implementation time. See
    *Content-Security-Policy is emitted by the bridge* for the value and the
    reason each directive is what it is.
25. **The desktop log reuses `LogSettings` and `RotatingFileWriter`.** It lives
    under the per-user app-data logs directory, is bounded and rotated like the
    server's mode-specific log, and carries the identity of the copy that wrote
    each line if the shared writer needs an instance tag. It must not introduce
    a competing `LogDestination` API, an unbounded file, or a CWD-relative path.
    Because two desktop instances in the same mode share one file name,
    **rotation is made safe across processes** (see *Startup failure, logging,
    and shutdown*); whole-line appends and instance tags alone do not make it so.
26. **Runtime path overrides must be absolute.** A relative `TTTB_DATABASE_URL`
    filename, `TTTB_LOG_FILE` or `TTTB_BACKUP_DIR` is rejected by the backend
    configuration in every shell, with the failure routed the way each resource
    already fails: the ledger as a startup error, the log as degraded terminal
    logging, the backup as a failed backup status. Absolute defaults do not make
    overrides absolute, and a shortcut's working directory must not be able to
    choose which file opens. See *Nothing in the desktop process is located
    relative to the working directory*.
27. **Build profiles are preserved.** The launcher already selects a release
    build and executable outside Vite development mode and a debug pair inside
    it. Relocating the target directory keeps that selection; the desktop mode
    adopts the same rule. Application mode (`TTTB_MODE`) and build profile are
    separate from whether a shell uses Vite.
28. **Drills run on verified snapshots, never on a live copy of the real ledger,
    and never require moving, deleting or force-killing the production ledger's
    process.** Scratch ledgers come from the existing verified launch snapshots
    (2026-09-14) or SQLite's own backup mechanism. See *Scratch ledgers for
    drills*.

### Refinements this plan makes to the brief (flagged, not silent)

- **`tower` is promoted in the *desktop* crate, not the backend.** The brief
  expected `backend/Cargo.toml`'s dev-only `tower` to become a real dependency.
  The bridge belongs in `desktop/` — the backend has no business knowing about
  webviews — so `tower = { version = "0.5", features = ["util"] }` becomes a
  normal dependency of the desktop crate and the backend's stays dev-only. The
  backend's public surface already exposes everything the bridge and its tests
  need (`api::router_with_static_assets`, `state::AppState::for_tests`,
  `AppState::with_mode`, `db::testing::memory_pool`,
  `providers::FakePriceProvider`).
- **Refuse-to-create is the default for *both* entry points**, not only the
  desktop one. The brief permitted the server to keep `create_if_missing(true)`.
  Confirmed by the user: silently creating an empty ledger when the path is wrong
  is the same bug in both processes, and the ledger rename makes a wrong path
  likely exactly once. Settled decision 10 states the two consequences a user
  meets first.

### Platform claims this plan takes as given, and how they get settled

Three statements about Tauri v2 / Wry / WebView2 behavior are load-bearing below.
They came from an external review citing upstream sources; **they could not be
verified first-hand while writing this plan** (no Tauri or Wry sources are
present in the local cargo registry and no network fetch was available). They are
therefore written as *assumptions with a defined settling mechanism*, not as
facts:

1. **Wry rewrites the browser-visible `http://<scheme>.localhost/...` URL back to
   `<scheme>://localhost/...` before invoking the handler on Windows.** Settled
   empirically: the in-window probe records the page's `location.origin`
   alongside the URI string the handler actually received, and both go into the
   probe report. The bridge is written to route on the **path** and to accept
   either form, so it is correct whichever way this resolves.
2. **Tauri's generated `app.security.csp` is applied while Tauri resolves its own
   assets and is not injected into responses from a registered custom handler.**
   Handled by making the action identical either way: the bridge emits the header
   itself and `tauri.conf.json` sets `csp: null`, so there is one source
   regardless. The probe's enforcement check then confirms the policy is live.
3. **Omitting the top-level `version` in `tauri.conf.json` makes Tauri inherit
   the Cargo package version.** Settled by the probe report, which records the
   version Tauri reports at runtime; if it does not match the workspace version,
   that is a finding to raise, not something to paper over.

An implementer who has network access should confirm all three against upstream
before relying on them, but the plan does not depend on that confirmation
happening.

## The three cases

The walking skeleton must prove these and nothing else:

1. **A rejected write returns a non-2xx status with an `ApiError` JSON body the
   page can read.** The API layer maps 400/403/422; demo mode returns
   `403 demo_read_only` via the `demo_read_only_layer`, which the probe uses
   because it is reachable without seeding a ledger.
2. **A `204 No Content` response arrives with an empty body and no bogus
   `content-length`.** `parseBody` in `client.ts` special-cases status 204;
   `DELETE /api/transactions/{id}` and `DELETE /api/instruments/{id}` are the real
   producers.
3. **A large `text/csv` POST body survives the round trip.** This is the real
   Avanza/Sharesight import path; `apiSendBytes` sends an `ArrayBuffer`.

### Two layers, two different properties — say which is which

These cases mix two properties that are easy to conflate, and conflating them is
how a gate stops being a gate:

- **Transport fidelity** — does WebView2 deliver the method, deliver the body
  intact, carry a non-2xx status with its JSON body, and carry an empty 204?
  This is the unknown, it lives in a platform stack we do not control, and it can
  **only** be proven in the window. Proven by *The in-window probe*.
- **Routing and limits** — does the real import route accept an 8 MiB body, does
  the API-wide limit apply where it should, does an oversized body produce the
  `payload_too_large` envelope? This is our own composition, it is deterministic,
  and it is proven by the Phase-2-step-0 **bare-router tests** and by the adapter
  tests, in non-demo state.

An earlier draft asked one probe case to prove both ("the real route accepted the
real size"). It could not: the probe runs in demo mode, and `demo_read_only_layer`
is a `route_layer` on the `/api` nest that rejects every non-GET method with `403`
**before** any body extractor or `DefaultBodyLimit` runs. That case would have
returned 403, satisfied a literal "not 413" assertion, and shown a green check for
a route the body never reached. The split above is the fix; the probe no longer
claims the routing property.

**Adapter tests do not prove transport fidelity.** A Rust test that constructs a
request and calls `serve_request` directly exercises axum and the adapter and
never touches WebView2. Those tests are still written, because they are the
regression net that keeps the adapter honest afterwards and because they are where
the routing property is pinned — but they are **not the gate**.

**Body-size finding — this plan fixes it rather than working around it.**
Investigation for this plan found that the import handlers take axum's `Bytes`
extractor and no router sets a `DefaultBodyLimit`, so axum's implicit **2 MB**
default applies *today, on the web build*, to every import POST. A large enough
export therefore fails in the browser too, with a bare `413` whose plain-text
body `client.ts` renders as "Request failed: 413". That is a latent defect in the
web app, not something the desktop work introduces — and the skeleton cannot
write a coherent large-body case against an implicit framework default. The
limits therefore become **explicit** and the import limit is raised, in the same
phase as the skeleton and immediately before it. See *Request body limits are
explicit*.

**Both body sizes are derived from the limit, never hard-coded.** A literal
"40 MiB oversized" case silently stops being oversized the day someone raises the
constant. Every producer of a test or probe body computes:

```
under_limit = IMPORT_BODY_LIMIT_BYTES / 4          //  8 MiB at the current 32 MiB
over_limit  = IMPORT_BODY_LIMIT_BYTES + 1 MiB      // 33 MiB at the current 32 MiB
```

`under_limit` is comfortably past any plausible WebView2 chunking boundary and
comfortably inside the limit; `over_limit` is over it by construction. The real
CSV exports in `docs/` are private and git-ignored (2026-06-12 *Private
Sharesight Exports*), so both bodies are **generated** — a header row plus
repeated synthetic rows to the target size. Do not check a multi-megabyte fixture
into the repository.

The bridge itself must impose **no** limit of its own — it hands the body through
untouched, so the router's limit is the only one.

## The in-window probe

The probe is the mechanism that makes the skeleton phase a real gate. It runs
inside the actual Tauri window, over the actual custom scheme, driven by the
page's own `fetch`, against the actual application router.

**Shape.** `desktop/src/webview_probe.rs`, reached by
`ticker-tape-tally-board-desktop --probe-webview` (and, from Phase 5,
`scripts/start.ps1 -Desktop -ProbeWebView`). In probe mode the desktop crate
**wraps** the application router with probe-only routes rather than modifying it,
so nothing probe-related can leak into the shipped API surface:

- `GET /__probe` — the harness page. **It contains no inline script**; it loads
  `/__probe/probe.js`. That is deliberate: if the CSP is live, an inline-script
  page would be blocked, so a no-inline-script page is itself the first
  enforcement assertion (see below).
- `GET /__probe/probe.js` — the harness script.
- `POST /__probe/echo` — returns `{ received_bytes, sha256 }` for whatever body it
  was given. The pure transport proof: it does not depend on the CSV parser
  agreeing with us about what a valid export looks like. **Its extractor and
  limit are explicit:** the handler takes the body through the same `ImportBody`
  extractor and carries `DefaultBodyLimit::max(IMPORT_BODY_LIMIT_BYTES)` on the
  handler, exactly like the import routes. Written with a bare `Bytes`
  extractor, axum's implicit 2 MiB default would reject the 8 MiB body *before*
  the handler ran — a deterministic harness defect that would masquerade as a
  WebView2 limitation. A bare-router test
  (`probe_echo_accepts_under_limit_body`) sends the generated `under_limit`
  body to this exact route and asserts `received_bytes` equals the sent length,
  so the harness is proven before the window is.
- `POST /__probe/echo-limited` — the same echo handler, but carrying
  `DefaultBodyLimit::max(IMPORT_BODY_LIMIT_BYTES)` and the **same `ImportBody`
  extractor the real import routes use**, so the oversized-body path returns the
  real `payload_too_large` envelope through the window.
- `DELETE /__probe/no-content` — returns a bare `204`.
- `POST /__probe/report` — receives the harness's JSON result.

Probe mode builds the application in **demo mode**, so the probe seeds nothing,
opens no ledger, makes no network call, and can produce `403 demo_read_only` on
demand.

**Why the 204 and oversized cases use probe routes rather than real ones.** Every
`/api` route sits behind `demo_read_only_layer`, a `route_layer` that rejects any
non-GET/HEAD/OPTIONS request with `403` before the handler, the body extractor or
any `DefaultBodyLimit` runs. In demo mode a real `DELETE` returns 403, never 204,
and a real oversized POST returns 403, never 413. Probe routes sidestep that
without weakening the proof, because the property being tested is transport
fidelity, and a bare 204 or a real `ImportBody` rejection carries exactly the same
bytes over the same channel as the real routes would.

**The alternative — a non-demo probe against a seeded temporary ledger — is
rejected**, and not only because case 1 would lose its `demo_read_only` producer
(other non-2xx producers exist: a malformed `POST /api/transactions` yields a 422
envelope). The deciding reason is that it would make the gate's signal ambiguous:
a red case could mean "WebView2 broke" or "the seeding broke", and a gate whose
failures need interpretation is a weak gate. It would also give the probe
filesystem state to create and clean up — reintroducing, in a new place, exactly
the stray-database hazard this plan removes elsewhere.

**What ties the probe routes to the real ones** is an adapter test, not hope:
`probe_limited_route_matches_real_import_route` asserts that
`POST /__probe/echo-limited` and a real import route produce an identical status
and an identical body for an over-limit request in non-demo state. If someone
later changes the import limit or the extractor and forgets the probe route, that
test fails rather than the probe quietly testing something else.

**What the harness runs, in the window:**

| # | Case | Assertion | Property |
|---|---|---|---|
| 1 | `POST /api/transactions` | status `403`, body parses as JSON, `error.code === "demo_read_only"` | POST delivered; non-2xx + JSON body readable |
| 2 | `DELETE /__probe/no-content` | status `204`, `await res.text() === ""`, no `content-length` claiming otherwise | DELETE delivered; empty 204 carried |
| 3 | `POST /__probe/echo`, `under_limit` bytes | `received_bytes` equals the sent length **and** `sha256` matches a digest computed in the page — bytes arrived intact, not merely in quantity | large body carried intact |
| 4 | `POST /__probe/echo-limited`, `over_limit` bytes | status `413`, body parses as JSON, `error.code === "payload_too_large"` | oversized body reaches the limit layer; error envelope carried back |
| 5 | `GET /api/health` | status `200`, body parses — the trivial baseline, so a total failure is distinguishable from a case-specific one | GET delivered end to end |
| 6a | `fetch("https://example.invalid/")` | **rejects**, **and** a `securitypolicyviolation` event arrives whose `effectiveDirective` is `connect-src` and whose `blockedURI` names `https://example.invalid` — the rejection alone proves nothing, because `.invalid` is reserved to fail name resolution and the fetch rejects with or without a policy | `connect-src 'self'` enforced |
| 6b | inline-script sentinel | the harness (an allowed external script) inserts `<script>window.__probeInlineRan = true</script>` into the DOM; the flag must **stay unset** **and** a `securitypolicyviolation` event with `effectiveDirective` `script-src` and `blockedURI` `inline` must arrive. A page that merely loads an external script runs identically with or without the policy, so the no-inline-script page is not itself an assertion | `script-src 'self'` enforced |
| 7 | URI form | the page reports `location.origin`; the handler reports the URI string it received; both are recorded verbatim | settles the Wry rewrite question |

Cases 6a and 6b distinguish *enforcement* from *network failure or absence*: each
requires the specific violation event, with the specific directive and blocked
URI, that only an enforced policy produces. The `securitypolicyviolation` event
and its fields are defined by CSP Level 3. **Negative check, once, in Phase 2:**
run the probe with the bridge's CSP header temporarily removed (a local,
uncommitted edit); 6a and 6b must go red while every transport case stays green.
Restore the header and rerun; all green. A run with DNS failure or no network
must never pass 6a on its own.

Cases 3 and 4 also record wall-clock duration, so a body size that "works" but
takes 30 seconds is visible rather than merely passing.

**Whether the real import route accepts an 8 MiB body is proven in Phase 2 step 0,
on the bare router, in non-demo state** — not here. That is the routing property,
and the probe deliberately no longer claims it.

**How the outcome is observed and recorded — this is the gate's evidence:**

- The harness renders a pass/fail table **in the window**, so a human sees the
  result immediately without reading a file.
- It `POST`s the full result to `/__probe/report`, which writes
  `.local/probe/webview-probe-<timestamp>.json` and logs every case at `info`
  through `engine_logging`.
- The report additionally records the exact **`tauri`, `wry`, `webview2-com` and
  WebView2 runtime versions**, the resolved `IMPORT_BODY_LIMIT_BYTES`, the
  derived body sizes, the version Tauri reports for the app, `location.origin`,
  and the handler-observed URI. The decisive behavior lives in that platform
  stack, so the evidence is worthless without the versions it was produced on.
- The probe process **exits non-zero if any case failed**, so it is scriptable
  and cannot be "passed" by not looking.
- **A hung case is a failure, not a wait.** This matters as much as the demo-mode
  defect above, and for the same reason: it is another way the gate can quietly
  stop being a gate. Two independent timeouts, because either side can be the one
  that hangs:
  - *In the harness*, every case runs under a per-case timeout (proposed 60 s,
    generous against the plan's own worry about a body that "works but takes 30
    seconds"). A timed-out case is recorded as **failed** with its elapsed time
    and the reason `timeout`, and the harness continues to the next case rather
    than stopping, so one hang does not hide the results of the others.
  - *In the Rust process*, a watchdog fires if no report has arrived within a
    bound **derived from the case schedule**: the sum of the per-case timeouts
    plus a fixed slack for page load and report delivery, computed from the same
    constants the harness uses — not an independent literal that drifts the
    first time a case is added or a timeout changed. It writes a report marked
    `incomplete` naming which cases never reported, logs it through
    `engine_logging`, and exits non-zero.

  Without these, a hang produces an idle window, no report, no exit code, and a
  gate that depends on a human noticing nothing is happening — and a hung case
  becomes indistinguishable from a merely slow one.
- The generated JSON is pasted into the implementation notes and quoted in the
  Context field of the transport decision-log entry. "The skeleton passed" means
  *that file exists, it is not marked `incomplete`, and every case in it is
  green* — nothing else.

**The probe is diagnostic tooling, not a shipped feature.** It stays in the
repository (it is the thing to re-run when a Tauri or WebView2 update misbehaves,
which is the same role `scripts/probe-connectivity.ps1` already plays), but it is
reachable only through an explicit flag and its routes exist only in that mode.

### Documented fallback: a single generic `invoke` bridge

If the **probe** shows WebView2's custom-protocol handling cannot carry one of
the three cases, fall back to a single generic
`invoke("http_request", { method, path, headers, body })` command feeding the
**same** router — one command, not 23.

**What would trigger the switch:** a *reproduced transport limitation* — a
non-2xx response body swallowed or replaced by a browser error page; a 204
arriving with a synthesized body or failing to resolve; a request body truncated,
re-encoded, or not delivered to the handler; the handler unable to see the
request method; or a large body that only completes on a timescale that makes
import unusable — **after harness, bridge and configuration defects have been
excluded.**

A red or `incomplete` report is a **gate against proceeding**, but it is not by
itself platform evidence. The probe is our own code too, and it has deterministic
ways to fail that say nothing about WebView2: an echo route with the wrong body
limit, a missing or malformed CSP header, a report-writing failure, a
misconfigured watchdog. The required response to a red report is therefore:

1. **Classify.** Read the failed case's recorded status, body and elapsed time.
   A `413` from `/__probe/echo` is a limit defect; a case 6a/6b failure with
   every transport case green is a header defect; an `incomplete` report with
   the window still responsive is a watchdog or report-route defect.
2. **Repair the defect in the harness or bridge and rerun the same gate.** No
   transport change is involved in clearing a defect of our own making.
3. **Only when a case fails with those defects excluded** — the harness route is
   proven on the bare router, the header is present, the watchdog is derived
   from the schedule — is the failure a transport limitation. Then, and only
   then, take the fallback question back to the user.

An adapter-test failure is likewise a bug in our code and is simply fixed.

**What it would cost:** settled decision 2 breaks — `client.ts` gains a transport
branch and every `fetch` call becomes an `invoke`; `ArrayBuffer` bodies must be
base64-encoded across the invoke boundary, roughly doubling peak memory for an
import and adding an encode/decode pass; response streaming is lost (already
irrelevant here, all responses are buffered); the SPA's own asset loading still
needs a page origin, so the fallback does **not** remove the custom scheme, it
only stops `/api/*` from going through it. Because the frontend change is
explicitly forbidden by the brief, taking this fallback requires going back to
the user, not deciding it during implementation.

---

## Architecture

### Workspace layout

```
Cargo.toml            # [workspace] members = ["backend", "desktop"]
backend/              # unchanged crate name: ticker-tape-tally-board-backend
desktop/              # new crate: ticker-tape-tally-board-desktop
target/               # one shared target directory (moves out of backend/)
frontend/
```

`[workspace.package] version` carries **one** Rust version number; both member
crates use `version.workspace = true`, and `tauri.conf.json` carries no version
key so Tauri inherits the same number. That is why no third version value is
introduced and no third one *can* be: there is exactly one, and the three places
that could have disagreed all read it. `Agents.md`'s version clause is updated in
the same phase to name `[workspace.package].version` instead of
`backend/Cargo.toml`, since the old wording becomes untrue the moment the backend
inherits.

Accepted, enumerated costs:

- **One full rebuild** when this lands — the target directory moves from
  `backend/target` to `target/`. `.gitignore`'s existing unanchored `target` and
  `debug` entries already cover the new location; verify, do not assume.
- `scripts/start.ps1` computes `$BackendExe` as
  `Join-Path $BackendDir "target/$BuildProfile/ticker-tape-tally-board-backend.exe"`,
  where `$BuildProfile` is `release` outside Vite development mode and `debug`
  inside it, and the build step passes `--release` to match. It **throws** if
  the executable is missing. Only the *root* changes — it must point at the
  workspace target — and the profile selection is preserved exactly. Changing
  only one half (the path but not the build flag, or vice versa) either launches
  a stale binary or fails; changing both to `debug` would silently turn the
  documented production launch into a debug one, undoing the 2026-09-06
  *Production Run Model* decision.
- `backend/Cargo.lock` is tracked. A Cargo workspace uses a **root** lockfile,
  so the first workspace build would otherwise resolve a fresh dependency graph
  and leave the tracked member lockfile unused — an uncontrolled dependency
  update hiding inside a mechanics-only change. The lockfile is moved to the
  root *before* the first workspace build (Phase 1), and the phase builds with
  `--locked`.
- `README.md` documents running cargo from `backend/`, including
  `cargo run --example sharesight_import_spike`, which needs
  `-p ticker-tape-tally-board-backend` from a workspace root.
- `Agents.md` says build and clippy from `backend/`.

**The inner loop must not get slower.** The desktop crate links WebView2 and is
substantially slower to build than the backend, so the default commands stay
per-package and only widen when `desktop/` or the workspace manifest is touched.
Exact `Agents.md` wording is specified in Phase 1.

### One composition root, two entry points

`app.rs` today interleaves state construction, launch-refresh spawning, router
selection, TCP binding and the ctrl-c shutdown signal in one function. The
desktop entry point needs everything except the TCP bind and ctrl-c. Duplicating
that would put two router constructions and two state constructions in the tree,
which `Agents.md`'s DRY rule forbids.

Split by behavior (module names describe what they do, never a phase):

```
backend/src/app/mod.rs           # thin wrapper: `pub mod composition; pub mod server;` + re-exports
backend/src/app/composition.rs   # the composition root, shared by both entry points
backend/src/app/server.rs        # the TCP serve loop; used only by backend's main.rs
```

```rust
// backend/src/app/composition.rs
pub struct Application {
    pub state: AppState,
    pub router: axum::Router,
    launch_refresh: Option<tokio::task::JoinHandle<()>>,
}

impl Application {
    /// Open the ledger, build state, spawn the launch refresh, and build the
    /// router. `config.asset_policy` decides whether absent static assets are a
    /// warning or a startup failure.
    pub async fn build(config: &AppConfig) -> Result<Self, StartupError>;

    /// Stop background work and close the SQLite pool. Idempotent.
    pub async fn shutdown(self);
}
```

`server::serve(config)` = `Application::build` → `TcpListener::bind` →
`axum::serve(...).with_graceful_shutdown(ctrl_c)` → `Application::shutdown`.
The desktop launch path = `Application::build` → register the protocol handler
over `application.router` → open the window → on exit, `Application::shutdown`.
Router construction, state construction, launch-refresh gating and pool shutdown
each exist exactly once.

`StartupError` is a typed enum (ledger missing, ledger unreadable, migration
failed, static assets missing, config invalid) carrying the resolved paths, so
both the log line and the native dialog can name them. `backend/src/main.rs`
stays a thin wrapper; `desktop/src/main.rs` and `desktop/src/lib.rs` likewise.

**Missing static assets have one explicit policy, not two implicit ones.** The
current behavior — warn and build an API-only router — is right for the server
(it is how `cargo run` behaves before the first `npm run build`) and *wrong* for
the desktop shell, where it would open a window showing the backend root instead
of the portfolio, and would do so **instead of** the native failure dialog the
plan promises. Without an explicit rule those two requirements silently
contradict each other.

```rust
pub enum AssetPolicy { Optional, Required }
```

**The policy is already landed for the web server and is derived from mode:**
`Mode::asset_policy()` returns `Required` for production and `Optional` for
development and demo, and `app.rs` fails startup when a required asset set is
unavailable. That is the September production decision and it must not be
undone. What this plan adds is a **shell-level override**: the desktop entry
point requires assets in *every* mode, demo included, while the web server keeps
its mode-derived behavior. One matrix, used everywhere in this plan:

| Shell | Production | Development | Demo |
|---|---|---|---|
| Web server | Required (landed) | Optional (landed) | Optional (landed) |
| Desktop | Required | Required | Required |

`Required` reuses the landed check — a **nonempty** `<static_assets_dir>/index.html`,
not merely a directory or a zero-byte file, since a stale or empty `dist/` would
otherwise pass and serve nothing — and returns
`StartupError::StaticAssetsMissing { path }`, which produces the dialog and no
window. Every cell is tested with missing, empty and valid `index.html`. An
earlier draft described the server as universally `Optional`; that predates the
production hardening and is wrong.

### The in-process HTTP bridge

Lives in the desktop crate — it is a shell adapter, not backend concern:

```rust
// desktop/src/in_process_http.rs

/// The one definition of the custom scheme. Registration, the window's launch
/// URL, the browser-visible origin and every test derive from this; the string
/// "tttb" appears nowhere else.
pub const WEBVIEW_SCHEME: &str = "tttb";

/// The URL the window is opened on: `tttb://localhost/`.
pub fn launch_url() -> tauri::WebviewUrl;

/// The origin the page will report on Windows: `http://tttb.localhost`.
pub fn browser_visible_origin() -> String;

/// Feed one webview request into the application router and return its response.
/// The only transport between the window and Rust; there is no socket.
pub async fn serve_request(
    router: axum::Router,
    request: http::Request<Vec<u8>>,
) -> http::Response<Vec<u8>>;
```

Implementation notes that are load-bearing, in the order they bite:

- `Router` is `Clone`; `oneshot` consumes it, so clone per request.
- **Do not assume which URI form reaches the handler.** The page's origin on
  Windows is `http://tttb.localhost` (WebView2 maps `scheme://domain` to
  `http://scheme.domain`), but the review reports that Wry rewrites that back to
  `tttb://localhost/...` before invoking the handler — so the handler most likely
  sees the custom-scheme form, not the http one. The plan does not depend on
  either: **the bridge routes on the path**, normalizing whatever it receives
  (absolute-form with either scheme, or an origin-form path) to the path axum
  matches on. The adapter test feeds **all three forms** and asserts identical
  routing, and the probe records the form actually observed, so this is settled by
  evidence rather than by assertion. An earlier draft of this plan asserted the
  `http://tttb.localhost` form as fact; that was wrong to state as fact and is
  corrected here.
- **Preserve `path_and_query`, not only `path`, for every accepted URI form.**
  Report ranges, watchlist options and import commit parameters all travel in
  the query string, and a bridge that normalized to the path alone would drop
  them silently while `/api/health` kept passing. The URI-form adapter test
  therefore uses a query-bearing request whose *response* reflects the query
  (a report endpoint returning its resolved period, for example) and asserts
  the same resolved value for all three forms.
- Copy status and **all** response headers verbatim. Do not synthesize
  `content-length`; let the responder handle it, and assert in the 204 test that
  no body and no misleading length is produced.
- **Add the Content-Security-Policy header here**, on the way out — see
  *Content-Security-Policy is emitted by the bridge* below.
- Collect the body with `axum::body::to_bytes(body, usize::MAX)`; a collection
  failure becomes a `500` with an `ApiError`-shaped JSON body and an
  `engine_error!` naming the method and path (never a silent empty response).
- The protocol handler is invoked on the webview thread. The router future must
  run on the tokio runtime: capture a `tokio::runtime::Handle` when the
  `Application` is built and `handle.spawn(...)` inside the handler, calling
  `responder.respond(...)` from that task.
- **Runtime construction trap.** `tauri::Builder::run` must own the main thread,
  and `Handle::block_on` panics if called from inside a runtime context.
  Therefore the desktop entry point builds the tokio runtime **manually**
  (`tokio::runtime::Builder::new_multi_thread().enable_all().build()`), uses
  `handle.block_on(...)` for startup and shutdown from the main thread, and never
  wraps `main` in `#[tokio::main]`.

### Tauri build wiring and configuration

**`desktop/build.rs` is required and is easy to forget:**

```rust
fn main() {
    tauri_build::build();
}
```

Without it `tauri-build` never runs, the configuration is never processed, and
the failure mode is confusing rather than obvious. The same `build.rs` also emits
the shared repository paths (see *One definition of the default ledger path*), so
it has two jobs and both fail the build loudly if their inputs are wrong.

`desktop/tauri.conf.json`:

- `identifier` — **required by Tauri**; use a reverse-DNS string
  (`se.pensjo.tickertapetallyboard`). It is not branding and needs no decision
  beyond being stable and unique on the machine.
- **No top-level `version` key.** Omitting it makes Tauri inherit the Cargo
  package version, which is the workspace version — so the "exactly one Rust
  version number" property survives. Writing a version here would create the
  third source this plan exists to avoid.
- `app.windows: []` — the window is created programmatically with
  `WebviewUrl::CustomProtocol`, so no config-declared window can open on the
  wrong URL.
- `app.withGlobalTauri: false` — the frontend uses no Tauri JS API.
- `app.security.csp: null` — see the next subsection. The policy is emitted by
  the bridge; having it in two places would mean two policies that browsers
  intersect, which fails in ways that are very hard to read.
- **`build.frontendDist` is omitted (null).** It is nullable, and a directory
  value would embed those assets into the binary — exactly what settled decision
  6 rejects. The earlier plan's "checked-in empty placeholder directory" was an
  unnecessary workaround for a problem that does not exist and is dropped. If a
  null value is rejected by the toolchain in practice, use a **URL** value, which
  also embeds nothing; never a real directory.
- Bundling is off. If `tauri-build` insists on an icon for the Windows resource,
  add one minimal placeholder `.ico` and record it as a build artifact, not
  branding — icons remain out of scope.

### Content-Security-Policy is emitted by the bridge

Tauri's generated CSP is applied while Tauri resolves **its own** assets.
Responses from a registered custom URI-scheme handler are forwarded as the
handler produced them. Since axum and `ServeDir` supply the HTML here, a policy
configured in `tauri.conf.json` could be dutifully written, human-checked and
never actually enforced — the worst kind of security control.

So: `desktop/src/content_security_policy.rs` holds the one policy string, and the
bridge adds `Content-Security-Policy` to every response on the way out. Starting
value:

```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
font-src 'self'; img-src 'self' data:; connect-src 'self';
object-src 'none'; base-uri 'self'; frame-ancestors 'none'
```

**This value is settled on evidence from the built bundle, not proposed for
discovery.** Every directive below has a reason that was checked against
`frontend/` and against `dist/`:

- **`script-src 'self'`, no `'unsafe-inline'`.** `frontend/index.html` contains
  exactly one script tag, `<script type="module" src="/src/main.tsx">`, which the
  build rewrites to an external `/assets/*.js`. There is no inline script to
  accommodate — and withholding `'unsafe-inline'` here is precisely what the
  probe's no-inline-script harness page checks.
- **`style-src 'self' 'unsafe-inline'`.** Required by React's `style` attributes
  and by Lightweight Charts' generated elements. Under CSP Level 3, style
  *attributes* are governed by `style-src-attr`, which falls back to `style-src`
  when unset, so one directive covers both.
- **`font-src 'self'`, with no `data:`.** Fonts are self-hosted
  (`@fontsource/inter/latin-{400,500,600}`,
  `@fontsource/jetbrains-mono/latin-{500,600}` — the five stylesheets
  `frontend/src/main.tsx` imports; no Google Fonts or other remote origin). Their
  five woff2 files are 21–25 KB, far above Vite's default `assetsInlineLimit` of
  4096 bytes, and `frontend/vite.config.ts` does not override it — so they cannot
  be inlined as `data:` URIs. (The `@fontsource` packages do contain woff2 files
  under 4 KB, but they are all `cyrillic-ext` subsets that none of the imported
  stylesheets reference.) An earlier draft carried `data:` here defensively; it is
  not needed and is dropped.
- **`img-src 'self' data:` — the `data:` is required, for one nameable reason.**
  The favicon at `frontend/index.html:8` is an inline
  `href="data:image/svg+xml,…"` SVG, and browsers enforce `img-src` on favicons.
  This is the **only** genuine `data:` URI in the built output; every other
  `data:` occurrence in `dist/` is minified object-literal syntax (`{data: r}`,
  `{data: null}`), not a URI. The application ships no image assets at all — no
  `.png`, `.jpg`, `.svg`, `.gif`, `.webp` or `.ico` under `frontend/src/` or
  `frontend/public/` — so nothing else can arrive inlined.
- **`connect-src 'self'`.** All API traffic is same-origin; there is no `Worker`,
  `Blob`, `blob:` or `createObjectURL` usage anywhere in `frontend/`, so the
  `worker-src` and `blob:` relaxations an earlier draft was braced for are not
  needed.
- **`object-src 'none'`, `base-uri 'self'`, `frame-ancestors 'none'`.** Free
  hardening; the application uses none of what they forbid.

**Considered and rejected: moving the favicon to `frontend/public/favicon.svg` so
`img-src` could tighten to `'self'`.** It would work, but it is a frontend change
outside the two this plan has committed to, for a directive that is already narrow
(`data:` images cannot execute). Recorded so a later reader knows the `data:` is a
deliberate concession to one specific asset, not slack.

**Maintenance note — what would change this value.** Two things, both easy to do
accidentally: shipping an image asset **under 4 KB** (Vite would inline it, which
the existing `data:` already covers, but a larger image would need no change at
all — the risk is the reverse, that someone removes `data:` after deleting the
favicon), and anything that puts an **inline script** into the built
`index.html`. The second is the dangerous one: `script-src 'self'` would blank the
window entirely. Neither the adapter test nor the probe would catch it — the probe
serves its own page — so it surfaces in the ordinary-app human check as a window
that renders nothing.

Two verifications, because a header that is present is not the same as a policy
that is enforced — the value being settled does not make the enforcement question
go away:

- **Adapter test:** the header is present, with exactly the expected value, on
  the SPA index response, on a static asset response, and on an API response.
- **Enforcement check (in-window probe), cases 6a and 6b:** the harness listens
  for `securitypolicyviolation` and requires the specific violation — directive
  and blocked URI — for both an attempted cross-origin connection and an
  inserted inline-script sentinel that must not execute. A rejected `fetch` to a
  `.invalid` host or a page that happens to have no inline script proves
  nothing on its own: both behave identically with no policy at all. The
  negative run (header removed locally, once) confirms the checks detect
  absence.

If the probe shows the policy is *not* enforced, that is a finding to raise
before proceeding, not something to route around. If the ordinary-app human check
shows a font, chart, treemap or favicon failing, widen the **specific** directive
that failed and record why — never fall back to a permissive policy.

### Request body limits are explicit

`backend/src/api/body_limits.rs` — named for what it governs — holds the only
definitions, so the router and the tests cannot drift apart:

```rust
/// Every API route. JSON writes are one transaction or one conviction batch;
/// this is already ~1000x the largest realistic body.
pub const API_BODY_LIMIT_BYTES: usize = 1024 * 1024;          // 1 MiB

/// The four CSV import routes, which take a whole broker export in one body.
pub const IMPORT_BODY_LIMIT_BYTES: usize = 32 * 1024 * 1024;  // 32 MiB

/// Test and probe body sizes are *derived*, never written as literals, so
/// raising the limit cannot silently turn the oversized case into a valid one.
pub const fn under_import_limit_bytes() -> usize { IMPORT_BODY_LIMIT_BYTES / 4 }
pub const fn over_import_limit_bytes() -> usize { IMPORT_BODY_LIMIT_BYTES + 1024 * 1024 }
```

**Why an explicit limit at all, when 1 MiB sits near the framework default:** an
implicit default is invisible to a reader, cannot be asserted on without
asserting on framework behavior, and — as this plan's own investigation showed —
silently governs a route it was never chosen for. An explicit constant that
happens to land near a default is still the thing the plan, the tests and a
future reader can point at.

**Why 32 MiB for import:** the largest realistic Avanza or Sharesight
full-history export is a few hundred KB (the recorded Sharesight export is 189
rows), so 32 MiB is roughly two orders of magnitude of headroom and cannot be hit
by real data. It is also bounded: the handler holds the body as `Bytes`, then
parsed rows, then normalized row snapshots for the command log, so peak memory is
a small multiple of the body — a few hundred MB worst case on a home PC, which is
acceptable, where an unbounded body is not.

**Why the API-wide limit is separate and lower:** the import routes are the only
ones that legitimately take a large body. Applying 32 MiB everywhere would let
any other route be used to allocate 32 MiB on an interface the project has
committed to leaving unauthenticated on the LAN (2026-06-12 *Phase 0 Planning
Decisions*).

**Wiring, with the trap stated:** `api_router()` gains
`.layer(DefaultBodyLimit::max(API_BODY_LIMIT_BYTES))`, and each of the four
import routes applies `.layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT_BYTES))`
**to the handler** (`post(import::avanza_commit).layer(...)`), not to the router.
The innermost layer is the one the extractor sees, so a router-level layer would
win and the raise would silently not happen.

**The `413` becomes a real API error — on every route, not just import.** Axum's
bare rejection returns a plain-text body, which `client.ts` surfaces as "Request
failed: 413". Fixing that only for the import routes would leave settled decision
20 and the corresponding decision-log entry making a promise the API does not
keep: every other route uses axum's `Json` extractor directly, whose rejection is
also plain text. So `backend/src/api/extract.rs` gains **two** extractors sharing
one rejection-mapping helper:

```rust
pub struct ApiJson<T>(pub T);     // replaces axum::Json<T> in handler signatures
pub struct ImportBody(pub Bytes); // replaces Bytes in the four import handlers
```

The shared mapping turns a length-limit rejection into `ApiError`
`payload_too_large` naming the limit in MB, and — a free improvement, since the
match has to be written anyway — turns the other `JsonRejection` variants into
`invalid_json`, `unsupported_media_type` and `missing_body`.

**What that improves, stated precisely.** Today a malformed JSON body produces no
`ApiError` at all: axum's built-in `JsonRejection` returns its own plain-text
response and never reaches `api/error.rs`. The frontend's `parseBody` then finds
no `error` object and falls back to `new ApiError("unknown", "Request failed: 400")`.
So the user-visible code really is `unknown` — but it is synthesized by the
*frontend*, and no `"unknown"` code exists anywhere in the backend. The change is
therefore "from no envelope at all to a specific code in the standard envelope",
not "from one backend code to another".

Replacing `Json<T>` touches **seven** handler signatures — `instruments.rs` (3),
`transactions.rs` (2), `prices.rs` (1), `provider_symbols.rs` (1); the other
`Json<...>` occurrences in the tree are response types, not extractors. It is
mechanical and compiler-guided.

**The alternative was to narrow the promise to import routes**; it was rejected
because it leaves a documented API contract untrue for most of the API, and
because the malformed-JSON message quality was worth having anyway. This is a bug
fix in the web app as much as in the window, so it ships with regression tests
per `Agents.md`.

### Nothing in the desktop process is located relative to the working directory

One rule. A GUI executable launched from a shortcut has an arbitrary, possibly
unwritable CWD and, under `windows_subsystem = "windows"`, no stderr.

The desktop process uses two deliberate path policies:

- the ledger and runtime log come from the backend's per-user app-data
  configuration, never from the working directory;
- the static-assets directory is the one desktop-only path anchored to the
  build tree, because this undistributed shell serves its checked-out
  `frontend/dist`.

```rust
// desktop/src/app_paths.rs
/// Absolute path baked at build time. The desktop app is deliberately tied to
/// its build tree; no installer is in scope (see Deliberately out of scope).
const DESKTOP_CRATE_DIR: &str = env!("CARGO_MANIFEST_DIR"); // <repo>/desktop

pub struct AppPaths {
    pub static_assets_dir: PathBuf, // <abs>/frontend/dist
}
```

`TTTB_STATIC_DIR` still overrides (needed for asset checks and drills), but a
**relative** static-assets override is resolved against the build-tree anchor,
never against the CWD. The desktop log uses the existing `LogSettings` and
`RotatingFileWriter` with a mode/shell-specific file under
`%LOCALAPPDATA%\TickerTapeTallyBoard\logs\`, and is bounded and rotated like
the server log. `AppPaths` is pure and unit-tested against a supplied anchor
rather than reading the real environment in tests.

**Runtime path overrides: the backend configuration does *not* currently
prevent CWD dependence, and this plan adds the rule.** Absolute defaults do not
make overrides absolute. Today `ledger::resolve` keeps the parsed filename and
the original URL as given; it neither anchors nor rejects a relative filename,
and `AppConfig::from_env` passes the override through unchanged. The log-file
and backup-directory overrides likewise accept any `PathBuf`. So a shortcut
launched with `TTTB_DATABASE_URL=sqlite://scratch.sqlite` would open a
*different existing file* depending on its working directory, and relative
`TTTB_LOG_FILE` / `TTTB_BACKUP_DIR` values would scatter logs and backups the
same way. An earlier draft said the backend configuration already prevented
this; it did not.

The policy, applied in the backend configuration so both shells get it and there
is one rule rather than a desktop-only branch:

| Override | Relative value | Failure route (existing rule reused) |
|---|---|---|
| `TTTB_DATABASE_URL` (file-backed, non-demo) | **rejected** | `ConfigError` → `StartupError` naming the value; dialog in the desktop shell, log line in the server |
| `TTTB_LOG_FILE` | **rejected** | `LogFile::Unresolved("… must be absolute")` → terminal-only logging, startup continues |
| `TTTB_BACKUP_DIR` | **rejected** | `BackupDirectory::Unresolved("… must be absolute")` → failed backup status, startup continues |
| `TTTB_STATIC_DIR` | server: CWD-relative as today; desktop: anchored to the build tree | unchanged |

Rejection, not rewriting: the ledger URL is passed to SQLite unchanged, so its
query options (`?mode=…` and friends) are preserved, and the default ledger's
single source of truth is untouched. Demo mode still ignores the ledger override
entirely. The launcher already produces absolute values (`ConvertTo-SqliteUrl`
calls `GetFullPath`), so this changes nothing for `scripts/start.ps1` users and
closes the hole only for hand-built shortcuts and environments.

The **server** entry point now uses the same app-data ledger and mode-specific
runtime-log conventions. Only its default `TTTB_STATIC_DIR` remains
CWD-relative; `scripts/start.ps1` passes that directory as an absolute path.
The desktop plan's working-directory rule therefore applies to the desktop
process without reintroducing a competing ledger or logging convention.

### Ledger identity is observable, and never silently created

Three separate obligations, one mechanism.

**Rename.** `.local/db/tttb-ledger-test.sqlite` was moved to the production
ledger at `%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio.sqlite`, with the
`.sqlite`, `.sqlite-wal` and `.sqlite-shm` files kept together. The launcher
guard refuses non-demo starts while legacy artifacts remain, rather than
starting on a new empty file.

**One resolution path.** The landed `backend/src/ledger/location.rs` provides
the shared location type and mode-aware resolution:

```rust
pub struct LedgerLocation { url: String, path: Option<PathBuf> } // None for sqlite::memory:
pub enum LedgerLocationError {
    MustBeFileBacked { url: String, mode: Mode },
    UnsupportedUrl { url: String },
    NotAFile { path: PathBuf },
}

pub fn resolve(url: &str, mode: Mode) -> Result<LedgerLocation, LedgerLocationError>;
```

`AppConfig` already stores a `LedgerLocation` and a mode, with absolute
app-data defaults outside demo. `db::open` already takes the location plus an
explicit `CreateMissing::{Yes, No}`, and missing-file creation is gated by
`TTTB_CREATE_LEDGER_IF_MISSING=1`. The remaining desktop-plan tidy is to move
`memory_pool` and `RepoError` out of `db/mod.rs` so it becomes the thin wrapper
`Agents.md` asks for. Any desktop `from_env_anchored` helper is for the static
assets directory only; the desktop log is resolved from the app-data convention
through `LogSettings`.

**Refuse to create.** `create_ledger_if_missing` defaults to `false` for both
entry points; `TTTB_CREATE_LEDGER_IF_MISSING=1` (and
`scripts/start.ps1 -InitLedger`) opts in. The default configuration exercises the
new refuse path, per the repo's rule that new behavior behind a flag must default
to the new path.

**One definition of the default ledger path, consumed by both launch paths.**
The backend configuration owns the absolute app-data default, and
`scripts/start.ps1` leaves `TTTB_DATABASE_URL` unset unless the operator gives
an explicit override. The desktop crate uses the same configuration rather than
reading a second path definition. No second machine-readable path file is
created or read.

The desktop build-tree helper has one remaining job: resolve the static-assets
directory for the undistributed shell. The desktop runtime log is not build-tree
anchored; it uses the mode/shell-specific app-data log convention through
`LogSettings`.

**Demo mode never resolves a ledger.** `AppConfig` resolves its ledger to
`LedgerLocation::memory()` whenever `demo_mode` is set, *before* any filesystem
work happens: `TTTB_DATABASE_URL` is ignored, `create_ledger_if_missing` is
irrelevant, `ledger_path()` is `None`, and `StartupError::LedgerMissing` is
structurally unreachable. This is written as a rule rather than left to fall out
of `Application::build`'s existing demo branch, because it is exactly the
interaction that would otherwise pop a "ledger not found" dialog for a mode that
has no ledger by design. It is pinned by a test: demo config with
`TTTB_DATABASE_URL` pointing at a nonexistent path starts successfully and
touches no file.

**Observable.** The landed `AppState` carries the mode, ledger path and backup
state. `/api/health` carries `mode`, `ledger` and `backup` (and no retired
`demo` field):

```json
{ "ledger": { "mode": "file" | "memory", "path": "C:/…/portfolio.sqlite" | null } }
```

**`shell` is deliberately not on the wire.** A native window is self-evidently
the desktop app, and no `DESKTOP` footer chip is added. If the desktop entry
point needs to identify itself in startup and shutdown banners, that remains an
internal composition/logging concern: the desktop work adds `AppShell` to
`AppState` for those banners, without reintroducing an unconsumed health field.

Demo mode reports `{"mode":"memory","path":null}` in **both** shells, so a
presentation screenshot never shows a local path — and the desktop demo window is
therefore a first-class presentation surface rather than a window that leaks the
developer's file layout.

**Is a filesystem path acceptable over `/api/health`? Decided: yes, expose the
full path.** Loopback-only binding limits the reachable audience to processes on
the host today. The one genuine incremental leak is the Windows user name
embedded in the path; that is recorded in the risk table and in the decision-log
entry. If the mobile work ever lifts the binding boundary, this field belongs
behind the authentication gate as well.

### Startup failure, logging, and shutdown

**Logging.** The landed `engine_logging::initialize(&LogSettings)` already
creates the configured app-data directory, never panics when the file cannot be
opened, and uses a bounded `RotatingFileWriter`. The desktop entry point reuses
that API with a mode/shell-specific file under
`%LOCALAPPDATA%\TickerTapeTallyBoard\logs\`; it does not introduce a competing
`LogDestination` API.

```rust
pub struct LogSettings {
    pub file_path: PathBuf,
    pub max_bytes: u64,
    pub kept_rotations: usize,
    pub terminal: bool,
}
pub struct LogInitOutcome {
    pub file_path: Option<PathBuf>,
    pub file_error: Option<String>,
}

pub fn initialize(settings: &LogSettings) -> LogInitOutcome;
```

Failure to open the file **never panics**: the outcome carries the error, the
desktop launch path can report it alongside a later startup failure, and the app
starts regardless with terminal logging where available. `Agents.md`'s rule
that backend code logs through `engine_logging` is unchanged and applies to the
desktop crate too.

**One known app-data location, with a distinct desktop file.** Two copies may run
at once. The desktop log is mode/shell-specific under
`%LOCALAPPDATA%\TickerTapeTallyBoard\logs\`, rather than inside the repository;
it is bounded and rotated by the shared writer. If the implementation adds a
per-line instance tag, it extends `LogSettings` and the existing writer rather
than creating a second logging abstraction.

**Rotation must be safe across processes, because two desktop instances in the
same mode share one file name.** Whole-line appends and instance tags make
*records* attributable; they do nothing for *rotation*. The landed
`RotatingFileWriter` caches its own byte count and holds an open handle. If
instance A rotates, B still holds the archived file and an obsolete count: B
keeps writing into `engine-desktop.log.1`, and when B's own count crosses the
threshold it renames paths A now owns. The plan's earlier shared-buffer test
could not exercise this, because it never touched a filesystem. Three changes to
the existing writer close it, and they stay inside `engine_logging`:

1. **Size is read from the file, not from a cached count.** Before each record
   the writer checks the *live* length of the file it holds (`file.metadata()`)
   — on an append-mode handle that length reflects every process's writes — and
   rotates when the record would cross the limit. The cached counter goes away.
2. **Rotation happens under a cross-process lock.** The writer takes an
   exclusive lock on a sibling `<log>.lock` file (opened with no sharing on
   Windows) for the duration of the rename ladder. After acquiring it, the
   writer re-checks whether the file at the log path is still the file it
   holds; if another process rotated in the meantime, it simply reopens the
   path and does not rotate again.
3. **A foreign rotation is detected and followed.** Before each record the
   writer compares the identity of the file at the path with the handle it
   holds (creation time plus length is sufficient on Windows; a missing path
   counts as rotated). On mismatch it reopens the path in append mode, so it
   never keeps writing into an archive.

Together these keep the existing commitments — one bounded timeline per
shell/mode, `kept_rotations` archives, no partial lines — true with two writers.
*Considered and rejected:* per-instance files (`engine-desktop.<pid>.log`) with
aggregate retention. It would avoid the lock, but it changes the shared-timeline
promise that settled decision 25 and the risk table rely on, and it adds a
second retention mechanism for something the existing ladder already bounds.

**If a per-line tag is retained, it goes on every line, not only in a banner.**
A banner identifies a session; it does nothing for the lines that follow. The
tagged writer must extend the existing settings/writer path:

- `RotatingFileWriter` **already** buffers to newline boundaries (its `pending`
  buffer and `write_complete_records`), so no new line-buffering writer is
  introduced. The tag is emitted by that existing path: `[<instance_tag>] `
  followed by the completed line in **one** `write_all`. Prefixing each raw
  `write` call would corrupt output, because `simplelog` emits a record through
  several writes.
- Writing whole lines in single appends is also what *bounds* the interleaving:
  on a Windows append-mode handle a single write does not split, so two copies can
  interleave whole lines but not fragments of a line.
- `instance_tag` is `<shell>:<pid>` — `desktop:12345`, `server:9876` — if the
  shared writer exposes that setting. The shell is included so a future shared
  file needs no change.
- The tagged writer is applied to the **file** sink through `LogSettings`, never
  through a competing `LogDestination` API. Both the desktop and server files
  remain bounded and mode/shell-specific under app-data.
- The **terminal** sink is not tagged: a console belongs to exactly one process by
  construction, so the prefix would be noise.

The startup and shutdown banners stay, and carry what a per-line tag cannot: the
executable path, resolved ledger path, static assets directory and log path. The
banner renders an absent ledger path as `in-memory (demo)`, never as an empty
field — a `None` path is a normal state, not a formatting accident.

**Accepted consequence:** if two copies ever share a configured file, their
whole-line records may interleave. The tag keeps those lines attributable; it is
not a reason to reintroduce an unbounded or repository-local log.

**Native failure dialog.** Failures happen before any Tauri `AppHandle` exists,
so `tauri_plugin_dialog` cannot be used. `desktop/src/startup_failure.rs` shows a
Win32 `MessageBoxW` (`MB_ICONERROR | MB_OK`) via `windows-sys`, then exits
non-zero. The message names: what failed, the resolved ledger path, the resolved
static assets directory, and the log file path. (`rfd` is the alternative if a
dependency on `windows-sys` is unwanted; it pulls more in for one call.)

**Shutdown.** `tokio::signal::ctrl_c()` never fires in a GUI app. The desktop
path handles Tauri's `RunEvent::Exit` and calls `handle.block_on(application.shutdown())`,
which aborts the launch-refresh task and closes the SQLite pool. Because
`shutdown()` is on the shared composition root, the server path gets the same
pool close it does not perform today — a small correctness improvement, verified
by the existing backend tests staying green.

**Nothing in the future composition root or shutdown path may assume a
file-backed pool.** The existing web entry point already branches before
filesystem work for demo mode, where the seeded in-memory pool skips launch
refresh and backup. The shared composition extracted for the desktop shell must
preserve that behavior: shutdown must close a file pool when present but do no
path-based cleanup or unconditional WAL work for demo.

### Cross-process refresh coordination

Today `backend/src/market_data/refresh.rs` guards refresh with an in-memory
`Arc<AtomicBool>` + `compare_exchange`, holds the active run in
`Arc<Mutex<Option<RefreshRunSummary>>>`, and `status()` reports `refreshing` from
that same in-memory flag. Two processes on one ledger therefore run two
concurrent launch refreshes against Yahoo/Frankfurter, write two overlapping
`market_data_refresh_runs` rows, and each UI reports a refresh state blind to the
other (`usePriceStatus` polls every 2 s while `refreshing` is true). This is a
genuine blocker for `docs/plans/Design.mobile-view-design.md`, where two
processes on one ledger is the *intended* setup.

Move the claim into the database:

- Additive migration `add_refresh_run_claim.sql` adds two nullable columns to
  `market_data_refresh_runs`: `claim_owner TEXT` and `heartbeat_at TEXT`. No new
  status values, so the existing status CHECK is untouched and the migration
  stays forward-only and additive (2026-06-14 *Backend Persistence Stack*).
- `claim_owner` is a process identity string (`<hostname>:<pid>:<process start
  time>`) so a recycled PID cannot be mistaken for the original owner.
- Claim acquisition runs in a **`BEGIN IMMEDIATE`** transaction (a deferred
  transaction would take a read lock first and can fail to upgrade): look for a
  `RUNNING` row with a fresh heartbeat; if one exists, do not start; otherwise
  insert the new `RUNNING` row with `claim_owner` and `heartbeat_at` set, and
  commit.
- **Staleness handling is mandatory.** A `RUNNING` row whose `heartbeat_at` is
  older than a `REFRESH_CLAIM_STALE_AFTER` constant (proposed 120 s, one shared
  definition, no duplicate literals) is reclaimable: the reclaiming process
  finishes the abandoned row as `FAILED` with message `abandoned` and
  `finished_at` = now **in the same transaction** as its own claim, so a process
  killed mid-refresh never blocks refreshes forever.
- The owner heartbeats every 30 s from a task tied to the existing flight guard's
  lifetime, so the heartbeat stops exactly when the guard drops.
- `is_refreshing()` and `active_run()` stop reading memory and read the live
  claim. Both become `async` and take the pool; the compiler enumerates the
  callers — `status()`, `running_response()` **and the `/api/data-version`
  handler**, which publishes `prices_refreshing` from `is_refreshing()` today.
  The in-memory `AtomicBool`/`Mutex` pair is **deleted**, not kept alongside —
  one source of truth.
- `status()`'s `refreshing` and `latest_run` therefore reflect *any* process's
  run, and a second window shows the first window's refresh.

#### The data revision is shared too

Showing the other process's spinner does not invalidate anything. The frontend's
query token is `<data_revision>@<valuation_date>` — it does **not** include
`prices_refreshing` — and `data_revision` today is a process-local
`<session>:<counter>` held in `DataRevision` (`backend/src/data_revision.rs`).
So if the desktop process commits a transaction or finishes a price refresh, the
web process's revision is unchanged, its heartbeat keeps succeeding, and its
mounted panels keep serving cached portfolio values indefinitely. The 2026-09-12
*Data Is Served And Requested By Revision* decision says exactly this:
"sharing a ledger between processes would need a persisted counter." This phase
delivers it.

- The same additive migration adds a one-row `data_revision` table
  (`id INTEGER PRIMARY KEY CHECK (id = 1), counter INTEGER NOT NULL`), seeded
  with `0`.
- `DataRevision::bump()` becomes an atomic `UPDATE … SET counter = counter + 1`
  and `current()` a read of that row; both become `async` over the pool. The
  published string keeps a stable shape (`<counter>`), and **every** stamped
  response — holdings, gains, rebalance, portfolio value history — and
  `/api/data-version` read it from the database. No caching in memory: the
  point is that process B reads process A's bump on B's next heartbeat, with no
  cross-process notification needed.
- **Bump sites are unchanged in kind, extended in coverage.** The middleware
  still bumps for a mutating request that does not return a client error; the
  launch refresh still bumps when it finishes. Two additions, both required for
  the cross-process promise:
  - a refresh that **fails after partial writes** bumps — the data changed even
    though the run did not succeed;
  - **reclaiming an abandoned run bumps**, in the reclaim transaction, because
    the abandoned owner may already have written prices before it stalled and
    nobody else will announce that.
- **Startup bumps once, after migrations.** The 2026-09-12 entry promises that a
  restart refetches; with a persisted counter a restart would otherwise be
  invisible, and a migration that rewrites data would go unannounced. Demo
  mode's counter lives in the seeded in-memory database and is written before
  `query_only` is set; no demo mutation succeeds, so it never bumps afterwards,
  which a test pins.
- The heartbeat and response-stamp contract (`client.ts`, `dataVersion.ts`, the
  cache-token shape) is preserved unchanged: the frontend sees a string that
  changes when the data does, exactly as today.

**Reclaiming is only half the problem: the reclaimed owner must also stop.** A
lease that can be taken away creates a second failure mode that the first draft
of this plan missed — a process that stalled past the timeout (a long GC pause, a
suspended laptop, a provider call that hung) is *not dead*. It wakes up, and
unless it is told otherwise it keeps fetching, keeps writing, and finalizes a run
row it no longer owns — overwriting the reclaimer's row or resurrecting its own
abandoned one. Three rules close that:

1. **Every write to the run row is owner-conditional.** `heartbeat` and
   `finish_run` carry `AND claim_owner = ? AND status = 'RUNNING'`. A stale owner's
   finalization therefore affects zero rows and is a no-op; it cannot overwrite
   the reclaimer's row and cannot un-abandon its own.
2. **Zero affected rows means the lease is lost, and that is a signal, not a
   shrug.** The heartbeat task inspects the affected-row count each beat. Zero
   sets a cancellation flag on the flight guard and logs an `engine_warn!` naming
   the run id, this process's owner string, and the owner that now holds the lease.
3. **Losing the lease stops the work promptly.** The refresh loop checks the
   flag at each natural checkpoint — between instruments and between providers —
   and on loss stops, writes nothing further, and returns
   `RefreshRunStatus::Failed` with message `lease_lost`.
4. **But the flag is not the guarantee; the database is.** Owner-conditional
   heartbeat and finalization protect the *run record*. They do nothing for the
   prices, FX rates and provider mappings, which are the data that matters.
   Consider A suspended inside a provider call: B reclaims and starts writing
   fresh values; A resumes, and — before its next heartbeat notices anything —
   completes its current unit and writes stale results over B's. A flag check
   between units cannot catch a unit already in flight, and even an immediate
   check before each write races with reclamation. Today every unit writes after
   awaiting its provider (`price_source_refresh.rs`: the recovered-symbol
   mapping, the currency-mismatch disable, the price upsert loop;
   `fx_refresh.rs`: the FX upsert loop; `symbol_seeding.rs`: mapping upserts),
   so each is exposed.

   The rule: **every batch of refresh-derived database changes is conditional on
   ownership inside the same write transaction that applies the batch.** Each
   unit does its provider I/O *outside* any transaction, then opens one
   `BEGIN IMMEDIATE` transaction, checks
   `SELECT 1 FROM market_data_refresh_runs WHERE id = ? AND claim_owner = ? AND status = 'RUNNING'`,
   and applies the whole batch — price rows, FX rows, or mapping change — inside
   it, or rolls back and discards the provider result if the check finds no row.
   Reclamation (the `FAILED`/`abandoned` update plus the new claim) already runs
   in its own `BEGIN IMMEDIATE` transaction; SQLite allows one writer at a time,
   so the check-and-write and the reclaim are serialized by construction and
   there is no window between check and write. The db write helpers take an
   executor rather than the pool so they can run inside the fenced transaction.
   This covers mapping recovery, mapping disable and symbol seeding, not only
   price and FX inserts. Cancellation remains for prompt stopping; it is never
   the sole guard against stale writes.

This phase is **committed work, not optional.** It is last because it is
independent of the window, not because it can be dropped; the closing
documentation and verification assume it landed.

### Frontend — exactly two touches

The ledger-identity frontend work is already landed. `HealthResponse` now
contains `mode`, `ledger` and `backup` and omits the retired `demo` field;
`AppFooter` renders the mode chip, ledger label and backup status through pure
view-models. No further frontend change remains for the desktop ledger path, and
`frontend/src/api/client.ts` stays untouched.

The footer shows the file name with the full path in a tooltip, renders
`In-memory demo` for demo mode, and uses the existing neutral and warning chip
tokens. A pending or failed health query omits ledger and backup spans rather
than rendering placeholders.

**Accepted consequence — `localStorage` does not carry between shells.** The
desktop window's origin is distinct from the browser build's origin, so each
shell keeps its own view preferences. The promise to survive reload and
navigation still holds within each shell; this is inherent to the second origin,
not a ledger-identity gap.

`frontend/src/components/AddInstrumentDialog.tsx` holds the only API path literal
outside the api layer (`apiGet<PriceStatusResponse>("/api/prices/status")`). It
routes through `apiGet`, so the transport is unaffected. **Noted, not changed.**

**Accepted consequence — `localStorage` does not carry between shells.** The
desktop window's origin (`http://tttb.localhost` on Windows) is not the browser
build's origin, so every preference under the 2026-07-10 *View settings are
persisted client-side* decision — `holdings.sorting`, `gains.returnMethod`, the
shared performance interval, rebalance parameters, chart-mode selections — starts
at its default in the desktop window and does not track the browser. That
decision's promise ("survives reload and navigation") still holds *within* each
shell. This is inherent to having a second origin and is accepted; it is called
out here and in the transport decision-log entry so it is not discovered later as
a bug.
The desktop origin is stable across restarts, so desktop preferences do persist
desktop-to-desktop.

### Scratch ledgers for drills

Every human drill in this plan that mutates data, kills a process, suspends one,
or tests a missing-ledger path runs on a **scratch ledger**, never on the
production file and never on a hand-made copy of it. Copying a live SQLite main
file can miss WAL contents or produce an inconsistent copy, and copying the
sidecars separately while writes continue does not fix that; SQLite's own
backup and how-to-corrupt guidance is explicit about it.

**Source.** A scratch ledger is created from one of two supported sources:

- an existing **verified launch snapshot** from the backup directory — every
  one has already passed the integrity and foreign-key check (2026-09-14), so
  it is a known-consistent starting point; or
- SQLite's supported online backup (`VACUUM INTO` or the backup API) taken from
  a running instance, which produces a consistent single file regardless of
  WAL state.

**Setup, every time:**

1. Copy the snapshot to an absolute scratch path, e.g.
   `%LOCALAPPDATA%\TickerTapeTallyBoard\scratch\<drill>-<date>.sqlite`. Not
   under the repository, not under a syncing folder.
2. Run `PRAGMA integrity_check` on the copy and record the result.
3. Launch with an **absolute** `TTTB_DATABASE_URL` naming that file **and** an
   **absolute, isolated** `TTTB_BACKUP_DIR` (e.g. a sibling `…\scratch\backups`),
   so a drill's launch-time snapshots do not land in, or prune, the real backup
   ladder. `TTTB_BACKUP_DIR` passes through the launcher's environment
   untouched, so set it in the shell before `scripts/start.ps1`.
4. **Before any mutation, confirm identity:** `/api/health` (or the footer
   tooltip) names the scratch path and the scratch backup directory. A drill
   that skips this step and mutates is a drill against an unknown file.
5. After **both** processes have stopped, delete the scratch ledger, its
   `-wal`/`-shm` sidecars and the scratch backup directory.

Missing-ledger checks point the same absolute override at a path that does not
exist; nothing in this plan requires renaming, deleting or force-killing the
production ledger's process. The completed rename (2026-09-13) is confirmed
read-only: launch, read the footer, done.

### Module and entry-point structure

Confirmed unchanged by review: `main.rs`, `lib.rs` and `mod.rs` stay thin
wrappers in both crates, and every runtime module is named for the behavior it
owns — `composition`, `server`, `ledger_location`, `app_paths`,
`in_process_http`, `content_security_policy`, `webview_probe`,
`startup_failure`, `body_limits`, `extract`. None is named for a phase or a
milestone, and none needs renaming when this plan is deleted.

---

## Phases

Plans are ephemeral. Durable documents and code must name the behavior, never
these phase numbers. **Decision-log entries land with their own phase**, not in a
batch at the end, so no phase finishes with the durable record describing
something that is not yet true.

**Verification commands.** After Phase 1 all cargo commands run from the
**repository root**:

| Scope | Commands |
|---|---|
| Backend-only change | `cargo build -p ticker-tape-tally-board-backend`, `cargo test -p ticker-tape-tally-board-backend`, `cargo clippy -p ticker-tape-tally-board-backend --all-targets -- -D warnings`, `cargo fmt` |
| `desktop/` or workspace manifest touched | `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt` |
| Frontend change | from `frontend/`: `npm run check` (covers `tsc --noEmit`, Biome and `vitest run`), then `npm run fmt` |

When launching npm through `Start-Process`, use `npm.cmd` explicitly.

---

### Phase 1 — Re-verify, then move to a Cargo workspace (mechanics only)

The walking skeleton cannot come first in wall-clock order: a standalone desktop
crate would create the second ~8.6 GB target tree the workspace decision exists
to avoid. This phase is therefore deliberately mechanical — there is nothing to
learn in it — and the risk phase follows immediately.

**Step 0 — re-verify the remaining assumptions against the current tree.** The
production hardening has already changed the ledger and logging seams, so verify
these facts before editing:

- `AppConfig` already carries a `LedgerLocation` and an explicit mode.
- `create_if_missing` is already gated by `CreateMissing` and the explicit
  `TTTB_CREATE_LEDGER_IF_MISSING=1` opt-in.
- `engine_logging` already takes `LogSettings`, never panics on file-open
  failure, and rotates bounded app-data logs.
- `backend/src/app.rs` no longer opens the ledger inline; it delegates to
  `ledger::open`.
- The default port is already `8480` and the highest migration is `0007`.
- `api::router`, `api::router_with_static_assets`, `AppState::for_tests`,
  `AppState::with_mode`, `db::testing::memory_pool` and
  `providers::FakePriceProvider` are still available where the bridge tests
  require them.

The future composition-root split, rather than the old monolithic `serve`, is
the remaining desktop work.

The bridge assumptions remain as specified below: it must consume the shared
router and state without adding a frontend transport or CORS change, and the
existing import-body and refresh behavior must remain available for the later
desktop phases. Cross-process refresh coordination remains this plan's future
work; the current service still has its process-local `AtomicBool` and `Mutex`.

**Step 1 — workspace.**

1. **Move the lockfile first:** `git mv backend/Cargo.lock Cargo.lock` before
   any workspace build, so the first build resolves against the locked graph
   rather than a fresh one. Then the root `Cargo.toml` with
   `[workspace] resolver = "2"` — stated explicitly, and `"2"` because the
   backend is edition 2021 and the mechanical move must preserve its current
   resolver semantics, not adopt the edition-2024 resolver as a side effect —
   `members = ["backend"]` for now, and `[workspace.package]` carrying `version`
   and `edition`. `backend/Cargo.toml` switches to `version.workspace = true` /
   `edition.workspace = true`, keeping the number it currently has. Every cargo
   command in this phase runs with `--locked`; if it fails, the move changed the
   dependency graph and that must be fixed, not accepted. Tauri's dependency
   changes arrive in Phase 2 and are reviewed there, separately.
2. `.gitignore`: verify it still covers the relocated `target/` (its `target` and
   `debug` entries are unanchored, so it should — confirm, do not assume) and that
   no stale `backend/target` remains after the move. No ledger-pattern edit is
   part of this work: the live `portfolio.sqlite` is outside the repository, and
   the retired repository-local ledger is already under the ignored `.local/`
   directory. Treat any unexpected ledger elsewhere in the tree as a data-safety
   hazard to investigate, not as a file this phase should silently ignore.
3. `scripts/start.ps1`: `$BackendExe` resolves to
   `<repo>/target/$BuildProfile/ticker-tape-tally-board-backend.exe`, keeping
   the existing `$BuildProfile` selection (`release` outside Vite development
   mode, `debug` inside it); the build step runs
   `cargo build -p ticker-tape-tally-board-backend` from `$RepoRoot`, still
   adding `--release` under the same condition it does today. Only the target
   root changes. Its "Run without -SkipBuild first" error message stays
   accurate.
4. `README.md`: cargo commands run from the repository root; the Sharesight spike
   becomes
   `cargo run -p ticker-tape-tally-board-backend --example sharesight_import_spike`.
5. `Agents.md` Workflow section, exact replacement for the two cargo bullets.
   **This phase creates a backend-only workspace, so the text must not yet name
   `desktop/`** — a document that describes a member which does not exist is
   exactly the inaccurate-durable-documentation problem this plan is trying to
   avoid. Phase 2 amends it when the member appears.

   > - The repository is a Cargo workspace sharing one `target/` directory. Cargo
   >   commands run from the repository root.
   > - Build with `cargo build -p ticker-tape-tally-board-backend`.
   > - When a backend task is complete, run
   >   `cargo clippy -p ticker-tape-tally-board-backend --all-targets -- -D warnings`
   >   and then `cargo fmt`.

6. `Agents.md` version wording, same phase, because it becomes inaccurate the
   moment the backend inherits its version. Replace the existing
   "backend from `backend/Cargo.toml` via `/api/health`" clause with:

   > - The UI displays two version values: frontend from `frontend/package.json`
   >   and backend from the workspace `[workspace.package].version` (inherited by
   >   the backend crate) via `/api/health`. Bump these versions as needed.

7. **Decision-log entry, landing with this change, not batched at the end:** the
   Cargo-workspace entry (see *Decision-log entries*), appended to the end of
   `docs/DecisionLog.md` in the log's own template.

Tests: none new. The point of this phase is that the existing suite passes
unchanged.

Verify:
- Backend command sequence from the repository root **with `--locked`**; whole
  suite green.
- `git diff --stat` shows the lockfile as a rename with **no** dependency-version
  changes, and `git ls-files backend/Cargo.lock` is empty.
- From a clean target tree, each of the following selects matching artifacts:
  default web (release build, release exe), `-Dev` (debug build, debug exe),
  and each with `-SkipBuild` after the corresponding build (starts) and without
  it (throws the "Run without -SkipBuild first" message, does not launch a stale
  binary from the other profile).
- `scripts/start.ps1 -SkipInstall` builds and starts, proving the relocated
  executable path.
- The `/api/health` version still matches the manifest after the version move.
- **External human testing recommended:** run `scripts/start.ps1` once end to end
  and confirm the app loads in the browser exactly as before. This phase changes
  no behavior, so anything that moves is a regression.

---

### Phase 2 — Walking skeleton: prove WebView2 carries the awkward cases

The risk phase, and the gate for the whole plan. It proves the three cases and
nothing else. Some ~20 lines of `main` wiring here are deliberately throwaway and
are replaced in Phase 3; the durable artifacts are the bridge, the adapter tests
and the probe, all of which survive.

**Step 0 — make the request body limits explicit (backend only, verifiable on its
own).** Not desktop work, but it comes first inside this phase because the
skeleton's third case cannot be written coherently against an implicit framework
default, and because it is a latent web-app defect this plan found (see *Request
body limits are explicit*).

- `backend/src/api/body_limits.rs` with `API_BODY_LIMIT_BYTES`,
  `IMPORT_BODY_LIMIT_BYTES` and the two derived-size helpers — the only
  definitions.
- `api_router()` layers the API-wide limit; each of the four import routes layers
  the import limit **on the handler**, not on the router.
- `backend/src/api/extract.rs` with `ApiJson<T>` and `ImportBody`, sharing one
  rejection-mapping helper. Replace `axum::Json<T>` in the **seven** handler
  signatures that take it as an extractor and `Bytes` in the four import handlers.
- **Decision-log entry lands here:** the request-body-limits entry.

  Tests (bare router, `oneshot`, **non-demo** state, no bridge involved — so this
  step is green before any desktop code exists), body sizes **derived** from the
  constants:
  - `under_import_limit_bytes()` of generated CSV reaches the import preview
    handler. This is also what proves the per-route raise is applied at all: if
    the 1 MiB router-level layer shadowed it, 8 MiB would be rejected.
  - `over_import_limit_bytes()` is rejected with `413` and an `ApiError` body
    whose `error.code` is `payload_too_large` — the **regression test** for the
    plain-text-`413` defect.
  - **The discriminating pair.** One body size between `API_BODY_LIMIT_BYTES` and
    the old implicit 2 MB default (1.5 MiB) sent to two routes: it **succeeds** on
    an import route and is **rejected** with `payload_too_large` on a non-import
    route. One size, two outcomes, so the test cannot pass unless the two limits
    are genuinely different and applied to the right routes. 1.5 MiB is also the
    size that always worked before this change, which makes the import half a
    regression test for the original defect rather than a duplicate of the 8 MiB
    case. (An earlier draft asserted an under-1-MiB import body succeeds; that
    passes under *both* limits and therefore proves nothing.)
  - Malformed JSON on a normal route yields `invalid_json` **in the standard
    envelope** — where today it produces no envelope at all and the frontend
    synthesizes `unknown`.

**Step 1 — the desktop crate and the bridge.**

1. `desktop/` crate: `ticker-tape-tally-board-desktop`, added to the workspace
   members, `version.workspace = true`. Dependencies: `tauri = "2"`,
   `tauri-build = "2"` (build-dependency), `tower = { version = "0.5", features = ["util"] }`,
   `axum`, `http`, `tokio`, `serde_json`, the backend crate by path.
   `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` is **not**
   set yet — the skeleton keeps a console so failures are visible; it is set in
   Phase 5 together with the dialog that replaces it.
2. **`desktop/build.rs` calling `tauri_build::build()`** — required, and the thing
   most likely to be forgotten. It has no path-reading job; ledger and logging
   paths come from backend configuration and app-data conventions.
3. `desktop/tauri.conf.json` per *Tauri build wiring and configuration*:
   `identifier` set, **no top-level `version`**, `app.windows: []`,
   `withGlobalTauri: false`, `app.security.csp: null`, `build.frontendDist`
   omitted, bundling off.
4. `desktop/src/in_process_http.rs` — `WEBVIEW_SCHEME`, `launch_url()`,
   `browser_visible_origin()`, `serve_request(router, request)`. The scheme string
   is defined once here and everything else derives from it.
5. `desktop/src/content_security_policy.rs` — the one policy string; the bridge
   adds the header on the way out.
6. `desktop/src/main.rs` (thin) + `desktop/src/lib.rs` (thin) +
   `desktop/src/launch.rs`: build the tokio runtime manually, build an
   `AppState`/router the crude way (whatever `app.rs` exposes today), register the
   asynchronous URI-scheme protocol over the router, open one window on
   `launch_url()`, run.
7. **`Agents.md` amendment:** now that `desktop/` exists, extend the Phase 1
   workspace bullet to name both members and add the widen-when-touched rule:

   > - The repository is a Cargo workspace (`backend/`, `desktop/`) sharing one
   >   `target/` directory. Cargo commands run from the repository root.
   > - When `desktop/` or the workspace manifest is touched, widen to
   >   `cargo clippy --workspace --all-targets -- -D warnings`. The desktop crate
   >   links WebView2 and is slow to build, which is why the per-package command
   >   stays the default for backend-only work.

Adapter tests (`desktop/src/in_process_http.rs`, `oneshot`-style, mirroring
`backend/src/api/mod.rs`). **These are the regression net, not the gate.** Each
one states its state, because the wrong state makes several of them pass
vacuously — a demo-mode state turns every non-GET into `403`, and the unseeded
`AppState::for_tests()` turns a `DELETE` into `404`:

- `bridge_carries_rejected_write_status_and_error_body` — **demo-mode state**;
  `POST /api/transactions` through the bridge returns `403` and a body whose
  `error.code` is `demo_read_only`, byte-identical to the same request through
  `oneshot` on the bare router.
- `bridge_carries_no_content_without_a_body` — **non-demo `AppState::for_tests()`
  with one transaction seeded first**, then `DELETE /api/transactions/{id}`
  returns `204` with a zero-length body and no synthesized `content-length`.
  Against the unseeded default this would be a `404` and would prove nothing.
- `bridge_carries_large_csv_upload` — **non-demo state**;
  `under_import_limit_bytes()` of generated `text/csv` reaches the import preview
  handler with its bytes intact.
- `bridge_matches_bare_router_for_oversized_upload` — **non-demo state**;
  `over_import_limit_bytes()` produces the same `413` and the same
  `payload_too_large` body through the bridge as through `oneshot`. In demo state
  both sides would return `403` and the test would pass without testing anything.
- `probe_limited_route_matches_real_import_route` — **non-demo state**;
  `POST /__probe/echo-limited` and a real import route return an identical status
  and body for an over-limit request. This is what keeps the probe's substitute
  route representative of the route it stands in for.
- `bridge_routes_every_request_uri_form` — the same **query-bearing** request
  expressed as `tttb://localhost/<path>?<query>`,
  `http://tttb.localhost/<path>?<query>` and `/<path>?<query>` reaches the
  handler identically **and the response reflects the query** (use a report
  endpoint that returns its resolved period, or an equivalent whose output
  depends on a parameter). A bare `/api/health` cannot detect a bridge that
  keeps `path` and drops `path_and_query`, which would silently break report
  ranges, watchlist options and import commit parameters. This is what makes
  the bridge independent of which form Wry actually delivers.
- `probe_echo_accepts_under_limit_body` — bare router, the exact
  `POST /__probe/echo` route, the generated `under_import_limit_bytes()` body:
  `received_bytes` equals the sent length. This proves the harness before the
  window does, so a `413` from the echo route can never be mistaken for a
  WebView2 limitation.
- `bridge_emits_content_security_policy_header` — the exact policy value is
  present on the SPA index, on a static asset, and on an API response.
- `bridge_reports_body_collection_failure_as_json_error` — a router whose
  response body fails to collect yields a `500` with an `ApiError`-shaped body.

**Step 2 — the in-window probe. This is the gate.**

8. `desktop/src/webview_probe.rs` and the `--probe-webview` flag, exactly as
   specified in *The in-window probe*: the probe-only route set (harness page,
   `probe.js`, `echo`, `echo-limited`, `no-content`, `report`) wrapping — never
   modifying — the application router; a no-inline-script harness page; the seven
   cases; per-case harness timeouts and a Rust-side watchdog; the JSON report
   written under `.local/probe/` and logged; the platform versions recorded;
   non-zero exit on any failure or on an incomplete report.
9. In this phase the probe is run directly —
   `cargo run -p ticker-tape-tally-board-desktop -- --probe-webview` from the
   repository root, after `npm run build` — because `scripts/start.ps1` does not
   gain its `-Desktop` mode until Phase 5. That phase adds `-ProbeWebView` as a
   convenience wrapper over the same flag; the flag, not the script, is the
   interface.

Verify:
- Step 0 alone, before any desktop code exists: backend command sequence and its
  bare-router tests green.
- **External human testing for step 0 (web app):** run `scripts/start.ps1` and
  confirm a real Avanza or Sharesight export still previews and commits. The body
  limit change lands in the browser build too, so a mistake here is a web-app
  regression, not a desktop one.
- Workspace command sequence; adapter tests green.
- **External human testing REQUIRED — the gate.** Build the frontend
  (`npm run build` from `frontend/`), then run
  `cargo run -p ticker-tape-tally-board-desktop -- --probe-webview`. The window
  must show every probe case green, the report must not be marked `incomplete`,
  the process must exit zero, and `.local/probe/webview-probe-<timestamp>.json`
  must exist. **Paste that JSON into the implementation notes.** It is the
  evidence the rest of the plan rests on, and the Context field of the transport
  decision-log entry quotes its version block. (The probe runs in demo mode and
  needs no ledger preparation.)
- **External human testing REQUIRED — the ordinary app. Prepare the ledger
  first; this is a step, not a caveat.**

  The desktop shell must use the landed configuration: production resolves the
  ledger at `%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio.sqlite`, refuses a
  missing file unless `TTTB_CREATE_LEDGER_IF_MISSING=1`, binds no TCP listener,
  and uses the app-data log convention. The drill runs on a **scratch ledger
  prepared per *Scratch ledgers for drills*** — never on a copy of the live
  file. So, in order:

  1. Prepare the scratch ledger from a verified launch snapshot, with an
     absolute `TTTB_DATABASE_URL` and an isolated absolute `TTTB_BACKUP_DIR`,
     as that section specifies.
  2. Launch the desktop app without the probe flag and **confirm identity
     before any mutation**: `/api/health` (via the footer tooltip) names the
     scratch path, not the app-data ledger.
  3. Confirm in the window: the dashboard renders with real data; a deep
     link/reload of `/board` and `/asset/:id` still renders (the router side is
     already covered by `static_router_uses_index_fallback_for_frontend_routes`);
     a real CSV import preview on the Import page succeeds; **deleting a
     transaction (204) succeeds and the table refreshes — against the scratch
     ledger, never the real portfolio**; fonts, Lightweight Charts and the
     treemap all render under the CSP; and DevTools' network panel shows the
     requests on the custom scheme with no CORS preflight.
  4. Confirm **no listener exists**: `Get-NetTCPConnection -OwningProcess <pid>`
     returns nothing and no Windows Firewall prompt appeared.
  5. Clean up per the scratch-ledger section once the process has stopped.

  This drill uses the landed refusal and app-data path rules while proving the
  desktop transport behavior.
- **If any probe case is red, or the report is marked `incomplete`, stop and
  classify before concluding anything.** Follow the three-step rule under
  *Documented fallback*: a harness, bridge or configuration defect (wrong echo
  limit, missing CSP header, watchdog misconfiguration, report-writing failure)
  is repaired and the same gate rerun; only a failure reproduced with those
  excluded is platform evidence, and only that takes the fallback question back
  to the user. A hung case counts as red, not as "needs more time".
- **Negative CSP run, once:** with the bridge's header removed locally (not
  committed), cases 6a and 6b go red while every transport case stays green;
  restored, everything is green. Record both reports.

---

### Phase 3 — One composition root shared by both entry points

Backend refactor. The web server's behavior must be byte-identical afterwards.

1. Extract the desktop composition from the existing `app::run()` while keeping
   `main.rs` and the server entry point thin. `ledger::open` remains the single
   ledger-opening sequence; the desktop entry point consumes the shared state and
   router rather than constructing competing copies. Extend the existing typed
   `StartupError` enum; do not introduce the old sketch as a second error type.
2. `Application::shutdown` aborts the launch-refresh handle and closes the
   SQLite pool; `server::serve` calls it after `axum::serve` returns.
3. **`AssetPolicy::{Optional, Required}` is already defined, but
   `AppConfig::asset_policy()` is derived only from mode:** production is
   `Required`, while development and demo are `Optional`. Add a shell-level
   policy override (a field or composition parameter) so the desktop shell can
   require `index.html` in every mode, including demo, while the web server keeps
   its current mode-derived behavior. A missing required asset returns the
   existing `StartupError::StaticAssetsMissing`.
4. Move the existing `app.rs` tests (`launch_refresh_spawns_background_job`,
   `launch_refresh_is_skipped_*`, `demo_state_is_seeded_and_query_only`) to
   whichever module now owns the behavior. They must pass unchanged.
5. `desktop/src/launch.rs` calls the extracted shared composition and shutdown
   path, with `AssetPolicy::Required`.
6. **Decision-log entry lands here:** the in-process desktop transport entry,
   whose Context quotes the probe report's version block from Phase 2.

Tests:
- `Application::build` produces a router that serves both `/api/health` and the
  SPA index when the assets directory exists.
- **The asset matrix, every cell, three inputs each** — missing `index.html`,
  empty (zero-byte) `index.html`, valid `index.html`: web development and demo
  produce the API-only router plus the existing warning for the first two and
  the full router for the third; web production and every desktop mode return
  `StartupError::StaticAssetsMissing` for the first two and build for the third.
  The empty-file input is what proves the nonempty check survived the refactor.
- `Application::shutdown` closes the pool: a query after shutdown fails.
- A demo `Application` builds and shuts down with no filesystem access.
- Existing app tests green in their new home.

Verify:
- Workspace command sequence.
- **External human testing recommended:** `scripts/start.ps1` still starts,
  serves and stops cleanly, and the desktop executable still opens the window —
  both entry points now go through one construction path, so a mistake here
  breaks both. Then rename `frontend/dist` aside and confirm the matrix by hand:
  `scripts/start.ps1 -Dev` (development) starts and serves API routes only;
  `scripts/start.ps1` (production) **fails** naming the missing assets, as the
  landed production behavior requires; the desktop executable fails loudly
  instead of opening a window on the backend root.

---

### Phase 4 — Ledger identity: rename, absolute resolution, refuse-to-create, observability

1. **Mostly landed.** `backend/src/ledger/location.rs` provides the shared
   `LedgerLocation`, mode-aware defaults and `LedgerLocation::memory()` before
   filesystem work in demo. `AppConfig` already carries a `LedgerLocation` and a
   mode. The remaining tidy is moving `memory_pool` and `RepoError` out of
   `db/mod.rs` so it becomes the thin wrapper `Agents.md` asks for.
2. **Landed.** `db::open` takes `CreateMissing`; `create_ledger_if_missing`
   defaults to `false` and reads `TTTB_CREATE_LEDGER_IF_MISSING`. The existing
   pool tests pass `CreateMissing::Yes` explicitly when creation is intended.
3. **Landed.** A missing ledger produces `StartupError::LedgerMissing` with the
   resolved path and is logged; no empty file is invented.
4. **Landed.** `AppState` carries mode, ledger path and backup state.
   `/api/health` exposes `mode`, `ledger` and `backup`, but no shell field; demo
   reports memory with a null path.
5. The former repository-paths reader and the old production-flag path are
   removed by the landed launcher. The desktop shell must consume backend config
   for the ledger, with `-DatabaseUrl` as the explicit override.
6. **Ledger rename and refuse-to-create are landed.** The legacy guard covers the
   database and SQLite sidecars; `README.md` records the app-data location and
   the one-time `-InitLedger` requirement.
7. **No script extraction.** The existing launcher remains the single place for
   the legacy guard, flag validation and environment save/restore. No
   `scripts/Common.ps1` is needed.
8. The frontend health and footer work is already landed; see *Frontend —
   exactly two touches*. No further ledger-identity change is required there.
9. **Absolute-override rule (settled decision 26).** `ledger::resolve` rejects a
    relative filename outside demo with a new `LedgerLocationError::NotAbsolute`
    mapped through `ConfigError` to `StartupError`; `resolve_log_file` and
    `resolve_backup_dir` turn a relative override into their existing
    `Unresolved` variants with a message naming the rule. The URL and its query
    options are otherwise passed through unchanged.
10. **Decision-log entry lands here:** the observable-ledger-identity entry.

Tests:
- `ledger/location` unit tests cover mode-aware defaults, memory resolution,
  absolute URL handling and unsupported URLs, **and relative filenames are
  rejected outside demo while a URL's query options survive resolution
  untouched.**
- Relative `TTTB_LOG_FILE` yields `LogFile::Unresolved` and relative
  `TTTB_BACKUP_DIR` yields `BackupDirectory::Unresolved`, each with a message
  naming the absolute-path rule; absolute values resolve exactly as before.
- Demo with a relative `TTTB_DATABASE_URL` still resolves to memory and touches
  no file (the override is ignored, not validated).
- `db::connect` with `CreateMissing::No` against a non-existent path returns an
  error and **creates no file** (assert the file is still absent afterwards).
- **Demo config never resolves a ledger:** `demo_mode` with `TTTB_DATABASE_URL`
  pointing at a nonexistent path yields `ledger_path() == None`, builds
  successfully, and leaves no file behind. This is the interaction that would
  otherwise pop a startup dialog for a mode with no ledger by design.
- `/api/health` contract test: `ledger.mode`/`ledger.path` for a file-backed
  state and `memory`/`null` for a demo state. Also assert the response carries
  **no** `shell` key, so a future reader does not reintroduce an unconsumed field.
- Vitest: the footer label helper renders the file name for a file ledger, the
  in-memory wording for a `null` path, and never emits an empty span or a stray
  separator; a pending/failed health query omits the ledger span entirely.

Verify:
- Workspace and frontend command sequences.
- **External human testing — read-only confirmation of the landed location.**
  The rename and legacy guard are complete (2026-09-13); nothing here moves the
  real ledger. `scripts/start.ps1` opens the real portfolio and the footer shows
  `portfolio.sqlite` with the app-data path on hover; the footer's version and
  row counts match the previous run. That is the whole real-ledger check.
- **Everything else runs on scratch data**, per *Scratch ledgers for drills*:
  pointing an absolute `TTTB_DATABASE_URL` at a nonexistent scratch path fails
  to start and leaves no file behind; `-InitLedger` with a fresh scratch path
  does create and migrate one; a **relative** `TTTB_DATABASE_URL`, launched
  from two different working directories, is rejected identically both times
  with a message naming the rule, and no file is created in either directory;
  `scripts/start.ps1 -Demo` shows the `DEMO` chip and `In-memory demo` with no
  path and no empty span.

---

### Phase 5 — Desktop hardening: anchoring, logging, failure dialog, shutdown, launch mode

The window becomes something you can put on the Start menu.

1. **`desktop/build.rs` keeps only `tauri_build::build()`.** It must not read
   a repository path file or emit ledger/log path variables; that machinery is
   not part of the architecture.
2. `desktop/src/app_paths.rs` derives the desktop static-assets directory from
   the build-tree anchor. `TTTB_STATIC_DIR` overrides it, and a relative value is
   resolved against that anchor rather than the CWD. The ledger remains owned by
   backend `AppConfig`; the desktop log remains owned by the app-data convention.
3. Reuse `engine_logging::initialize(&LogSettings) -> LogInitOutcome`; it never
   panics. The desktop entry point supplies a mode/shell-specific log path under
   `%LOCALAPPDATA%\TickerTapeTallyBoard\logs\`, with the existing bounded
   rotation settings. If per-line process identity is needed, extend
   `LogSettings`/`RotatingFileWriter` rather than adding `LogDestination`.
4. The line-buffering tagged writer, if retained, wraps the existing file sink:
   buffer to a newline, then emit `[<instance_tag>] ` plus the completed line in
   one `write_all`. The terminal sink is untagged.
5. Startup and shutdown banners at `info`, each naming **which shell**
   (`AppState`'s `AppShell`), executable path, ledger path, static assets
   directory and log path — the context a per-line tag cannot carry.
6. `desktop/src/startup_failure.rs` — `MessageBoxW` for every `StartupError`
   variant (including `StaticAssetsMissing`) and for a panic during startup,
   naming the resolved paths and the log file. Then
   `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` is
   enabled, so the release build has no console.
7. Window close drives graceful shutdown: handle `RunEvent::Exit` and call
   `handle.block_on(application.shutdown())`.
8. **`scripts/start.ps1 -Desktop` — the desktop launch is a mode of the existing
   script, not a second script.** One script keeps ledger resolution, the
   legacy-name guard, the demo wiring and environment save/restore in one place,
   which is the same DRY argument the plan applies to the router and the
   composition root. `scripts/Common.ps1` is **not** created: its only purpose was
   to share those pieces between two scripts.

   The mode branches at the point where the script stops being about two
   processes. Shared and unchanged: `Assert-Command`, `Invoke-Step`,
   `Invoke-NativeCommand`, `ConvertTo-SqliteUrl`, the legacy-ledger guard, the
   `-Demo` argument validation, and the current environment save-and-restore
   `finally` block. Demo uses `TTTB_MODE=demo`, not `TTTB_DEMO_MODE`.

   In `-Desktop` mode the script:
   - builds the frontend (unless `-SkipBuild`) — `frontend/dist` is the desktop
     app's asset source, so this is required, not optional;
   - builds `cargo build -p ticker-tape-tally-board-desktop` instead of the
     backend package (the desktop crate depends on the backend crate, so the
     backend still compiles; its *binary* is simply not needed), **with the
     same profile rule as the web shell**: `--release` and
     `target/release/…desktop.exe` by default, `debug` under `-Dev`. The
     no-console attribute is `cfg_attr(not(debug_assertions), …)`, so a normal
     desktop launch — release — has no console window, and a `-Dev` desktop
     launch keeps one, which is the useful behavior for development. `-Dev`
     selects development *mode* and the debug *profile* exactly as it does for
     the web shell; only the Vite server is web-specific. `-SkipBuild` selects
     the executable for the same profile and throws if it is missing, never
     falling back to the other profile;
   - skips `Stop-OrphanVite`, `Resolve-FrontendPort`, `Resolve-BackendPort`,
     `TTTB_PORT`, the Vite process, both `Wait-Url` calls, and the browser open;
   - prints the resolved ledger (or `demo`) and the desktop log path from the
     backend/app-data conventions, then starts the executable and **waits for it
     to exit**. Waiting
     keeps the existing
     "the script owns the run" contract, gives a place to report a non-zero exit
     code — which is how a startup failure signals itself alongside the dialog —
     and keeps the environment restore correctly ordered.

   The Desktop branch composes with the launcher's current flag surface:
   `-Dev`, `-Demo`, `-InitLedger`, `-NoBackup`, `-NoRefresh` and
   `-DatabaseUrl`. `-Dev` selects development mode and the debug profile,
   `-Demo` selects the seeded in-memory mode, `-InitLedger` is the explicit
   creation opt-in for a non-demo ledger, the backup and refresh switches
   retain their existing scopes, and the database switch remains an explicit
   override. `-Port` is **not** in that list: there is no listener in desktop
   mode, so it cannot have the web switch's meaning and is rejected. There is
   no desktop `-ProductionDb` path; the launcher's compatibility parameter
   remains only as a throwing retired-flag stub.

   Switch composition and rejected combinations, in the style the script
   already uses for `-Demo` plus retired database selectors:
   - `-Desktop -Demo` — **supported**, and the reason demo now reaches the window.
   - `-DatabaseUrl` — supported; the script sets `TTTB_DATABASE_URL` and the
     desktop process uses that explicit override through backend config. The
     launcher makes the value absolute (`ConvertTo-SqliteUrl`), and the backend
     rejects a relative one regardless. `-LocalDatabaseUrl`
     and `-ProductionDatabaseUrl` are retired and throw guidance errors.
   - `-Desktop -SkipInstall`, `-SkipBuild`, `-BuildOnly`, `-InitLedger`,
     `-NoBackup` and `-NoRefresh` — **supported**, same meanings.
   - `-Desktop -Port <n>` — **rejected**: no listener exists, so the switch can
     only mislead. Detected with `$PSBoundParameters.ContainsKey("Port")`.
   - `-Desktop -ProbeWebView` — **supported**; a convenience wrapper over the
     desktop executable's `--probe-webview` flag, which is the real interface and
     already worked before this switch existed. It prints where the report landed
     and propagates the probe's exit code. Rejected with any database switch,
     since the probe always runs in demo mode.
   - `-Demo -InitLedger` — **rejected** in both modes, joining the existing
     `-Demo` exclusions: demo never opens a ledger, so asking to create one can
     only mean the user misunderstands what will run.
   - `-Desktop -FrontendPort <n>` — **rejected**: there is no Vite dev server in
     desktop mode. Detected with `$PSBoundParameters.ContainsKey("FrontendPort")`
     so the parameter's default value is not mistaken for an explicit one. Note
     that `$PSBoundParameters` is **not currently used anywhere in the script** —
     this introduces the pattern. It is the correct idiom for the job, but it is a
     new one here, not an existing convention being followed.
   - `-Desktop -NoBrowser` — **rejected**: no browser is opened in desktop mode,
     so the switch can only mislead.
   - `-ProbeWebView` without `-Desktop` — **rejected**: there is no probe for the
     web shell.
   - `-Demo` still rejects `-DatabaseUrl` and `-InitLedger`; the retired database
     selectors throw in every mode.

   Note in the script's help and in `README.md`: **close the window to stop the
   app.** Ctrl+C in the console falls into the existing `Stop-ProcessTree`
   `finally`, which is a hard kill that skips the graceful pool close. SQLite
   recovers from that through WAL, but it is not the intended path.
9. **Demo mode reaches the desktop shell.** The desktop entry point reads
   `TTTB_MODE=demo` exactly like the server does — no shell-specific override, no
   ignore-warning. `scripts/start.ps1 -Desktop -Demo` therefore opens the seeded,
   read-only, in-memory demo in the native window, and the demo becomes a
   presentation surface you can show someone without a browser chrome around it.
   Everything that makes this safe is already in place and is *verified*, not
   assumed, in this phase:
   - the ledger is resolved to `LedgerLocation::memory()` before any filesystem
     work, so neither the refuse-to-create rule nor absolute-path resolution can
     fire;
   - the launch refresh is already skipped in demo mode, so the window makes no
     network calls;
   - `PRAGMA query_only = ON` and the `demo_read_only` route guard are unchanged,
     so `403 demo_read_only` is now reachable end-to-end in the window and not
     only in the bridge tests;
   - `/api/health` reports `mode: "memory"`, `path: null`, and the footer renders
     `In-memory demo` beside the `DEMO` chip (Phase 4).
10. **Decision-log entries land here:** the working-directory-independence /
    logging / visible-failure entry, and the demo-in-both-shells refinement entry.

Tests:
- `app_paths` unit tests: defaults derive from a supplied anchor; a relative
  `TTTB_STATIC_DIR` resolves against the anchor, not the CWD; an absolute
  override wins.
- `app_paths` derives only the desktop static-assets path from the supplied
  build-tree anchor; ledger resolution remains covered by backend configuration
  tests, and the desktop log path is covered by the app-data logging tests.
- `app_paths` in demo mode produces no ledger URL at all, so a nonexistent
  default ledger path cannot fail a demo launch.
- `engine_logging::initialize` returns `file_error` and still initializes when
  the target path is unopenable (point it at a directory), and does not panic.
- **The tagged writer is unit-tested directly, without a logger:** a multi-line
  payload written in several `write` calls — including one that splits a line
  mid-word, which is how `simplelog` actually emits a record — produces output in
  which *every* line begins with `[tag] ` exactly once, and no partial line is
  emitted before its newline arrives. This is the assertion that makes "the tag is
  on every line" true rather than aspirational.
- **Cross-process rotation, on real files:** two independent
  `RotatingFileWriter` instances (independent state is what makes them stand in
  for two processes) opened on the same temporary path with a small
  `max_bytes`, writing alternately and repeatedly crossing the limit. Assert:
  every record in the live file and every archive is complete and carries
  exactly one tag; both writers keep writing successfully after every rotation
  (no writes land in an archive after it was rotated away); the archive set is
  exactly `kept_rotations` files with the expected names; and total bytes across
  all files stay bounded by `(kept_rotations + 1) × max_bytes` plus one record.
  A shared in-memory buffer cannot exercise any of this.
- The writer follows a foreign rotation: after the test renames the live file
  out from under an open writer, that writer's next record lands in a freshly
  created file at the path, not in the renamed one.
- The startup/shutdown banner renders an absent ledger path as `in-memory (demo)`
  rather than an empty field, and names the shell.
- `StartupError` `Display` includes the resolved path for each variant, including
  `StaticAssetsMissing` (the dialog and the log line share that text — one source
  of truth).

Verify:
- Workspace command sequence.
- **External human testing REQUIRED — none of this is automatable.**
  - Create a Windows shortcut to the built executable with its "Start in" field
    **cleared or set to `C:\`**, launch from it, and confirm the window opens
    with the real portfolio (proving the ledger and log are not CWD-relative) and
    that the app-data desktop log received the startup banner.
  - Point an absolute `TTTB_DATABASE_URL` at a nonexistent scratch path and
    launch: a native error dialog appears naming the missing path and the log
    file, and **no window and no empty ledger** appear. (The real ledger is
    never renamed aside; the same failure is reproduced on scratch
    configuration.)
  - Launch from a shortcut with `TTTB_DATABASE_URL=sqlite://scratch.sqlite`
    (relative) and "Start in" set to two different directories: both launches
    fail with the same dialog naming the absolute-path rule, and neither
    directory gains a file.
  - Rename `frontend/dist` aside and launch: a native error dialog names the
    missing assets directory, and **no window** appears — not a window showing
    the backend root.
  - Confirm no console window appears in a release build
    (`cargo build --release -p ticker-tape-tally-board-desktop`).
  - Close the window with the X and confirm the shutdown banner is written, the
    process exits, and no `-wal` file is left mid-transaction.
  - Confirm again that `Get-NetTCPConnection -OwningProcess <pid>` shows no
    listener and no firewall prompt appears.
  - Launch two desktop instances at once (same mode, so they share one file),
    with a temporarily small rotation threshold so both cross it several times
    while in use. Then read `%LOCALAPPDATA%\TickerTapeTallyBoard\logs\` as a
    bounded timeline: every line is complete and tagged, both instances kept
    logging after each rotation, exactly `kept_rotations` archives exist, and
    no archive received lines after it was rotated. This is the Windows
    process-level check the unit test cannot give.
  - **Demo must not trip the ledger rules:** with an absolute `TTTB_DATABASE_URL`
    pointing at a nonexistent scratch path, `-Desktop -Demo` still starts
    normally — no startup dialog — proving demo never resolves a ledger path.
  - **Demo in the window:** `scripts/start.ps1 -Desktop -Demo` opens the seeded
    demo; the footer reads `DEMO` plus
    `In-memory demo` with no path and no empty span; attempting a write — adding a
    transaction, or setting a conviction — is rejected with the
    `demo_read_only` message rather than a raw `403`; no ledger file is created
    or opened (check `.local/db/` timestamps); and no network call is made
    (the launch refresh is skipped).
  - Reject-combination checks: `-Desktop -FrontendPort 5173`,
    `-Desktop -NoBrowser`, `-Desktop -Port 8480`, `-ProbeWebView` without
    `-Desktop`, and `-Demo -InitLedger` each fail immediately with a clear
    message and start nothing.
  - From a clean target tree: default `-Desktop` builds release and runs the
    release executable with no console; `-Desktop -Dev` builds debug and runs
    the debug executable with a console; each `-SkipBuild` variant selects the
    matching artifact or throws.
  - `-Desktop -ProbeWebView` reproduces the Phase 2 probe result and propagates a
    non-zero exit code when a case fails — the wrapper does not swallow the gate.

---

### Phase 6 — Cross-process refresh coordination

Committed work, placed last because it is independent of the window — not
because it is optional. It is also the phase most likely to overrun, so start it
with the failure modes in mind rather than discovering them.

1. Migration `add_refresh_run_claim.sql` (additive: `claim_owner TEXT`,
   `heartbeat_at TEXT` on `market_data_refresh_runs`; plus the one-row
   `data_revision` table seeded with `0`).
2. `db/market_data_runs.rs`:
   - `try_claim_run(pool, trigger, owner, now, stale_after)` running
     `BEGIN IMMEDIATE`, reclaiming a stale `RUNNING` row as `FAILED` with message
     `abandoned` **and bumping the persisted data revision** in the **same**
     transaction, and inserting the new claim;
   - `heartbeat(pool, run_id, owner, now) -> LeaseState` and
     `finish_run(..., owner)` — both **owner-conditional**
     (`AND claim_owner = ? AND status = 'RUNNING'`) and both reporting whether
     they affected a row;
   - `live_claim(pool, now, stale_after)`;
   - `fenced_write(pool, run_id, owner, |tx| …)` — opens `BEGIN IMMEDIATE`,
     checks the owner row inside it, runs the batch closure against the
     transaction, and commits; returns `LeaseLost` without writing if the
     check finds no row.
   - One shared `REFRESH_CLAIM_STALE_AFTER` constant (proposed 120 s).
3. `db/data_revision.rs` (or the module that fits): `bump(executor)` and
   `current(executor)` over the one-row table; `DataRevision` in `state` becomes
   an async wrapper over the pool, keeping its two method names so callers
   change only by awaiting. Every stamped response and `/api/data-version` read
   through it; the middleware, launch refresh, refresh-failure-after-writes and
   startup-after-migrations sites bump through it.
4. `refresh.rs`: delete the in-memory `AtomicBool`/`Mutex` pair. `refresh()`
   starts only when `try_claim_run` succeeds and otherwise returns the current
   status as it does today. The flight guard owns a heartbeat task that stops when
   the guard drops. `is_refreshing()` and `active_run()` become async reads of the
   live claim; `status()`'s `refreshing`, `latest_run` and the data-version
   handler's `prices_refreshing` follow.
5. **Every refresh-derived write goes through `fenced_write`.** The price
   upsert loop, the FX upsert loop, `persist_mapping` (recovery and disable) and
   the symbol-seeding mapping upserts each do their provider I/O first, then
   apply their batch inside one fenced transaction, and treat `LeaseLost` as the
   signal to stop with `lease_lost`. The `prices`, `fx_rates` and
   `provider_symbols` write helpers take an executor so they can run inside it.
6. **Lease loss also stops the loser promptly.** The heartbeat task treats a
   zero-affected-row update as lease loss: it sets a cancellation flag on the
   flight guard and logs an `engine_warn!` naming the run id, this owner and the
   current holder. The refresh loop checks that flag between instruments and
   between providers and on loss stops without starting another unit. The flag
   is for promptness; the fence is the guarantee.
7. Log the owner identity on claim, reclaim, lease loss and release, per the
   logging rules (enough context to identify the run and the owning process).
8. **Decision-log entry lands here:** the refresh-claim entry, which also
   refines the 2026-09-12 revision entry.

Tests. **Two pools opened on the same temporary file, not one shared in-memory
pool** — a single in-memory pool proves nothing about two independent SQLite
connections taking real locks, which is the actual scenario:
- **Regression test for the reported defect:** two `MarketDataService` instances
  over two pools on one file — the second `refresh()` does not start a provider
  run (assert the fake provider's call count) and reports the first one's run;
  both report `refreshing = true`.
- A claim whose `heartbeat_at` is older than the staleness window is reclaimed;
  the abandoned row ends `FAILED` with message `abandoned` and a `finished_at`,
  and a new run starts.
- A fresh foreign claim is **not** reclaimed.
- Heartbeat keeps a long run's claim alive past the staleness window.
- **Reclaimed-owner containment, the failure mode the first draft missed:** owner
  A stalls and is reclaimed by owner B; then A's heartbeat reports lease loss, A's
  `finish_run` affects **zero** rows, A's run row stays `FAILED`/`abandoned`, and
  B's row is untouched by A.
- **Stale writes are fenced, not merely discouraged — the test the earlier
  draft could not have passed.** A fake provider that blocks on a channel lets
  the test suspend A *inside* provider I/O, after A has claimed and started a
  unit. While A is parked: advance the clock past the staleness window, let B
  reclaim and write distinguishable prices, FX rates and a mapping. Then release
  A's provider response **before A's next heartbeat** so A completes its unit.
  Assert that A wrote **no** price, FX or mapping rows (B's distinguishable
  values are the only ones present), that A's run ends `FAILED`/`abandoned` and
  B's is untouched, and that A returns `lease_lost`. Run the same scenario with
  reclamation happening exactly at the ownership-check/write boundary (the
  reclaim is issued while A's fenced transaction is pending), and assert one
  of the two serialized outcomes and never a mix. Inspecting run records and
  later provider-call counts alone would miss the write from the already
  outstanding request.
- **Lease loss stops work:** after lease loss the fake provider's call count stops
  increasing and the refresh returns `lease_lost`.
- `status()` reports a foreign process's run in `latest_run`.
- **The data revision is shared:** with two independent states over two pools
  on one file, a successful mutation through A changes the revision published by
  B's `/api/data-version`; a launch refresh completing through A does too; a
  refresh through A that writes rows and then fails does too; and B reclaiming
  A's abandoned run (which had written rows) changes it as well. A rejected
  mutation through A leaves B's revision alone.
- **The frontend contract holds across processes:** a mounted client (Vitest,
  the existing query-layer tests) whose heartbeat token changes refetches its
  portfolio queries without focus changes or manual reload — this is the
  landed behavior; the test pins that nothing in the token shape changed.
- Startup after migrations bumps once; demo's revision is readable and never
  changes after seeding.
- Existing single-process refresh tests pass unchanged.

Verify:
- Workspace command sequence.
- **External human testing REQUIRED, on a scratch ledger per *Scratch ledgers
  for drills*, because this performs live provider calls and force-kills
  processes.** Start `scripts/start.ps1` and `scripts/start.ps1 -Desktop`
  against the same scratch ledger (same absolute `TTTB_DATABASE_URL` and
  isolated `TTTB_BACKUP_DIR` in both shells), and confirm both footers name the
  scratch path before doing anything else. Confirm: only one launch refresh
  runs; both UIs show the spinner during it and both stop when it ends; exactly
  one new `market_data_refresh_runs` row appears; pressing manual refresh in one
  while the other is refreshing reports the running state rather than starting
  a second run; **after the refresh finishes, the other window's panels refetch
  on their own** (the revision changed) without a focus change or reload; and
  editing a transaction in one window makes the other window's holdings update
  within the heartbeat interval. Then kill one process mid-refresh with
  `taskkill /F` and confirm that after the staleness window the other process
  can start a refresh and the abandoned row reads `FAILED` / `abandoned`.
- **Suspend rather than kill, to exercise the reclaimed-owner path:** suspend one
  process mid-refresh (Process Explorer, or a debugger break) for longer than the
  staleness window, let the other reclaim, then resume it. The resumed process
  must log lease loss, stop, and leave both run rows correct — and the price
  rows must carry only the reclaimer's `fetched_at` stamps for any instrument
  both touched.
- **The real ledger is never killed, suspended or mutated by this drill.** The
  real-ledger acceptance check is the normal launch of both shells, both footers
  naming `portfolio.sqlite`, one launch refresh between them, and normal
  shutdown. Clean up scratch files after both processes have stopped.

---

### Phase 7 — Documents, decision-log entries, version bumps, final sweep

1. `docs/Design.HighLevel.md`:
   - *Deployment model*: add the desktop shell as a second deployment of the same
     source — one window, in-process transport over a custom URI scheme, no
     listener, assets from disk — alongside the existing loopback-only web
     server. Note that the two shells have different origins and therefore
     separate client-side view preferences.
   - *Stack* table: a `Desktop shell` row (Tauri v2 + WebView2).
   - *Hardening & deployment*: record that the desktop window is an additional
     interactive shell alongside the existing loopback-only production model,
     and that "embed frontend in binary" is explicitly **not** what the desktop
     shell does.
   - *API surface (v1 sketch)*: record the explicit request-size limits (1 MiB
     API-wide, 32 MiB on the import routes) and the `payload_too_large` error
     code, so the contract is documented where the routes are.
   - Risk table: retain the accepted `/api/health` ledger-path exposure risk;
     loopback-only binding limits its audience today, and the mobile work must
     add authentication before lifting that boundary.
2. `README.md`: workspace commands from the repository root; `scripts/start.ps1
   -Desktop` and what it does, including `-Desktop -Demo` and "close the window to
   stop the app"; WebView2 runtime as a prerequisite; the app-data ledger and
   desktop log locations; the `TTTB_CREATE_LEDGER_IF_MISSING` variable and the
   `-InitLedger` switch; the import size limit.
3. `Agents.md`: one Architecture bullet — *the desktop shell consumes the
   backend's composition root; it must not construct its own router, state, or
   ledger path.* (The Workflow and version wording already landed in Phases 1
   and 2, with the changes they describe.)
4. `docs/DecisionLog.md`: **nothing new here.** Every entry landed with its own
   phase; this step only re-reads them against what was actually built and
   corrects any that drifted.
5. Version bumps, one release for the whole feature (intermediate phases are not
   released): the workspace package version takes a **minor** bump from the
   current backend `0.18.0` to `0.19.0`, which covers both Rust crates at once —
   and `tauri.conf.json` carries no version to update, by design;
   `frontend/package.json` takes a minor bump from the current `0.24.0` to
   `0.25.0` plus the matching `frontend/package-lock.json` version fields.
6. Record the CSP value in `docs/Design.HighLevel.md` — it is settled in this
   plan, so this is a copy, not a derivation; only correct it if Phase 2 forced a
   specific directive wider and recorded why. Include the note that the desktop
   response path emits it because Tauri's configured CSP does not cover
   custom-handler responses, and the two frontend properties the value depends on
   (no inline script in the built page; no image asset under the bundler's inline
   limit).
7. Full verification sweep on both stacks.

Verify:
- Workspace and frontend command sequences.
- **External human testing REQUIRED:** launch both shells and confirm the two
  version values in each footer match the bumped manifests, then re-walk the
  Phase 5 and Phase 6 checklists once against the real ledger.

---

## Decision-log entries

> **Each entry is appended when its own phase lands, not batched at the end.** A
> phase that changes governing commands or an API contract while leaving the
> durable record stale is not actually complete. The `Phase` column below says
> when each lands; the wording is drafted here and re-checked against what was
> actually built before it is written.
>
> Follow the log's own rules: **new entries go at the end of the file**, in the
> `## YYYY-MM-DD - <title>` / `Decision:` / `Context:` / `Consequences:` template,
> dated the day they are written. Per `Agents.md`, no entry may reference this
> plan or any phase number — each names the behavior instead. The drafts below
> are already written in that form so they can be pasted and dated.

| # | Entry | Lands with |
|---|---|---|
| 1 | Repository is a Cargo workspace | Phase 1 |
| 2 | Explicit API request-body limits | Phase 2 |
| 3 | In-process desktop transport over one custom URI scheme | Phase 3 |
| 4 | Observable ledger identity, never created by accident | Phase 4 |
| 5 | Desktop process independence from the working directory | Phase 5 |
| 6 | Demo mode in both shells (refines 2026-07-02) | Phase 5 |
| 7 | Per-ledger market-data refresh lease | Phase 6 |

---

**1.**

```
## YYYY-MM-DD - Repository Is A Cargo Workspace
Decision: The repository is a Cargo workspace whose members share one target
directory and one `[workspace.package].version`, inherited by every member crate
and exposed through `/api/health`. Cargo commands run from the repository root.
Routine backend work uses the per-package build and clippy commands; the
whole-workspace clippy command is used when a non-backend member or the workspace
manifest is touched.
Context: A second binary — a native desktop shell reusing the backend crate — is
exactly the "compile-time pressure" trigger the 2026-06-12 Repository Layout
entry named for introducing a workspace, and a separate target tree would cost
several gigabytes. The desktop member links a webview and is slow to build, so
widening every routine check to the workspace would slow the inner loop.
Consequences: This extends the 2026-06-12 Repository Layout entry rather than
reversing it; its other clauses stand. One full rebuild happens when the target
directory moves. Scripts and documentation that referenced the backend directory
for cargo commands or for the backend executable path point at the workspace
root. There is exactly one Rust version number, so no member can drift from
another.
```

**2.**

```
## YYYY-MM-DD - Explicit API Request-Body Limits
Decision: Every API route carries an explicit maximum request body size from one
shared definition, and the CSV import routes carry a higher one because they take
a whole broker export in a single body. Every route returns the project's standard
error envelope, with a code the UI can render, for a request body that is oversized
or malformed — never a bare plain-text framework rejection. This guarantee is
unconditional across the API, which is what requires shared request extractors
rather than the framework's own.
Context: The import routes were silently governed by a framework default that was
never chosen for them, so a large enough export would fail with an unreadable
error in the browser. An implicit default is also untestable without asserting on
framework behavior.
Consequences: The general limit stays low because the API is deliberately
unauthenticated (2026-06-12 Phase 0 Planning Decisions) and, today,
loopback-only, so only the import path is raised. Import bodies remain fully
buffered, which the limit makes bounded; a streaming parser would be a separate
change. Request-size behavior is now part of the documented API contract.
Malformed request bodies also move from
producing no envelope at all — a framework rejection the client could only render
as a generic failure — to carrying specific codes in the standard envelope.
```

**3.**

```
## YYYY-MM-DD - In-Process Desktop Transport Over One Custom URI Scheme
Decision: The desktop shell serves both the built frontend and the API to its
webview through a single registered custom URI scheme whose handler feeds
requests into the same router the web server uses. The desktop process opens no
TCP listener and no port. Because the page and the API share one origin, the
frontend's HTTP client needs no transport branch and the CORS layer is not
widened. Both shells are constructed from one composition root: one router
construction, one state construction, one ledger-path resolution, one shutdown.
The shell also emits its own Content-Security-Policy header, because a policy
configured in the webview framework covers only that framework's own asset
resolution and not responses produced by a registered handler. The policy value is
fixed in the shell's source, permits inline styles but not inline scripts, and
permits `data:` images solely because the application's favicon is an inline SVG.
Context: A native window was wanted without a firewall prompt or a port, while
the unauthenticated and, today, loopback-only web-server deployment had to keep
working unchanged from the same source. Typed per-endpoint commands were
rejected as doubling per-endpoint maintenance
forever; a two-origin split was rejected because it would have required relaxing
the CORS layer that guards the LAN server. The awkward cases — a rejected write
with a JSON error body, an empty 204, and a multi-megabyte upload — were proven
inside the real window before the design was adopted, on recorded platform
versions.
Consequences: The desktop window's origin differs from the browser's, so
client-side view preferences under the 2026-07-10 View settings entry are
per-shell; that entry's promise holds within each shell, not across them. Any
future work that would require the frontend to know which shell it is running in
reopens this decision. A documented fallback exists — one generic
request-forwarding command feeding the same router — and taking it would require
reopening the no-frontend-change commitment. Because the platform stack decides
the behavior, the in-window probe is kept and re-run when the webview or its
framework is upgraded. The content-security policy's value depends on the frontend
shipping no inline script and no image asset small enough for the bundler to
inline; introducing either is what would require the policy to change, and an
inline script would blank the window rather than degrade gracefully.
```

**4.**

```
## YYYY-MM-DD - Observable Ledger Identity, Never Created By Accident
Decision: The database the application opened is resolved once to an absolute
path, reported by `/api/health`, and rendered in the app footer. A missing
database is a startup failure with a visible error, not a newly created empty
file; creating one requires an explicit opt-in. Demo mode resolves to an
in-memory database before any filesystem work and therefore reports no path.
The default database location has a single machine-readable definition shared by
the launch script and the desktop shell.
Context: Two entry points and a renamed database file made "which ledger am I
looking at" a real question, and silently creating an empty database at a wrong
path is the same defect in either entry point. The backend configuration is the
single definition of the app-data default, so the desktop shell must consume it
rather than maintain a second path string.
Consequences: A first run on a fresh checkout, and any run pointed at a database
that does not exist yet, requires the explicit creation step once. The reported
path contains the operating-system user name and is readable by processes that
can reach the loopback health endpoint; this is accepted while binding remains
loopback-only and must be revisited before any remote exposure. This refines the
2026-06-13 Static Frontend Serving entry: only the default static-assets path
remains CWD-relative, while the ledger and logs use app-data conventions. Static
assets are required for the production web server and for every desktop mode,
and optional for the development and demo web servers.
```

**5.**

```
## YYYY-MM-DD - Desktop Process Independence From The Working Directory
Decision: Nothing the desktop process uses is located relative to its working
directory. The ledger and runtime log use the per-user app-data conventions; the
built frontend is the one desktop path resolved from the build-tree anchor. The
process has no console; it uses the bounded, rotating `LogSettings` /
`RotatingFileWriter` path, and a per-line tag may identify the copy that wrote a
line. Failing to open the log never prevents startup; any startup failure
produces a native dialog naming the failure and resolved paths rather than a
window that never appears.
Context: A windowed executable launched from a shortcut has an arbitrary and
possibly unwritable working directory and no standard error stream, so a
working-directory-relative log opened with an unwrap could terminate the process
before anything visible happened. The production logging decision already places
bounded mode-specific logs in app-data; the desktop shell must reuse that
behavior rather than put a runtime log back in the repository.
Runtime file overrides — the database URL, the log file and the backup
directory — must be absolute paths; a relative value is rejected through each
resource's existing failure route rather than resolved against whatever the
working directory happens to be.
Context: A windowed executable launched from a shortcut has an arbitrary and
possibly unwritable working directory and no standard error stream, so a
working-directory-relative log opened with an unwrap could terminate the process
before anything visible happened. The production logging decision already places
bounded mode-specific logs in app-data; the desktop shell must reuse that
behavior rather than put a runtime log back in the repository. Absolute defaults
did not make overrides absolute: a relative override could open a different
existing file depending on where the shortcut started.
Consequences: This refines the 2026-09-07 Runtime Logs entry and the 2026-06-13
Backend Logging Stack entry: logging still goes through the same facade,
initialization is non-fatal, and the desktop file is named for its shell or mode
under app-data. Two instances in the same mode share one log file, so rotation
is coordinated across processes — size is read from the file, rotation runs
under a lock, and a writer that finds its file rotated away reopens the path —
keeping the bounded-rotation contract true with concurrent writers; if instance
tagging is kept, whole-line writes make those records attributable. The desktop
executable is tied to its build tree only for static assets, which is acceptable
while it is not distributed; ledger and logs remain per-user app-data resources.
Launch scripts already pass absolute overrides, so the absolute-path rule
changes nothing for them and only closes the hole for hand-built shortcuts.
```

**6.**

```
## YYYY-MM-DD - Demo Mode Is Available In Every Shell
Decision: Demo mode is selectable for both the web and the desktop shell, and in
demo mode the application resolves an in-memory database before any filesystem
work, reports no database path, skips the launch market-data refresh, and makes
no network calls.
Context: The desktop window is the most presentable surface for a demonstration,
and demo mode must not be able to trip the rules that govern real databases —
refusing to create a missing file, and resolving an absolute path — for a mode
that has no database by design.
Consequences: Refines the 2026-07-02 Demo Mode Is Ephemeral And Read-Only entry,
whose toggle now spans both shells; every other clause of that entry — ephemeral
data, never opening the real ledger file, skipped launch refresh, read-only
pragma, and the rejection of mutating routes — is unchanged. Demo-affecting
changes now have two launch surfaces to check, though they share one flag and one
seeded database.
```

**7.**

```
## YYYY-MM-DD - Market-Data Refresh Uses A Per-Ledger Lease, And The Data Revision Is Persisted
Decision: The claim that a market-data refresh is in progress lives in the
database rather than in process memory, so every process sharing a ledger sees
it and the reported refresh status reflects cross-process reality. The claim is a
lease: its holder heartbeats while it works, a lease whose heartbeat has gone
stale may be taken over by another process, every write to the run record is
conditional on still holding the lease, and every batch of refresh-derived data
— prices, exchange rates and provider mappings — is applied inside one write
transaction that first verifies the lease is still held, so a holder that lost
its lease while waiting on a provider cannot write that provider's result.
Provider calls happen outside those transactions. A holder that discovers it has
lost the lease also stops promptly, but that stopping is a courtesy, not the
guarantee. The data revision that clients use to invalidate their caches is
likewise persisted in the ledger rather than held per process: it is bumped by
mutating requests that do not fail with a client error, by a refresh that
finishes or that fails after writing, by the reclaiming of an abandoned run, and
once at every startup after migrations, and every stamped response and the
version endpoint read it from the ledger.
Context: The previous single-flight guard was per-process, so two processes on
one ledger ran two concurrent provider refreshes, wrote overlapping run records,
and each reported a refresh state blind to the other. A desktop shell alongside
the web server makes that the normal case rather than an edge case. Reclaiming a
stale lease alone is not sufficient: a process that stalls past the timeout is
not dead, and on waking would otherwise finalize or overwrite a run it no longer
owns; and a cancellation flag checked between units cannot catch a unit whose
provider call was already outstanding. The revision entry of 2026-09-12 recorded
that sharing a ledger would need a persisted counter; without it, a change made
through one process left the other process's panels serving stale cached values
while its heartbeat kept succeeding.
Consequences: Refines the 2026-06-16 Market Data Service Injection And
Single-Flight Refresh entry — the guard is now per-ledger rather than
per-process, which is what a second shell and the planned LAN access from a phone
both require — and the 2026-09-12 Data Is Served And Requested By Revision entry,
whose process-local counter becomes a ledger-owned one; the restart-refetches
promise of that entry is kept by the startup bump. A process killed mid-refresh
cannot block refreshes indefinitely. Reading the revision costs one small query
per stamped response and per heartbeat. Correctness here depends on behavior
between independent database connections, so its tests use separate connection
pools over one file rather than a shared in-memory database, and the stale-write
test suspends a holder inside provider I/O rather than between units.
```

## Documents to update

| Document | Change | Phase |
|---|---|---|
| `Agents.md` | **No change for the landed production hardening:** backend builds still run from `backend/`, and the UI/backend version-source clause remains true. When the desktop crate lands, add one Architecture bullet that it consumes the backend composition root and must not build its own router/state/ledger path | desktop landing |
| `README.md` | Workspace commands; `-p` for the Sharesight spike; `start.ps1 -Desktop` (incl. `-Desktop -Demo`, `-ProbeWebView`, and "close the window to stop the app"); WebView2 prerequisite; app-data ledger and desktop log conventions; `TTTB_CREATE_LEDGER_IF_MISSING` / `-InitLedger`; the import size limit | 1, 4, 7 |
| `docs/Design.HighLevel.md` | Deployment model gains the desktop shell; Stack table gains a desktop row; hardening text keeps the desktop shell on disk assets and app-data logs; API surface gains the explicit request-size limits and standard-envelope guarantee; risk row for ledger-path exposure; the settled CSP value and its frontend prerequisites | 7 |
| `docs/DecisionLog.md` | Seven entries, **each appended when its own phase lands**, in the log's `Decision`/`Context`/`Consequences` template, at the end of the file, naming behaviors and never this plan | 1, 2, 3, 4, 5, 5, 6 |
| `scripts/start.ps1` | Workspace executable path and build command; `-Desktop` mode with its build/launch branch, `-ProbeWebView`, and its rejected switch combinations; backend-owned ledger configuration; legacy-name guard; `-InitLedger`. **`scripts/Common.ps1` is deliberately not created** — with one launch script there is nothing to share | 1, 2, 4, 5 |
| `backend/src/api/body_limits.rs` (new) | The two explicit request-size constants plus the derived test-size helpers — the only definitions | 2 |
| `backend/src/api/extract.rs` (new) | `ApiJson<T>` and `ImportBody`, sharing one rejection→`ApiError` mapping, so the `payload_too_large` promise holds on every route | 2 |
| `desktop/build.rs` (new) | `tauri_build::build()` (required); no ledger or log path reader | 2 |
| `desktop/tauri.conf.json` (new) | `identifier` set; **no top-level `version`** (inherits Cargo's); `app.windows: []`; `withGlobalTauri: false`; `app.security.csp: null`; `build.frontendDist` omitted; bundling off | 2 |
| Root `Cargo.toml` (new), `backend/Cargo.toml`, `desktop/Cargo.toml` (new) | Workspace members, explicit `resolver = "2"`, `[workspace.package] version`, member `version.workspace = true`, minor version bump at release | 1, 2, 7 |
| `Cargo.lock` (moved from `backend/Cargo.lock`) | `git mv` to the root **before** the first workspace build; Phase 1 builds `--locked` and the diff is a pure rename; Tauri's dependency additions arrive and are reviewed in Phase 2 | 1, 2 |
| `backend/src/config.rs`, `backend/src/ledger/location.rs` | Relative `TTTB_DATABASE_URL` filename, `TTTB_LOG_FILE` and `TTTB_BACKUP_DIR` rejected through each resource's existing failure route; shell-level asset-policy override so every desktop mode requires assets | 3, 4 |
| `backend/src/engine_logging.rs` | Cross-process-safe rotation: live file length, lock-guarded rename ladder, reopen after a foreign rotation; the instance tag rides the existing line buffer | 5 |
| `backend/src/data_revision.rs`, `backend/src/db/` | Persisted one-row revision counter replacing the process-local one; `fenced_write` and executor-taking write helpers for refresh-derived batches | 6 |
| `frontend/package.json` (+ `package-lock.json`) | Minor version bump at release. **No new dependency** — the frontend gains no Tauri package | 7 |
| `.gitignore` | Verify the unanchored build-output entries still cover the relocated Cargo target. No ledger-pattern change: live app-data ledgers are outside the repository, and the retired repository ledger is already under ignored `.local/` | 1 |
| `docs/VisualDesign.DarkTheme.md` | No change expected — the footer ledger label reuses existing footer styling and no new chip is added. The document has no footer-specific rules; its chip guidance is in *Badges / chips* and its density section is about tables. If implementation needs a new token, that is a document change to add here, not a silent addition | — |

## Deliberately out of scope (recorded, not overlooked)

- **MSI/installer, icons, branding, code signing.** The user is not distributing
  the app. This is what licenses the build-time path anchor: the executable is
  deliberately tied to its build tree. If distribution ever happens, the anchor
  must become an installed-location or per-user app-data resolution, and the
  shared-ledger decision must be revisited at the same time.
- **OS integration:** tray icon, native menus, notifications, file-drop CSV
  import, single-instance enforcement. Two instances may run; the plan makes that
  safe (Phase 6) rather than preventing it.
- **Any change to `frontend/src/api/client.ts` or to the CORS layer.**
- **Multi-portfolio support or a first-run ledger picker.** The production
  ledger already lives at the per-user app-data location; the desktop shell
  consumes that shared configuration rather than reopening that decision.
- **Streaming or chunked import upload.** The import handlers buffer the whole
  body; the 32 MiB limit makes that bounded rather than unbounded. A streaming
  parser would remove the ceiling entirely and is a separate piece of work with no
  current motivation.
- **Additional backup/restore behavior.** Launch-time backups, retention and the
  verified restore drill are already part of the web application's production
  run model; this plan must preserve them while adding the desktop shell.
- **The `AddInstrumentDialog` API path literal.** Noted, unchanged; it routes
  through `apiGet`, so the transport is unaffected.

## Risks and accepted consequences

- **WebView2's custom-protocol handling is the plan's central unknown.** Mitigated
  by making it the first behavioral phase and by proving it **inside the real
  window** with the probe, not with adapter tests that never touch WebView2 — the
  distinction the review caught and the reason the probe exists. A documented
  fallback has a stated trigger and a stated cost; if it is needed, the settled
  no-frontend-change decision must be reopened with the user.
- **Three Tauri/Wry/WebView2 behaviors are assumptions, not verified facts** (see
  *Platform claims this plan takes as given*). Each has a settling mechanism in
  the probe, and the design is written so that two of the three do not change the
  implementation whichever way they resolve.
- **The probe is a maintenance obligation.** It exists because the decisive
  behavior lives in a platform stack we do not control, which means it must be
  re-run after a Tauri, Wry or WebView2 upgrade. A probe nobody re-runs is worth
  little; the decision-log entry says so explicitly.
- **The probe tests two cases through substitute routes**, because the demo-mode
  read-only layer makes the real ones unreachable. The substitution is sound for
  the transport property it proves, and
  `probe_limited_route_matches_real_import_route` keeps the substitute honest —
  but it is a place where the gate could drift away from reality if that test is
  ever deleted. Recorded so the coupling is visible.
- **One full rebuild** when the target directory moves. One-off, unavoidable,
  budgeted.
- **The desktop app's static assets are tied to its build tree.** Moving or
  deleting the repo folder breaks the shortcut's asset path. Accepted because
  distribution is out of scope; the failure is visible (native dialog naming the
  missing path), not silent. The ledger and logs remain in app-data.
- **Dev runs and desktop runs share one ledger.** Intended. Phase 6 makes
  concurrent refresh safe; concurrent *writes* are still two processes on one
  SQLite file, protected only by SQLite's own locking. A write collision surfaces
  as a database-busy error, not corruption, but the app has no retry policy for
  it today. Recorded, not fixed here.
- **The refresh-lease phase is the most likely to overrun.** It is committed
  work, so overrunning means it takes longer, not that it gets dropped. The
  things that make it larger than it looks are the ownership fence inside every
  refresh write, the persisted data revision, the ownership rules for a
  reclaimed-then-resumed process, and the move from in-memory-pool tests to
  two-pools-on-one-file tests with a provider that can be parked mid-call; all
  are specified up front for that reason.
- **The ledger path on `/api/health` discloses the Windows user name.** This is
  accepted while binding is loopback-only; revisit it before the mobile work
  lifts that boundary and adds authentication. The production run model records
  the same risk; do not duplicate its mitigation here.
- **`localStorage` view preferences do not carry between desktop and browser.**
  Inherent to a second origin; accepted; recorded against the 2026-07-10 entry so
  it is not later filed as a bug.
- **The shared log file interleaves when two copies run at once.** Accepted, and
  chosen: one timeline is what you want when the two copies are the problem.
  Whole-line writes bound the interleaving to line granularity and the per-line
  tag keeps every line attributable, so the accepted cost is "lines from two
  copies alternate", not "output is corrupted". **Rotation is the part that is
  not free:** two writers with private byte counts and open handles would
  rotate each other's files, so the writer reads the live length, rotates under
  a lock file and follows a foreign rotation. That is a small change to the
  landed writer, and it is tested on real files, not a shared buffer.
- **Refresh-derived writes cost one short write transaction per batch.** The
  ownership fence turns each unit's row-by-row upserts into a single
  `BEGIN IMMEDIATE` batch, which is also fewer fsyncs than today. The cost is
  that the write helpers take an executor, and that the FX and price loops must
  not hold the transaction open across a provider call — which the design
  forbids by construction.
- **The persisted revision is one query per stamped response and per
  heartbeat.** Trivial on a local SQLite file; recorded so nobody later caches
  it in memory "for speed" and quietly reintroduces the cross-process staleness
  it exists to fix.
- **The CSP value depends on two properties of the frontend** — no inline script
  in the built `index.html`, and no image asset small enough for Vite to inline.
  Both are true today and both are easy to break accidentally. An inline script is
  the dangerous one: `script-src 'self'` would blank the window, and neither the
  adapter test nor the probe would catch it, because the probe serves its own
  page. The ordinary-app human check is the net.
- **Refuse-to-create is a behavior change to the web server**, not only the
  desktop app. A fresh clone needs one `-InitLedger` run before the web app
  starts; the retired `-ProductionDb` flag is not an alternate initialization
  path.
- **The desktop crate is slow to build** (WebView2 linking). Mitigated by keeping
  the default clippy/build commands per-package.
- **`windows-sys` is a new dependency** for one `MessageBoxW` call. Small and
  Windows-only, which the app already is; `rfd` is the alternative if a
  cross-platform dialog is ever wanted.
- **The body-limit change lands in the web app too.** Raising import to 32 MiB
  and adding a 1 MiB API-wide limit changes browser-build behavior, so a mistake
  here is a web regression rather than a desktop one. Mitigated by making it a
  self-contained step verifiable on the bare router before any desktop code
  exists, plus a human import check on the web app in the same phase.
- **The `ApiJson` rollout touches seven handlers this plan otherwise never
  opens.** Accepted as the price of an unconditional envelope guarantee: a
  guarantee true of four routes and false of the rest is worse than none, because
  the frontend would still have to handle both shapes. The change is mechanical
  and compiler-guided, and it only alters *rejection* paths — no successful
  response shape changes.
- **`start.ps1` grows a second mode.** Two modes in one script is more branching
  than one mode, and the reject-combination list is something a future switch has
  to be added to. Accepted because the alternative was two scripts that both had
  to know about ledger resolution, the legacy-name guard and the demo wiring —
  the bigger duplication of the two. The branch is narrow: everything up to the
  build step is shared, and only the launch half differs.
- **Ctrl+C in the console hard-kills the desktop process.** The existing
  `Stop-ProcessTree` `finally` skips the graceful pool close. SQLite recovers via
  WAL, so this is safe rather than dangerous, but closing the window is the
  intended stop and the script's help says so.
- **The desktop static-assets path is build-tree anchored.** A shortcut launched
  after the repository moves may fail to find `frontend/dist`; the native failure
  dialog names the missing path. The ledger and log have no second build-tree
  definition and are resolved by the app-data conventions.
- **Two ways to reach demo mode now exist** (`-Demo` and `-Desktop -Demo`), so
  any future demo-affecting change has two surfaces to check. Accepted: they share
  one config flag and one seeded pool, and the difference is only which shell
  renders it.

## Open Questions

**None remain.** All three that this plan carried through review — the
Content-Security-Policy value, the log-file layout, and the breadth of the
`ApiJson` rollout — are settled and recorded below.

That is not a claim that nothing is uncertain. Two kinds of residual unknown
exist, and both live in sections of their own because they are not decisions
anyone needs to make:

- **Three Tauri / Wry / WebView2 behaviors are assumptions**, recorded in
  *Platform claims this plan takes as given*, each with a mechanism that settles
  it empirically during the skeleton phase. They are questions for the platform,
  not for the user.
- **Two build-toolchain contingencies have pre-decided fallbacks**: if
  `tauri-build` demands an icon, add a placeholder and record it as a build
  artifact; if a null `build.frontendDist` is rejected, use a URL value, never a
  real directory. Both are stated where they arise.

If implementation turns up a genuine choice, add it here rather than resolving it
silently — `Agents.md` requires that, and an empty section is only worth having
while it is true.

## Resolved during planning (recorded so they are not reopened)

- **Ledger file name:** `%LOCALAPPDATA%\\TickerTapeTallyBoard\\portfolio.sqlite`.
  Confirmed by the user; the desktop process consumes the backend's app-data
  default rather than defining a repository-local path.
- **Launch surface:** `scripts/start.ps1 -Desktop`, not a second script; and
  `scripts/Common.ps1` is consequently not created.
- **Demo mode:** reachable from both shells, with an in-memory ledger resolved
  before any filesystem work so the ledger rules cannot fire.
- **Import body size:** raised to an explicit 32 MiB, with an explicit 1 MiB
  API-wide limit and a `payload_too_large` error envelope.
- **No `DESKTOP` footer chip**, and therefore no `shell` field on `/api/health`.
  A native window is self-evidently the desktop app; the footer's new information
  is the ledger.
- **Refuse-to-create applies to both entry points.** No fallback retained.
- **Cross-process refresh coordination is committed work**, last in the ordering
  but not optional; the closing documentation assumes it landed.
- **The walking skeleton's gate is an in-window probe**, not adapter tests.
  Adapter tests remain as the regression net.
- **The probe runs in demo mode and uses probe-only routes** for the 204 and
  oversized-body cases, because the demo read-only layer rejects every non-GET
  before the handler. A non-demo probe against a seeded temporary ledger was
  considered and rejected: it would make a red case ambiguous between a platform
  failure and a seeding failure, and give the probe filesystem state to manage.
- **The probe proves transport fidelity; the bare-router and adapter tests prove
  routing and limits.** The plan no longer claims the probe proves the second.
- **The Content-Security-Policy value is settled directive by directive**, on
  evidence from the built bundle: `font-src 'self'` with no `data:` (the five
  imported woff2 files are 21–25 KB, far above Vite's 4096-byte inline limit);
  `img-src 'self' data:` because the favicon at `frontend/index.html:8` is an
  inline SVG and is the only genuine `data:` URI in `dist/`; `script-src 'self'`
  with no `'unsafe-inline'` because the page has no inline script; no `worker-src`
  or `blob:` because the frontend uses neither. Moving the favicon to a file to
  drop `data:` was considered and rejected as a frontend change outside this
  plan's scope.
- **Bounded rotating logs under app-data, named for the running shell or mode.**
  The desktop process reuses `LogSettings` and `RotatingFileWriter`, so its log
  is outside the repository and remains attributable without requiring a
  second logging abstraction. If instance tags are retained, they are an
  extension of that shared writer rather than a separate file-location rule.
- **The `ApiJson` rollout is confirmed at all seven handler signatures.** Every
  route returns the standard envelope for oversized and malformed bodies; the
  import-only variant is dropped, not held in reserve.
