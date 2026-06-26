# Investigation: Performance Percentage Methods

Purpose: record why the Gains view's percentage returns diverge sharply from Sharesight,
even though the gain *amounts* in SEK match, and what that implies for how the app should
compute return percentages. This is a durable analysis document, not a plan.

> Privacy: per the 2026-06-12 "Private Sharesight Exports" decision, this document contains
> only sanitized, aggregate findings and synthetic illustrations. The raw Avanza/Sharesight
> trade ledger used to derive the figures is not reproduced here.

## Symptom

For a single AI-portfolio holding compared against Sharesight:

- Gain **in SEK** matched Sharesight to the öre.
- Gain **in percent** did not: the app showed roughly **2.7×** Sharesight's figure
  (on the order of ~1,130% in the app vs ~421% in Sharesight).

The mismatch is entirely in the **denominator** of the percentage — the gain (numerator) is
computed the same way in both systems.

## Background: the prior assumption

The 2026-06-19 "Sharesight-Style Performance Returns Method" decision adopted **Modified
Dietz** money-weighted returns for the Gains totals and row percentages, on the stated
belief that "Sharesight documents its Performance Report as dollar-weighted / money-weighted
using a Modified Dietz variation."

This investigation shows that belief does not hold for the figure Sharesight actually
displays: the app's **single-shot** Modified Dietz does not reproduce Sharesight's number,
and neither does any standard single-pass money-weighted calculation.

## Root cause: a collapsing denominator

The app computes one Modified Dietz equation across the entire reporting window:

```
return = total_return / (begin_mv + Σ weight_i × cash_flow_i)
```

Modified Dietz is only accurate over a **short** period with **small** cash flows relative
to the position. Applied across a long window (~a year) with **large profit-taking sells
late in the period**, two effects shrink the denominator dramatically:

1. A large sell is a weighted **withdrawal** that subtracts from the denominator, while the
   gain it realized stays in the numerator.
2. The reporting window starts at the portfolio-wide earliest transaction
   (2026-06-20 "Canonical Performance Period" decision), which lowers the time-weight on the
   original purchase, shrinking its contribution to the denominator further.

For the holding studied, the denominator collapsed to roughly **one third** of the actual
capital that had been employed, inflating the percentage by ~3×.

This is a known weakness of single-shot Modified Dietz, not an arithmetic bug.

## Why the *amount* matches but the *percent* does not

The two systems agree on the gain (numerator) and disagree only on what to divide it by:

| | Numerator (gain) | Denominator |
|---|---|---|
| App | total return (price + FX) | Modified Dietz time-weighted base — **collapses** with late withdrawals |
| Sharesight | same | a **stable** base close to the average capital actually employed |

## Symmetric illustration (synthetic, non-private)

Each method is distorted by the *opposite* kind of cash flow. One stock, one-year period;
the SEK gain is identical in every case — only the percentage differs.

**Example A — buy once, never sell.** Invest 100k, ends at 120k.
Both methods report **20%**. No difference.

**Example B — take profits out mid-year.** Invest 100k; it doubles to 200k by mid-year;
sell 75% (150k cash out), keep 50k; flat to year-end. You doubled your money.
- Simple (gain ÷ cost) and linked/sub-period money-weighted: **~100%** (sensible)
- Single-shot Modified Dietz: **~400%** (the artifact — the withdrawal craters the base)

**Example C — add money to a winner late.** Invest 100k; it doubles to 200k; add 100k fresh
on the last day; year-end value 300k.
- Single-shot / linked money-weighted: **~100%** (sensible — the late deposit barely counts)
- Simple (gain ÷ cost base): **~50%** (understates — fresh money dilutes the headline)

Takeaway: **withdrawals** distort money-weighted Modified Dietz; **deposits** distort a
simple cost-basis return. Neither single number is "correct" for a churned position; they
answer different questions.

## Method comparison for the studied holding (sanitized)

All computed from the same validated cash flows (which reproduce the app's gain amount and
cost basis exactly). Percentages only; absolute amounts omitted.

| Method | Result |
|---|---|
| Simple: gain ÷ total cash ever invested | ~222% |
| **Sharesight "Return"** | **~421%** |
| FIFO-parcel average capital employed | ~465% |
| Average-cost average capital employed | ~494% |
| Simple: gain ÷ current cost basis | ~617% |
| Time-weighted return (price + FX) | ~643% |
| Money-weighted IRR, de-annualized to the period | ~688% |
| Modified Dietz, single-shot (from the holding's first buy) | ~864% |
| **App today (single-shot Modified Dietz, portfolio-wide start)** | **~1,130%** |
| Money-weighted IRR, annualized | very large (>1,000%/yr) |

## Findings

1. **Single-shot Modified Dietz is invalid here.** Over a long, actively churned history
   with large late sells, it produces non-credible figures and is the clear outlier.
2. **No standard single-pass method reproduced Sharesight's figure.** Simple, single-shot
   Modified Dietz, money-weighted IRR, and time-weighted return all miss ~421%. Sharesight's
   implied denominator sits near the **average capital employed** over the holding period,
   which is consistent with a dollar-weighted return computed over **sub-periods and linked**
   (or a true rate solve), rather than in one shot. The exact Sharesight variant could not be
   reconstructed from transaction-level data alone.
3. **The per-row percentage depends on unrelated instruments.** Because the canonical start
   is the portfolio-wide earliest transaction, a holding's percentage shifts if an unrelated
   instrument was bought earlier. This is acceptable for additive footers (its original
   purpose) but makes the per-row percentage a poor standalone "how did this asset do" number.
4. **Currency-gain sign differs from Sharesight.** The app reports the studied holding's
   currency component as negative while Sharesight reports it positive — a different
   decomposition, tracked as separate follow-up work.

## Direction (to be ratified in the DecisionLog)

- **Per-asset rows:** use a stable, intuitive denominator (cost basis). Matching Sharesight's
  exact per-asset percentage is explicitly a non-goal.
- **Portfolio total:** this is the number that matters (reported to others as "since start"
  and "year to date"). It should follow Sharesight's methodology and be validated against an
  actual Sharesight portfolio-level figure, since no method was confirmed by reverse-engineering
  alone.

## Candidate: Capital-and-Time Weighted Geometric Mean

### Motivation

Standard TWR chains sub-period returns with equal contribution per sub-period:
`Π(1 + r_i) − 1`. A tiny residual position held for years links equally with a
large position held for months. The capital-and-time weighted variant weights each
sub-period by both the capital at risk and the duration, so large long-running
positions dominate and zero-capital gaps are excluded automatically.

### Formula

Split history at every buy or sell. For each segment `i`:

- `MV_i` = market value at the **start** of segment `i` (quantity × price × fx_rate_to_base)
- `t_i` = segment duration in days
- `r_i` = sub-period return (see below)
- `w_i` = `(MV_i × t_i) / Σ(MV_j × t_j)` — normalized weights, sum to 1

```
result = Π(1 + r_i)^w_i − 1
```

Equivalently in log-space: `exp(Σ w_i × ln(1 + r_i)) − 1`.

A zero-capital segment has `MV_i = 0`, so `w_i = 0` and its factor is `(1 + r_i)^0 = 1`
— it contributes nothing without any special-casing.

### Capital measure: market value, not cost basis

The weight uses **market value at segment start**, not cost basis. The key reason:
after a partial sell following a large price increase, the remaining position has far
more at risk than the original purchase cost reflects. Market value captures the
opportunity cost; cost basis does not.

Cost basis is constant within a segment, so both measures converge for tiny
sub-segments (where price barely moves). The choice only matters at transaction
boundaries where a sell leaves a position whose value differs substantially from its
original cost.

### Computing the sub-period return

The return `r_i` must hold quantity constant during the segment. Since quantity
cancels, it simplifies to a pure price-and-FX ratio:

```
r_i = (price_next / exchange_rate_next) / (price_this / exchange_rate_this) − 1
```

where `exchange_rate` is the Sharesight CSV convention (instrument currency per SEK),
so `price / exchange_rate` is the per-share value in SEK.

**Do not** compute `r_i` as `(MV_next_row − MV_this_row) / MV_this_row` using
consecutive rows directly: consecutive rows have different quantities after buy/sell
events, so the MV difference mixes quantity changes with price changes and produces
wrong results (including spurious negative returns in rising-price segments).

### What the result means

The result is the **weighted geometric average of sub-period growth factors**,
not the total cumulative return. For two equal-weight segments each returning 39%,
standard TWR gives `1.39² − 1 = 93%`; this formula gives 39%.

It answers: "what is the single characteristic return that represents all segments,
weighted by capital × time exposure?" This is useful for comparing investment quality
across periods with very different capital levels, but it does not tell you how much
total wealth was created.

Total cumulative return cannot be recovered from the weighted geometric mean alone
without the individual segment returns and weights — at that point it is simpler
to compute TWR directly.

### Validation on a real holding (Micron Technology Inc.)

Applied to 12 transaction segments spanning 283 days (2025-09-16 to 2026-06-26),
with mixed buys and sells and a cumulative holding of 43 shares at end:

- Total weight sum (MV × days): 68,215,444 SEK·days
- Largest single weight: segment with 80-day hold and high market value (~32%)
- Sum of weighted log gains: 0.331
- Product of gain factors `Π(1 + r_i)`: 7.23
- Characteristic segment return (approach B): **~39%** (`exp(0.331) − 1`)
- Standard TWR (approach A): **~623%** (`7.23 − 1`)

The result illustrates that a heavily churned position with large late-period sells
produces a stable, moderate characteristic return under this method — in contrast to
the >100% figures from single-shot Modified Dietz on comparable holdings.

### Drawbacks

- The result is unfamiliar: not total return, not annualized return.
- Adding capital to a winning position reduces the result (the new capital starts
  at a high market value and earns returns from that higher base, pulling the
  weighted average down).
- Cannot be directly compared against Sharesight or other standard benchmarks.
- Requires per-transaction market values and FX rates at each transaction date.

## Open questions for implementation

- Which methodology for the portfolio total (money-weighted / simple / time-weighted)?
- Cumulative period return vs annualized for display?
- A Sharesight **portfolio-level** total return (since inception and YTD) is needed as a
  validation target; the per-holding screenshot is not sufficient.
