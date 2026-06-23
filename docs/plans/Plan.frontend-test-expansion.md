# Frontend Test Expansion Plan

> **For agentic workers:** Implement this plan task-by-task, in order. Steps use checkbox (`- [x]`) syntax for tracking. Each task ends by *staging* its files (`git add …`) for review — do **not** `git commit`. Follow each task's verification step before moving on.

**Goal:** Build on the Vitest runner introduced by the Phase 4 charts work and extend automated coverage to the parts of the frontend that carry real logic but are currently untested: the three `useReducer` reducers, the pure view-model / selector helpers, the duplicated number-parsing helper, and the `api/client.ts` fetch/error layer.

**Why:** `Agents.md` mandates a unidirectional `input -> action -> reducer -> state -> render` flow with **pure, unit-testable reducers** and a preference for "tests of reducer behavior, emitted effects, and public contracts over internal details." The reducers and pure helpers are the highest-value, lowest-friction targets and exactly the contracts that doc calls out. The runner, `@testing-library/react`, and `jsdom` are already installed and already gated through `npm run check`, so no new tooling is required.

**Tech stack:** React + TypeScript, Vitest, `@testing-library/react`, jsdom, Biome. No new dependencies.

## Global Constraints

- Frontend: after each task run `npm run check` then `npm run fmt`, from `frontend/`. `npm run check` already runs `tsc --noEmit`, Biome, **and** `vitest run` — one completion gate. Use `npm.cmd` under `Start-Process`.
- Tests live next to the unit under test as `*.test.ts` / `*.test.tsx` (matches the established Phase 4 pattern: `dashboardSelectors.test.ts`, `instrumentChartViewModel.test.ts`).
- Pure-logic tests run in the default `node` environment. Component / DOM tests **must** opt in per-file with a `// @vitest-environment jsdom` pragma on the first line (the convention `TimeSeriesChart.test.tsx` already uses). Do **not** change the global `environment: "node"` in `vitest.config.ts` — node is the fast default.
- Prefer testing **public contracts**: a reducer's `(state, action) => state` mapping, a selector's output, a parser's return value. Do not test React rendering details unless the behavior only exists in a component.
- Keep production behavior unchanged. The only production edits this plan makes are: (a) widening reducer visibility to `export` so they can be imported by tests, and (b) the DRY extraction of `parseFiniteNumber` in Phase 4. Both are behavior-preserving.
- **Staging, not committing:** each task ends with `git add` of the listed files. Committing is a separate human action after review.

## Open Questions (resolved during implementation)

1. **Reducer export style.** Resolved: in-place export used. Reducers exported as `importReducer`, `addTransactionReducer` with their types and factories; components' `useReducer` calls updated accordingly.
2. **Version bump.** Skipped — test/refactor-only change with no user-facing behavior change.
3. **DecisionLog.** Added. Entry dated 2026-06-23 documents the node-vs-jsdom convention, parseFiniteNumber consolidation, and the client test mock approach for null-body status codes.

---

## File Structure

**Modified (production — behavior-preserving):**
- `frontend/src/components/ImportView.tsx` — export the reducer + initial-state.
- `frontend/src/components/AddTransactionForm.tsx` — export the reducer + `createInitialState`.
- `frontend/src/components/GainsTable.tsx` — drop local `parseFiniteNumber`, import the shared one.
- `frontend/src/components/HoldingsTable.tsx` — drop local `parseFiniteNumber`, import the shared one.
- `frontend/src/components/valuationDisplay.tsx` — export a single shared `parseFiniteNumber`.

**Created (tests):**
- `frontend/src/components/ImportView.reducer.test.ts`
- `frontend/src/components/AddTransactionForm.reducer.test.ts`
- `frontend/src/components/assetViewModel.test.ts`
- `frontend/src/components/valuationDisplay.test.ts`
- `frontend/src/api/client.test.ts`

**Already done (code drift):**
- `frontend/src/components/GainsPage.reducer.test.ts` — `BoardView.tsx` was refactored away; gains-page UI state now lives in `GainsPage.tsx` as the exported `gainsPageReducer`, and its test already exists.

**Docs:**
- `docs/DecisionLog.md` — testing-strategy entry.
- `frontend/package.json` — optional patch bump.

---

## Phase 1 — Reducer tests (highest value)

These reducers are pure functions today; the only blocker to testing is that they are module-private. Each task exports the reducer (behavior-preserving) and adds a `node`-environment test of its action contract.

### Task 1.1: `AddTransactionForm` reducer (smallest, proves the pattern)

**Files:**
- Modify: `frontend/src/components/AddTransactionForm.tsx`
- Create: `frontend/src/components/AddTransactionForm.reducer.test.ts`

- [x] **Step 1: Export the reducer and types**

In `frontend/src/components/AddTransactionForm.tsx`:
- Change `function reducer(` to `export function addTransactionReducer(`.
- Change `function createInitialState(` to `export function createInitialState(`.
- Export the `FormState` and `FormAction` types (`export interface FormState` / `export type FormAction`).
- Update the internal `useReducer(reducer, …)` call site to `useReducer(addTransactionReducer, …)`.

Run from `frontend/`: `npm run check` to confirm the rename compiles and the component still wires up.

- [x] **Step 2: Write the reducer test**

Create `frontend/src/components/AddTransactionForm.reducer.test.ts`. Cover the public contract:
- `fieldChanged` updates the named field and clears `error`.
- `instrumentModeChanged` switches mode and clears `error`.
- `transactionTypeChanged` / `instrumentTypeChanged` set their field and clear `error`.
- `submitStarted` sets `submitting: true`, `error: null`.
- `submitFailed` sets `submitting: false` and the message.
- `submitSucceeded` does a **partial** reset: it spreads the existing state and explicitly resets `instrumentId`, `symbol`, `exchange`, `name`, `instrumentType` → `"Stock"`, `type` → `"Buy"`, `quantity`, `price`, `currency` → `"USD"`, `fxRate`, `brokerage`, `note` → `""`, plus `submitting`/`error`. It **preserves** only `instrumentMode`, `tradeDate`, and `instrumentCurrency` (via the spread). Assert both the reset and the preserved fields. (Verify against the `submitSucceeded` branch in the source before writing assertions.)
- `createInitialState(true)` yields `instrumentMode: "existing"`; `createInitialState(false)` yields `"new"`.

```ts
import { describe, expect, it } from "vitest";
import {
  addTransactionReducer,
  createInitialState,
  type FormState,
} from "./AddTransactionForm";

const base = (): FormState => createInitialState(true);

describe("addTransactionReducer", () => {
  it("updates a text field and clears the error", () => {
    const next = addTransactionReducer(
      { ...base(), error: "boom" },
      { type: "fieldChanged", field: "quantity", value: "10" },
    );
    expect(next.quantity).toBe("10");
    expect(next.error).toBeNull();
  });

  it("marks submitting on submitStarted and records a failure message", () => {
    const started = addTransactionReducer(base(), { type: "submitStarted" });
    expect(started.submitting).toBe(true);
    expect(started.error).toBeNull();

    const failed = addTransactionReducer(started, {
      type: "submitFailed",
      message: "nope",
    });
    expect(failed.submitting).toBe(false);
    expect(failed.error).toBe("nope");
  });

  it("seeds the instrument mode from whether instruments exist", () => {
    expect(createInitialState(true).instrumentMode).toBe("existing");
    expect(createInitialState(false).instrumentMode).toBe("new");
  });
});
```
Add the remaining cases (`instrumentModeChanged`, `transactionTypeChanged`, `instrumentTypeChanged`, `submitSucceeded`) following the same shape, asserting exactly what the source branches do.

- [x] **Step 3: Verify, format, stage**
```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/components/AddTransactionForm.tsx frontend/src/components/AddTransactionForm.reducer.test.ts
```

---

### Task 1.2: `GainsPage` reducer — **already done (code drift)**

`BoardView.tsx` no longer exists. The board was refactored into separate page components. The gains-page UI state (`includeClosedPositions`, `datePreset`, `dateRange`, `returnMethod`) now lives in `GainsPage.tsx` as the exported `gainsPageReducer`. That reducer is already exported and tested in `frontend/src/components/GainsPage.reducer.test.ts`.

No implementation work required. Skip to Task 1.3.

---

### Task 1.3: `ImportView` reducer (most complex — the import state machine)

**Files:**
- Modify: `frontend/src/components/ImportView.tsx`
- Create: `frontend/src/components/ImportView.reducer.test.ts`

**Interfaces:** the reducer drives the phase machine `idle → previewing → previewReady → committing → committed | error`, plus `confirmDuplicate`/`cancelDuplicate` and per-asset `toggleAsset` selection.

- [x] **Step 1: Export the reducer, types, and helpers**

In `frontend/src/components/ImportView.tsx`:
- Change `function reducer(` to `export function importReducer(`.
- Export `INITIAL_STATE` (or wrap it in an exported `createInitialState()` if you prefer not to share a mutable constant — `INITIAL_STATE` is fine since the reducer treats it as immutable).
- Export the `State`, `Action`, and `Phase` types.
- Export the `selectedFromPreview` helper (it has its own derivable contract worth asserting).
- Update the `useReducer(reducer, INITIAL_STATE)` call site to `useReducer(importReducer, …)`.

Run `npm run check`.

- [x] **Step 2: Write the state-machine test**

Create `frontend/src/components/ImportView.reducer.test.ts` (node env). Build minimal `ImportPreview` fixtures (read `../api/types` for the exact shape; only the fields the reducer touches — `assets[].asset_key`, `default_selected`, `skipped_reason` — need realistic values). Cover:
- `sourceSelected` resets phase to `idle` and clears `fileName`/`preview`/`result`/`selected`/`error`.
- `fileSelected` moves to `previewing` and records `fileName`, clearing prior preview/result.
- `previewReady` moves to `previewReady`, stores the preview, and seeds `selected` from `selectedFromPreview` (assert `default_selected` honored).
- `confirmDuplicate` / `cancelDuplicate` flip `confirmingDuplicate` and clear `error`.
- `toggleAsset` flips a selectable asset; a missing asset or one with `skipped_reason` is a no-op returning the same state (assert reference equality or deep equality per the source — read the branch).
- `commitStarted` → `committing`; `committed` → `committed` with the result; `failed` → `error` with the message.
- `reset` returns to the initial state.
- `selectedFromPreview` directly: maps each asset key to its `default_selected`.

```ts
import { describe, expect, it } from "vitest";
import {
  importReducer,
  INITIAL_STATE,
  selectedFromPreview,
  type State,
} from "./ImportView";
import type { ImportPreview } from "../api/types";

function preview(): ImportPreview {
  // Fill only the fields the reducer reads; copy the real shape from ../api/types.
  return {
    /* … assets: [{ asset_key: "A", default_selected: true, skipped_reason: null, … }] … */
  } as ImportPreview;
}

describe("importReducer", () => {
  it("seeds selection from the preview on previewReady", () => {
    const next = importReducer(INITIAL_STATE, {
      type: "previewReady",
      preview: preview(),
      fileName: "trades.csv",
    });
    expect(next.phase).toBe("previewReady");
    expect(next.fileName).toBe("trades.csv");
    expect(next.selected).toEqual(selectedFromPreview(preview()));
  });

  it("ignores toggling an unknown asset", () => {
    const ready = importReducer(INITIAL_STATE, {
      type: "previewReady",
      preview: preview(),
      fileName: "trades.csv",
    });
    const same = importReducer(ready, { type: "toggleAsset", assetKey: "does-not-exist" });
    expect(same).toEqual(ready);
  });
});
```
Expand with the remaining transitions above. This is the most valuable test in the plan — the import flow has the most states and the most user-visible consequences.

- [x] **Step 3: Verify, format, stage**
```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/components/ImportView.tsx frontend/src/components/ImportView.reducer.test.ts
```

---

## Phase 2 — Pure helper / view-model tests

### Task 2.1: `assetViewModel` finders and derivations

**Files:**
- Create: `frontend/src/components/assetViewModel.test.ts`

All functions here are already exported and pure — no production edit needed.

- [x] **Step 1: Write the test (node env)**

Create `frontend/src/components/assetViewModel.test.ts` covering:
- `parseInstrumentId`: a valid numeric string → number; `undefined`, empty, non-numeric, negative/zero (read the function to confirm whether it rejects ≤0) → `null`.
- `findInstrument` / `findGainsRow` / `findHolding` / `findPriceStatus`: hit returns the matching record; miss returns `null`.
- `instrumentTransactions`: filters to the given instrument id, preserving order.
- `sharesSold`: sums sold quantities per its definition (read the body for the exact rule).
- If time allows: a focused case for `deriveAssetData` / `tilesView` / `headerStatus` using small fixtures. Prioritize the finders and `parseInstrumentId` first — they are the cheapest and most reused.

Use tiny inline fixtures built from `../api/types` (an `Instrument`, a couple of `GainsRow`/`Holding`/`Transaction` records). Keep fixtures minimal — only the fields the function reads.

- [x] **Step 2: Verify, format, stage**
```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/components/assetViewModel.test.ts
```

---

### Task 2.2: `valuationDisplay` pure helpers

**Files:**
- Create: `frontend/src/components/valuationDisplay.test.ts`

The pure helpers (`isAvailable`, `availabilityNumber`, `availabilitySortValues`, `signedTone`, `formatGroupedNumber`, `unavailableValue`) are exported already. `availabilitySortRows` depends on a TanStack `Row` and is better left to integration; skip it here.

- [x] **Step 1: Write the test (node env)**

Create `frontend/src/components/valuationDisplay.test.ts` covering:
- `isAvailable`: `available` → true; `unavailable`/`undefined` → false (type-guard narrowing).
- `availabilityNumber`: parses an available numeric value; returns `Number.NEGATIVE_INFINITY` for unavailable **and** for a non-finite available value (so sorts push them to the bottom).
- `availabilitySortValues`: orders two values by their numeric value; unavailable sorts below available.
- `signedTone`: positive → `"up"`, negative → `"down"`, zero / non-finite → `"flat"`.
- `formatGroupedNumber`: groups thousands and preserves the decimal/sign; a non-numeric string is returned unchanged (read the regex branch — assert the documented passthrough). Cover negative numbers and values with/without a fractional part.

```ts
import { describe, expect, it } from "vitest";
import {
  availabilityNumber,
  formatGroupedNumber,
  isAvailable,
  signedTone,
} from "./valuationDisplay";

describe("availabilityNumber", () => {
  it("returns -Infinity for unavailable and non-finite values", () => {
    expect(availabilityNumber({ status: "unavailable", reasons: ["x"] })).toBe(
      Number.NEGATIVE_INFINITY,
    );
    expect(availabilityNumber({ status: "available", value: "1234.5" })).toBe(1234.5);
  });
});

describe("signedTone", () => {
  it("classifies sign", () => {
    expect(signedTone("1")).toBe("up");
    expect(signedTone("-1")).toBe("down");
    expect(signedTone("0")).toBe("flat");
    expect(signedTone("nope")).toBe("flat");
  });
});

describe("formatGroupedNumber", () => {
  it("groups thousands with commas and passes through non-numerics", () => {
    expect(formatGroupedNumber("1234567.89")).toBe("1,234,567.89");
    expect(formatGroupedNumber("-1234.5")).toBe("-1,234.5");
    expect(formatGroupedNumber("n/a")).toBe("n/a");
  });
});
```
The current implementation groups with a comma separator (`replace(/\B(?=(\d{3})+(?!\d))/g, ",")`) and preserves the sign and fractional part, so `"1234567.89" → "1,234,567.89"`. If the implementation's separator ever changes, update the expectation to match the source.

- [x] **Step 2: Verify, format, stage**
```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/components/valuationDisplay.test.ts
```

---

## Phase 3 — DRY the duplicated `parseFiniteNumber`, then test it once

`parseFiniteNumber` is defined twice — `GainsTable.tsx` (takes `string`) and `HoldingsTable.tsx` (takes `string | number`). This violates the "one source of truth" rule in `Agents.md`. Consolidate into one exported helper and test it once.

### Task 3.1: Extract and reuse

**Files:**
- Modify: `frontend/src/components/valuationDisplay.tsx`
- Modify: `frontend/src/components/GainsTable.tsx`
- Modify: `frontend/src/components/HoldingsTable.tsx`
- Create: `frontend/src/components/valuationDisplay.test.ts` is extended (or add to the Phase 2 file)

- [x] **Step 1: Add the shared helper**

In `frontend/src/components/valuationDisplay.tsx`, add the more general signature (matches the HoldingsTable variant so both call sites compile):
```ts
export function parseFiniteNumber(value: string | number): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}
```

- [x] **Step 2: Replace the two local copies**

- In `frontend/src/components/GainsTable.tsx`: delete the local `function parseFiniteNumber(…)` and import `parseFiniteNumber` from `./valuationDisplay` (merge into the existing `./valuationDisplay` import block).
- In `frontend/src/components/HoldingsTable.tsx`: delete the local `function parseFiniteNumber(…)` and import it from `./valuationDisplay`.

Confirm the local `availableNumber` / `moneyValue` helpers in each file still compile against the shared function.

- [x] **Step 3: Test the shared helper**

Add to `frontend/src/components/valuationDisplay.test.ts`:
```ts
import { parseFiniteNumber } from "./valuationDisplay";

describe("parseFiniteNumber", () => {
  it("parses finite strings and numbers, rejects the rest", () => {
    expect(parseFiniteNumber("12.5")).toBe(12.5);
    expect(parseFiniteNumber(7)).toBe(7);
    expect(parseFiniteNumber("nope")).toBeNull();
    expect(parseFiniteNumber(Number.POSITIVE_INFINITY)).toBeNull();
    expect(parseFiniteNumber("")).toBeNull(); // Number("") === 0 — confirm desired behavior!
  });
});
```
Note: `Number("")` is `0`, which **is** finite, so `parseFiniteNumber("")` returns `0`, not `null`. Read the call sites to confirm that is acceptable (the original helpers had the same behavior, so this is behavior-preserving). Adjust the assertion to the true result and add a comment if it is a surprising-but-intended edge.

- [x] **Step 4: Verify, format, stage**
```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/components/valuationDisplay.tsx frontend/src/components/GainsTable.tsx frontend/src/components/HoldingsTable.tsx frontend/src/components/valuationDisplay.test.ts
```

---

## Phase 4 — API client error/parse layer

`api/client.ts` has untested branches: 204 no-content, malformed JSON, `!response.ok` error mapping, and the body/no-body request shaping. The core logic lives in the module-private `parse<T>(response)`; **do not export it just for tests.** Exercise it through its public callers — `apiGet`, `apiSend`, and/or `apiSendBytes` — which is the right public-contract shape. Test by stubbing `global.fetch` with `vi.fn()` — no MSW, no extra dependency.

### Task 4.1: Client tests

**Files:**
- Create: `frontend/src/api/client.test.ts`

- [x] **Step 1: Write the test (node env — `fetch`/`Response` are global in the Vitest runtime)**

Create `frontend/src/api/client.test.ts`. Stub `global.fetch` per-case with `vi.fn()` returning a `Response`. Restore it in `afterEach`. Cover (all via the public callers — `parse` and `parseJson` stay private):
- `apiGet` success: returns the parsed JSON body.
- 204 (through `apiGet`): resolves to `undefined` without calling `.json()`/`.text()` expecting content.
- malformed JSON body on a 200 (through `apiGet`): the private `parseJson` swallows it → resolves to `null` (assert the documented fallback).
- `!response.ok` with an `{ error: { code, message } }` body: throws `ApiError` with that `code` and `message`.
- `!response.ok` with no/!parseable body: throws `ApiError` with code `"unknown"` and a `Request failed: <status>` message.
- `apiSend`: when `body` is `undefined`, the `fetch` init `body` is `undefined`; when provided, it is `JSON.stringify(body)` and the `content-type: application/json` header is set (assert via the `fetch` mock's call args).

```ts
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, apiGet, apiSend } from "./client";

function jsonResponse(status: number, body: unknown): Response {
  return new Response(body === undefined ? "" : JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("client error mapping", () => {
  it("maps a structured error body to ApiError", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        jsonResponse(400, { error: { code: "invalid_date_range", message: "bad" } }),
      ),
    );
    await expect(apiGet("/api/x")).rejects.toMatchObject({
      name: "ApiError",
      code: "invalid_date_range",
      message: "bad",
    });
  });

  it("falls back to unknown for a bodyless error", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("", { status: 500 })));
    await expect(apiGet("/api/x")).rejects.toMatchObject({ code: "unknown" });
  });

  it("returns undefined for 204", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(null, { status: 204 })));
    await expect(apiGet("/api/x")).resolves.toBeUndefined();
  });

  it("stringifies the body and sets the json content-type on send", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { ok: true }));
    vi.stubGlobal("fetch", fetchMock);
    await apiSend("POST", "/api/x", { a: 1 });
    const [, init] = fetchMock.mock.calls[0];
    expect(init.body).toBe(JSON.stringify({ a: 1 }));
    expect(init.headers["content-type"]).toBe("application/json");
  });
});
```
Confirm `vi.stubGlobal`/`new Response(...)` are available in the configured Vitest runtime; if `Response` is not global in `node` env, add `// @vitest-environment jsdom` to the top of this file (jsdom provides `fetch`/`Response`), or import from `undici`. Prefer the pragma over a new dependency.

- [x] **Step 2: Verify, format, stage**
```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/api/client.test.ts
```

---

## Final Task: DecisionLog + version bump

**Files:**
- Modify: `docs/DecisionLog.md`
- Modify: `frontend/package.json` (optional patch bump — see Open Question 2)

- [x] **Step 1: DecisionLog entry**

Append to `docs/DecisionLog.md` (match the existing dated-heading style):
```markdown
## 2026-06-22 - Frontend automated test strategy
Decision: Extend the Vitest runner (introduced with the charts work) to cover the frontend's pure logic: the three `useReducer` reducers (import, board, add-transaction), the `assetViewModel` finders/derivations, the `valuationDisplay` pure helpers, the consolidated `parseFiniteNumber`, and the `api/client.ts` error/parse layer. Reducers are exported under explicit names so tests import the `(state, action) => state` contract directly; components keep their internal `useReducer` wiring. Pure-logic tests run in the default `node` Vitest environment; DOM/component tests opt in per-file via a `// @vitest-environment jsdom` pragma (the global default stays `node` for speed). All tests are gated through `npm run check` alongside `tsc` and Biome.
Context: `Agents.md` mandates pure, unit-testable reducers and prefers tests of reducer behavior and public contracts; these were untested before this work.
Consequences: The duplicated `parseFiniteNumber` was consolidated into one exported helper (one source of truth). No new dependencies; no runtime behavior change beyond that behavior-preserving extraction.
```

- [x] **Step 2: Optional patch bump**

If versioning test/refactor changes (Open Question 2), bump `frontend/package.json` `"version"` by one patch level and refresh the lockfile: `npm.cmd install --package-lock-only` from `frontend/`. Otherwise skip.

- [x] **Step 3: Full verification**

Run from `frontend/`: `npm run check` then `npm run fmt`. Expected: types, Biome, and the full Vitest suite all green.

- [x] **Step 4: Stage**
```bash
git add docs/DecisionLog.md frontend/package.json frontend/package-lock.json
```
Leave the changes staged for review; do not commit.

---

## Coverage map

| Area | Task | Kind |
|------|------|------|
| AddTransactionForm reducer | 1.1 | node reducer test |
| GainsPage reducer | 1.2 | **already done** — `GainsPage.reducer.test.ts` exists |
| ImportView reducer (state machine) | 1.3 | node reducer test |
| assetViewModel finders/derivations | 2.1 | node pure test |
| valuationDisplay helpers | 2.2 | node pure test |
| `parseFiniteNumber` DRY + test | 3.1 | refactor + node test |
| api/client error/parse layer | 4.1 | fetch-stub test |
| Strategy decision + version | Final | docs |

**Deliberately out of scope (revisit later if needed):** MSW / React Query hook integration tests (the hooks are thin wrappers over the now-tested `client.ts`); snapshot tests (brittle, low-signal); full component-render tests beyond the existing `TimeSeriesChart.test.tsx` (reserve `@testing-library/react` for genuinely interactive behavior once the logic layer is covered).
