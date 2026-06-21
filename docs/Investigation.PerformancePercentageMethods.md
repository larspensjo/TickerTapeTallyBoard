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

## Open questions for implementation

- Which methodology for the portfolio total (money-weighted / simple / time-weighted)?
- Cumulative period return vs annualized for display?
- A Sharesight **portfolio-level** total return (since inception and YTD) is needed as a
  validation target; the per-holding screenshot is not sufficient.
