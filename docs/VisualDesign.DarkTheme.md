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
- **Semantic up/down as color, not fills** — green/red are text colors (two
  reserved exceptions: soft-tinted chips, below, and the Dashboard treemap,
  whose whole point is a per-tile heatmap — its tiles use the same `--up-soft`/
  `--down-soft` fills).
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
(`--radius-xs: 4px` exists for tiny tags like the exchange label.)

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
- Exchange tag: 10px mono in a `--radius-xs` outline chip, `--text-muted`.

## Components (notes)

**App bar** — ~56px, `--canvas` floor, bottom `--hairline`. Brand wordmark with a
9px accent dot left; nav items (13/500) with active item marked by a 2px
`--accent` bottom indicator; right side: outline "Refresh" + primary "Add
transaction". Directly below, a totals band: portfolio value in Number-lg + a
today-change figure in `--up`/`--down`.

**Holdings / transactions tables** — the density spec above. Transactions adds a
filter input (`--surface-2`) and neutral type chips (Buy/Sell/Split use neutral
chips, **not** semantic colors — a Sell is not "bad").

**Transaction form** — inputs on `--surface-2`, `--radius-md`, 1px `--hairline`;
labels in Label-strong/`--text-muted`; focus = 1.5px `--accent` border + soft
ring. Primary submit is the accent pill.

**Buttons** (height ~36, `--radius-pill`):
- Primary: `--accent` fill, `--text-on-accent`; hover `--accent-hover`, active `--accent-active`.
- Secondary: `--surface-2` fill + `--hairline`; hover lightens.
- Outline: transparent + `--hairline`.
- Text: `--accent-link`, no fill.

**Badges / chips** (`--radius-pill`, 11/600):
- Status (neutral): `--surface-2` + `--hairline`, `--text-secondary` — e.g. "EOD · 17:30".
- Semantic soft (reserved): `--up-soft`/`--down-soft`/`--warning-soft` backgrounds
  with the matching text color — only for movers/alerts, never for ordinary cells.

**Inputs / selects** — `--surface-2`, `--radius-md`, `--hairline` border,
placeholder `--text-muted`, focus as in the form.

**Slider** — the rebalance trade-count range input uses a `--surface-2` track,
an `--accent` filled progress segment, an `--accent` thumb with a canvas
border and hairline outline, and the shared `--focus-ring` when focused.

**States**:
- Empty: centered `--text-muted` message + a primary CTA.
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
```

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

## Future

Tokens are semantic, so a light theme could later be added by supplying an
alternate `:root` value set (e.g. under `[data-theme="light"]`) without touching
component CSS. A light theme is **out of scope** for v1.
