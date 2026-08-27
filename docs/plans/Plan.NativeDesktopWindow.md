# Plan — TickerTapeTallyBoard as a native Windows window

## Summary

Give the app its own Windows application window (Tauri v2 + WebView2) in which the
UI reaches Rust **in-process** — no TCP listener, no port, no firewall prompt —
while the existing web-server deployment keeps working unchanged from the same
source.

Done means: launching the desktop app shows the real portfolio in a native
window; every page and every `/api/*` route behaves identically to the browser
build, proven by a probe that runs **inside the real WebView2 window** and by
adapter-level Rust tests underneath it (not by a route count);
`scripts/start.ps1` still runs the web app exactly as it does today; the ledger
the window opens is named and visible in the UI, and the app refuses to start
rather than silently creating an empty one; and a market-data refresh is
coordinated across every process on that ledger.

**Out of scope:** MSI/installer, icons, branding, code signing; tray icon, native
menus, notifications, file-drop import, single-instance UI; multi-portfolio
support, a ledger picker, a per-user app-data ledger location. See *Deliberately
out of scope*.

## Landing order — read this before editing anything

This plan was written expecting `docs/plans/Plan.NasdaqNordicPriceProvider.md` to
land first. **As of writing, it has not** — the backend is still `0.14.2`, the
frontend `0.22.11`, migrations stop at `0006`, and no Nasdaq provider exists. So
either order is possible and the plan is written to survive both.

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
- Version bumps are stated **relative to whatever is current when this lands** —
  `0.14.2` / `0.22.11` today, `0.15.x` / `0.23.x` if the Nasdaq work lands
  first — never as absolute values.
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
8. **One shared ledger.** The desktop app opens the same database file
   `scripts/start.ps1` opens. Accepted: the app is tied to its build tree, and
   dev runs and desktop runs share data.
9. **That file is renamed** away from a name containing "test" (it holds the real
   portfolio and is at risk from any future cleanup of test artifacts), with a
   safe migration for the `-wal`/`-shm` sidecars.
10. **A missing ledger is a hard startup failure, not an empty new file — in
    *both* entry points.** Creating one requires an explicit opt-in
    (`TTTB_CREATE_LEDGER_IF_MISSING=1`, surfaced as `scripts/start.ps1
    -InitLedger`). Two consequences a reader will hit and which the README must
    state plainly: a **fresh clone** needs one `-InitLedger` run before the web
    app starts, and **`-ProductionDb` on this machine** needs one too, because
    its (now corrected) default path has never existed.
11. **The resolved ledger path is observable** through `/api/health` and rendered
    in the app footer. This deliberately touches the frontend; it is a separate
    feature, not a transport change. **No shell/`DESKTOP` badge is added** —
    a native window is self-evidently the desktop app, and a third badge would
    crowd the one piece of information that is genuinely not otherwise visible.
    `/api/health` therefore gains `ledger` and nothing else; which shell is
    running is recorded in the log banner, where it is actually needed.
12. **The repository becomes a Cargo workspace** with `backend/` and the new
    `desktop/` crate as members, sharing one target directory.
13. **Nothing in the desktop process is located relative to the working
    directory** — not the log file, not the ledger, not the static assets
    directory. One rule, not three fixes.
14. **No console window** (`windows_subsystem = "windows"`), file logging at one
    fixed known location, failure to open the log never prevents startup, and a
    startup failure produces a **native dialog** rather than a window that never
    appears.
15. **Cross-process refresh coordination is committed work.** It is the last
    phase because it is independent of the window, not because it is optional.
    The closing documentation and verification assume it landed.
16. **Decision-log entries are written as each phase lands, not batched at the
    end**, and are rendered in the log's own `Decision`/`Context`/`Consequences`
    template. See *Decision-log entries*.
17. **The renamed ledger is `.local/db/tttb-portfolio.sqlite`.**
    `tttb-ledger.sqlite` is deliberately not reused — it is the existing
    working-directory-relative `DEFAULT_DATABASE_URL` and would be ambiguous
    between the two shells.
18. **The desktop app launches through `scripts/start.ps1 -Desktop`, not a second
    script.** One launch script owns ledger resolution, the legacy-name guard,
    environment save/restore and build orchestration for both shells.
    **`scripts/Common.ps1` is not created:** the only reason to extract it was to
    share those pieces between two launch scripts, and with one script the DRY
    pressure disappears. Extracting helpers for hypothetical future consumers
    (`probe-connectivity.ps1`, `project-stats.ps1`) would be speculative and is
    not this plan's work.
19. **Demo mode is reachable from both shells.** `scripts/start.ps1 -Desktop -Demo`
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
21. **The default ledger path has one machine-readable definition** that both
    `scripts/start.ps1` and the desktop crate read, rather than the same string
    typed into PowerShell and Rust.
22. **Static assets are required for the desktop shell and optional for the
    server.** An explicit policy, not an accident of which router got built.
23. **The desktop response path emits its own Content-Security-Policy header**,
    and `tauri.conf.json` sets `csp: null`, so there is exactly one policy source
    and it is on the path that actually serves the HTML.
24. **The policy value is settled, directive by directive, on evidence from the
    built bundle** — not left to be discovered at implementation time. See
    *Content-Security-Policy is emitted by the bridge* for the value and the
    reason each directive is what it is.
25. **One log file, one fixed known location, and every line carries the identity
    of the copy that wrote it.** Per-process files were rejected: a problem caused
    by two copies interfering is exactly the problem you want to read as a single
    interleaved timeline, and one fixed location is worth more than clean
    separation. The tag must be on **every line**, not only in a startup banner,
    or the interleaved output is unreadable in precisely the case it exists for.

### Refinements this plan makes to the brief (flagged, not silent)

- **`tower` is promoted in the *desktop* crate, not the backend.** The brief
  expected `backend/Cargo.toml`'s dev-only `tower` to become a real dependency.
  The bridge belongs in `desktop/` — the backend has no business knowing about
  webviews — so `tower = { version = "0.5", features = ["util"] }` becomes a
  normal dependency of the desktop crate and the backend's stays dev-only. The
  backend's public surface already exposes everything the bridge and its tests
  need (`api::router_with_static_assets`, `state::AppState::for_tests`,
  `AppState::with_demo_mode`, `db::testing::memory_pool`,
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
  agreeing with us about what a valid export looks like.
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
| 6 | `fetch("https://example.invalid/")` | **rejects** — `connect-src 'self'` is enforced. Combined with the page's no-inline-script requirement, this is the CSP enforcement check | CSP live |
| 7 | URI form | the page reports `location.origin`; the handler reports the URI string it received; both are recorded verbatim | settles the Wry rewrite question |

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
    bound covering every case plus slack (proposed 300 s). It writes a report
    marked `incomplete` naming which cases never reported, logs it through
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

**What would trigger the switch:** any red case in the probe report, or a report
marked `incomplete` — a non-2xx
response body swallowed or replaced by a browser error page; a 204 arriving with
a synthesized body or failing to resolve; a request body truncated, re-encoded,
or not delivered to the handler; the handler unable to see the request method; or
a large body that only completes on a timescale that makes import unusable. An
adapter-test failure is a bug in our code and is simply fixed; only a probe
failure is evidence about the platform.

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
- `scripts/start.ps1` hardcodes
  `$BackendExe = Join-Path $BackendDir "target/debug/ticker-tape-tally-board-backend.exe"`
  and **throws** if it is missing. It must point at the workspace target.
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

`AppConfig` carries it: the server sets `Optional` (behavior unchanged), the
desktop entry point sets `Required`. `Required` checks for
`<static_assets_dir>/index.html`, not merely the directory — a stale or empty
`dist/` would otherwise pass the check and then serve nothing — and returns
`StartupError::StaticAssetsMissing { path }`, which produces the dialog and no
window. Both branches are tested.

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
- **Enforcement check (in-window probe):** the harness page carries no inline
  script — if the policy is live and `script-src 'self'` holds, an inline-script
  page would not run at all — and the cross-origin `fetch` case asserts
  `connect-src 'self'` is enforced.

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

The desktop process resolves everything against a **build-tree anchor baked at
compile time**:

```rust
// desktop/src/app_paths.rs
/// Absolute path baked at build time. The desktop app is deliberately tied to
/// its build tree; no installer is in scope (see Deliberately out of scope).
const DESKTOP_CRATE_DIR: &str = env!("CARGO_MANIFEST_DIR"); // <repo>/desktop

pub struct AppPaths {
    pub ledger_url: String,        // sqlite://<abs>/.local/db/<ledger>.sqlite
    pub static_assets_dir: PathBuf, // <abs>/frontend/dist
    pub log_file: PathBuf,          // <abs>/.local/logs/desktop.log
}
```

`TTTB_DATABASE_URL` and `TTTB_STATIC_DIR` still override (needed for the
copy-of-the-real-database drills), but a **relative** override is resolved
against the build-tree anchor, never against the CWD. `AppPaths` is pure and
unit-tested against a supplied anchor rather than reading the real environment
in tests.

The **server** entry point keeps its current CWD-relative semantics
(`../frontend/dist`, `engine.log`, `sqlite://tttb-ledger.sqlite`) — the rule is
scoped to the desktop process by design, because the server is always started by
a script that sets its working directory, and changing it would be an unrelated
behavior change. Stated here so a later reader does not read it as an oversight.

### Ledger identity is observable, and never silently created

Three separate obligations, one mechanism.

**Rename.** `.local/db/tttb-ledger-test.sqlite` → `.local/db/tttb-portfolio.sqlite`.
`tttb-ledger.sqlite` is deliberately *not* reused because it is the existing
CWD-relative `DEFAULT_DATABASE_URL` and would be ambiguous. Migration is a human
step (both processes stopped, then move `.sqlite`, `.sqlite-wal` and
`.sqlite-shm` together). `scripts/start.ps1` gains a guard: if the legacy name
exists and the new one does not, it **fails with the exact command to run**
rather than starting on a new empty file.

**One resolution path.** A new `backend/src/db/ledger_location.rs`:

```rust
pub struct LedgerLocation { url: String, path: Option<PathBuf> } // None for sqlite::memory:
pub enum LedgerLocationError { NotAFile, UnsupportedUrl, RelativeWithoutAnchor }

pub fn resolve(url: &str, anchor: &Path) -> Result<LedgerLocation, LedgerLocationError>;
```

`AppConfig` stores a `LedgerLocation` instead of a bare `String`;
`AppConfig::from_env()` anchors at the CWD (unchanged server behavior) and a new
`AppConfig::from_env_anchored(anchor)` serves the desktop. `database_url()` keeps
its `&str` signature; `ledger_path()` is added. `db::connect` takes the location
plus an explicit `CreateMissing::{Yes, No}`. While in the file, move the stray
`memory_pool` and `RepoError` out of `db/mod.rs` so it becomes the thin wrapper
`Agents.md` asks for.

**Refuse to create.** `create_ledger_if_missing` defaults to `false` for both
entry points; `TTTB_CREATE_LEDGER_IF_MISSING=1` (and
`scripts/start.ps1 -InitLedger`) opts in. The default configuration exercises the
new refuse path, per the repo's rule that new behavior behind a flag must default
to the new path.

**One definition of the default ledger path, read by both launch paths.**
`scripts/start.ps1` sets `TTTB_DATABASE_URL` explicitly, so it decides when it
launches the app; the desktop crate's build-tree default decides for a
**shortcut** launch, which is the whole point of the feature. If those two ever
disagree the app quietly opens two ledgers that look like one — a data-integrity
failure, not a cosmetic one — so a comment pointing at the other definition and a
one-time human comparison are not enough. There is one machine-readable source:

```jsonc
// repo-paths.json, at the repository root
{
  "defaultLedgerRelativePath": ".local/db/tttb-portfolio.sqlite",
  "desktopLogRelativePath":    ".local/logs/desktop.log"
}
```

- `scripts/start.ps1` reads it with `ConvertFrom-Json` and derives
  `$DefaultLocalDatabasePath` from it. The literal path string is deleted from the
  script.
- `desktop/build.rs` reads it, validates it, and emits
  `cargo:rustc-env=TTTB_DEFAULT_LEDGER_RELPATH` / `TTTB_DESKTOP_LOG_RELPATH`,
  which `app_paths.rs` consumes through `env!(...)`. A missing or malformed file
  is a **build failure**, not a runtime surprise, and a change to the file
  triggers a rebuild via `cargo:rerun-if-changed`.

Both readers resolve the relative path against the repository root, so they
cannot produce different absolute paths from the same input. The Phase 5 human
comparison of the two footer paths stays as a cheap confirmation, but it is no
longer the mechanism.

The **backend** does not read this file: its own default stays
working-directory-relative and `scripts/start.ps1` always sets
`TTTB_DATABASE_URL` for the server, so adding a third reader would be machinery
without a purpose.

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

**Observable.** `AppState` gains `ledger_path: Option<PathBuf>` and
`shell: AppShell { Server, Desktop }`. `/api/health` grows exactly one field:

```json
{ "ledger": { "mode": "file" | "memory", "path": "C:/…/tttb-portfolio.sqlite" | null } }
```

**`shell` is deliberately not on the wire.** An earlier draft exposed it to drive
a `DESKTOP` footer chip; the user settled that there is no such chip — a native
window is self-evidently the desktop app, and a third badge would crowd the
ledger field, which is the one thing genuinely not otherwise visible. With
nothing rendering it, putting `shell` in the health response would be an
unconsumed wire field. `AppShell` still exists on `AppState`, because the
**startup and shutdown log banners** name it, and that is where "which process
wrote this line" actually matters.

Demo mode reports `{"mode":"memory","path":null}` in **both** shells, so a
presentation screenshot never shows a local path — and the desktop demo window is
therefore a first-class presentation surface rather than a window that leaks the
developer's file layout.

**Is a filesystem path acceptable over the LAN-exposed `/api/health`? Decided:
yes, expose the full path.** Rationale: the LAN deployment is trusted without
authentication by the 2026-06-12 *Phase 0 Planning Decisions* commitment, so
anyone who can read `/api/health` can already read the entire portfolio through
`/api/transactions` — a path is strictly less sensitive than the data it points
at. The one genuine incremental leak is the Windows user name embedded in the
path; that is recorded in the risk table and in the decision-log entry rather
than mitigated, because truncating the path would defeat the feature's purpose
("which ledger am I looking at" must never be a guess). If remote exposure is
ever added, this field belongs behind the same auth gate as everything else.

### Startup failure, logging, and shutdown

**Logging.** `engine_logging::initialize()` currently opens `engine.log` relative
to the CWD with `.expect("Failed to open engine.log")` and installs a
`TermLogger` on stderr — both fatal for a GUI process. It becomes:

```rust
pub enum LogDestination {
    TerminalAndFile { path: PathBuf },  // server entry point
    FileOnly { path: PathBuf },         // desktop entry point: no stderr exists
}
pub struct LogInitOutcome { pub file_path: Option<PathBuf>, pub file_error: Option<String> }

/// `instance_tag` identifies the copy that wrote a line, e.g. "desktop:12345".
pub fn initialize(destination: LogDestination, instance_tag: &str) -> LogInitOutcome;
```

Failure to open the file **never panics**: the outcome carries the error, the
desktop launch path folds it into the startup dialog if a later failure occurs,
and the app starts regardless. `Agents.md`'s rule that backend code logs through
`engine_logging` is unchanged and applies to the desktop crate too.

**One fixed log file, and every line says who wrote it.** Two copies may run at
once. The choice is a single fixed known location (`.local/logs/desktop.log`)
rather than per-process files: a problem *caused* by two copies interfering is
exactly the problem you want to read as one interleaved timeline, and a fixed
location you can always point someone at is worth more than clean separation.

**The tag goes on every line, not only in a banner.** A banner identifies a
session; it does nothing for the fifty interleaved lines that follow, which is the
situation the shared file exists to make readable. Concretely:

- `engine_logging` gains a small line-buffering writer that wraps the log file:
  it accumulates bytes until a newline, then emits `[<instance_tag>] ` followed by
  the completed line in **one** `write_all`. Prefixing each raw `write` call would
  corrupt output, because `simplelog` emits a record through several writes.
- Writing whole lines in single appends is also what *bounds* the interleaving:
  on a Windows append-mode handle a single write does not split, so two copies can
  interleave whole lines but not fragments of a line.
- `instance_tag` is `<shell>:<pid>` — `desktop:12345`, `server:9876`. The shell is
  included even though today each shell has its own file, so that a future shared
  file needs no change.
- The tagged writer is applied to the **file** sink in both `LogDestination`
  variants, not only the desktop one. Consistency costs nothing, and it makes
  `engine.log` attributable if two servers are ever run. This changes
  `engine.log`'s line format; nothing parses it, so that is acceptable, but it is
  a deliberate change rather than a side effect.
- The **terminal** sink is not tagged: a console belongs to exactly one process by
  construction, so the prefix would be noise.

The startup and shutdown banners stay, and carry what a per-line tag cannot: the
executable path, resolved ledger path, static assets directory and log path. The
banner renders an absent ledger path as `in-memory (demo)`, never as an empty
field — a `None` path is a normal state, not a formatting accident.

**Accepted consequence:** two copies writing at once produce an interleaved file.
Line-granular interleaving is the intended reading experience; it is not a
degradation to be fixed later.

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

**Nothing in the composition root or the shutdown path may assume a file-backed
pool.** Checked explicitly for this plan: `Application::build` already branches
on `demo_mode` to build the seeded in-memory pool and skip the launch refresh,
and `shutdown()` only aborts the (absent) refresh task and closes the pool, which
is correct for a single-connection in-memory database — it simply ceases to
exist. There is no WAL checkpoint, no path-based cleanup and no unconditional
path formatting outside the banner fixed above. Pinned by a test that builds and
shuts down a demo `Application` with no filesystem access.

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
  callers (`status()` and `running_response()` today). The in-memory
  `AtomicBool`/`Mutex` pair is **deleted**, not kept alongside — one source of
  truth.
- `status()`'s `refreshing` and `latest_run` therefore reflect *any* process's
  run, and a second window shows the first window's refresh.

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
3. **Losing the lease stops the work.** The refresh loop checks the flag at each
   natural checkpoint — between instruments and between providers — and on loss
   stops immediately, writes nothing further (no prices, no FX, no run row), and
   returns `RefreshRunStatus::Failed` with message `lease_lost`. Stopping between
   units rather than mid-write means no partial row is left behind.

This phase is **committed work, not optional.** It is last because it is
independent of the window, not because it can be dropped; the closing
documentation and verification assume it landed.

### Frontend — exactly two touches

Everything else in `frontend/` is untouched, including `api/client.ts`.

1. `frontend/src/api/types.ts`: `HealthResponse` gains
   `ledger: { mode: "file" | "memory"; path: string | null }`. Nothing else — no
   `shell` field, because nothing renders one.
2. `frontend/src/components/AppFooter.tsx`: renders the ledger identity next to
   the existing UI/API version spans. **No new chip**; the existing `DEMO` chip is
   the only badge. The footer shows the **file name** with the full path in a
   `title` tooltip, so it reuses the existing footer styling and needs no new
   tokens. (`docs/VisualDesign.DarkTheme.md` has **no footer-specific rules** — its
   "Density" section is explicitly about tables, and its chip guidance lives in
   the *Badges / chips* notes, which the existing `DEMO` chip already follows. An
   earlier draft claimed the footer "stays within the dark theme's density rules";
   that cited a rule that does not apply. The accurate claim is the narrower one:
   nothing new is introduced.) Label derivation (name-from-path, memory/demo
   wording) is a pure exported helper with Vitest coverage, not inline JSX logic —
   the `state -> render` analogue of pure reducers.

   **The null-path case is the normal demo case, not an edge case.** With demo
   mode reachable from both shells, `path: null` renders as `In-memory demo` with
   no tooltip — never an empty span, never `undefined`, never a stray separator.
   In a demo launch the footer therefore reads
   `UI x.y.z · API ok x.y.z · DEMO · In-memory demo`, in either shell. A pending
   or failed `/api/health` query leaves the ledger span out entirely rather than
   rendering a placeholder, matching how `apiStatusLabel` already handles that
   state.

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

**Step 0 — re-verify the plan's assumptions against the current tree.** Do this
before editing and write the findings into the implementation notes:

- Does `backend/src/app.rs` still interleave state construction, launch refresh,
  router selection, bind and ctrl-c in one `serve`? It did when this plan was
  written, and nothing else planned touches that structure, so this is a
  confirmation rather than a real fork.
- Are `api::router`, `api::router_with_static_assets`, `AppState::for_tests`,
  `AppState::with_demo_mode`, `db::testing::memory_pool` and
  `providers::FakePriceProvider` still `pub` and unconditionally compiled? The
  bridge tests depend on all six from outside the crate.
- Is `AppConfig`'s ledger still a plain `String` URL read from
  `TTTB_DATABASE_URL`, and does `db::connect` still use `create_if_missing(true)`?
- Do the import handlers still take `Bytes` with no `DefaultBodyLimit` layer?
- What is the highest migration number, and what are the current
  `backend/Cargo.toml` and `frontend/package.json` versions?
- Does `refresh.rs` still hold the in-memory `AtomicBool` + `Mutex` pair and read
  `refreshing` from it?

**Step 1 — workspace.**

1. Root `Cargo.toml` with `[workspace] resolver = "3"` (or `"2"`, matching the
   edition in use), `members = ["backend"]` for now, and `[workspace.package]`
   carrying `version` and `edition`. `backend/Cargo.toml` switches to
   `version.workspace = true` / `edition.workspace = true`, keeping the number it
   currently has.
2. `.gitignore`: verify it still covers the relocated `target/` (its `target` and
   `debug` entries are unanchored, so it should — confirm, do not assume) and that
   no stale `backend/target` remains after the move. **Also unanchor the ledger
   pattern:** `backend/tttb-ledger.sqlite*` becomes `tttb-ledger.sqlite*`, so a
   stray database created anywhere in the tree is ignored rather than sitting one
   `git add -A` away from a commit. This is a one-line structural fix for a hazard
   the Phase 2 manual verification would otherwise rely on a human remembering.
3. `scripts/start.ps1`: `$BackendExe` resolves to
   `<repo>/target/debug/ticker-tape-tally-board-backend.exe`; the build step runs
   `cargo build -p ticker-tape-tally-board-backend` from `$RepoRoot`. Its "Run
   without -SkipBuild first" error message stays accurate.
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
- Backend command sequence from the repository root; whole suite green.
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
   most likely to be forgotten. (It gains its second job, emitting the shared
   repository paths, in Phase 5.)
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
- `bridge_routes_every_request_uri_form` — the same request expressed as
  `tttb://localhost/api/health`, `http://tttb.localhost/api/health` and
  `/api/health` all reach the health handler identically. This is what makes the
  bridge independent of which form Wry actually delivers.
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

  In this phase the skeleton still uses today's configuration, which defaults to
  `sqlite://tttb-ledger.sqlite` **resolved against the working directory** and
  still creates it if missing. Phase 4 is what makes both of those impossible;
  until then the tester is the only guard. So, in order:

  1. Copy the real ledger to a scratch path, e.g.
     `.local/db/tttb-probe-copy.sqlite`.
  2. Set `TTTB_DATABASE_URL` to **that copy** before launching. Do not launch
     without it: an unset variable silently creates an empty database wherever the
     executable was started from. (Phase 1 unanchors the `.gitignore` pattern so
     such a file cannot be committed, but it will still exist and still be the
     wrong ledger.)
  3. Launch the desktop app without the probe flag and confirm in the window: the
     dashboard renders with real data; a deep link/reload of `/board` and
     `/asset/:id` still renders (the router side is already covered by
     `static_router_uses_index_fallback_for_frontend_routes`); a real CSV import
     preview on the Import page succeeds; **deleting a transaction (204) succeeds
     and the table refreshes — against the copy, never the real portfolio**; fonts,
     Lightweight Charts and the treemap all render under the CSP; and DevTools'
     network panel shows the requests on the custom scheme with no CORS preflight.
  4. Confirm **no listener exists**: `Get-NetTCPConnection -OwningProcess <pid>`
     returns nothing and no Windows Firewall prompt appeared.
  5. Delete the copy when finished.

  This drill also rehearses, deliberately, the exact failure mode Phase 4 removes.
- **If any probe case is red, or the report is marked `incomplete`, stop.** Take
  the documented fallback question back to the user. An adapter-test failure is
  our bug and is simply fixed; only a probe failure is evidence about the
  platform. A hung case counts as red, not as "needs more time".

---

### Phase 3 — One composition root shared by both entry points

Backend refactor. The web server's behavior must be byte-identical afterwards.

1. Split `backend/src/app.rs` into `app/mod.rs` (thin), `app/composition.rs` and
   `app/server.rs` as described in *One composition root, two entry points*.
   `Application::build`, `Application::shutdown`, `StartupError`.
2. `Application::shutdown` aborts the launch-refresh handle and closes the
   SQLite pool; `server::serve` calls it after `axum::serve` returns.
3. **`AssetPolicy::{Optional, Required}` on `AppConfig`**, resolving the
   contradiction between "absent assets build an API-only router" and "a desktop
   startup failure must show a dialog". Server sets `Optional`; desktop sets
   `Required`, which checks for `index.html` (not just the directory) and returns
   `StartupError::StaticAssetsMissing { path }`.
4. Move the existing `app.rs` tests (`launch_refresh_spawns_background_job`,
   `launch_refresh_is_skipped_*`, `demo_state_is_seeded_and_query_only`) to
   whichever module now owns the behavior. They must pass unchanged.
5. `desktop/src/launch.rs` drops its throwaway wiring and calls
   `Application::build` / `Application::shutdown`, with `AssetPolicy::Required`.
6. **Decision-log entry lands here:** the in-process desktop transport entry,
   whose Context quotes the probe report's version block from Phase 2.

Tests:
- `Application::build` produces a router that serves both `/api/health` and the
  SPA index when the assets directory exists.
- `AssetPolicy::Optional` with an absent assets directory still produces the
  API-only router and the existing warning — the server's behavior is unchanged.
- `AssetPolicy::Required` with an absent assets directory returns
  `StartupError::StaticAssetsMissing`, and **also** does so for a directory that
  exists but has no `index.html` (the stale-`dist/` case).
- `Application::shutdown` closes the pool: a query after shutdown fails.
- A demo `Application` builds and shuts down with no filesystem access.
- Existing app tests green in their new home.

Verify:
- Workspace command sequence.
- **External human testing recommended:** `scripts/start.ps1` still starts,
  serves and stops cleanly, and the desktop executable still opens the window —
  both entry points now go through one construction path, so a mistake here
  breaks both. Then rename `frontend/dist` aside and confirm the asset policy
  split: the server starts and serves API routes only, while the desktop
  executable fails loudly instead of opening a window on the backend root.

---

### Phase 4 — Ledger identity: rename, absolute resolution, refuse-to-create, observability

1. `backend/src/db/ledger_location.rs` (`LedgerLocation`, `resolve(url, anchor)`,
   `LedgerLocation::memory()`, `LedgerLocationError`); `AppConfig` stores a
   `LedgerLocation` and resolves to `memory()` whenever `demo_mode` is set,
   before any filesystem work; `AppConfig::from_env_anchored(anchor)` added
   alongside `from_env()`. Tidy `db/mod.rs` into a thin wrapper by moving
   `memory_pool` and `RepoError` out.
2. `db::connect` takes `CreateMissing`; `create_ledger_if_missing` on
   `AppConfig` defaults to `false`, read from `TTTB_CREATE_LEDGER_IF_MISSING`.
   The existing `db/pool.rs` test that creates a file passes `CreateMissing::Yes`
   explicitly.
3. A missing ledger produces `StartupError::LedgerMissing { path }` with an
   `engine_error!` naming the path — never an empty new file.
4. `AppState` gains `ledger_path` and `shell`. `/api/health` serializes
   **`ledger` only** — `shell` stays internal and is used by the log banner, since
   no UI renders it. Demo reports `memory`/`null`.
5. **`repo-paths.json` at the repository root** with
   `defaultLedgerRelativePath` = `.local/db/tttb-portfolio.sqlite` and
   `desktopLogRelativePath` = `.local/logs/desktop.log`.
   `scripts/start.ps1` reads it once via `ConvertFrom-Json` and derives **both**
   values from it — `$DefaultLocalDatabasePath` now, and the desktop log path it
   prints in `-Desktop` mode from Phase 5. Both literals are deleted from the
   script; neither is reintroduced later. (The desktop crate's reader is wired in
   Phase 5, where `app_paths.rs` appears.)
6. **Ledger rename**, using that value, with a guard that detects the legacy
   `tttb-ledger-test.sqlite` (and its `-wal`/`-shm` sidecars) and fails with the
   exact `Move-Item` commands to run. `README.md` records the new name.
7. **`-ProductionDb` default fix.** `$DefaultProductionDatabasePath` moves off
   `MyDocuments` (which resolves inside OneDrive on this machine, and has never
   existed) to `Join-Path $env:LOCALAPPDATA "TickerTapeTallyBoard/portfolio.sqlite"`,
   with a comment recording why: a live SQLite file plus `-wal`/`-shm` sidecars
   under a syncing folder is a corruption risk. `README.md` updated, **including
   the plain statement that `-ProductionDb` needs one `-InitLedger` run on this
   machine before it works, and that a fresh clone needs one too.**
8. **No script extraction.** `Assert-Command`, `Invoke-Step`,
   `Invoke-NativeCommand`, `ConvertTo-SqliteUrl`, `Resolve-DatabaseUrl` and the
   new legacy-name guard all stay where they are in `scripts/start.ps1`, because
   the desktop launch becomes a mode of that same script rather than a second one
   (see Phase 5). Recorded as a deliberate non-action so a reader does not expect
   a `scripts/Common.ps1` that never appears.
9. Frontend: `HealthResponse` type, footer render, pure label helper + Vitest
   tests (see *Frontend — exactly two touches*).
10. **Decision-log entry lands here:** the observable-ledger-identity entry.

Tests:
- `ledger_location` unit tests: absolute URL passes through; a relative URL
  resolves against the supplied anchor and never against the CWD;
  `sqlite::memory:` yields `path: None`; a non-sqlite URL is rejected.
- `db::connect` with `CreateMissing::No` against a non-existent path returns an
  error and **creates no file** (assert the file is still absent afterwards).
- **Demo config never resolves a ledger:** `demo_mode` with `TTTB_DATABASE_URL`
  pointing at a nonexistent path yields `ledger_path() == None`, builds
  successfully, and leaves no file behind. This is the interaction that would
  otherwise pop a startup dialog for a mode with no ledger by design.
- `/api/health` contract test: `ledger.mode`/`ledger.path` for a file-backed
  state and `memory`/`null` for a demo state. Also assert the response carries
  **no** `shell` key, so a future reader does not reintroduce an unconsumed field.
- `repo-paths.json` round-trip: the path `scripts/start.ps1` derives and the path
  the desktop crate derives resolve to the same absolute file (asserted in the
  desktop crate's tests once `app_paths.rs` exists in Phase 5; in this phase, the
  script's derived value is asserted against the JSON by inspection).
- Vitest: the footer label helper renders the file name for a file ledger, the
  in-memory wording for a `null` path, and never emits an empty span or a stray
  separator; a pending/failed health query omits the ledger span entirely.

Verify:
- Workspace and frontend command sequences.
- **External human testing REQUIRED (data safety — take a backup first).**
  Stop everything, perform the rename, then: `scripts/start.ps1` opens the real
  portfolio and the footer shows the new file name (full path on hover); running
  `scripts/start.ps1` *before* renaming fails with the guidance message and
  creates nothing; pointing `TTTB_DATABASE_URL` at a nonexistent path fails to
  start and leaves no file behind; `scripts/start.ps1 -InitLedger` with a fresh
  path does create and migrate one; `scripts/start.ps1 -Demo` shows the `DEMO`
  chip and `In-memory demo` with no path and no empty span. Confirm the ledger row
  counts (`transactions`, `prices`, `instruments`) match the pre-rename backup.

---

### Phase 5 — Desktop hardening: anchoring, logging, failure dialog, shutdown, launch mode

The window becomes something you can put on the Start menu.

1. **`desktop/build.rs` gains its second job:** read `repo-paths.json`, validate
   it, emit `cargo:rustc-env=TTTB_DEFAULT_LEDGER_RELPATH` and
   `TTTB_DESKTOP_LOG_RELPATH`, and `cargo:rerun-if-changed=../repo-paths.json`. A
   missing or malformed file fails the build.
2. `desktop/src/app_paths.rs` — `AppPaths` from the build-tree anchor plus those
   two baked-in relative paths, with `TTTB_DATABASE_URL` / `TTTB_STATIC_DIR`
   overrides resolved against that anchor when relative. Pure and unit-tested
   against a supplied anchor.
3. `engine_logging::initialize(LogDestination, instance_tag) -> LogInitOutcome`;
   never panics. The server entry point passes
   `TerminalAndFile { path: "engine.log" }` (unchanged path semantics); the
   desktop entry point passes
   `FileOnly { path: <anchor>/<TTTB_DESKTOP_LOG_RELPATH> }`. Both pass
   `<shell>:<pid>`.
4. The line-buffering tagged writer in `engine_logging`, wrapping the file sink in
   both variants: buffer to a newline, then emit `[<instance_tag>] ` plus the
   completed line in one `write_all`. The terminal sink is untagged.
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
   `Invoke-NativeCommand`, `ConvertTo-SqliteUrl`, `Resolve-DatabaseUrl`, the
   legacy-ledger guard, the `-Demo` argument validation, and the
   `TTTB_DATABASE_URL` / `TTTB_DEMO_MODE` save-and-restore `finally` block.

   In `-Desktop` mode the script:
   - builds the frontend (unless `-SkipBuild`) — `frontend/dist` is the desktop
     app's asset source, so this is required, not optional;
   - builds `cargo build -p ticker-tape-tally-board-desktop` instead of the
     backend package (the desktop crate depends on the backend crate, so the
     backend still compiles; its *binary* is simply not needed);
   - skips `Stop-OrphanVite`, `Resolve-FrontendPort`, `Resolve-BackendPort`,
     `TTTB_PORT`, the Vite process, both `Wait-Url` calls, and the browser open;
   - prints the resolved ledger (or `demo`) and the desktop log path — both from
     the `repo-paths.json` values already read in Phase 4, not from a fresh
     literal — then starts the executable and **waits for it to exit**. Waiting
     keeps the existing
     "the script owns the run" contract, gives a place to report a non-zero exit
     code — which is how a startup failure signals itself alongside the dialog —
     and keeps the environment restore correctly ordered.

   Switch composition, rejected combinations in the style the script already uses
   for `-Demo` + `-ProductionDb`:
   - `-Desktop -Demo` — **supported**, and the reason demo now reaches the window.
   - `-Desktop -ProductionDb`, `-Desktop -LocalDatabaseUrl`,
     `-Desktop -ProductionDatabaseUrl` — **supported**; the script sets
     `TTTB_DATABASE_URL` and the desktop process's env override wins over its
     build-tree default (resolved absolutely, never against the CWD). Note the
     interaction with refuse-to-create: the production default has never existed
     on this machine, so `-ProductionDb` in *either* mode now fails with a clear
     error until it is created once with `-InitLedger`. That is the intended
     behavior, not a regression, but it will be the first thing a user hits.
   - `-Desktop -SkipInstall`, `-SkipBuild`, `-BuildOnly`, `-InitLedger` —
     **supported**, same meanings.
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
   - Existing `-Demo` exclusions (`-ProductionDb`, `-LocalDatabaseUrl`,
     `-ProductionDatabaseUrl`) are unchanged and apply in both modes.

   Note in the script's help and in `README.md`: **close the window to stop the
   app.** Ctrl+C in the console falls into the existing `Stop-ProcessTree`
   `finally`, which is a hard kill that skips the graceful pool close. SQLite
   recovers from that through WAL, but it is not the intended path.
9. **Demo mode reaches the desktop shell.** The desktop entry point reads
   `TTTB_DEMO_MODE` exactly like the server does — no shell-specific override, no
   ignore-warning. `scripts/start.ps1 -Desktop -Demo` therefore opens the seeded,
   read-only, in-memory demo in the native window, and the demo becomes a
   presentation surface you can show someone without a browser chrome around it.
   Everything that makes this safe is already in place and is *verified*, not
   assumed, in this phase:
   - the ledger is resolved to `LedgerLocation::memory()` before any filesystem
     work, so neither the refuse-to-create rule nor absolute-path resolution can
     fire (Phase 4);
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
- `app_paths` derives the default ledger path from the value `build.rs` baked out
  of `repo-paths.json`, and that path equals the one `scripts/start.ps1` derives
  from the same file — the machine-readable check that replaces "two definitions
  with comments pointing at each other".
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
- Two writers with different tags sharing one buffer interleave at line
  granularity only — no line contains both tags.
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
    with the real portfolio (proving nothing is CWD-relative) and that
    `.local/logs/desktop.log` received the startup banner.
  - **Both readers of `repo-paths.json` agree:** the footer path from that
    shortcut launch is byte-identical to the footer path from
    `scripts/start.ps1 -Desktop`. The automated test above is the mechanism; this
    is the cheap confirmation that the mechanism is wired to reality.
  - Rename the ledger aside and launch: a native error dialog appears naming the
    missing path and the log file, and **no window and no empty ledger** appear.
  - Rename `frontend/dist` aside and launch: a native error dialog names the
    missing assets directory, and **no window** appears — not a window showing
    the backend root.
  - Confirm no console window appears in a release build
    (`cargo build --release -p ticker-tape-tally-board-desktop`).
  - Close the window with the X and confirm the shutdown banner is written, the
    process exits, and no `-wal` file is left mid-transaction.
  - Confirm again that `Get-NetTCPConnection -OwningProcess <pid>` shows no
    listener and no firewall prompt appears.
  - Launch two desktop instances at once, use both, then read
    `.local/logs/desktop.log` as a single timeline: every line carries a
    `[desktop:<pid>]` tag, the two PIDs are distinguishable, and no line contains
    fragments of both. This is the scenario the shared-file choice exists to
    serve, so it is checked rather than assumed.
  - **Demo in the window:** `scripts/start.ps1 -Desktop -Demo` opens the seeded
    demo; the footer reads `DEMO` plus
    `In-memory demo` with no path and no empty span; attempting a write — adding a
    transaction, or setting a conviction — is rejected with the
    `demo_read_only` message rather than a raw `403`; no ledger file is created
    or opened (check `.local/db/` timestamps); and no network call is made
    (the launch refresh is skipped).
  - **Demo must not trip the ledger rules:** with the real ledger renamed aside,
    `-Desktop -Demo` still starts normally — no startup dialog — proving demo
    never resolves a ledger path.
  - Reject-combination checks: `-Desktop -FrontendPort 5173`,
    `-Desktop -NoBrowser`, `-ProbeWebView` without `-Desktop`, and
    `-Demo -InitLedger` each fail immediately with a clear message and start
    nothing.
  - `-Desktop -ProbeWebView` reproduces the Phase 2 probe result and propagates a
    non-zero exit code when a case fails — the wrapper does not swallow the gate.

---

### Phase 6 — Cross-process refresh coordination

Committed work, placed last because it is independent of the window — not
because it is optional. It is also the phase most likely to overrun, so start it
with the failure modes in mind rather than discovering them.

1. Migration `add_refresh_run_claim.sql` (additive: `claim_owner TEXT`,
   `heartbeat_at TEXT` on `market_data_refresh_runs`).
2. `db/market_data_runs.rs`:
   - `try_claim_run(pool, trigger, owner, now, stale_after)` running
     `BEGIN IMMEDIATE`, reclaiming a stale `RUNNING` row as `FAILED` with message
     `abandoned` in the **same** transaction, and inserting the new claim;
   - `heartbeat(pool, run_id, owner, now) -> LeaseState` and
     `finish_run(..., owner)` — both **owner-conditional**
     (`AND claim_owner = ? AND status = 'RUNNING'`) and both reporting whether
     they affected a row;
   - `live_claim(pool, now, stale_after)`.
   - One shared `REFRESH_CLAIM_STALE_AFTER` constant (proposed 120 s).
3. `refresh.rs`: delete the in-memory `AtomicBool`/`Mutex` pair. `refresh()`
   starts only when `try_claim_run` succeeds and otherwise returns the current
   status as it does today. The flight guard owns a heartbeat task that stops when
   the guard drops. `is_refreshing()` and `active_run()` become async reads of the
   live claim; `status()`'s `refreshing` and `latest_run` follow.
4. **Lease loss stops the loser.** The heartbeat task treats a zero-affected-row
   update as lease loss: it sets a cancellation flag on the flight guard and logs
   an `engine_warn!` naming the run id, this owner and the current holder. The
   refresh loop checks that flag between instruments and between providers, and on
   loss stops immediately, writes nothing further — no prices, no FX, no run row —
   and returns `RefreshRunStatus::Failed` with message `lease_lost`. Without this,
   a stalled-then-reclaimed process wakes up and keeps writing.
5. Log the owner identity on claim, reclaim, lease loss and release, per the
   logging rules (enough context to identify the run and the owning process).
6. **Decision-log entry lands here:** the refresh-claim entry.

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
- **Lease loss stops work:** after lease loss the fake provider's call count stops
  increasing and the refresh returns `lease_lost`.
- `status()` reports a foreign process's run in `latest_run`.
- Existing single-process refresh tests pass unchanged.

Verify:
- Workspace command sequence.
- **External human testing REQUIRED (against a copy of the real database first,
  because this performs live provider calls).** Start `scripts/start.ps1` and
  `scripts/start.ps1 -Desktop` against the same ledger. Confirm: only one launch
  refresh runs; both UIs show the spinner during it and both stop when it ends;
  exactly one new `market_data_refresh_runs` row appears; pressing manual refresh
  in one while the other is refreshing reports the running state rather than
  starting a second run. Then kill one process mid-refresh with `taskkill /F` and
  confirm that after the staleness window the other process can start a refresh
  and the abandoned row reads `FAILED` / `abandoned`.
- **Suspend rather than kill, to exercise the reclaimed-owner path:** suspend one
  process mid-refresh (Process Explorer, or a debugger break) for longer than the
  staleness window, let the other reclaim, then resume it. The resumed process
  must log lease loss, stop, and leave both run rows correct.
- Then repeat the kill drill once against the real ledger.

---

### Phase 7 — Documents, decision-log entries, version bumps, final sweep

1. `docs/Design.HighLevel.md`:
   - *Deployment model*: add the desktop shell as a second deployment of the same
     source — one window, in-process transport over a custom URI scheme, no
     listener, assets from disk — alongside the existing LAN server. Note that
     the two shells have different origins and therefore separate client-side
     view preferences.
   - *Stack* table: a `Desktop shell` row (Tauri v2 + WebView2).
   - *Phase 5 — Hardening & deployment*: record that the desktop window replaces
     the "Windows scheduled task or service" framing for interactive use, and
     that "embed frontend in binary" is explicitly **not** what the desktop shell
     does.
   - *API surface (v1 sketch)*: record the explicit request-size limits (1 MiB
     API-wide, 32 MiB on the import routes) and the `payload_too_large` error
     code, so the contract is documented where the routes are.
   - Risk table: a row for the ledger path being exposed over `/api/health`
     (username disclosure on an unauthenticated LAN surface), mitigated only by
     the existing LAN-trust boundary.
2. `README.md`: workspace commands from the repository root; `scripts/start.ps1
   -Desktop` and what it does, including `-Desktop -Demo` and "close the window to
   stop the app"; WebView2 runtime as a prerequisite; the renamed ledger; the
   corrected `-ProductionDb` default and why; the new
   `TTTB_CREATE_LEDGER_IF_MISSING` variable and the `-InitLedger` switch; the
   desktop log location; the import size limit.
3. `Agents.md`: one Architecture bullet — *the desktop shell consumes the
   backend's composition root; it must not construct its own router, state, or
   ledger path.* (The Workflow and version wording already landed in Phases 1
   and 2, with the changes they describe.)
4. `docs/DecisionLog.md`: **nothing new here.** Every entry landed with its own
   phase; this step only re-reads them against what was actually built and
   corrects any that drifted.
5. Version bumps, one release for the whole feature (intermediate phases are not
   released): the workspace package version takes a **minor** bump from whatever
   is current (expected `0.15.x → 0.16.0`), which covers both Rust crates at
   once — and `tauri.conf.json` carries no version to update, by design;
   `frontend/package.json` takes a minor bump (expected `0.23.x → 0.24.0`) plus
   the matching `frontend/package-lock.json` version fields.
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
unauthenticated on the LAN (2026-06-12 Phase 0 Planning Decisions), so only the
import path is raised. Import bodies remain fully buffered, which the limit makes
bounded; a streaming parser would be a separate change. Request-size behavior is
now part of the documented API contract. Malformed request bodies also move from
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
the LAN web-server deployment had to keep working unchanged from the same source.
Typed per-endpoint commands were rejected as doubling per-endpoint maintenance
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
path is the same defect in either entry point. Two independently maintained
copies of the default path would eventually open two databases that look like
one.
Consequences: A first run on a fresh checkout, and any run pointed at a database
that does not exist yet, requires the explicit creation step once. The reported
path contains the operating-system user name and is readable by anyone who can
reach the health endpoint on the LAN; this is accepted under the existing
LAN-trust boundary and must be revisited before any remote exposure. This refines
the 2026-06-13 Static Frontend Serving entry, whose working-directory-relative
assumption still holds for the server but not for the desktop shell. Static
assets are required for the desktop shell and optional for the server, as an
explicit policy rather than a consequence of which router was built.
```

**5.**

```
## YYYY-MM-DD - Desktop Process Independence From The Working Directory
Decision: Nothing the desktop process uses is located relative to its working
directory — not the log file, not the database, not the built frontend. All
resolve from an anchor fixed at build time. The process has no console; it logs to
one fixed known file that every copy appends to, and **every logged line carries a
tag identifying the copy that wrote it**, not merely a banner at the start of a
session; failing to open that file never prevents startup; and any startup failure
produces a native dialog naming the failure and the resolved paths rather than a
window that never appears.
Context: A windowed executable launched from a shortcut has an arbitrary and
possibly unwritable working directory and no standard error stream, so a
working-directory-relative log opened with an unwrap could terminate the process
before anything visible happened. Two copies may run at once, and a problem caused
by two copies interfering is exactly the problem best read as one interleaved
timeline — which is only true if each line says who wrote it, so per-copy files
were rejected and per-line tagging was made a requirement rather than a nicety.
Consequences: This refines the 2026-06-13 Backend Logging Stack entry: logging
still goes through the same facade, but initialization takes an explicit
destination and an instance tag, and is non-fatal. Log lines are written whole,
which both carries the tag and bounds concurrent writes to line granularity;
interleaving at that granularity is the intended reading experience, not a
defect to be fixed later. The file line format changes for the server as well as
the desktop shell, deliberately, so attribution works the same way everywhere.
The desktop executable is tied to the tree it was built in, which is acceptable
while it is not distributed; distribution would require replacing the build-time
anchor with an installed-location or per-user resolution.
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
## YYYY-MM-DD - Market-Data Refresh Uses A Per-Ledger Lease
Decision: The claim that a market-data refresh is in progress lives in the
database rather than in process memory, so every process sharing a ledger sees
it and the reported refresh status reflects cross-process reality. The claim is a
lease: its holder heartbeats while it works, a lease whose heartbeat has gone
stale may be taken over by another process, every write to the run record is
conditional on still holding the lease, and a holder that discovers it has lost
the lease stops its work immediately and writes nothing further.
Context: The previous single-flight guard was per-process, so two processes on
one ledger ran two concurrent provider refreshes, wrote overlapping run records,
and each reported a refresh state blind to the other. A desktop shell alongside
the web server makes that the normal case rather than an edge case. Reclaiming a
stale lease alone is not sufficient: a process that stalls past the timeout is
not dead, and on waking would otherwise finalize or overwrite a run it no longer
owns.
Consequences: Refines the 2026-06-16 Market Data Service Injection And
Single-Flight Refresh entry — the guard is now per-ledger rather than
per-process, which is what a second shell and the planned LAN access from a phone
both require. A process killed mid-refresh cannot block refreshes indefinitely.
Correctness here depends on behavior between independent database connections, so
its tests use separate connection pools over one file rather than a shared
in-memory database.
```

## Documents to update

| Document | Change | Phase |
|---|---|---|
| `Agents.md` | **Phase 1:** workspace commands from the repository root, per-package backend clippy, and the version-source clause moved from `backend/Cargo.toml` to `[workspace.package].version`. **Phase 2:** name both members and add the widen-when-`desktop/`-is-touched rule (deliberately *not* in Phase 1, when the member does not exist yet). **Phase 7:** one Architecture bullet — the desktop shell consumes the backend composition root and must not build its own router/state/ledger path | 1, 2, 7 |
| `README.md` | Workspace commands; `-p` for the Sharesight spike; `start.ps1 -Desktop` (incl. `-Desktop -Demo`, `-ProbeWebView`, and "close the window to stop the app"); WebView2 prerequisite; renamed ledger; corrected `-ProductionDb` default and why; `TTTB_CREATE_LEDGER_IF_MISSING` / `-InitLedger` **and the two cases that need it — a fresh clone and `-ProductionDb`**; desktop log location; the import size limit | 1 (commands), 4 (ledger, `-ProductionDb`), 7 (rest) |
| `docs/Design.HighLevel.md` | Deployment model gains the desktop shell; Stack table gains a desktop row; Phase 5 hardening text corrected (the desktop shell does **not** embed assets); API surface gains the explicit request-size limits and the standard-envelope guarantee for oversized and malformed bodies; risk row for the ledger path on `/api/health`; the CSP value (already settled — copy it, do not re-derive it), why the desktop response path emits it, and the two frontend properties it depends on | 7 |
| `docs/DecisionLog.md` | Seven entries, **each appended when its own phase lands**, in the log's `Decision`/`Context`/`Consequences` template, at the end of the file, naming behaviors and never this plan | 1, 2, 3, 4, 5, 5, 6 |
| `scripts/start.ps1` | Workspace executable path and build command; `-Desktop` mode with its build/launch branch, `-ProbeWebView`, and its rejected switch combinations; ledger default derived from `repo-paths.json`; legacy-name guard; corrected `-ProductionDb` default; `-InitLedger`. **`scripts/Common.ps1` is deliberately not created** — with one launch script there is nothing to share | 1, 2, 4, 5 |
| `repo-paths.json` (new, repository root) | The one definition of the default ledger and desktop-log relative paths, read by `scripts/start.ps1` and by `desktop/build.rs` | 4 (script reader), 5 (crate reader) |
| `backend/src/api/body_limits.rs` (new) | The two explicit request-size constants plus the derived test-size helpers — the only definitions | 2 |
| `backend/src/api/extract.rs` (new) | `ApiJson<T>` and `ImportBody`, sharing one rejection→`ApiError` mapping, so the `payload_too_large` promise holds on every route | 2 |
| `desktop/build.rs` (new) | `tauri_build::build()` (required), plus emitting the `repo-paths.json` values as build-time env | 2 (tauri build), 5 (paths) |
| `desktop/tauri.conf.json` (new) | `identifier` set; **no top-level `version`** (inherits Cargo's); `app.windows: []`; `withGlobalTauri: false`; `app.security.csp: null`; `build.frontendDist` omitted; bundling off | 2 |
| Root `Cargo.toml` (new), `backend/Cargo.toml`, `desktop/Cargo.toml` (new) | Workspace members, `[workspace.package] version`, member `version.workspace = true`, minor version bump at release | 1, 2, 7 |
| `frontend/package.json` (+ `package-lock.json`) | Minor version bump at release. **No new dependency** — the frontend gains no Tauri package | 7 |
| `.gitignore` | Unanchor the ledger pattern (`backend/tttb-ledger.sqlite*` → `tttb-ledger.sqlite*`) so a stray database anywhere in the tree cannot be committed | 1 |
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
- **Multi-portfolio support, a first-run ledger picker, or a per-user app-data
  ledger location.**
- **Streaming or chunked import upload.** The import handlers buffer the whole
  body; the 32 MiB limit makes that bounded rather than unbounded. A streaming
  parser would remove the ceiling entirely and is a separate piece of work with no
  current motivation.
- **A tested database backup/restore procedure.** The ledger rename in Phase 4
  is a copy-first drill, but real backup/retention/restore is whole-database work
  independent of this feature — and `docs/Design.HighLevel.md`'s Phase 5 already
  lists it.
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
- **The desktop app is tied to its build tree.** Moving or deleting the repo
  folder breaks the shortcut. Accepted because distribution is out of scope; the
  failure is visible (native dialog naming the missing path), not silent.
- **Dev runs and desktop runs share one ledger.** Intended. Phase 6 makes
  concurrent refresh safe; concurrent *writes* are still two processes on one
  SQLite file, protected only by SQLite's own locking. A write collision surfaces
  as a database-busy error, not corruption, but the app has no retry policy for
  it today. Recorded, not fixed here.
- **The refresh-lease phase is the most likely to overrun.** It is committed
  work, so overrunning means it takes longer, not that it gets dropped. The two
  things that make it larger than it looks are the ownership rules for a
  reclaimed-then-resumed process and the move from in-memory-pool tests to
  two-pools-on-one-file tests; both are specified up front for that reason.
- **The ledger path on `/api/health` discloses the Windows user name** to anyone
  on the LAN. Accepted under the existing LAN-trust boundary; must be revisited
  before any remote exposure.
- **`localStorage` view preferences do not carry between desktop and browser.**
  Inherent to a second origin; accepted; recorded against the 2026-07-10 entry so
  it is not later filed as a bug.
- **The shared log file interleaves when two copies run at once.** Accepted, and
  chosen: one timeline is what you want when the two copies are the problem.
  Whole-line writes bound the interleaving to line granularity and the per-line
  tag keeps every line attributable, so the accepted cost is "lines from two
  copies alternate", not "output is corrupted".
- **The CSP value depends on two properties of the frontend** — no inline script
  in the built `index.html`, and no image asset small enough for Vite to inline.
  Both are true today and both are easy to break accidentally. An inline script is
  the dangerous one: `script-src 'self'` would blank the window, and neither the
  adapter test nor the probe would catch it, because the probe serves its own
  page. The ordinary-app human check is the net.
- **Refuse-to-create is a behavior change to the web server**, not only the
  desktop app. Settled by the user. Two consequences a reader meets in practice
  and which the README states plainly: a fresh clone needs one `-InitLedger` run
  before the web app starts, and `-ProductionDb` on this machine needs one too,
  because its corrected default path has never existed.
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
- **The default ledger path has two *readers*** — `scripts/start.ps1` and the
  desktop crate — even though it now has one definition in `repo-paths.json`. A
  reader that resolves the relative path against the wrong root would still open
  the wrong file. Mitigated by an automated test comparing the two derived
  absolute paths, by the build failing if the JSON is missing or malformed, and by
  a cheap human confirmation of the footer path from both launch routes.
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

- **Ledger file name:** `.local/db/tttb-portfolio.sqlite`. Confirmed by the user.
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
- **One log file at a fixed location, with every line tagged** `<shell>:<pid>`
  via a line-buffering writer, rather than per-process files. Interleaving at line
  granularity is the intended reading experience.
- **The `ApiJson` rollout is confirmed at all seven handler signatures.** Every
  route returns the standard envelope for oversized and malformed bodies; the
  import-only variant is dropped, not held in reserve.
