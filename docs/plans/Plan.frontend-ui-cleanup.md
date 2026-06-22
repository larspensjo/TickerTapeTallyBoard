# Frontend UI Cleanup Plan

> **For agentic workers:** Implement this plan task-by-task, in order. Steps use checkbox (`- [ ]`) syntax for tracking. Each task ends by *staging* its files (`git add …`) for review — do **not** `git commit`. Run each task's verification step before moving on.

**Goal:** Make the frontend sleek, intuitive, and low-friction to use without documentation. Flatten the two-level navigation into a single level, remove presentation redundancy (portfolio totals shown three different ways), strip developer-only chrome out of the daily path, and tighten the look-and-feel so it matches `docs/VisualDesign.DarkTheme.md`.

**Why:** Navigation is currently two levels deep — three top tabs (*Dashboard*, *Board*, *Import*) where *Board* hides three more pages (*Holdings*, *Gains*, *Transactions*). "Tabs within tabs" is hard to overview. We flatten to a single level — **Dashboard · Holdings · Gains · Transactions · Import** — and dissolve the *Board* container: the portfolio totals + primary actions (Refresh prices, Add transaction) move into a **persistent shell** shared by every portfolio page, so the Dashboard overview and the former Board pages live under one roof. The same totals are also currently rendered three ways (Dashboard tiles, Board totals-band, Board inline metrics); the shell renders them once. **The Holdings and Gains tables are intentionally kept separate** — Gains is expected to grow its own feature set, so merging them now would be premature.

**Tech stack:** React + TypeScript, React Router, TanStack Query, plain CSS with design tokens, Vitest + Biome. No new dependencies.

## Design constraints (from `Agents.md` and `VisualDesign.DarkTheme.md`)

- Preserve the unidirectional `input -> action -> reducer -> state -> render` flow; reducers stay pure and unit-testable.
- Keep shared constants/behavior DRY — one source of truth.
- Never inline hex; use the `:root` design tokens. (This plan adds chart palette tokens so the allocation colors stop bypassing the system.)
- "Calm chrome, vivid data": the shell stays quiet; numbers do the talking.
- Name runtime modules after stable behavior/domain concepts, not plan phases.

## Global constraints

- Frontend: after each task run `npm run check` then `npm run fmt`, from `frontend/`. `npm run check` covers `tsc --noEmit`, Biome, and `vitest run`. Under `Start-Process` use `npm.cmd` explicitly.
- Keep reducers pure; add/extend reducer tests when a reducer's action set changes.
- Bump `frontend/package.json` (currently `0.9.0`) by a **minor** level at the end — this is a user-facing cleanup. Backend (`0.6.0`) is untouched.
- **Staging, not committing:** each task ends with `git add` of the listed files. Committing is a separate human action after review.

## Open questions (resolve before / during implementation)

1. **Shared shell scope.** The portfolio totals band + Refresh-prices / Add-transaction actions move into a persistent layout shown on Dashboard, Holdings, Gains, and Transactions — but **not** Import or Asset detail (those stand alone). Confirm that grouping. **Assumption: those four pages share the shell; Import and Asset are standalone routes.**
2. **Add-transaction surface.** Today the form is an inline panel toggled on the Board. With the action lifted to the shell, the form becomes available from any portfolio page. **Assumption: an inline panel at the top of the active page, toggled from the shell action (no modal).**
3. **Where versions/health go.** Phase 2 removes the developer `status-strip` from the main view. This plan relocates UI/API version + API health into a small footer. Alternative: an "About" popover. **Assumption: footer.**
4. **Price-freshness chip wording.** Phase 2 keeps exactly one user-facing freshness indicator. Confirm the label vocabulary (e.g. `Prices: EOD 17:30` / `Stale`). **Assumption: reuse existing freshness helpers in `valuationDisplay`.**

---

## File structure

**Created:**
- `frontend/src/components/AsyncBoundary.tsx` — shared pending/error/empty wrapper (replaces the repeated `board-state` skeleton/retry blocks).
- `frontend/src/components/AsyncBoundary.test.tsx` — DOM test of the three states.
- `frontend/src/components/AppFooter.tsx` — version + API status footer.
- `frontend/src/components/PortfolioSummary.tsx` — the totals band, rendered **once** in the shell.
- `frontend/src/components/PortfolioLayout.tsx` — persistent shell (totals band + Refresh-prices / Add-transaction toolbar + freshness chip + React Router `<Outlet/>`) wrapping the portfolio pages. This is the Board↔Dashboard merge point.
- `frontend/src/components/HoldingsPage.tsx`, `GainsPage.tsx`, `TransactionsPage.tsx` — route components extracted from `BoardView`. Each owns its own filter state; `GainsPage` owns a dedicated pure reducer for date/method/closed controls.
- `frontend/src/components/GainsPage.reducer.test.ts` — reducer contract test for the Gains controls.

**Modified:**
- `frontend/src/styles.css` — add `--chart-1..6` tokens; remove `status-strip` and the dissolved board-view segmented-control styling (the allocation segmented-control stays).
- `frontend/src/App.tsx` — flat single-level nav (Dashboard · Holdings · Gains · Transactions · Import); layout route; render `AppFooter`; redirect `/board` → `/holdings`.
- `frontend/src/components/Dashboard.tsx` — drop its own `metric-tiles` summary (now in the shell); keep value chart + top movers + allocation; tokenized palette; use `AsyncBoundary`.
- `frontend/src/components/AssetView.tsx` — use `AsyncBoundary`.

**Removed:**
- `frontend/src/components/BoardView.tsx` — dissolved into `PortfolioLayout` + the three page components. Its `uiReducer` splits: Gains controls → `GainsPage` reducer; per-table filter → page-local state; Add-transaction `formOpen` → shell state.

**Untouched (intentionally):** `frontend/src/components/HoldingsTable.tsx`, `frontend/src/components/GainsTable.tsx` — reused by the new pages, kept separate for planned Gains expansion.

**Docs:**
- `docs/DecisionLog.md` — flat-navigation + chart-token entries.
- `frontend/package.json` — minor version bump.

---

## Phase 1 — Safe quick wins (zero behavior change)

Low-risk groundwork that immediately reduces duplication and tokenizes color. No IA change yet, so each step is independently shippable and easy to review.

### Task 1.1: Tokenize the allocation palette
**Files:** Modify `frontend/src/styles.css`, `frontend/src/components/Dashboard.tsx`.
- [x] Add `--chart-1 … --chart-6` to `:root` in `styles.css`, derived from the theme (accent/up/down/warning family) per `VisualDesign.DarkTheme.md` — no raw hex left in the component.
- [x] Replace the hardcoded `palette = ["#4f9cff", …]` array in `Dashboard.tsx` with the tokens (read via CSS vars or a token map).
- [x] **Verify:** `npm run check` passes; allocation bar renders with theme-consistent colors (manual glance). Stage both files.

### Task 1.2: Extract `AsyncBoundary`
**Files:** Create `frontend/src/components/AsyncBoundary.tsx` + `AsyncBoundary.test.tsx`; modify `Dashboard.tsx`, `BoardView.tsx`, `AssetView.tsx`.
- [x] Create `AsyncBoundary` taking `isPending` / `isError` / `isEmpty` / `onRetry` / `emptyMessage` / `children`, rendering the existing skeleton, error (`down` + outline Retry), and muted-empty markup verbatim.
- [x] Replace the duplicated `board-state` blocks in `BoardView` (`BoardSection`), `AssetView`, and `Dashboard` with `AsyncBoundary`.
- [x] Add a jsdom test covering pending → error (retry fires) → empty → children.
- [x] **Verify:** `npm run check` passes; all three screens still show identical loading/error/empty states. Stage the files.

**Human test recommended:** click through Dashboard/Board/Asset with the backend stopped to confirm error + Retry still work.

---

## Phase 2 — De-clutter the chrome

Remove developer-facing noise from the daily path and fix the duplicate Refresh action. Still no structural change — safe to ship on its own. (Phase 3 later relocates this now-decluttered toolbar + freshness chip into the persistent shell.)

### Task 2.1: Relocate versions + health to a footer
**Files:** Create `frontend/src/components/AppFooter.tsx`; modify `BoardView.tsx`, `App.tsx`, `styles.css`.
- [x] Build `AppFooter` showing UI version (`package.json`), API version + status (`/api/health`), and the static facts ("Manual entry", "SEK base") in quiet `--text-muted` type.
- [x] Remove the `status-strip` chips from `BoardView`; delete now-dead `status-strip` CSS.
- [x] Keep **one** user-facing price-freshness chip near the totals (reuse `freshnessLabel`/`freshnessTone` from `valuationDisplay`).
- [x] **Verify:** `npm run check` passes; top of the page is visibly calmer; version/health still discoverable in the footer. Stage the files.

### Task 2.2: Resolve the two "Refresh" buttons
**Files:** Modify `BoardView.tsx` (+ tests if the reducer/action set changes).
- [x] Keep one primary **Refresh prices** action. Trigger the data refetch automatically on its success (or demote manual refetch to an icon-only control on error states only).
- [x] Ensure the spinner/disabled states still reflect in-flight refresh.
- [x] **Verify:** `npm run check` passes; only one "Refresh" affordance is visible; prices + tables update after it. Stage the file.

**Human test recommended:** trigger a price refresh and confirm the tables reflect new values without a second click.

---

## Phase 3 — Flatten navigation & dissolve the Board

The structural win. Collapse the two-level nav into one level and merge the *Board* container with the *Dashboard* by moving the shared totals + actions into a persistent shell. After this phase there are no hidden sub-pages: every destination is a single top-level tab.

Target nav (flat): **Dashboard · Holdings · Gains · Transactions · Import** (Asset detail remains a non-nav route).

### Task 3.1: Persistent portfolio shell (the Board ↔ Dashboard merge)
**Files:** Create `frontend/src/components/PortfolioLayout.tsx`, `frontend/src/components/PortfolioSummary.tsx`; modify `App.tsx`, `Dashboard.tsx`.
- [ ] Create `PortfolioSummary` rendering the totals **one** way (Total value, Day change, Unrealized) in the metric-tile styling.
- [ ] Create `PortfolioLayout`: the persistent shell hosting `PortfolioSummary` + the toolbar (Refresh prices, Add transaction) + the single freshness chip, with a React Router `<Outlet/>` for the page body.
- [ ] Wire a layout route in `App.tsx` so Dashboard + the (coming) table pages render inside `PortfolioLayout`. Remove the Dashboard's own `metric-tiles` block (now supplied by the shell).
- [ ] **Verify:** `npm run check` passes; the totals + actions appear once, persistently, above the page body. Stage the files.

**Human test recommended:** confirm the totals/actions stay put when switching pages and don’t flash/reflow.

### Task 3.2: Split the Board into route pages
**Files:** Create `HoldingsPage.tsx`, `GainsPage.tsx`, `TransactionsPage.tsx`, `GainsPage.reducer.test.ts`; remove `BoardView.tsx`.
- [ ] Extract each former Board sub-view into its own route component that renders the existing `HoldingsTable` / `GainsTable` / `TransactionsTable`.
- [ ] Redistribute `BoardView`'s `uiReducer` state: per-table filter → page-local state; Gains date/method/closed controls → a dedicated **pure** `GainsPage` reducer (with a contract test); Add-transaction `formOpen` → `PortfolioLayout` shell state so the form is reachable from any portfolio page.
- [ ] Delete `BoardView.tsx`.
- [ ] **Verify:** `npm run check` passes (new/updated reducer test green); each page renders its table with filtering/sorting intact. Stage the files.

### Task 3.3: Flatten the nav
**Files:** Modify `App.tsx`, `styles.css`.
- [ ] Nav becomes a single level: Dashboard · Holdings · Gains · Transactions · Import. Routes: `/`, `/holdings`, `/gains`, `/transactions`, `/import`, `/asset/:id`.
- [ ] Remove the in-page board-view segmented control (its job is now the top-level nav) and drop its dead CSS; keep the allocation segmented-control.
- [ ] Redirect `/board` → `/holdings` so old bookmarks/links still resolve.
- [ ] **Verify:** `npm run check` passes; nav is one level deep with five clear destinations and nothing hidden. Stage the files.

**Human test recommended:** full click-through — every former Board page reachable from the top nav; `/board` redirects; Asset detail + Import unaffected.

---

## Phase 4 — Wrap up

### Task 4.1: Version bump + DecisionLog
**Files:** Modify `frontend/package.json`, `docs/DecisionLog.md`.
- [ ] Bump `frontend/package.json` minor (`0.9.0` → `0.10.0`).
- [ ] Add DecisionLog entries: (a) **flat single-level navigation** — the *Board* container is dissolved into a persistent portfolio shell (totals + actions) shared with the Dashboard; pages are Dashboard · Holdings · Gains · Transactions · Import. Note Holdings and Gains stay separate pending Gains expansion. (b) chart palette tokens (`--chart-1..6`) so allocation/series colors stay within the design system.
- [ ] **Verify:** `npm run check` passes; UI footer shows the new version. Stage the files.

---

## Verification summary

| Phase | Automated gate | Human check |
|---|---|---|
| 1 | `npm run check` (incl. `AsyncBoundary` test) | error/Retry on all screens |
| 2 | `npm run check` | calmer header; one Refresh; freshness chip |
| 3 | `npm run check` (incl. `GainsPage` reducer test) | flat 5-tab nav; persistent totals/actions; `/board` redirect; nothing hidden |
| 4 | `npm run check` | footer shows new version |

Phases 1–2 are independently shippable and reversible. Phase 3 is the structural navigation change and should be reviewed as a unit before staging is promoted to a commit. The Holdings/Gains merge is deliberately **out of scope** — revisit it only after the planned Gains feature work settles.
