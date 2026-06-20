# Mobile View — Design (LAN MVP)

Date: 2026-06-20
Status: Draft for review (review findings applied 2026-06-20)

## Goal

Let the user view their portfolio on a mobile phone, with live data. Start with
a same-WiFi (LAN) MVP and a dedicated, read-only, mobile-first UI, architected so
that away-from-home access and an installable PWA can be added later without
reworking the app.

## Scope

**In scope**
- LAN access: the backend listens on the local network so a phone on the same
  WiFi can reach it.
- A dedicated, mobile-first, read-only route served by the existing SPA.
- Live data only (no snapshots/exports).

**Out of scope (deliberately unblocked for later)**
- Remote access away from home (Tailscale / Cloudflare Tunnel / cloud hosting).
- PWA install ("Add to Home Screen").
- Any write path or data entry *initiated from the mobile UI* (add transaction,
  import).
- Authentication and any real access-control boundary. The MVP is LAN-only and
  intentionally has no auth; adding auth is the explicit gate that must be
  cleared before any remote exposure.

**"Read-only" is a mobile-UI convention, not an enforced boundary (review #1).**
The same SPA and API are served to the phone, so a phone on the LAN can still
open `/`, `/import`, `/asset/:id`, or call write APIs directly. `MobileShell`
omitting desktop navigation is a UI affordance only — it does not protect write
routes. Real read-only/write enforcement is deliberately deferred and bundled
with the authentication work above (it belongs to the same trust boundary). The
verification plan must confirm that desktop and write routes remain reachable on
the LAN and are knowingly unprotected in the MVP.

## Chosen approach

- **Networking:** bind the backend to the LAN (`0.0.0.0`) and serve the built
  frontend it already serves today. (Approach "A".)
- **UI:** a dedicated `/m` route subtree with mobile-first components that reuse
  the existing react-query API layer. (Approach "F".)

Both later layers — Tailscale ("D") and PWA ("H") — sit on top of this without
changing the application code.

## Architecture

The app keeps its current shape: a React + Vite SPA built to `frontend/dist`,
served as static assets by the Rust backend, which also exposes `/api/*`. The
mobile view is additional routes and components inside the same SPA; it consumes
the same API. The only backend-facing change is the bind address, which is
already configuration-driven.

### 1. Backend — LAN serving (mostly configuration)

`TTTB_HOST` already controls the bind address (default `127.0.0.1`), so the
backend needs **no code change** to listen on the LAN. The work is convenience
and safety.

`-Lan` is a **production-style static run, not the two-process dev runner**
(review #4). The current `scripts/start.ps1` starts the backend plus a Vite dev
server on loopback; in `-Lan` mode the script must instead:

- set `TTTB_HOST=0.0.0.0`,
- build the frontend (`npm run build`),
- set or confirm `TTTB_STATIC_DIR` points at `frontend/dist`,
- **not** start Vite (the backend serves the built assets),
- wait on backend readiness via loopback (`http://127.0.0.1:<port>/`),
- print the phone-reachable URL (`http://<LAN-IP>:<port>/m`),
- preserve and restore any environment variables it overrides on exit.

Supporting requirements:
- Document how the script selects the LAN IP when several adapters exist (e.g.
  prefer the active non-loopback IPv4, or let the user pass it explicitly), and
  print the chosen value so it can be corrected.
- Document (and optionally let the script add) a Windows Firewall inbound rule
  for the backend port.
- Recommend a stable IP (DHCP reservation) so the phone URL does not drift.
- No auth — acceptable for LAN-only, and called out as the gate before any
  remote exposure. See the read-only note in Scope: write/desktop routes are
  reachable but unprotected by design in the MVP.

### 2. Frontend — mobile route structure

- New route subtree under `/m`:
  - `/m` — overview screen.
  - `/m/asset/:id` — per-holding detail.
- Routes added to `App.tsx`'s `Routes`.
- A `MobileShell` layout (compact sticky header, no desktop app-bar/nav), kept
  separate from the desktop shell.
- New components live under `frontend/src/components/mobile/`, consuming the
  existing `useGains` hook (and `useHoldings` / `useTransactions` only where the
  reused asset view-model needs them) — no new API code.

#### Redirect / override contract (review #2)

The auto-redirect must be governed by an explicit, persisted contract so the
escape link cannot become the trap it is meant to prevent:

- **Trigger scope:** the redirect only applies to `/` (the desktop board). It
  does **not** redirect `/asset/:id`, `/import`, or any other existing route — a
  phone can still reach those directly (consistent with the read-only-by-
  convention note).
- **Override store:** a `localStorage` flag, set/cleared via query params:
  - `?view=desktop` sets the override (stay on desktop) and clears the param;
  - `?view=mobile` clears the override (re-enable mobile redirect).
- **Decision rule:** the viewport hook redirects `/` → `/m` only when the
  viewport is narrow **and** no desktop override is active. The mobile shell's
  "Desktop view" link navigates to `/?view=desktop`; the desktop board's
  "Mobile view" link navigates to `/m?view=mobile`.
- This guarantees a narrow phone that chose "Desktop view" stays on `/` across
  reloads until it explicitly opts back into mobile.

#### Asset detail reuse (review #3)

`/m/asset/:id` must **reuse the existing asset view-model** in
`components/assetViewModel.ts` (`deriveAssetData`, `tilesView`, `breakdownView`,
`headerStatus`) rather than introducing a parallel mapping. The mobile asset
screen is only a mobile-shell rendering of that same model, so it cannot drift
from the desktop asset page. No mobile-specific asset mapping logic is
introduced; if any field is intentionally omitted on mobile, it is omitted at
the render layer and documented here. (No fields are currently planned for
omission.)

### 3. Mobile content (read-only)

- **Overview screen:**
  - Summary card from `GainsResponse.summary`: total market value, total
    unrealized gain (value + %), day change (value + %), all in
    `base_currency`.
  - A scannable list of holding cards sourced from **`GainsResponse.rows`**, not
    `Holding[]` (review #6). Each row already carries `market_value_base`,
    `unrealized_gain_percent`, and both `day_change_base` and
    `day_change_percent`, so the cards show market value, gain %, and day change
    (amount + percent) with **no join** between holdings and gains. By default
    the list shows open positions (matching the desktop board default);
    `Holding[]` is not needed for the overview.
  - Cards are color-coded gain/loss per the dark theme.
- **Asset detail screen:** tapping a card opens the fuller per-holding numbers
  via the reused `assetViewModel` (cost basis, quantity, unrealized gain
  breakdown).

#### Unavailable-value disclosure on touch (review #5)

The existing helpers (`AvailabilityValueCell`, `SummaryAvailabilityValue`)
expose unavailable reasons only through the `title` attribute, which is
hover-oriented and unreliable on touch. The mobile view must not depend on
`title` for reasons:

- Reuse `reasonLabel` / `reasonSummary` for the human-readable text, but surface
  it through a touch-friendly disclosure — a small `<details>`/sheet/popover
  control opened by tapping the "—"/"Unavailable" chip — rather than a hover
  tooltip.
- Available values still render via the existing formatting helpers
  (`FormattedNumber`, grouped numbers, signed tone).
- The human-test list includes a mobile interaction check for opening a reason
  disclosure.

## Data flow

Unchanged from the desktop: input → action → reducer → state → render, with the
API behind react-query. The mobile components are pure consumers of `useGains`
(and the hooks the reused asset view-model already requires); they add no new
effects or mutations.

## Error / edge handling

- "Unavailable" valuations render as a neutral placeholder with a touch
  disclosure for the reason (see review #5 above) rather than breaking layout.
- Loading and error states reuse the existing query states from the shared
  hooks.
- The redirect must never loop and must always expose the escape link to the
  other layout. The override contract (review #2) is what prevents a loop: once
  a desktop override is stored, `/` stops redirecting until mobile is
  re-enabled.

## Testing & verification

- Bump `frontend/package.json` version per repo convention.
- Unit-test the pure pieces:
  - The viewport-redirect decision rule, enumerating: direct `/` on a narrow
    viewport (redirects), the desktop escape on a narrow viewport (no redirect
    while override active), `/m` on a wide viewport (no forced redirect away),
    and a reload after a desktop override is stored (still no redirect).
  - Any mobile overview row-mapping derived from `GainsResponse.rows`, including
    unavailable and closed-position rows.
- Smoke-check that the backend static fallback serves `/m` in `-Lan` mode (the
  SPA deep-link must resolve through the static handler, not 404).
- **Human test (recommended):** build, run `scripts/start.ps1 -Lan`, open
  `http://<PC-IP>:<port>/m` on the actual phone over WiFi; confirm the summary
  and holding cards render, an "unavailable" value's reason can be opened by
  tapping (not hover), and the desktop/mobile escape links behave per the
  override contract.

## Decision log

Add an entry to `docs/DecisionLog.md`: the mobile MVP is LAN-only with no auth,
and authentication is a required gate before any remote (away-from-home)
exposure.

## Later layers (out of scope, unblocked)

- **Tailscale (D):** install on PC + phone; reach the same `/m` URL over the
  tailnet. No application change.
- **PWA (H):** add a web app manifest and a network-first service worker for
  "Add to Home Screen", deliberately avoiding offline caching of stale prices.
