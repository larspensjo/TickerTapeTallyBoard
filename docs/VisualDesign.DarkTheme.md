# Visual Design — Dark Theme

**Status:** Approved design · **Date:** 2026-06-13 · **Owner:** Lars
**Inspired by:** [visual_design.md](visual_design.md) (Coinbase analysis — borrowed vocabulary, not inverted wholesale)

This is the visual specification for TickerTapeTallyBoard's dark UI. It is the
single source of truth for color, type, spacing, and component appearance. It is
deliberately lean: concrete token values, a paste-ready CSS block, and just-enough
component notes. It does not prescribe layout markup or React structure.

The frontend uses plain CSS with semantic class names, so tokens are expressed as
CSS custom properties on `:root`. Use the tokens — never inline hex.

## Character

**Calm chrome, vivid data.** The shell (app bar, panels, labels) is quiet and
restrained — borrowed from Coinbase's institutional calm. The data (prices, P&L,
totals) is high-contrast and vivid so the numbers do the talking. This is a
compact, information-dense board meant to be scanned daily, not a marketing page.

What we kept from the Coinbase system:

- **One accent** ("brand voltage") — Coinbase Blue `#0052ff`, used scarcely.
- **Mono on every number** — all tabular figures in a monospace face.
- **Semantic up/down as color, not fills** — green/red are text colors (three
  reserved exceptions: soft-tinted chips, below; the Dashboard treemap, whose
  whole point is a per-tile heatmap — its tiles use the same `--up-soft`/
  `--down-soft` fills; and the Dashboard gain chart, whose whole point is
  above-or-below zero, so it fills green above the baseline and red below).
- **Depth by layering, not shadows** — panels float on the canvas via a surface
  step + 1px hairline; drop shadows are reserved for overlays only.
- **Pill/rounded geometry** — pill CTAs, rounded panels, circular glyphs.

Deliberate divergences from the Coinbase doc (it described a *marketing* surface;
this is an *in-product* dashboard):

- **Headings are weight 600, not 400.** Coinbase's weight-400 display is a brand
  signal for large hero type; at dashboard sizes 600 reads clearer.
- **Tighter geometry and spacing.** Panels use 16px radius (not 24px); no 96px
  marketing section rhythm.
- **Semantics are vivid**, tuned for legibility on near-black (Coinbase's
  `#05b169`/`#cf202f` go muddy on a dark canvas).

## Colors

### Surfaces
| Token | Hex | Role |
|---|---|---|
| `--canvas` | `#0a0b0d` | Page floor (near-black). |
| `--surface-1` | `#0e1114` | Panels, cards, table containers — floats on canvas. |
| `--surface-2` | `#14181d` | Raised: row hover, inputs, menus, secondary buttons. |
| `--hairline` | `#20242b` | 1px borders on panels and raised elements. |
| `--hairline-soft` | `#181c22` | Table row dividers, chart grid lines. |

### Gains waterfall neutral bars
| Token | Hex | Role |
|---|---|---|
| `--wf-bar-base` | `#2b3138` | Cost-basis bar fill (darkest neutral). |
| `--wf-bar-subtotal` | `#39414b` | Subtotal bar fill (mid neutral). |
| `--wf-bar-total` | `#4a525d` | Total bar fill (lightest neutral). |
| `--wf-hatch-stripe` | `#353b44` | Hatch-stripe fill for unavailable/placeholder bars. |

### Text
| Token | Hex | Role |
|---|---|---|
| `--text-primary` | `#f5f6f7` | Headings, instrument names, monetary values. Softened off-white, not `#fff`. |
| `--text-secondary` | `#a8acb3` | Body, secondary figures, the "flat/unchanged" semantic. |
| `--text-muted` | `#6b7280` | Column headers, eyebrows, captions, disabled. |
| `--text-on-accent` | `#ffffff` | Text on accent fills. |

### Accent (single brand voltage)
| Token | Hex | Role |
|---|---|---|
| `--accent` | `#0052ff` | **Fills only**: primary button, active-nav indicator, focus border. |
| `--accent-hover` | `#2f6bff` | Primary button hover (lightens on dark). |
| `--accent-active` | `#003ecc` | Primary button pressed. |
| `--accent-link` | `#4d8bff` | **Text on dark**: links, inline accents, chart line/crosshair. Lifted for contrast — `#0052ff` is too dark as small text on near-black. |

### Semantics
| Token | Hex / value | Role |
|---|---|---|
| `--up` | `#16c784` | Price/P&L up. Text color. |
| `--down` | `#ff4d4f` | Price/P&L down. Text color. |
| `--up-soft` | `rgba(22,199,132,0.13)` | Reserved tint — "top mover" chip background, Dashboard treemap up-tile fill. |
| `--down-soft` | `rgba(255,77,79,0.13)` | Reserved tint — "laggard" chip background, Dashboard treemap down-tile fill. |
| `--warning` | `#f4b000` | Stale price, import warnings. Text/chip. |
| `--warning-soft` | `rgba(244,176,0,0.14)` | Warning chip background. |

"Flat / unchanged" uses `--text-secondary`, not a third semantic color.

## Typography

```
--font-ui:   'Inter', -apple-system, system-ui, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif
--font-mono: 'JetBrains Mono', 'Geist Mono', ui-monospace, 'Cascadia Mono', Consolas, monospace
```

| Role | Family | Size | Weight | Notes |
|---|---|---|---|---|
| Page title (rare) | ui | 24px | 600 | -0.4px tracking. Empty states, page headers. |
| Section title | ui | 16px | 600 | Panel headings. |
| Label-strong | ui | 13px | 600 | Form labels, list labels. |
| Body | ui | 14px | 400 | Default running text, line-height 1.5. |
| Body-sm | ui | 13px | 400 | Dense secondary text. |
| Eyebrow / column header | ui | 11px | 600 | UPPERCASE, +0.04em, `--text-muted`. |
| Nav | ui | 13px | 500 | App-bar menu items. |
| Button | ui | 13px | 600 | — |
| Number | **mono** | 13px | 500 | All tabular figures (qty, price, value, %). |
| Number-lg | **mono** | 22px | 600 | Portfolio total, panel totals. |

**Every number is mono.** Quantities, prices, currency values, percentages, dates
in tables. Mixing proportional and mono numerals in a column is the most common
way to make a financial table look amateur.

## Spacing & Radius

4px base scale. No marketing-scale section spacing.

```
--space-1: 4px   --space-2: 8px   --space-3: 12px  --space-4: 16px
--space-5: 20px  --space-6: 24px  --space-8: 32px  --space-12: 48px
```

```
--radius-sm: 8px    inputs (compact), small chips
--radius-md: 12px   inputs, selects
--radius-lg: 16px   panels, cards, table containers
--radius-pill: 100px  buttons, badges, chips
--radius-full: 9999px circular glyphs / avatars
```
(`--radius-xs: 4px` exists for tiny tags.)

## Elevation & Depth

| Level | Treatment | Use |
|---|---|---|
| Flat | No border, no shadow | Default. |
| Panel | `--surface-1` + 1px `--hairline` | Cards, tables — "card on canvas". |
| Overlay | `--surface-2` + 1px `--hairline` + `0 8px 24px rgba(0,0,0,0.5)` | Dropdowns, menus, popovers **only**. |

No decorative shadows on cards. Depth comes from the surface step, not blur.

## Density (tables)

The board's defining surface. Holdings and transactions share one table system.

- Row height ≈ 36px; cell padding `8px 14px`.
- Column headers: eyebrow style (11px/600 UPPERCASE, `--text-muted`).
- Numbers right-aligned, `--font-mono`; instrument/text columns left-aligned.
- Row divider: 1px `--hairline-soft`. Row hover: `--surface-2`.
- Values in `--text-primary`; up/down columns in `--up`/`--down`; flat in
  `--text-secondary`.
- Shared instrument cell: the name is a link, with the display ticker beneath in
  12px mono `--text-muted`. With no ticker, show the name alone. ISINs and broker
  or exchange tags do not appear in this cell.

## Components (notes)

**App bar** — ~56px, `--canvas` floor, bottom `--hairline`. Brand wordmark with a
9px accent dot left; nav items (13/500) with active item marked by a 2px
`--accent` bottom indicator; right side: an app-wide actions cluster with a compact outline
"Refresh" button and icon, plus a warning indicator beside it. The cluster is
reserved for future Undo/History controls and stays on the navigation row at
≤900px. The summary band beneath belongs to the portfolio tabs only, showing
portfolio value in Number-lg and today-change in `--up`/`--down`. "Add
transaction" is a secondary button in the Transactions panel header.

**Holdings / transactions tables** — the density spec above. Transactions adds a
filter input (`--surface-2`) and neutral type chips (Buy/Sell/Split use neutral
chips, **not** semantic colors — a Sell is not "bad").

Holdings columns are Instrument, Qty, Cost, Value, P&L, Target & conviction,
P/E·P/S, and Next report. Target & conviction puts a fixed-width diverging gap bar
first and the conviction selector beside it, with its target value in SEK beneath
the selector. Reserve the bar's space even when there is no target so the bars and
selectors align down the table. The column sorts by signed gap percent; conviction
rank has no separate sort. Gap semantics and plain-sign colouring are unchanged.
The ratios stack P/E above P/S and sort by P/E. Ratios show one decimal;
percentages retain two decimals.

Company figures use certainty markers as text: approximate values have an amber
"~" prefix and `--warning` text, with the explanation in the title and accessible
label. An unavailable figure is a muted dash (—) with its reason on hover;
its accessible label says "unavailable". Not applicable is muted, italic "n/a",
visually distinct from the dash, with "not applicable" in its accessible label. Complete
figures have no marker. Missing figures sort after available values in either
direction. Header descriptions explain these markers.

Next report stacks the date, its small muted label, and a days-away hint supplied
by the backend. The labels are "confirmed" for a company-announced date, "Nasdaq
expected" for Nasdaq's estimate, and "est." for the app's estimate. A passed date
with an unresolved-report reason reads "expected <date>" and keeps its label,
with the visible hint "figures not seen yet" instead of a days-ago count. This column
sorts by the backend's days-until count, soonest first, with no-date rows last.

**Stock-page company figures** — "How the company is doing" sits directly above
the transaction list. Four labelled values (P/E, P/S, profit margin, sales growth) use
the Holdings certainty markers and precision. Next and last report dates include
the backend days-away label, observation window and quarter ending where known.
A currency-crossing conversion note names the currencies, rate and price date.
An awaiting message replaces chart history after a wrong-company correction until
an attempt completes; it persists while the last attempt failed.

Two compact DOM bar charts show sales and EPS in the latest report currency.
Other currencies are excluded from the scale and shown as hatched placeholders
with the currency-change reason.
Bars use neutral `--wf-bar-total` fill; negative EPS uses `--wf-bar-subtotal` and
extends below a visible `--text-muted` zero axis. Missing quarters have explicit
hatched placeholders: `--surface-2` alternates with `--wf-hatch-stripe`, bordered
by `--hairline`. Their accessible name is "No figures reported for the quarter ending ...".
Reported but excluded slots say "Figures excluded for the quarter ending ..."
with the reason. Period-end dates and values use mono type. Each chart uses a
count-derived grid with shrinking bars and bounded gaps so all quarters fit,
oldest first and newest last, without scrolling as history grows. The two charts
stack on narrow windows. Date labels abbreviate backend period ends to YYYY-MM.
Company-figure amount labels use k/M/B at the thousand/million/billion thresholds
with one decimal, including EPS; smaller amounts retain unit-price precision,
with whole amounts dropping a trailing .0. Other surfaces keep their precision.
Value labels hide below a 5rem slot width and dates below 3rem; full period-end
dates and unabridged amounts remain in every bar's accessible label and title.
Set-aside quarters are absent from the response and charts.

The Data & mapping panel is kept compact. Provider and price source share one
"Price source" row when the only configured source is the effective one. The
conviction dropdown sits on the Conviction heading line, and the target status
chip rides on the Target gap row. Company-figures source details show a "Symbol"
row (symbol · automatic or set by hand · security type in plain words) and one
"Updated" row when the last attempt succeeded on the day of the last success;
otherwise separate last-success and last-attempt rows. The preview-then-save
correction form stays folded behind a "Change source" button on the Symbol row.
Existing figures require an explicit same-company or wrong-company choice; there
is no default radio selection. Pending updates say "Figures load on the next
refresh" while retained figures stay visible. Source update and attempt dates
use backend local calendar dates. Demo shows source details with no correction
controls.

**Opportunities table** — the density spec above, read-only, no column-header
sorting. The panel header carries two segmented controls: the window ("1 week",
"2 weeks", "30 days") and the order ("Best candidates first", the default,
"Biggest fallers", "Biggest risers"); the
period subtitle under the title is `--text-muted`, replaced by a neutral
"Updating…" chip while the rows shown belong to another window. Percent columns
use plain-sign colouring; an unavailable value is a neutral chip with its
plain-language reason. "Below your average" is a neutral status chip beside the
vs-average percent, not a semantic soft chip (the number already carries the
sign colour). Stale signals use the minor clock icon or the `--warning-soft`
chip: on the latest price from its own freshness, and separately beside the SEK
gain when its price or exchange rate is stale. "Vs target" reuses the Holdings
diverging target-gap bar and tooltip, blank without a target. The Verdict column
follows Instrument and shows a verdict chip whose text carries the meaning,
a wrapping one-sentence explanation, and neutral compact caution chips. The
Sentiment-dip candidate chip uses `--up-soft` with `--up` text; Fell for a reason?
uses `--warning-soft` with `--warning` text. These coloured chips have a matching
text-colour border at 28% opacity. Possible candidate, report timing unknown,
Not enough data, Normal movement and Unknown verdict use neutral status chips.
The explanation uses Body-sm (13px) in `--text-secondary`. At the 1258px desktop
window (1180px panel), long explanations wrap onto two lines and instrument names
are not truncated.
When verdicts are present, a 13px
`--text-secondary` note below the table reads: "A verdict describes the dip: how
unusual the fall is and what is known about the company. It does not predict the
price. A replay of stored price history found no measurable difference in the
following month between dips the price checks flagged and other dips; the company
checks could not be replayed."

Every row with a verdict has a full-width evidence line beneath it, spanning all
columns, capped to the scroll area's visible width without widening the table.
It stays in view while the data columns scroll sideways. The line
belongs to the holding above, with no divider between them and the usual hairline
after the evidence. Hovering either part uses `--surface-2` for both. It retains
one accessible data row per holding; its text is reachable in a labelled evidence
group described by the instrument cell. Rows without a verdict remain single-line. Checks appear as "Name: result" in this order: Unusual drop,
Off its high, Report in the period, Report coming soon, Business, Market, Stopped
falling, Ex-dividend, Room to buy. Labels may use `--text-muted`; results and
unknown reasons use Body-sm (13px) in `--text-secondary`, with figures in mono.
Not applicable is italic "n/a". Approximate values reuse the company-figure amber
"~" prefix and `--warning` text; the explanation appears in the title and, unless
already visible in the same check, the accessible label. Report dates retain
source certainty, and period-only knowledge is an interval or bound, never a
report date. The market tooltip states that the
median mixes markets and currencies, includes funds, and can be self-referential
in a concentrated portfolio. Results keep the usual move, dividend-adjusted change
when supplied, compact business figures, distance below the high and the period
low context visible. Values already shown in columns are omitted. Counts,
thresholds, comparison margins and 1-year high/low prices belong in the entry tooltip
and accessible description. Timing hints use the UI font.

The separate muted 12px footnote explains native-currency changes without dividends
added back, that ex-dividend days are detected where the price source reports
dividends, and that during market hours the latest price can be intraday, so fell
and stopped falling can change before the close.

**Transaction form** — inputs on `--surface-2`, `--radius-md`, 1px `--hairline`;
labels in Label-strong/`--text-muted`; focus = 1.5px `--accent` border + soft
ring. Primary submit is the accent pill.

**Buttons** (height ~36, `--radius-pill`):
- Primary: `--accent` fill, `--text-on-accent`; hover `--accent-hover`, active `--accent-active`.
- Secondary: `--surface-2` fill + `--hairline`; hover lightens.
- Outline: transparent + `--hairline`.
- Compact: ~32px tall, with 12px horizontal padding, for app-bar controls.
- Text: `--accent-link`, no fill.

**Badges / chips** (`--radius-pill`, 11/600):
- Status (neutral): `--surface-2` + `--hairline`, `--text-secondary` — e.g. "EOD · 17:30".
- Semantic soft (reserved): `--up-soft`/`--down-soft`/`--warning-soft` backgrounds
  with the matching text color — only for movers/alerts, never for ordinary cells.
- Footer `DEV` uses the neutral chip; a failed-backup footer chip uses `--warning-soft`.

**Inputs / selects** — `--surface-2`, `--radius-md`, `--hairline` border,
placeholder `--text-muted`, focus as in the form.

**Slider** — the rebalance trade-count range input uses a `--surface-2` track,
an `--accent` filled progress segment, an `--accent` thumb with a canvas
border and hairline outline, and the shared `--focus-ring` when focused.

**States**:
- Empty: centered `--text-muted` message; when the action lives on another page,
  inline `--accent-link` links to it instead of a CTA button. Omit the invitation
  where data cannot be changed (demo).
- Loading: skeleton bars in `--surface-2` (shimmer optional).
- Error: `--down` (or `--warning`) message + outline "Retry".
- Stale price: `--warning-soft` chip on the affected row/value.

**Charts (Lightweight Charts)** — map theme options to tokens:
```
layout.background        = --canvas (#0a0b0d)
layout.textColor         = --text-secondary (#a8acb3)
grid.vert/horzLines.color= --hairline-soft (#181c22)
crosshair.color          = --accent-link (#4d8bff)
areaSeries.lineColor     = --accent-link (#4d8bff)   // 1px stroke stays visible on near-black
areaSeries.topColor      = rgba(0,82,255,0.32)
areaSeries.bottomColor   = rgba(0,82,255,0.00)
up markers / candles     = --up (#16c784)
down markers / candles   = --down (#ff4d4f)

// Running average cost line (asset price chart)
runningAverageCost.color = gold #e0b15e, 1px, LineStyle.Dotted, LineType.WithSteps
runningAverageCost.series= one line series per held period, so closed periods stay gaps
runningAverageCost.tag   = right-axis tag only on an open position, drawn as a custom
                           price-axis label (gold back, black text, like the library's
                           own tag) because its text is supplied by the backend, not
                           the library's default formatter

// Gain chart (baseline series, zero baseline always in view)
baseline.baseValue       = 0
baseline.topLineColor    = --up (#16c784)
baseline.topFillColor1/2 = rgba(22,199,132,0.30) -> rgba(22,199,132,0.02)
baseline.bottomLineColor = --down (#ff4d4f)
baseline.bottomFill1/2   = rgba(255,77,79,0.02) -> rgba(255,77,79,0.30)

// Axes
priceScale.scaleMargins  = { top: 0.12, bottom: 0.12 }   // fitted axis, not zero-pinned
percent axis format      = two decimals + "%", minMove 0.01
currency axis format     = compact, 3 significant digits, minMove 1
```

**Chart edge tag** — names a reference line that falls outside the fitted axis,
so it reads as off-screen at a known level rather than as missing: pill chip,
`--surface-2` on a `--hairline` border, gold `#e0b15e` text at 11px/600,
positioned at the top or bottom edge of the chart it belongs to.

**Chart caption** — the always-visible explanation under a chart legend:
`--text-muted`, 12px, max ~78 characters wide. Not a tooltip, because the
distinction between the chart's percentage and the money-weighted percentages
elsewhere must be legible without hovering.

## CSS tokens (`:root`)

Paste-ready. This block is the implementation contract.

```css
:root {
  /* surfaces */
  --canvas: #0a0b0d;
  --surface-1: #0e1114;
  --surface-2: #14181d;
  --hairline: #20242b;
  --hairline-soft: #181c22;

  /* gains waterfall neutral bars (base < subtotal < total, progressively lighter) */
  --wf-bar-base: #2b3138;
  --wf-bar-subtotal: #39414b;
  --wf-bar-total: #4a525d;
  --wf-hatch-stripe: #353b44;

  /* text */
  --text-primary: #f5f6f7;
  --text-secondary: #a8acb3;
  --text-muted: #6b7280;
  --text-on-accent: #ffffff;

  /* accent (single brand voltage) */
  --accent: #0052ff;
  --accent-hover: #2f6bff;
  --accent-active: #003ecc;
  --accent-link: #4d8bff;
  --focus-ring: 0 0 0 3px rgba(0, 82, 255, 0.35);

  /* semantics */
  --up: #16c784;
  --down: #ff4d4f;
  --up-soft: rgba(22, 199, 132, 0.13);
  --down-soft: rgba(255, 77, 79, 0.13);
  --warning: #f4b000;
  --warning-soft: rgba(244, 176, 0, 0.14);

  /* type */
  --font-ui: 'Inter', -apple-system, system-ui, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
  --font-mono: 'JetBrains Mono', 'Geist Mono', ui-monospace, 'Cascadia Mono', Consolas, monospace;

  /* spacing */
  --space-1: 4px;  --space-2: 8px;   --space-3: 12px; --space-4: 16px;
  --space-5: 20px; --space-6: 24px;  --space-8: 32px; --space-12: 48px;

  /* radius */
  --radius-xs: 4px; --radius-sm: 8px; --radius-md: 12px;
  --radius-lg: 16px; --radius-pill: 100px; --radius-full: 9999px;
}
```

## Accessibility / contrast

Checked against the dark canvas; values are approximate WCAG ratios.

- `--text-primary` on `--canvas`: ~17:1 — AAA.
- `--text-secondary` on `--canvas`: ~9:1 — AAA.
- `--text-muted` on `--canvas`: ~4.2:1 — AA for the non-essential labels it carries
  (column headers, captions). Do not use it for body or critical values.
- `--accent-link` on `--canvas`: ~5.5:1 — AA for links/inline accents.
- `--up` / `--down` on `--canvas`: ~7:1 / ~5:1 — comfortably legible.
- `--text-on-accent` (`#fff`) on `--accent`: ~4.6:1 — AA for the 13px/600 button
  label (bold). Do not place small `--accent` *text* on the canvas — use
  `--accent-link` instead.

## Do / Don't

**Do**
- Reserve `--accent` for fills (primary CTA, active nav, focus); use `--accent-link` for any accent *text*.
- Put every number in `--font-mono`.
- Keep semantics as text color; use the soft-tinted chips only for movers/alerts.
- Build depth from the surface step + hairline.

**Don't**
- Use `--accent` (`#0052ff`) as small text on the dark canvas.
- Use `--up`/`--down` as a button background.
- Use pure `#000` or pure `#fff` for large fields.
- Add drop shadows to cards, or introduce a second accent color.
- Color a Sell/Buy/Split type chip green/red — transaction type is not P&L.

## Application icon

The mark is the Dashboard's gains waterfall reduced to three stacked bars on a
rounded `--canvas` tile with a `--hairline` edge: cost basis, market value, and
the total return in `--up`. It carries no accent — the blue stays reserved for
fills inside the product, and the dashboard reads as green from across the room.

Two deliberate departures from the tokens above, both forced by the 16px size at
which the icon is almost always seen:

- The neutral bars use `--wf-bar-total` and `--text-muted` rather than the
  waterfall's own `--wf-bar-subtotal`/`--wf-bar-total` pair. The darker values
  are indistinguishable from the tile at favicon size.
- Each raster size snaps the bars to its own whole pixels instead of being
  downscaled from a larger rendering, which would leave half-pixel edges and
  blur the three bars into one grey smear.

The geometry lives in one place, `scripts/build-app-icons.py`, which generates
every artefact — the scalable icon, the legacy fallback, the home-screen image
and the Windows icon. Edit the geometry and regenerate; never edit an output.

## Future

Tokens are semantic, so a light theme could later be added by supplying an
alternate `:root` value set (e.g. under `[data-theme="light"]`) without touching
component CSS. A light theme is **out of scope** for v1.
