# Plan — Production setup and hardening

## Summary

Turn the de-facto production environment into a deliberate one.

Done means:

- The real ledger lives at `%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio.sqlite`
  — outside the repository tree, outside OneDrive, truthfully named — and the
  backend refuses to invent it.
- Every launch writes an integrity-checked `VACUUM INTO` snapshot into a
  OneDrive-synced folder, pruned by a tiered retention ladder, plus one
  **mandatory** snapshot before any schema migration.
- A restore drill is documented **and executed once for real**, with a
  falsifiable pass criterion: zero changed rows between aggregate captures taken
  from the live app and from a second backend serving a restored snapshot.
- `scripts/start.ps1` with no flags runs the documented production model — a
  release backend serving built static assets as one process on a pinned,
  uncommon, loopback-only port — and the dev loop (debug build + Vite) moves
  behind `-Dev`.
- Startup and data-integrity problems fail loudly instead of degrading quietly.
- Which database is open, which mode it is running in, and when it was last
  backed up are visible in the UI footer and on `/api/health`.

**Scope of "fails loudly" is deliberately narrow** (see *Fail-loud scope*):
missing ledger, non-loopback host, memory ledger outside demo, busy pinned port,
missing built frontend in production, failed **pre-migration** backup, failed
migration. The quiet market-data behaviours committed on 2026-08-29
(cross-source day-change suppression, API/log-only provider ambiguity,
`history_clamped`) are **excluded on purpose**; a refresh-problem UI surface is
separate future work and this plan must not be read as delivering it.

**Out of scope, user-confirmed:** auto-start at logon; LAN exposure, firewall
work and authentication; browser-preference migration across the origin change;
mid-session or pre-destructive-operation backup triggers (considered and
rejected — do not re-raise); refresh-problem UI surfacing; everything owned by
`docs/plans/Plan.NativeDesktopWindow.md` (workspace, Tauri bridge, CSP, body
limits, in-window probe, cross-process refresh lease).

## Current state this plan starts from (verified)

- `.local/db/tttb-ledger-test.sqlite` (~3.7 MB) holds the real portfolio,
  git-ignored via `.local/`, updated daily. No `-wal`/`-shm` sidecars present
  while the app is stopped.
- `scripts/start.ps1` always builds a **debug** backend plus `npm run build`,
  then runs the debug exe **and** `npm run dev`; the built `frontend/dist` is
  produced every run and then never served. `Resolve-BackendPort` scans upward
  from 8080 silently. `.local/logs/*.log` are deleted on every start. The script
  blocks until Ctrl+C.
- `-ProductionDb` points at `Documents\TickerTapeTallyBoard\portfolio.sqlite`;
  that file has never existed, and `Documents` is OneDrive-redirected to
  `C:\Users\larsp\OneDrive\Dokument`.
- `backend/src/config.rs`: `TTTB_HOST` (127.0.0.1, but **any** IP is accepted —
  an existing test pins `0.0.0.0` as valid), `TTTB_PORT`/`PORT` (8080),
  `TTTB_DATABASE_URL` (default `sqlite://tttb-ledger.sqlite`, CWD-relative),
  `TTTB_DEMO_MODE`, two market-data refresh flags, `TTTB_STATIC_DIR`
  (`../frontend/dist`).
- `backend/src/db/pool.rs::connect` does `create_if_missing(true)`,
  `foreign_keys(true)`, then runs embedded migrations in the same call.
- `backend/src/engine_logging.rs` opens `engine.log` **CWD-relative** with
  `.expect(...)`, append-only, unbounded.
- `/api/health` exposes `status`, `version`, `demo`, `build`. The footer renders
  UI/API versions and a `DEMO` badge.
- `scripts/capture-aggregates.ps1` already supports `-BaseUrl`/`-Port` for
  capturing from a second backend "backed by a restored database copy", and
  diffs before/after captures. All three captured endpoints are `GET`, so
  capturing writes nothing. It is the restore drill's measuring instrument.
- No backups exist. `docs/plans/Plan.HandEnteredPrices.md` deferred "backup,
  retention and a restore drill" to this plan.

## Settled decisions (implement these; do not re-litigate)

1. **The production ledger is `%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio.sqlite`.**
   Outside the repository, outside OneDrive (a live SQLite file plus `-wal`/`-shm`
   in a syncing folder is a corruption and conflict risk), truthfully named.
2. **Mode is an explicit configuration value, not an inference.**
   `TTTB_MODE ∈ {production, development, demo}`, default `production`.
   `TTTB_DEMO_MODE` is retired in favour of `TTTB_MODE=demo`. Mode governs the
   ledger default, the backup policy, the static-asset policy, the log file name
   and the port-conflict behaviour — one input, one source of truth, one place
   to test. The default value exercises every new code path.
3. **The backend config is the single source of truth for the production ledger
   path.** In production and development modes `scripts/start.ps1` does **not**
   set `TTTB_DATABASE_URL` at all; it only needs path knowledge for the one-time
   migration and the legacy-name guard. `repo-paths.json` from
   `Plan.NativeDesktopWindow.md` is never built.
4. **Refuse to create a missing ledger.** `create_if_missing` is gated on the
   opt-in `TTTB_CREATE_LEDGER_IF_MISSING=1`, surfaced as
   `scripts/start.ps1 -InitLedger`. Names taken verbatim from the desktop plan;
   no parallel names are invented.
5. **Outside demo mode the ledger must be file-backed.** An in-memory URL in
   production or development is a startup failure, not an ephemeral ledger that
   silently discards writes.
6. **Backups are backend-integrated and launch-time only.** On startup the
   backend writes a `VACUUM INTO` snapshot, integrity-checks it, prunes by a
   tiered ladder and records the outcome. One additional, **mandatory** snapshot
   is taken immediately before pending migrations are applied. No mid-session
   and no pre-destructive-operation triggers.
7. **One backup-failure contract, no exceptions** (see *Backup failure is one
   rule*): **every** ordinary launch-snapshot failure — including an
   unresolvable backup directory — starts the application with a visible warning
   status and is never fatal. The **pre-migration** snapshot is mandatory, cannot
   be disabled by any flag, and its failure blocks startup before the migration
   runs.
8. **Snapshots are plain, unencrypted SQLite files in OneDrive.** Privacy
   considered and accepted.
9. **Honesty rule.** "Backed up" means *a snapshot was written locally into the
   synced folder*. The asynchronous OneDrive upload is not verified and is never
   claimed. Off-machine protection depends on OneDrive actually syncing.
10. **The production port is pinned and fails loudly when busy.** Default 8480.
    No silent scan-upward in production. Development and demo may still scan,
    because development protects only a dev ledger and demo opens none.
11. **Binding is loopback-only, and that is now enforced.** A non-loopback
    `TTTB_HOST` is rejected at startup with a clear error. The 2026-06-12
    LAN-trust commitment is narrowed, not honoured by convention;
    `docs/plans/Design.mobile-view-design.md` is the work that lifts this
    deliberately, together with the authentication it will need.
12. **`scripts/start.ps1` with no flags is the production run**: `cargo build
    --release`, `npm run build`, one process serving `frontend/dist` statically.
    `-Dev` restores debug build + Vite. `-Demo` keeps its ephemeral seeded
    in-memory read-only behaviour and composes with `-Dev`.
13. **The one-time browser-storage reset caused by the origin/port change is
    accepted.** No preference migration, no goal of preference stability.
14. **Demo mode never opens the ledger and never takes a backup.** The 2026-07-02
    commitment is preserved structurally: demo resolves to an in-memory ledger
    before any filesystem work happens.

## Merge contract with `Plan.NativeDesktopWindow.md`

That plan is pending and will be implemented **after** this one. This plan takes
over ledger identity wholesale and also changes `AppConfig`, the startup
composition, `StartupError`, the asset policy and `engine_logging`'s API, path
and rotation behaviour. Left unedited, that plan would instruct its implementer
to introduce a competing logger API, restore the server's CWD-relative
`engine.log`, treat server assets as always optional, and re-refactor an
`app.rs` shape that no longer exists. The closing phase therefore **rebases every
affected section** of that document while leaving its workspace, bridge, CSP,
body-limit, probe and lease work untouched. The full edit list is in Phase 8.

| Desktop-plan item | Disposition |
|---|---|
| Settled decision 8 — "one shared ledger", tied to the build tree | **Partially superseded.** The shared ledger becomes the app-data production ledger resolved by `AppConfig`. The desktop crate reads it from config, not from a build-tree anchor. Build-tree anchoring still applies to static assets and the desktop log. |
| Settled decision 9 — rename away from the "test" name | **Landed here**, to the app-data location rather than a new name inside `.local/db/`. |
| Settled decision 10 — refuse-to-create, `TTTB_CREATE_LEDGER_IF_MISSING=1`, `-InitLedger` | **Landed here, names reused verbatim.** Its `-ProductionDb` caveat is obsolete — that flag is retired. |
| Settled decision 11 — `/api/health` `ledger` + footer render | **Landed here and deliberately extended** with `mode` and `backup`. The "`ledger` and nothing else" clause was aimed at a shell badge; backup visibility is a different requirement. The "no shell/`DESKTOP` badge" part still stands. |
| Settled decision 13 + *Nothing in the desktop process is located relative to the working directory* | **Rule intact for desktop; its stated server exemptions are now false.** The server's log and ledger are no longer CWD-relative; only `TTTB_STATIC_DIR`'s default still is, and the launch script passes it absolutely. |
| Settled decision 17 — `.local/db/tttb-portfolio.sqlite` | **Superseded** by the app-data location. |
| Settled decision 21 + `repo-paths.json` | **Dropped.** With an absolute app-data default, the backend config is the one definition; a second machine-readable file would be machinery without a purpose. `desktop/build.rs` keeps only `tauri_build::build()`. |
| Settled decision 22 — assets required for desktop, optional for server | **Refined here:** required in production mode, optional in development mode, and still required for the desktop shell. Flagged as an adjustment, not a silent change. |
| Settled decision 25 + the `LogDestination`/`instance_tag` design | **Rebased, not replaced.** `engine_logging` already takes a settings struct, already never panics, already writes outside the repository and already rotates. The desktop plan's per-line instance tag extends that struct; it must not reintroduce an unbounded file or a CWD-relative path. |
| *One composition root, two entry points* (its Phase 3) | **Partially pre-done.** `app::run()` and `ledger::open` already exist and `StartupError` is already a typed enum; the desktop plan's split and its `StartupError` proposal rebase onto them rather than inventing them. |
| Legacy-name guard in `scripts/start.ps1` | **Moved into this plan.** |
| Its Phase 4 (ledger identity) | **Mostly landed.** What remains for that plan: `AppShell` in the log banner, build-tree anchoring for the desktop's *static dir and log*, and the `db/mod.rs` tidy (`memory_pool`/`RepoError` moved out). |
| "Deliberately out of scope: a tested database backup/restore procedure" | **No longer out of scope** — landed here. |
| Migration numbering note | **Still true.** This plan adds **no** migration: refuse-to-create and `VACUUM INTO` live at the connection/pool layer, not in schema. |
| Cross-process refresh lease (its Phase 6) | **Stays desktop-plan scope.** The pinned production port is this plan's interim guard against two backends silently sharing one ledger. Residual risk recorded below. |

`docs/plans/Design.mobile-view-design.md`'s future `-Lan` mode now has an extra
obligation: it must lift the loopback-only enforcement introduced here, together
with the authentication that makes lifting it safe. One line is added to that
document in Phase 8; nothing else here is designed for it.

## Architecture

### Modules, named for behaviour

```
backend/src/config.rs                 # Mode, host/ledger/backup/log/static resolution (extended)
backend/src/ledger/mod.rs             # thin wrapper
backend/src/ledger/location.rs        # LedgerLocation, resolve(url, mode), memory()
backend/src/ledger/open.rs            # open(): verify -> pool -> mandatory pre-migration snapshot -> migrate -> launch snapshot -> prune
backend/src/ledger/backup.rs          # VACUUM INTO snapshot + integrity check + collision-safe naming
backend/src/ledger/retention.rs       # pure prune planner over parsed snapshot names
backend/src/ledger/backup_status.rs   # derive backup status from a directory listing
backend/src/startup_error.rs          # StartupError enum + operator-facing formatting
backend/src/db/pool.rs                # open(location, CreateMissing) + pending_migrations + migrate, split apart
backend/src/engine_logging.rs         # LogSettings, rotation planner, RotatingFileWriter
```

No module is named for a phase or a milestone; none needs renaming when this
plan is deleted. `main.rs`, `lib.rs` and every `mod.rs` stay thin wrappers:
`main.rs` becomes a `std::process::ExitCode` shell over `app::run()`, which
sequences config → logging → ledger → serve and formats a `StartupError` once.

`db::connect` is split so a snapshot can be taken between opening the pool and
running migrations:

```rust
// backend/src/db/pool.rs
pub enum CreateMissing { Yes, No }
pub struct OpenedLedger { pub pool: SqlitePool, pub created_now: bool }

pub async fn open(location: &LedgerLocation, create: CreateMissing) -> Result<OpenedLedger, sqlx::Error>;
pub async fn pending_migrations(pool: &SqlitePool) -> Result<usize, sqlx::Error>;
pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error>;
```

**Pending-migration discovery uses runtime sqlx queries only** — no compile-time
macros and no `.sqlx/` metadata, per the 2026-06-14 Backend Persistence Stack
commitment. It compares the embedded `sqlx::migrate!` migrator's version list
against the rows in `_sqlx_migrations`, and treats an **absent
`_sqlx_migrations` table as a fresh ledger with every embedded migration
pending** rather than as an error. `created_now` distinguishes the two fresh
cases: a ledger this process just created under `-InitLedger` has nothing to
protect and skips the pre-migration snapshot; a pre-existing file with no
migration table does get one.

`ledger::open` is the only caller that composes these, so the ordering
(verify → open → mandatory pre-migration snapshot → migrate → launch snapshot →
prune) exists exactly once.

### Configuration surface

| Variable | Default | Notes |
|---|---|---|
| `TTTB_MODE` | `production` | `production` \| `development` \| `demo`. Replaces `TTTB_DEMO_MODE`. |
| `TTTB_HOST` | `127.0.0.1` | **Must resolve to a loopback address** (`127.0.0.0/8` or `::1`). Anything else is a startup failure. |
| `TTTB_DATABASE_URL` | production: `sqlite://%LOCALAPPDATA%/TickerTapeTallyBoard/portfolio.sqlite`; development: `…/portfolio-dev.sqlite`; demo: in-memory, ignoring any override | Absolute. The CWD-relative `sqlite://tttb-ledger.sqlite` default is deleted. Outside demo, an in-memory URL is rejected. |
| `TTTB_CREATE_LEDGER_IF_MISSING` | `0` | `1` opts in; `-InitLedger` sets it. |
| `TTTB_PORT` / `PORT` | `8480` / hosting fallback | One default for all modes; only the *conflict behaviour* differs. |
| `TTTB_STATIC_DIR` | `../frontend/dist` | `scripts/start.ps1` now passes an absolute path so failure messages name a real location. |
| `TTTB_BACKUP_ENABLED` | `true` when mode ≠ demo | Disables **only** the launch snapshot. The pre-migration snapshot is never disabled. |
| `TTTB_BACKUP_DIR` | production: `%OneDrive%\TickerTapeTallyBoard\Backups`; development: `%LOCALAPPDATA%\TickerTapeTallyBoard\backups-dev` | Unresolvable (no `OneDrive` variable and no override) is **not** a config error: the launch snapshot fails with a visible warning, and only a *pending* pre-migration snapshot turns it into a startup failure. |
| `TTTB_LOG_FILE` | `%LOCALAPPDATA%\TickerTapeTallyBoard\logs\engine.log` (production), `engine-development.log`, `engine-demo.log` | Moves out of the repository tree. |
| `TTTB_MARKET_DATA_REFRESH_ENABLED`, `TTTB_MARKET_DATA_LAUNCH_REFRESH_ENABLED` | `true` | Unchanged; the restore drill turns launch refresh off. |
| Retired | `TTTB_DEMO_MODE`, `TTTB_PRODUCTION_DATABASE_URL`, `TTTB_LOCAL_DATABASE_URL` | Removed from code, README and script. |

`%LOCALAPPDATA%` and `%OneDrive%` are read as environment variables. No
known-folder crate is added; backups sit under the OneDrive root rather than
under the localised `Dokument` folder, which no environment variable exposes.

### Backups

**Where and what.** Backup directory per the table above, created if missing.
Names are UTC at **millisecond** precision — matching the convention
`scripts/capture-aggregates.ps1` already uses — with a numeric disambiguator
appended if the name is somehow already taken, so two launches in the same
millisecond cannot overwrite each other:

```
portfolio-20260829T143012457Z.sqlite                # launch snapshot
portfolio-20260829T143012457Z-2.sqlite              # collision retry (rare)
portfolio-20260829T143012457Z-premigration.sqlite   # mandatory, before pending migrations
portfolio-20260829T143012457Z.sqlite.partial        # in flight; never counted as a backup
portfolio-20260829T143012457Z.sqlite.failed         # integrity check failed; kept for diagnosis
```

The snapshot writer creates the `.partial` file with `create_new` semantics and
retries with the next disambiguator on collision, up to a small bound, so
naming can never silently clobber an existing snapshot.

**Sequence, synchronous, before the router serves and before the launch refresh
spawns** (the refresh writes prices, so a snapshot taken after it would not be
the ledger as found):

1. Resolve and create the backup directory.
2. `VACUUM INTO ?1` into `<name>.partial` (the `INTO` argument is an expression,
   so the path is bound, not interpolated).
3. Open the snapshot as a separate connection and run `PRAGMA integrity_check`
   and `PRAGMA foreign_key_check`. At ~4 MB both complete in well under a
   second; measure and record the real figure in the implementation notes.
4. Pass → rename to the final name. Fail → rename to `.failed`, log the
   integrity output through `engine_error!` with the ledger path and the
   snapshot path, and record a failed outcome.
5. Prune per the retention plan.
6. Record the outcome in `AppState` for `/api/health`.

#### Backup failure is one rule

There is exactly one contract, and it has no exceptions:

| Snapshot | Can it be disabled? | Failure behaviour |
|---|---|---|
| **Launch snapshot** | Yes — `TTTB_BACKUP_ENABLED=0` / `-NoBackup`, and structurally in demo | **Never fatal.** Any failure — unresolvable or unwritable directory, `VACUUM INTO` error, integrity failure — starts the application. `engine_error!` names the ledger, the target directory and the failing operation. `/api/health` reports `backup.launch_status = "failed"` with the reason; the footer shows a `--warning-soft` chip. Rationale: refusing to open the portfolio because a *copy* failed converts a backup problem into an availability outage, and the user can still see and fix it. |
| **Pre-migration snapshot** | **No.** No flag, no mode, no environment variable skips it. `TTTB_BACKUP_ENABLED=0` does not apply to it | **Fatal, before the migration runs.** `StartupError::PreMigrationBackupFailed` names the ledger, the backup directory and the failing operation. An unresolvable backup directory reaches the user here, not as a config error at parse time. Rationale: the next action would change schema irreversibly. |

Pruning failures on individual files are logged and never change either outcome —
the snapshot itself already succeeded. It is taken only when
`pending_migrations() > 0` **and** the ledger was not created by this process,
so an ordinary launch with an up-to-date schema, and a first `-InitLedger` run,
both skip it with nothing to protect.

**Retention ladder** — a pure planner, so the deletion rule is unit-tested
rather than observed in a live OneDrive folder:

```rust
pub struct RetentionPolicy {
    pub recent_launches: usize,           // 10  — the last ten launches, unconditionally
    pub weekly_weeks: usize,              //  8  — newest snapshot in each of the last 8 ISO weeks
    pub monthly_months: usize,            // 12  — newest snapshot in each of the last 12 months
    pub pre_migration_kept: usize,        // 10  — pre-migration snapshots have their own tier
    pub failed_kept: usize,               //  3  — failed-integrity files kept for diagnosis
    pub abandoned_partial_age: Duration,  //  1 hour — see below
}
pub fn plan_retention(files: &[SnapshotFile], now: DateTime<Utc>, policy: &RetentionPolicy) -> RetentionPlan;
```

**`.partial` files have a complete lifecycle.** A crashed or killed process
leaves one behind, and without a rule they would accumulate forever and falsify
the storage bound. The planner therefore **deletes any `.partial` whose parsed
timestamp is older than `abandoned_partial_age`** and leaves younger ones alone,
because a concurrent launch may legitimately be writing one. A `.partial` whose
name cannot be parsed is never touched.

**Safety rule: retention deletes only files in its own directory whose names it
can parse as its own snapshots.** Anything else is left alone and never counted.

Worst case ≈ 43 files × ~4 MB ≈ **170 MB** in OneDrive (10 recent + up to 8
weekly + up to 12 monthly survivors, plus 10 pre-migration and 3 failed), plus
at most one in-flight `.partial`. Overlap between the recent/weekly/monthly tiers
usually keeps the real figure well below that.

**Where "last backup" lives.** Derived from a directory listing at request
time — *not* stored in the database. A database-recorded timestamp cannot
witness the backup that contains it, and a listing is both simpler and honest.
Cost is a small directory read on a health request, which the footer fetches
once per mount.

### Health and footer surface

`/api/health` gains `mode`, `ledger` and `backup`, and **loses `demo`**:

```json
{
  "status": "ok",
  "version": "0.17.0",
  "mode": "production",
  "ledger": { "mode": "file", "path": "C:/Users/larsp/AppData/Local/TickerTapeTallyBoard/portfolio.sqlite" },
  "backup": {
    "directory": "C:/Users/larsp/OneDrive/TickerTapeTallyBoard/Backups",
    "last_snapshot_at": "2026-08-29T14:30:12.457Z",
    "snapshot_count": 12,
    "launch_status": "succeeded",
    "launch_error": null,
    "listing_error": null
  },
  "build": { "package": "ticker-tape-tally-board-backend", "profile": "release" }
}
```

`demo` is removed rather than kept alongside `mode`, because two wire fields
carrying one fact is exactly the duplication `Agents.md` forbids. The frontend
derives demo-ness in the existing pure view-model. `launch_status` is one of
`succeeded | failed | skipped | disabled`; `listing_error` keeps a missing value
explicit instead of rendering an absent folder as zero snapshots.

Exposing an absolute path on `/api/health` leaks the Windows user name to
anything that can reach the endpoint. With loopback-only binding enforced, the
reachable audience is processes on this machine; accepted, recorded in the
decision log, and to be revisited by the work that opens the port to the LAN.

**Frontend, three touches:**

1. `frontend/src/api/types.ts` — `HealthResponse` gains `mode`, `ledger`,
   `backup`; loses `demo`.
2. `frontend/src/components/appModeViewModel.ts` (+ `useAppMode.ts`) — takes
   `mode` instead of `demo`. Current semantics preserved exactly:
   `canMutate = mode !== undefined && mode !== "demo"`, so mutation stays
   disabled until health is known.
3. `frontend/src/components/appFooterViewModel.ts` (new, pure, Vitest-covered)
   + `AppFooter.tsx` — derives the mode chip, the ledger label and the backup
   label. The component renders; no derivation inline.

Footer reads, in the three modes:

```
UI 0.23.0 · API ok 0.17.0 · portfolio.sqlite · Backup today 14:30
UI 0.23.0 · API ok 0.17.0 · DEV · portfolio-dev.sqlite · Backup today 14:31
UI 0.23.0 · API ok 0.17.0 · DEMO · In-memory demo
```

- Production shows **no** mode chip — absence is the normal case, and the ledger
  file name already distinguishes it from the dev ledger. `DEV` and the existing
  `DEMO` chips mark the abnormal ones.
- Ledger shows the file name with the full path in a `title` tooltip.
- **The ledger label is derived from `ledger.mode` *and* `mode`, never from a
  null path alone.** `memory` + demo renders `In-memory demo`; `memory` outside
  demo renders `In-memory (unsaved)` — a state the backend now refuses to
  produce, rendered honestly rather than mislabelled as demo if it ever appears.
- Backup shows a relative-age label, tooltip "Snapshot written locally to the
  synced folder; OneDrive upload not verified." A failed launch backup renders
  as `Backup failed` in a `--warning-soft` chip.
- A pending or failed health query omits the ledger and backup spans entirely,
  matching how `apiStatusLabel` already behaves — never an empty span, never a
  stray separator.
- No new design tokens: `DEV` reuses the neutral outline chip style, the backup
  warning reuses `--warning-soft`, which `docs/VisualDesign.DarkTheme.md`
  already reserves for alerts.

### Fail-loud scope

`StartupError` is a typed enum carrying resolved values so one formatter
produces both the log line and the process exit message:

| Variant | Trigger | Message must name |
|---|---|---|
| `NonLoopbackHost` | `TTTB_HOST` is not `127.0.0.0/8` or `::1` | the value, and that LAN exposure is separate future work |
| `LedgerMissing` | ledger file absent, `CreateMissing::No` | the resolved path and `scripts/start.ps1 -InitLedger` |
| `LedgerMustBeFileBacked` | in-memory URL outside demo mode | the URL and the mode |
| `LedgerNotAFile` / `LedgerUnsupportedUrl` | path is a directory; non-sqlite URL | the value received |
| `LedgerOpenFailed` | pool open failed | path + driver error |
| `PreMigrationBackupFailed` | mandatory snapshot failed | ledger path, backup directory, failing operation |
| `MigrationFailed` | `sqlx::migrate` failed | ledger path + migration name |
| `StaticAssetsMissing` | production, `<static_dir>/index.html` absent or empty | resolved static dir + `npm run build` |
| `PortUnavailable` | bind failed | the socket address and `TTTB_PORT` |

A failed **launch** snapshot is deliberately absent from this table: it is a
warning surfaced in health and the footer, never a startup failure.

**Static assets:** the check is existence and non-emptiness of
`<static_dir>/index.html`, not the directory alone — an empty `dist/` would
otherwise pass and then serve nothing. **Mtime-based staleness detection is
deliberately rejected:** it is flaky across rebuilds and clock skew, the
production script always runs `npm run build` unless `-SkipBuild` is given (and
warns when it is), and the footer's UI-vs-API version pair is already the honest
staleness signal.

**Port:** the script pre-checks and names the occupying process via
`Get-NetTCPConnection` + `Get-Process` (falling back to `netstat -ano` parsing);
the backend's bind failure remains the source of truth. Production never scans
upward. `-Dev` and `-Demo` keep the existing scan.

**Not in scope, on purpose:** the 2026-08-29 market-data behaviours
(`previous_close_source_mismatch` day-change suppression, `ambiguous` /
`unavailable` refresh items being API- and log-only, `history_clamped`). They
are quiet by decision, and a refresh-problem UI is separate future work.

### `scripts/start.ps1` surface

```
scripts/start.ps1 [-Dev] [-Demo] [-InitLedger] [-NoBackup] [-NoRefresh]
                  [-DatabaseUrl <string>] [-Port <int>] [-FrontendPort <int>]
                  [-SkipInstall] [-SkipBuild] [-BuildOnly] [-NoBrowser]
```

| Flag | Effect |
|---|---|
| *(none)* | Production: `cargo build --release`, `npm run build`, run `target/release/…exe` with `TTTB_MODE=production` and an absolute `TTTB_STATIC_DIR`; one process; pinned port; browser opens `http://127.0.0.1:8480/`. |
| `-Dev` | `cargo build` (debug) + `npm run dev`; `TTTB_MODE=development`; port may scan upward; orphan-Vite cleanup runs; browser opens the Vite URL. `npm run build` is skipped — Vite serves the UI. |
| `-Demo` | `TTTB_MODE=demo`. Composes with `-Dev`; alone it uses the production serving model, so the demo is presentation-ready. Port may scan (no ledger to protect). |
| `-InitLedger` | `TTTB_CREATE_LEDGER_IF_MISSING=1`. Applies to whichever ledger the active mode resolves. |
| `-NoBackup` | `TTTB_BACKUP_ENABLED=0` — **disables the launch snapshot only.** The mandatory pre-migration snapshot still runs, and still blocks startup if it fails. The help text says so. |
| `-NoRefresh` | `TTTB_MARKET_DATA_LAUNCH_REFRESH_ENABLED=0`. |
| `-DatabaseUrl`, `-Port` | Explicit overrides, replacing `-ProductionDb` / `-LocalDatabaseUrl` / `-ProductionDatabaseUrl`. |
| Retired flags | `-ProductionDb`, `-LocalDatabaseUrl`, `-ProductionDatabaseUrl` remain declared but **throw immediately** with the replacement command, rather than producing PowerShell's unhelpful unknown-parameter error. Marked for deletion once muscle memory has moved. |

`-NoBackup` and `-NoRefresh` exist for the restore drill: a second instance on a
restored copy must neither pollute the retention ladder nor write fresh prices
into the copy it is being measured against.

Also: a **legacy-name guard** — if `.local/db/tttb-ledger-test.sqlite` still
exists, the script refuses to start and points at `scripts/migrate-ledger.ps1`
rather than quietly running against a different file.

**The script blocks until Ctrl+C.** Any procedure that needs two instances (the
restore drill) needs two PowerShell windows; this is stated wherever such a
procedure is documented.

### Logging

**`engine.log` moves out of the repository tree** to
`%LOCALAPPDATA%\TickerTapeTallyBoard\logs\`, with a mode-suffixed name so a dev
run cannot pollute the production timeline. `engine_logging::initialize` becomes:

```rust
pub struct LogSettings { pub file_path: PathBuf, pub max_bytes: u64, pub kept_rotations: usize, pub terminal: bool }
pub struct LogInitOutcome { pub file_path: Option<PathBuf>, pub file_error: Option<String> }
pub fn initialize(settings: &LogSettings) -> LogInitOutcome;
```

Failure to open the log **never panics** — the current `.expect("Failed to open
engine.log")` is replaced by an outcome the caller reports; the app starts with
terminal logging only. The directory is created if missing.

**Rotation under simplelog.** simplelog has no rotation, but `WriteLogger` takes
any `Write`, so `engine_logging` supplies a `RotatingFileWriter` that owns the
path, a byte counter, `max_bytes` (5 MiB) and `kept_rotations` (3) — at most
~20 MiB total. Rotation is checked on write and applied at whole-record
boundaries. The rename/delete decision is a **pure planner**
(`plan_rotation(base, kept) -> RotationPlan`) so it is unit-tested without IO;
the writer itself gets an integration test with a tiny cap against a temp
directory.

Because the config must be parsed before the log path is known, `app::run()`
orders config → logging → ledger → serve, and a config error prints to stderr
before logging exists. `main.rs` stays a thin `ExitCode` wrapper.

**Start-script logs** stop being deleted on every start. Each run writes to
`.local/logs/run-<UTC timestamp>/{backend,frontend}.{out,err}.log`, and the
script prunes to the newest 5 `run-*` directories. Pruning touches only
`run-*` directories, so the existing `connectivity-probe.log` and
`nasdaq-nordic-probe.log` are never removed.

### The one-time ledger migration

`scripts/migrate-ledger.ps1` — a script, not pasted commands, so it is
repeatable, refuses unsafe states, and can be re-read later.

1. Refuse if anything is listening on the backend port, or if a
   `ticker-tape-tally-board-backend` process is running.
2. Refuse if the target already exists (idempotent by refusal, never by
   overwrite).
3. Record size and SHA-256 of `.local/db/tttb-ledger-test.sqlite`.
4. Copy `.sqlite`, `.sqlite-wal`, `.sqlite-shm` (whichever exist) to
   `%LOCALAPPDATA%\TickerTapeTallyBoard\pre-move-backup\` as a safety copy.
5. Create `%LOCALAPPDATA%\TickerTapeTallyBoard\` and **move all three files
   together** to `portfolio.sqlite[-wal|-shm]`. Moving the sidecars with the
   database is what keeps SQLite's recovery correct.
6. Re-record size and SHA-256 of the moved `.sqlite` and fail if either differs.
7. Print the resulting paths and the next command to run.

The safety copy is kept until the restore drill passes, then deleted by hand as
a documented step.

### Dev ledger provenance

The old test-named file is **retired, not repurposed**. After the move it exists
only as the `pre-move-backup` safety copy and as OneDrive snapshots. The
development ledger is a **fresh, explicitly created, empty** database at
`%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio-dev.sqlite`, created once with
`scripts/start.ps1 -Dev -InitLedger`. Leaving real portfolio data sitting in a
ledger whose whole purpose is destructive experimentation is precisely the
hazard this plan exists to remove. Realistic dev data comes from `-Demo`, or by
deliberately copying a backup snapshot onto the dev path — a documented,
explicit act.

## Phases

Plans are ephemeral. Durable documents and code must name behaviours, never
these phase numbers.

**Decision-log entries land only in the phase where every sentence in them is
already shipped truth.** Committed entries are never edited afterwards, so an
entry that describes a not-yet-true state would be permanently wrong. The
schedule is: mode/startup contract → Phase 1; run model → Phase 3 (after the
human gate that proves it); logs → Phase 5; ledger location → Phase 6 (after
relocation); backups and the restore procedure → Phase 7 (after the drill has
actually been executed).

**Verification commands.**

| Scope | Commands |
|---|---|
| Backend | from `backend/`: `cargo build`, `cargo test`, then `cargo clippy --all-targets -- -D warnings` and `cargo fmt` |
| Frontend | from `frontend/`: `npm run check` (covers `tsc --noEmit`, Biome and Vitest), then `npm run fmt` |
| Scripts | from the repository root, in PowerShell |

When launching npm through `Start-Process`, use `npm.cmd` explicitly.

---

### Phase 1 — Startup contract: mode, host, ledger identity, observability

The smallest end-to-end slice: the app knows, enforces and shows what it opened.
Nothing moves on disk yet, and the everyday run keeps working because
`scripts/start.ps1` still passes the legacy URL explicitly.

**Backend**

1. `config.rs`: add `Mode` (`TTTB_MODE`, default `production`), retire
   `TTTB_DEMO_MODE`, add `create_ledger_if_missing`
   (`TTTB_CREATE_LEDGER_IF_MISSING`, default `false`) and `AssetPolicy`
   (`Required` in production, `Optional` otherwise). Replace the CWD-relative
   `DEFAULT_DATABASE_URL` with the mode-dependent absolute app-data defaults.
   Port stays 8080 in this phase.
2. **Loopback enforcement:** `parse_host` additionally rejects any non-loopback
   address with a message naming the value and pointing at the separate LAN
   work. The existing `from_env_uses_hosting_port_when_tttb_port_is_missing`
   test currently pins `0.0.0.0` as valid and must be changed to a loopback
   value, with a new test covering the rejection.
3. `ledger/location.rs`: `LedgerLocation { url, path: Option<PathBuf> }`,
   `resolve(url, mode)`, `memory()`, `LedgerLocationError`. `AppConfig` resolves
   to `memory()` whenever the mode is demo, **before any filesystem work**, so
   `LedgerMissing` is structurally unreachable in demo. Outside demo an
   in-memory URL is rejected as `LedgerMustBeFileBacked`.
4. Split `db/pool.rs` into `open(location, CreateMissing) -> OpenedLedger`,
   `pending_migrations` and `migrate`; add `ledger/open.rs` composing them.
   Pending-migration discovery is runtime-query based and treats an absent
   `_sqlx_migrations` table as "all embedded migrations pending".
5. `startup_error.rs` with the variants above and one operator-facing formatter.
   `app::run()` sequences config → logging → ledger → serve; `main.rs` becomes a
   thin `ExitCode` wrapper.
6. `AppState` gains `mode` and `ledger_path`. `/api/health` serializes `mode` and
   `ledger`, and drops `demo`.
7. Static-asset policy: production refuses to start when
   `<static_dir>/index.html` is absent or empty; development warns and serves
   API-only exactly as today.

**Frontend**

8. `HealthResponse` type; `appModeViewModel`/`useAppMode` take `mode`;
   new pure `appFooterViewModel.ts` (mode chip, ledger label + tooltip);
   `AppFooter` renders them.

**Script**

9. `-InitLedger` added. The script continues to set `TTTB_DATABASE_URL` to
   `.local/db/tttb-ledger-test.sqlite` and now sets `TTTB_MODE=development` for
   its (still debug + Vite) default run, so this phase changes no run behaviour.
   `-Demo` sets `TTTB_MODE=demo` instead of `TTTB_DEMO_MODE=1`.

**Docs**

10. `README.md` configuration list updated for `TTTB_MODE`,
    `TTTB_CREATE_LEDGER_IF_MISSING`, the loopback rule and the retired
    `TTTB_DEMO_MODE`.
11. **Decision-log entry lands here:** *Explicit Application Mode, Loopback-Only
    Binding, And A Ledger That Is Never Created By Accident*. Every sentence in
    it is true at the end of this phase; the ledger's new *location* is a
    separate entry that waits for Phase 6.

**Tests**

- `config.rs` (extending the existing `TestEnv` env-lock pattern): mode parsing
  and rejection of unknown values; per-mode ledger defaults; demo ignores
  `TTTB_DATABASE_URL`; `create_ledger_if_missing` default is `false`;
  `AssetPolicy` per mode; loopback accepted (`127.0.0.1`, `127.0.0.2`, `::1`)
  and non-loopback rejected (`0.0.0.0`, a LAN address).
- `ledger/location.rs`: absolute URL passes through; a non-sqlite URL is
  rejected; `sqlite::memory:` yields `path: None` **in demo only** and is
  rejected in production and development.
- `db::pool::open` with `CreateMissing::No` against a non-existent path returns
  an error **and leaves no file behind** (assert absence afterwards).
- `pending_migrations` against a migrated pool returns 0; against a
  pre-existing, never-migrated database returns the full embedded count;
  `open` reports `created_now` correctly for both a new and an existing file.
- `ledger::open` with `CreateMissing::Yes` creates and migrates.
- Demo config with `TTTB_DATABASE_URL` pointing at a non-existent path builds
  successfully and touches no file.
- `/api/health` contract: `mode` and `ledger` for a file-backed state;
  `memory`/`null` for demo; **no `demo` key**, so it is not reintroduced.
- Static-asset policy: `Required` with an empty dist directory fails;
  `Optional` warns and builds an API-only router.
- Vitest: `appFooterViewModel` renders the file name for a file ledger,
  `In-memory demo` for memory+demo, `In-memory (unsaved)` for memory outside
  demo, the `DEV` chip only in development, and never an empty span or stray
  separator; a pending/failed health query omits the spans. `appModeViewModel`
  keeps mutation disabled while `mode` is `undefined`.

**Verify**

- Backend and frontend command sequences, whole suite green.
- `scripts/start.ps1 -SkipInstall -SkipBuild` starts as before; the footer shows
  `DEV` and `tttb-ledger-test.sqlite` with the full path on hover.
- `TTTB_DATABASE_URL` pointed at a non-existent path fails to start and creates
  nothing; adding `-InitLedger` creates and migrates one.
- `TTTB_HOST=0.0.0.0` fails to start with the loopback message.
- `scripts/start.ps1 -Demo` shows `DEMO` and `In-memory demo`, no path.
- **External human testing recommended:** one ordinary session — dashboard,
  board, an asset page, import page — confirming nothing else moved.

---

### Phase 2 — Production run model

The everyday command becomes the documented production composition. The ledger
is still the legacy file (the script still passes its URL), so this phase changes
*how* the app runs, not *what data* it opens — one variable at a time.

1. `config.rs`: `DEFAULT_PORT` becomes `8480`.
2. `scripts/start.ps1` restructured to the surface in *Architecture*: production
   default (release build, `npm run build`, single process, absolute
   `TTTB_STATIC_DIR`, pinned port), `-Dev`, `-Demo` composition, `-NoBackup`,
   `-NoRefresh`, `-DatabaseUrl`, `-Port`, throwing stubs for the three retired
   flags. Orphan-Vite cleanup and frontend-port resolution run in `-Dev` only.
3. Pinned-port failure: production pre-checks the port and, when busy, throws
   naming the occupying process id and name; no scan-upward. `-Dev`/`-Demo`
   retain `Resolve-BackendPort`'s scan.
4. Coupled port references updated in the same change — this is the phase where
   8080 stops being true anywhere:
   - `frontend/vite.config.ts` fallback `8080` → `8480`.
   - `scripts/capture-aggregates.ps1` `-Port` default → `8480`.
   - `scripts/probe-connectivity.ps1`, `docs/Design.HighLevel.md` curl examples,
     `backend/src/providers/nasdaq_nordic.rs` comment, `README.md`.
     Grep for `8080` and leave none stale.
5. `README.md`: the start-flow section rewritten around the new default,
   `-Dev`, `-Demo`, the pinned port and the retired flags, including the note
   that the script blocks and a second instance needs a second window.
6. No decision-log entry yet — the run-model entry waits for the Phase 3 gate
   that proves the composition actually works.

**Tests**

- `config.rs`: default port is 8480; `TTTB_PORT` still wins over `PORT`.
- Existing suites unchanged and green.

**Verify**

- Backend/frontend command sequences.
- `scripts/start.ps1 -BuildOnly` completes a release build. First release build
  is slow (several minutes); expected, and noted in the README.
- `scripts/start.ps1 -Dev` still gives the debug + Vite loop with HMR.
- Occupy 8480 with a dummy listener and confirm the production start fails with
  a message naming the occupying process, and that nothing was started.
- `scripts/start.ps1 -Demo` opens the seeded demo through the release/static
  composition.

---

### Phase 3 — Release composition smoke (human gate)

**External human testing REQUIRED.** The release binary serving `frontend/dist`
has never actually run — everything to date has been debug + Vite. This phase
adds no product code; its output is a pass/fail record in the implementation
notes plus one decision-log entry. If something here fails, fix it before
proceeding rather than routing around it.

Run `scripts/start.ps1` (production default, still on the legacy ledger) and
check, in the browser at `http://127.0.0.1:8480/`:

1. **SPA deep links, entered directly in the address bar and reloaded:** `/`,
   `/board`, `/asset/<id>`, `/import`. The static-index fallback must serve the
   app, not a 404 — this is the property Vite provided for free until now.
2. **Browser Back/Forward** across those routes.
3. **Full import round trip:** an Avanza export through preview → commit →
   verify → rollback, including a file large enough to be realistic. (Note: the
   implicit ~2 MB axum body limit is a known latent defect owned by
   `Plan.NativeDesktopWindow.md`, not fixed here. If an import fails with a bare
   `413`, record it and continue — it is that plan's work, not a regression from
   this one.)
4. **Manual refresh** and the launch refresh indicator.
5. **Charts:** dashboard value chart, treemap, per-asset price chart with
   markers and the break-even line.
6. **Footer** shows UI and API versions, the ledger name, and no `DEV` chip.
7. Static asset caching/content types behave (no missing fonts, no unstyled
   flash).

Then confirm the fail-loud paths by hand: rename `frontend/dist` aside and
confirm production start refuses with a message naming the directory and
`npm run build`.

**Decision-log entry lands here, once the gate passes:** *Production Run Model
And Pinned Loopback Port*.

---

### Phase 4 — Launch and pre-migration backups

With the app running in production mode, backups default on and land in OneDrive
against the real ledger.

1. `ledger/backup.rs`: `VACUUM INTO` snapshot + `PRAGMA integrity_check` +
   `PRAGMA foreign_key_check`, with the collision-safe `create_new` `.partial` →
   final / `.failed` rename protocol and millisecond-precision names.
2. `ledger/retention.rs`: `SnapshotFile` parsing and the pure `plan_retention`
   planner with the policy above, **including abandoned-`.partial` reclamation**.
3. `ledger/backup_status.rs`: derive `last_snapshot_at`, `snapshot_count` and
   `listing_error` from a directory listing.
4. `ledger/open.rs`: take the **mandatory** `-premigration` snapshot when
   `pending_migrations() > 0` and `!created_now`, **before** `migrate`; a failure
   there is `StartupError::PreMigrationBackupFailed` and is not skippable by any
   flag. Take the launch snapshot after migrations and before the router serves
   / the launch refresh spawns, honouring `TTTB_BACKUP_ENABLED`. Prune once,
   after both.
5. `config.rs`: `TTTB_BACKUP_ENABLED` (default: mode ≠ demo, and scoped to the
   launch snapshot only) and `TTTB_BACKUP_DIR` with the per-mode defaults.
   **An unresolvable backup directory is not a config error** — it is carried as
   an unresolved value that fails the launch snapshot with a visible warning and
   fails startup only through a pending pre-migration snapshot.
6. `AppState` records the launch outcome; `/api/health` gains `backup`.
7. Frontend: `backup` in `HealthResponse`; `appFooterViewModel` gains the backup
   label, relative-age formatting, the honesty tooltip and the failed-backup
   warning tone; `AppFooter` renders it.
8. `docs/VisualDesign.DarkTheme.md`: one line under *Badges / chips* recording
   the footer's `DEV` (neutral) and failed-backup (`--warning-soft`) chips as
   uses of existing tokens.
9. `README.md`: a "Backups" section — location, cadence, retention ladder, the
   one failure rule (launch snapshot warns, pre-migration snapshot blocks and
   cannot be disabled), the honesty rule, and that snapshots are unencrypted
   SQLite files in OneDrive.
10. No decision-log entry yet — the backup entry claims an executed restore
    procedure and therefore waits for Phase 7.

**Tests**

- Snapshot against a temp ledger: a real file appears, opens, and passes
  integrity check; the `.partial` name does not survive a success.
- Collision safety: a pre-existing target name causes the next disambiguator to
  be used and never an overwrite.
- A deliberately corrupted snapshot target yields a `.failed` file, a failed
  outcome, and **no** final-named file.
- `plan_retention` pure tests: the ten most recent launches survive regardless of
  age; one per ISO week for eight weeks; one per month for twelve; pre-migration
  and failed files obey their own tiers; **a `.partial` older than the abandoned
  age is deleted and a younger one is kept**; unparsable names are never in the
  delete set; an empty directory yields an empty plan.
- `backup_status` derives a null timestamp and zero count for an empty or
  missing directory, with `listing_error` populated when unreadable, and never
  counts `.partial` or `.failed` files as backups.
- `ledger::open`: the pre-migration snapshot is taken exactly when migrations
  are pending on a pre-existing ledger; it is **not** taken for a
  just-created ledger; **it is still taken when `TTTB_BACKUP_ENABLED=0`**; and
  its failure refuses the start.
- A launch-snapshot failure — including an unresolvable/unwritable backup
  directory — yields a successful startup with `launch_status = "failed"`.
- Demo builds with both snapshots structurally skipped and no directory created.
- `/api/health` contract test for `backup` in succeeded / failed / disabled
  states.
- Vitest: backup label for a fresh snapshot, an old snapshot, a failed launch,
  and a missing directory — the last never rendering as "0 backups" without the
  error.

**Verify**

- Backend/frontend command sequences.
- **External human testing recommended:** run `scripts/start.ps1`, then confirm
  a new timestamped `.sqlite` appeared in
  `%OneDrive%\TickerTapeTallyBoard\Backups`, that OneDrive shows it as synced,
  and that the footer's backup label matches. Run the app several times and
  confirm the count stops growing at the recent-launch tier. Point
  `TTTB_BACKUP_DIR` at an unwritable path and confirm the app **still starts**,
  the footer shows the warning chip, and `engine.log` names the directory.
- Record the measured snapshot + integrity-check duration in the implementation
  notes.

---

### Phase 5 — Log locations and retention

1. `engine_logging`: `LogSettings` / `LogInitOutcome`, non-panicking file open,
   directory creation, the pure `plan_rotation` planner and `RotatingFileWriter`
   (5 MiB cap, 3 kept rotations).
2. `config.rs`: `TTTB_LOG_FILE` with the mode-suffixed app-data defaults.
3. `app::run()` reports a log-open failure through the terminal logger and a
   startup banner line rather than failing.
4. Startup banner (one `engine_info!`): mode, resolved ledger path (or
   `in-memory (demo)`), backup directory, static assets directory, log path,
   listen address. An absent ledger path is a normal state, never an empty field.
5. `scripts/start.ps1`: per-run log directories under `.local/logs/run-<stamp>/`
   with pruning to the newest 5; the delete-on-start behaviour is removed.
6. Delete the stray `backend/engine.log` if present; leave the `.gitignore`
   `engine.log` rule in place as cheap insurance.
7. `README.md`: where logs live now, and how much is kept.
8. **Decision-log entry lands here:** *Runtime Logs Live Outside The Repository
   And Are Bounded*.

**Tests**

- `plan_rotation` pure tests: descending renames, deletion beyond the kept
  count, a fresh path yielding no renames.
- `RotatingFileWriter` integration test with a tiny cap in a temp directory:
  file count is bounded, the base file holds the newest lines, and no record is
  split across a rotation.
- `initialize` with an unwritable path returns an outcome carrying the error and
  does not panic.

**Verify**

- Backend command sequence.
- Run the app; confirm `engine.log` appears under app-data with the startup
  banner, and that no `engine.log` appears in the repository tree.
- Run the app five times; confirm `.local/logs/` holds five `run-*` directories,
  that the probe logs are untouched, and that a sixth run prunes the oldest.

---

### Phase 6 — Relocate the live ledger

Now that backups exist and the run model is proven, move the real data. The app
must be stopped throughout.

1. `scripts/migrate-ledger.ps1` as specified in *Architecture*.
2. `scripts/start.ps1` stops setting `TTTB_DATABASE_URL` in production and
   development modes — the backend config becomes the only definition — and
   gains the legacy-name guard pointing at the migration script.
3. Execute the migration once, for real.
4. Create the development ledger: `scripts/start.ps1 -Dev -InitLedger` once,
   producing an empty `portfolio-dev.sqlite`.
5. `README.md`: the ledger location, first-run `-InitLedger` (fresh clone and
   new dev ledger both need it once), the retired flags, and the statement that
   the old test-named file is retired rather than reused.
6. **Decision-log entry lands here:** *Production Ledger Location Is
   Application-Owned Configuration*. It waits until now because it asserts that
   the launch script no longer carries the path and that the production ledger
   lives in app-data — neither of which was true before this phase.

**Tests**

- No new automated tests; this is data movement over behaviour that Phase 1
  already pinned. The pass criterion is the human verification below and the
  drill in Phase 7.

**Verify**

- **External human testing REQUIRED (data safety).**
  1. Stop everything. Confirm a recent OneDrive snapshot exists and is synced.
  2. Run `scripts/migrate-ledger.ps1`; confirm the reported hashes match and the
     safety copy exists under `pre-move-backup`.
  3. Run `scripts/start.ps1` with no flags; the footer shows `portfolio.sqlite`
     at the app-data path and no `DEV` chip.
  4. Spot-check the dashboard total, the holdings count and the transaction
     count against what the app showed before the move.
  5. Re-run `scripts/start.ps1` from a shell where the legacy file has been
     recreated as an empty file, and confirm the legacy guard refuses to start.
     Delete the decoy afterwards.
  6. `scripts/start.ps1 -Dev` opens an empty dev ledger and shows the `DEV` chip
     and `portfolio-dev.sqlite`.
- Keep the `pre-move-backup` copy until Phase 7 passes.

---

### Phase 7 — Restore drill: documented and executed

The falsifiable proof that a snapshot is actually a portfolio.

1. `scripts/capture-aggregates.ps1` gains **`-FailOnChange`** for diff mode:
   with it, the script exits non-zero when any gains row or value-history point
   is added, removed or changed. Without it, diff mode behaves exactly as today
   (the Nasdaq backfill work legitimately expected changes and read the output).
   This removes the drill's dependence on a human reading a summary correctly.
2. Document the drill in `README.md` under "Backups and restore", as a numbered
   procedure a future reader can run without reconstructing it.
3. Execute it once, for real, and paste the diff summary and exit code into the
   implementation notes.

**Why the naive version does not measure what it claims.** A snapshot taken at
launch reflects the ledger *before* the launch refresh; a capture taken later
reflects the ledger *after* it, and after any user writes. Comparing those two
would report differences that have nothing to do with restore fidelity. The
drill therefore makes the snapshot and the captured state provably identical by
disabling the launch refresh on the live instance and capturing immediately,
with no writes in between. All three captured endpoints are `GET`, so capturing
itself writes nothing.

**The drill.** Two PowerShell windows are needed, because `scripts/start.ps1`
blocks until Ctrl+C.

```powershell
# --- Window 1 ---------------------------------------------------------------
# 1. Start production with launch refresh OFF. Its launch snapshot is therefore
#    byte-for-byte the state it is about to serve.
.\scripts\start.ps1 -SkipInstall -SkipBuild -NoBrowser -NoRefresh

# 2. In a second window, capture from it immediately; make no writes.
.\scripts\capture-aggregates.ps1 -EndDate 2026-08-29
#    -> note the capture directory, e.g. .local/aggregates/capture-A

# 3. Stop the app in window 1 (Ctrl+C).

# 4. Identify the snapshot that launch just wrote, and restore it into a FRESH
#    scratch directory. Removing the directory first matters: a stale -wal/-shm
#    left from an earlier drill would be replayed over the restored file.
$src = Get-ChildItem "$env:OneDrive\TickerTapeTallyBoard\Backups\portfolio-*.sqlite" |
       Where-Object { $_.Name -notlike "*premigration*" } |
       Sort-Object Name -Descending | Select-Object -First 1
$drill = "$env:LOCALAPPDATA\TickerTapeTallyBoard\restore-drill"
Remove-Item -Recurse -Force $drill -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $drill | Out-Null
Copy-Item $src.FullName "$drill\portfolio.sqlite"

# 5. Serve the restored copy from a second instance, with no side effects.
#    This window blocks; leave it running.
.\scripts\start.ps1 -SkipInstall -SkipBuild -NoBrowser -NoBackup -NoRefresh -Port 8481 `
  -DatabaseUrl "sqlite:///$env:LOCALAPPDATA/TickerTapeTallyBoard/restore-drill/portfolio.sqlite"

# --- Window 2 ---------------------------------------------------------------
# 6. Capture from the restored instance with the SAME -EndDate and -Method.
.\scripts\capture-aggregates.ps1 -Port 8481 -EndDate 2026-08-29
#    -> capture-B

# 7. Diff, failing the shell on any difference.
.\scripts\capture-aggregates.ps1 -BeforeDirectory <capture-A> -AfterDirectory <capture-B> -FailOnChange
$LASTEXITCODE   # must be 0
```

**Pass criterion:** `-FailOnChange` exits 0 — zero changed, added or removed
gains rows and zero changed value-history points.

`-NoBackup` and `-NoRefresh` on the restored instance are not optional: without
them it would pollute the retention ladder and write fresh prices into the very
copy it is being measured against. `-NoBackup` does **not** disable the
mandatory pre-migration snapshot; if the chosen snapshot predates a schema
change, that instance will legitimately take one — and such a snapshot is not a
clean measurement anyway, so always drill with a snapshot from the current
schema.

**Verify**

- **External human testing REQUIRED.** The drill is the verification.
- **Decision-log entry lands here, once the drill passes:** *Launch-Time Ledger
  Backups With Integrity Check, Tiered Retention, And A Verified Restore*. It
  waits until now because it asserts the restore procedure has been executed
  against real data.
- On pass: delete the `pre-move-backup` safety copy and the `restore-drill`
  directory, and record the drill date and exit code in the implementation notes.
- On fail: stop. Do not delete the safety copy. A non-zero diff here means
  either the backup or the relocation is wrong, and that is a finding to raise,
  not to route around.

---

### Phase 8 — Documents, versions, and the desktop-plan rebase

1. `docs/Design.HighLevel.md`:
   - **Goals / deployment model:** "Accessible from any device on the LAN via
     browser" and "reachable at `http://<host>:8080` on the LAN" are no longer
     true — binding is loopback-only and enforced. Rewrite both to describe the
     current reality and point at `docs/plans/Design.mobile-view-design.md` as
     the work that lifts it, with authentication.
   - **Deployment model** rewritten: production is a release binary serving
     `frontend/dist` from disk on a pinned loopback port; one executable, one
     database in app-data, launch-time snapshot backups to a synced folder.
     Remove "embedded" as the production option (2026-06-13 chose disk) and
     remove "Windows scheduled task or service" (out of scope).
   - **Phase 5 — Hardening & deployment**: mark the backup/retention, log and
     error-surfacing items done, with auto-start and LAN still open.
   - **Acceptance criterion 5** annotated as demonstrated by an executed restore
     drill.
   - Risk table: one row for the ledger path exposed on `/api/health`.
   - Confirm no `8080` references remain (Phase 2 did the sweep).
2. `README.md` final sweep: start flows, first run and `-InitLedger`, the
   configuration table, the loopback rule, backups and the single failure rule,
   the restore drill, log locations, retired flags and environment variables,
   and the two-window note.
3. `docs/plans/Design.mobile-view-design.md`: one line stating that its `-Lan`
   work must lift the loopback-only enforcement introduced here, together with
   the authentication that makes lifting it safe.
4. `docs/DecisionLog.md`: confirm the five entries landed with their phases and
   read correctly end to end (no plan phase numbers, behaviours named).
5. **Rebase `docs/plans/Plan.NativeDesktopWindow.md`.** Editing only its ledger
   decisions would leave instructions that revert this hardening or block its
   own implementation. Every item below is edited; its workspace, bridge, CSP,
   body-limit, probe and lease work is left untouched.
   - *Landing order*: record that this plan landed, and that versions are now
     `0.17.0` / `0.23.0`.
   - *Settled decisions* **8, 9, 10, 11, 17, 21, 22** per the merge-contract
     table; **13** (its server exemptions for `engine.log` and the ledger are now
     false — only `TTTB_STATIC_DIR`'s default remains CWD-relative, and the
     launch script passes it absolutely); **19** (demo is `TTTB_MODE=demo`, not
     `TTTB_DEMO_MODE`); **25** (rebase the per-line instance tag onto the
     existing `LogSettings`/`RotatingFileWriter`; it must not reintroduce an
     unbounded file, a CWD-relative path, or a competing `LogDestination` API).
   - *Architecture → One composition root, two entry points*: `app::run()`,
     `ledger::open` and a typed `StartupError` already exist; the split rebases
     onto them and its `StartupError` sketch is replaced by "extend the existing
     enum". `AssetPolicy` already exists — desktop sets `Required`.
   - *Architecture → Nothing in the desktop process is located relative to the
     working directory*: rewrite the closing paragraph about the server's
     CWD-relative semantics.
   - *Architecture → Ledger identity is observable, and never silently created*:
     mark landed; what remains is the `db/mod.rs` tidy (`memory_pool` and
     `RepoError` moved out so it is a thin wrapper) and the desktop's
     `from_env_anchored` for the **static dir and log only**.
   - *Architecture → Startup failure, logging, and shutdown*: rebase the logging
     paragraph; keep the native dialog and shutdown work.
   - *Frontend — exactly two touches*: the `HealthResponse`/footer touches are
     already made; the section becomes "no frontend change remains for ledger
     identity", with the `localStorage`-per-origin consequence retained.
   - *Phase 1 step 0* re-verification list: `AppConfig` already carries a
     `LedgerLocation` and a mode; `create_if_missing` is already gated;
     `engine_logging` already takes settings, never panics and rotates;
     `app.rs` no longer opens the ledger inline; the port is already 8480; the
     highest migration is `0007`.
   - *Phase 3, Phase 4, Phase 5*: drop the `repo-paths.json` reader from both
     `scripts/start.ps1` and `desktop/build.rs`; derive the desktop log path
     from the app-data convention; mark Phase 4 mostly landed with the residue
     named.
   - *Phase 5 launch mode*: `-Desktop` composes with the **new** flag surface
     (`-Dev`, `-Demo`, `-InitLedger`, `-NoBackup`, `-NoRefresh`,
     `-DatabaseUrl`, `-Port`); `-ProductionDb` is gone.
   - *Documents to update* table: remove the `repo-paths.json` row; adjust the
     `README.md`, `Agents.md` and `Design.HighLevel.md` rows for what this plan
     already changed.
   - *Deliberately out of scope*: remove "a tested database backup/restore
     procedure"; note the per-user app-data ledger location is now the reality
     rather than an excluded idea.
   - *Risks*: the ledger-path exposure risk is already accepted and recorded
     here; cross-reference rather than duplicate.
   - Leave its migration-numbering note as is — this plan adds no migration.
6. Version bumps: `backend/Cargo.toml` `0.16.0` → `0.17.0`;
   `frontend/package.json` `0.22.13` → `0.23.0` (+ `package-lock.json`).
7. Confirm `Agents.md` needs no change: `cargo build` from `backend/` and the
   two-version display clause both remain true. Recorded as a deliberate
   non-action.

**Verify**

- Backend and frontend command sequences.
- `/api/health` reports `0.17.0` and the footer reports both new versions.
- Read each decision-log entry against the shipped behaviour; anything that does
  not match is a bug in one of the two.
- Re-read `Plan.NativeDesktopWindow.md` end to end and confirm no instruction in
  it would revert this plan's behaviour.

## Decision-log entries (drafts — each lands in the phase named above)

**Phase 1:**

```
## YYYY-MM-DD - Explicit Application Mode, Loopback-Only Binding, And A Ledger That Is Never Created By Accident
Decision: The application runs in exactly one of three explicitly configured
modes — production, development or demo — and that single value selects the
database, the backup policy, the log destination, and whether missing built
frontend assets are fatal. Mode is never inferred from a path or a build profile.
The server binds only to a loopback address; a non-loopback host is a startup
failure. Outside demo the database must be file-backed, and a missing database
file is a startup failure naming the resolved path, never a newly created empty
file — creating one requires an explicit opt-in. Demo resolves to an in-memory
database before any filesystem work and therefore has no path. The resolved path
and the active mode are reported by the health endpoint and rendered in the app
footer.
Context: The database the application opened, and the assumptions it was running
under, were invisible; a wrong path silently produced an empty database that
looked like a working one. Binding was documented as local-only but accepted any
address, so the 2026-06-12 decision to trust the LAN without authentication was
being honoured by convention rather than by the code. Inferring the mode from
which path happened to be resolved would have made every fail-loud behaviour
depend on string comparison.
Consequences: This narrows the 2026-06-12 Phase 0 Planning Decisions commitment:
the application is not reachable from the LAN at all until the separate mobile
work adds both an explicit opt-in and authentication. A fresh checkout, and any
run pointed at a database that does not exist yet, needs the explicit creation
step once. The reported database path contains the operating-system user name and
is readable by any process on the machine that can reach the health endpoint.
```

**Phase 3:**

```
## YYYY-MM-DD - Production Run Model And Pinned Loopback Port
Decision: The default launch is the production composition — a release build of
the backend serving the built frontend from disk as a single process on a fixed,
uncommon loopback port. The development composition, a debug build plus the Vite
dev server, is an explicit opt-in. The production port is never silently
reassigned: a busy port fails the start with the occupying process named.
Development and demo may still fall back to the next free port, because
development protects only a development database and demo opens none. Production
refuses to start when the built frontend is absent; development serves API routes
only, as before.
Context: The everyday command built a release-shaped artifact and then discarded
it, running a debug binary behind a dev server instead, so the documented
production model had never actually executed. Silent port reassignment on a
machine where the previous default is frequently occupied hides the fact that two
copies are running, which is the failure that matters while one database is
shared.
Consequences: The origin change resets browser-stored view preferences once,
accepted without migration. A pinned port is the interim guard against two
backends sharing one database; it does not stop a manually launched second
backend, and a per-database coordination mechanism remains separate future work.
Frontend proxy configuration, capture tooling and documented examples all follow
the one port value. The launch script blocks, so any procedure needing two
instances needs two shells.
```

**Phase 5:**

```
## YYYY-MM-DD - Runtime Logs Live Outside The Repository And Are Bounded
Decision: The backend log file lives in the per-user local application-data
directory, named for the running mode, and is size-capped with a small number of
retained rotations rather than appended to forever. Failing to open the log never
prevents startup. Every start writes one banner line naming the mode, database,
backup directory, static assets directory, log path and listen address. The
launch script keeps a small number of recent per-run log directories instead of
deleting the previous run's output on every start.
Context: The log was opened relative to the working directory, so it accumulated
inside the repository tree without bound, and the launch script destroyed the
evidence of the previous run at the moment a user would want to look at it.
Consequences: Diagnosing a failed start means reading a fixed known path rather
than guessing a working directory. Log volume is bounded by construction, so a
long-running or noisy session cannot fill the disk. A future second process
sharing one log file would need per-line process identification, which is not
added here.
```

**Phase 6:**

```
## YYYY-MM-DD - Production Ledger Location Is Application-Owned Configuration
Decision: The production portfolio database lives in the per-user local
application-data directory under a truthful name, outside the repository working
tree and outside any file-syncing folder. Its location is resolved from the
application's own configuration, which is the single source of truth; launch
scripts do not pass it. The development database is a separate, initially empty
file in the same directory; the previous repository-local database is retired
rather than reused as development data.
Context: The real portfolio lived inside the repository tree under a name
containing "test", one cleanup away from deletion and one wrong path away from
being silently replaced. Two independently maintained copies of a default path
would eventually open two databases that look like one, so the launch script
stopped carrying one. A live SQLite file with its write-ahead-log sidecars in a
syncing folder is a corruption risk, which is why the live database and its
backups live in different places.
Consequences: Development runs open their own empty database rather than the real
portfolio; realistic development data comes from demo mode or from a
deliberately copied backup snapshot. Any future shell — including a native
desktop window — resolves the same database through the same configuration
rather than through its own path definition.
```

**Phase 7:**

```
## YYYY-MM-DD - Launch-Time Ledger Backups With Integrity Check, Tiered Retention, And A Verified Restore
Decision: The backend snapshots the portfolio database at every launch into a
file-syncing backup folder using SQLite's copy-into-a-new-file mechanism,
verifies each snapshot with an integrity and foreign-key check before accepting
it, and prunes older snapshots by a tiered ladder of recent launches, weekly and
monthly survivors, also reclaiming abandoned partial files. One additional
snapshot is taken immediately before pending schema migrations are applied; that
snapshot is mandatory, cannot be disabled by any option, and its failure blocks
startup before the migration runs. Every ordinary launch snapshot failure —
including an unresolvable backup location — starts the application and reports an
explicit failed backup status instead of locking the user out of the portfolio.
Snapshots are plain unencrypted database files. Backup status is derived from the
backup directory itself, never recorded inside the database. Demo mode takes no
snapshots. Backups are launch-time only: mid-session and
pre-destructive-operation triggers were considered and rejected.
Context: The portfolio had no backups at all, and hand-entered prices and
conviction metadata had already made the database data of record rather than a
reproducible cache. A database-recorded backup timestamp cannot witness the
backup that contains it, so the directory listing is both simpler and honest.
Refusing to open the portfolio because a copy failed would turn a backup problem
into an availability outage, whereas an irreversible schema change with no
recovery point is a different category of risk.
Consequences: "Backed up" means a snapshot was written locally into the synced
folder; the asynchronous upload is not verified, so off-machine protection
depends on the sync client actually running. Backup files are readable portfolio
data wherever the sync service stores them. Retention deletes only files in its
own directory whose names it can parse as its own snapshots. Restoring is a file
copy plus pointing an instance at it; that procedure is documented and has been
executed against real data, comparing portfolio aggregates before and after with
a tool that fails on any difference.
```

## Documents to update

| Document | Change | Phase |
|---|---|---|
| `README.md` | Configuration table (`TTTB_MODE`, loopback rule, ledger, backup, log, create-if-missing; retired variables); start flows and the new flag surface; the blocking-script/two-window note; first-run `-InitLedger`; backups section with the single failure rule; restore drill procedure; log locations; production port | 1, 2, 4, 5, 6, 7, 8 |
| `docs/Design.HighLevel.md` | LAN goal and LAN reachability corrected to loopback-only; deployment-model reality; hardening phase marked done for backup/logs/error surfacing; acceptance criterion 5 annotated; port references; risk row for the ledger path on the health endpoint | 2, 8 |
| `docs/DecisionLog.md` | Five entries above, each appended in the phase where every sentence in it is already true, in the file's own template, at the end, naming behaviours and never this plan | 1, 3, 5, 6, 7 |
| `docs/VisualDesign.DarkTheme.md` | One line under *Badges / chips*: the footer's `DEV` chip (neutral) and failed-backup chip (`--warning-soft`) as uses of existing tokens. No new tokens | 4 |
| `docs/plans/Plan.NativeDesktopWindow.md` | Full rebase per Phase 8 item 5 — settled decisions 8, 9, 10, 11, 13, 17, 19, 21, 22, 25; composition-root, working-directory, ledger-identity, logging and frontend sections; Phase 1 step 0; Phases 3–5; documents table; out-of-scope list; risks | 8 |
| `docs/plans/Design.mobile-view-design.md` | One line: its LAN work must lift the loopback-only enforcement together with authentication | 8 |
| `scripts/start.ps1` | New flag surface, production/dev/demo composition, pinned-port failure, `-NoBackup` scoped to the launch snapshot, legacy-name guard, per-run logs, stops passing `TTTB_DATABASE_URL` | 1, 2, 5, 6 |
| `scripts/migrate-ledger.ps1` (new) | The one-time, refusing, hash-verified relocation | 6 |
| `scripts/capture-aggregates.ps1` | Default port; `-FailOnChange` for diff mode | 2, 7 |
| `frontend/vite.config.ts` | Proxy fallback port | 2 |
| `backend/Cargo.toml`, `frontend/package.json` (+ lock) | `0.16.0` → `0.17.0`, `0.22.13` → `0.23.0` | 8 |
| `Agents.md` | **No change expected** — build commands and the two-version clause stay true. Recorded as a deliberate non-action | — |

## Risks and accepted consequences

- **Two backends can still share one ledger.** The pinned production port stops
  the accidental case (a second `scripts/start.ps1`), not a manually launched
  second backend with an explicit `-Port`. Both would run a launch refresh and a
  launch snapshot against one file. Accepted for now; the per-ledger lease is
  `Plan.NativeDesktopWindow.md`'s committed work.
- **OneDrive upload is not verified.** A snapshot can exist locally while the
  machine is offline or OneDrive is paused. The status wording says exactly what
  is known and no more.
- **Backups are unencrypted portfolio data in a cloud folder.** User-confirmed.
- **The health endpoint leaks the OS user name** in the ledger and backup paths,
  now only to processes on this machine.
- **A launch backup can fail silently to the user who never looks at the
  footer.** Accepted as the cost of not turning a backup problem into an
  availability outage; the failure is in the footer, in the health response and
  in `engine.log`.
- **A pre-migration snapshot failure blocks the app**, including when the backup
  directory is unavailable and a schema change is pending. Deliberate: the
  alternative is applying an irreversible schema change with no way back.
- **First release build is slow** (several minutes) and every backend change
  now rebuilds in release for the everyday run. `-Dev` remains the fast loop.
- **The origin/port change resets browser-stored view preferences once.**
  User-dismissed as unimportant.
- **Retention worst case ≈ 170 MB** in OneDrive.

## Open Questions

**None remain.** Every question raised while writing this plan was put to the
user in review and settled with the value the plan proposed, so the implementer
has no stop-and-ask items and no contradictory guidance. Recorded here so the
choices are not silently reopened:

| Question | Settled as |
|---|---|
| Backup folder | `%OneDrive%\TickerTapeTallyBoard\Backups` — the OneDrive root, not the localised `Dokument` folder, and no known-folder crate |
| Retention ladder | 10 recent launches / 8 ISO weeks / 12 months, plus 10 pre-migration and 3 failed-integrity, plus abandoned-`.partial` reclamation |
| Production port | 8480 |
| Backup-failure behaviour | One rule: launch snapshot never fatal; pre-migration snapshot mandatory, undisableable, and fatal on failure |
| Health schema | `TTTB_MODE` enum replacing `TTTB_DEMO_MODE`; `mode` + `ledger` + `backup` on `/api/health`; `demo` removed from the wire |
| Dev ledger | Fresh and empty; the old test-named file retired, not reused |
| `DEV` footer chip | Yes; production shows no chip |
| Retired script flags | Kept as declared parameters that throw a guidance error |

## Resolved during planning (recorded so they are not reopened)

- **Backups before relocation.** The phase order deliberately makes the real
  data recoverable before anything moves, rather than moving first and
  protecting afterwards.
- **Mode is explicit, not inferred** from whether the resolved path happens to
  equal a default. An inferred mode would make the fail-loud behaviours depend on
  string comparison of paths.
- **No mtime-based dist staleness check.** Flaky across rebuilds and clock skew;
  the existing UI-vs-API version pair in the footer is the honest signal, and the
  production script rebuilds unless told not to.
- **No new migration.** Refuse-to-create and `VACUUM INTO` live at the
  connection and pool layer; schema is untouched, so the forward-only additive
  migration commitment is not engaged.
- **Backup status derived from the directory, not stored in the database** — a
  stored timestamp cannot witness the backup containing it.
- **One port default for all modes**, with only the conflict behaviour differing.
  Two defaults would be a second thing to keep in sync.
- **Snapshot taken before the launch refresh spawns**, so it captures the ledger
  as found rather than as freshly repriced — the property the restore drill
  depends on.
- **`-FailOnChange` is opt-in on the diff tool**, not the new default: existing
  uses of that tool legitimately expect and read differences.
