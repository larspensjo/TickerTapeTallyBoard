use chrono::NaiveDate;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use super::availability::{Availability, ValuationReason};
use super::cash_flow::CashFlow;

#[derive(Debug, Clone)]
pub struct MoneyWeightedReturn {
    /// Annualized XIRR rate. `None` when it exceeds `Decimal`'s range — which happens for a
    /// large return over a very short period, where the annualized rate is astronomical even
    /// though `cumulative` (the displayed value) is modest and well-defined.
    pub annualized: Option<Decimal>,
    pub cumulative: Decimal,
    pub period_days: i64,
}

/// `cash_flows` MUST be the actual investor cash-flow series (real cash at trade date, with
/// no post-period split multiplication).
pub fn compute_money_weighted_return(
    begin_market_value: &Availability<Decimal>,
    cash_flows: &Availability<Vec<CashFlow>>,
    end_market_value: &Availability<Decimal>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Availability<MoneyWeightedReturn> {
    let begin = match begin_market_value {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => {
            return Availability::Unavailable {
                reasons: reasons.clone(),
            }
        }
    };
    let flows = match cash_flows {
        Availability::Available(v) => v,
        Availability::Unavailable { reasons } => {
            return Availability::Unavailable {
                reasons: reasons.clone(),
            }
        }
    };
    let end_mv = match end_market_value {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => {
            return Availability::Unavailable {
                reasons: reasons.clone(),
            }
        }
    };

    let period_days = (end_date - start_date).num_days();
    if period_days <= 0 {
        return Availability::unavailable(ValuationReason::ZeroOrInvalidPerformanceDenominator);
    }

    // Investor-perspective dated flows in f64 (display-only solve). Time is measured in
    // *period fractions* (0.0 at start_date, 1.0 at end_date), not years. Solving in period
    // units means the solved root is the cumulative period return directly; its magnitude is
    // bounded by the actual return. Solving in annual units instead makes the root explode
    // for short periods (a large return over days annualizes beyond any fixed scan cap),
    // which would falsely report non-convergence.
    let mut series: Vec<(f64, f64)> = Vec::with_capacity(flows.len() + 2);
    let period_fraction_at = |d: NaiveDate| (d - start_date).num_days() as f64 / period_days as f64;
    // Fallible conversion: a value that cannot become a finite f64 must surface as
    // unavailable, never silently become zero (which would alter the cash flows and
    // violate the "missing data is explicit, never zero" constraint).
    fn to_finite_f64(x: Decimal) -> Option<f64> {
        x.to_f64().filter(|v| v.is_finite())
    }
    let begin_f = match to_finite_f64(begin) {
        Some(v) => v,
        None => return Availability::unavailable(ValuationReason::PerformanceDidNotConverge),
    };
    series.push((0.0, -begin_f));
    for cf in flows {
        let cf_f = match to_finite_f64(cf.amount_base) {
            Some(v) => v,
            None => return Availability::unavailable(ValuationReason::PerformanceDidNotConverge),
        };
        series.push((period_fraction_at(cf.date), -cf_f));
    }
    let end_f = match to_finite_f64(end_mv) {
        Some(v) => v,
        None => return Availability::unavailable(ValuationReason::PerformanceDidNotConverge),
    };
    series.push((1.0, end_f));

    let npv = |rate: f64| -> f64 { series.iter().map(|(t, c)| c / (1.0 + rate).powf(*t)).sum() };

    // Scan the period-return range for sign-change sub-brackets instead of trusting the two
    // endpoints. Interleaved buys/sells make NPV non-monotonic: an interior root can sit
    // between two same-sign endpoints, and multiple roots can exist. We collect every
    // bracket; zero brackets = no root, more than one = ambiguous multi-IRR -> refuse.
    let mut scan: Vec<f64> = vec![-0.9999];
    let mut r = -0.99_f64;
    while r < 1.0 {
        scan.push(r);
        r += 0.01;
    }
    let mut r = 1.0_f64;
    while r < 1_000_000.0 {
        scan.push(r);
        r *= 2.0;
    }
    scan.push(1_000_000.0);

    let mut brackets: Vec<(f64, f64)> = Vec::new();
    let mut exact: Option<f64> = None;
    let mut exact_count: usize = 0;
    for w in scan.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let (flo, fhi) = (npv(lo), npv(hi));
        if flo.is_nan() || fhi.is_nan() {
            continue;
        }
        if flo == 0.0 {
            exact = Some(lo);
            exact_count += 1;
        } else if flo * fhi < 0.0 {
            brackets.push((lo, hi));
        }
    }
    let rate = if let Some(r) = exact {
        if !brackets.is_empty() || exact_count > 1 {
            // Exact root(s) plus additional crossings or multiple exact roots = ambiguous.
            return Availability::unavailable(ValuationReason::PerformanceDidNotConverge);
        }
        r
    } else {
        if brackets.len() != 1 {
            // Zero roots or an ambiguous multi-root series: do not guess.
            return Availability::unavailable(ValuationReason::PerformanceDidNotConverge);
        }
        let (mut a, mut b) = brackets[0];
        // Track the sign at `a`; move whichever endpoint matches the midpoint's sign so the
        // bracket invariant holds regardless of the curve's orientation.
        let sign_a = npv(a) > 0.0;
        for _ in 0..300 {
            let m = (a + b) / 2.0;
            if (npv(m) > 0.0) == sign_a {
                a = m;
            } else {
                b = m;
            }
        }
        (a + b) / 2.0
    };
    // `rate` is the cumulative period return (time was measured in period fractions).
    let cumulative = match Decimal::from_f64_retain(rate) {
        Some(v) => v,
        None => return Availability::unavailable(ValuationReason::PerformanceDidNotConverge),
    };
    // Annualize: (1 + period_return)^(365.25 / period_days) - 1. May exceed Decimal's range
    // for a large return over a short period; that is recorded as None, not a failure, since
    // the displayed value is `cumulative`.
    let annualized_f64 = (1.0 + rate).powf(365.25 / period_days as f64) - 1.0;
    let annualized = annualized_f64
        .is_finite()
        .then(|| Decimal::from_f64_retain(annualized_f64))
        .flatten();
    Availability::Available(MoneyWeightedReturn {
        annualized,
        cumulative,
        period_days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn money_weighted_simple_hold_matches_simple_return() {
        // Invest 100k at start, worth 120k after exactly one year, no flows.
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![CashFlow {
            date: date("2025-01-01"),
            amount_base: dec!(100000), // buy cost
        }]);
        let end = Availability::Available(dec!(120000));
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2025-01-01"),
            date("2026-01-01"),
        );
        let v = match r {
            Availability::Available(v) => v,
            _ => panic!("available"),
        };
        assert!(
            (v.cumulative - dec!(0.20)).abs() < dec!(0.001),
            "cumulative {}",
            v.cumulative
        );
    }

    #[test]
    fn money_weighted_converges_for_large_return_over_short_period() {
        // Regression: a ~29x return over 9 days has an astronomically large *annualized*
        // rate (~10^59), but a well-defined cumulative period return (~28.0 = 2804%). The
        // solver must bracket on the bounded period return, not the annualized rate, or it
        // finds no root within any fixed scan cap and falsely reports non-convergence.
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![CashFlow {
            date: date("2026-06-12"),
            amount_base: dec!(1250), // buy cost (10 sh @ $12.50, fx 10)
        }]);
        let end = Availability::Available(dec!(36300));
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2026-06-12"),
            date("2026-06-21"),
        );
        let v = match r {
            Availability::Available(v) => v,
            Availability::Unavailable { reasons } => {
                panic!("expected available, got {reasons:?}")
            }
        };
        // Cumulative period return ≈ 36300 / 1250 - 1 = 28.04.
        assert!(
            (v.cumulative - dec!(28.04)).abs() < dec!(0.1),
            "cumulative {}",
            v.cumulative
        );
    }

    #[test]
    fn money_weighted_is_cash_flow_neutral_for_same_day_trade() {
        // A buy today (cash out + equal market-value in) must not change the result.
        let start = date("2025-01-01");
        let end = date("2025-07-01");
        let base_flows = vec![CashFlow {
            date: start,
            amount_base: dec!(100000),
        }];
        let begin = Availability::Available(Decimal::ZERO);
        let end_mv = Availability::Available(dec!(150000));

        let r1 = compute_money_weighted_return(
            &begin,
            &Availability::Available(base_flows.clone()),
            &end_mv,
            start,
            end,
        );

        // Same trade today: +50k buy cost flow at end_date, end MV also +50k.
        let mut with_trade = base_flows.clone();
        with_trade.push(CashFlow {
            date: end,
            amount_base: dec!(50000),
        });
        let r2 = compute_money_weighted_return(
            &begin,
            &Availability::Available(with_trade),
            &Availability::Available(dec!(200000)),
            start,
            end,
        );

        let a = match r1 {
            Availability::Available(v) => v.annualized.expect("annualized representable"),
            _ => panic!(),
        };
        let b = match r2 {
            Availability::Available(v) => v.annualized.expect("annualized representable"),
            _ => panic!(),
        };
        assert!(
            (a - b).abs() < dec!(0.0001),
            "neutrality violated: {a} vs {b}"
        );
    }

    #[test]
    fn money_weighted_unavailable_when_no_sign_change() {
        // All inflows, no outflow -> no root.
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![CashFlow {
            date: date("2025-01-01"),
            amount_base: dec!(-100),
        }]);
        let end = Availability::Available(dec!(100));
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2025-01-01"),
            date("2026-01-01"),
        );
        assert!(matches!(r, Availability::Unavailable { .. }));
    }

    #[test]
    fn money_weighted_solves_with_interleaved_buy_and_sell() {
        // Alternating-sign flows: buy, partial sell mid-period, open remainder at end.
        // A single well-defined root must still be found (solver must not assume the NPV
        // curve is monotonic or that positive NPV always belongs to the lower bound).
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![
            CashFlow {
                date: date("2025-01-01"),
                amount_base: dec!(100000),
            }, // buy cost
            CashFlow {
                date: date("2025-07-01"),
                amount_base: dec!(-60000),
            }, // sell proceeds
        ]);
        let end = Availability::Available(dec!(70000));
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2025-01-01"),
            date("2026-01-01"),
        );
        let v = match r {
            Availability::Available(v) => v,
            _ => panic!("expected a root"),
        };
        // Verify: NPV at the solved rate is negligible (< 1.0 in SEK absolute terms).
        let rate_f64 = v
            .annualized
            .expect("annualized representable")
            .to_f64()
            .expect("finite");
        let t_sell = 181.0_f64 / 365.25;
        let t_end = 365.0_f64 / 365.25;
        let npv_residual = -100000.0_f64
            + 60000.0 / (1.0 + rate_f64).powf(t_sell)
            + 70000.0 / (1.0 + rate_f64).powf(t_end);
        assert!(
            npv_residual.abs() < 1.0,
            "NPV residual at solved rate: {npv_residual}"
        );
    }

    #[test]
    fn money_weighted_unavailable_when_multiple_roots() {
        // A flow series that produces more than one sign change / IRR root must return
        // PerformanceDidNotConverge rather than silently picking one. Construct a series with
        // two interior roots (large early inflow, larger outflow, inflow again).
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![
            CashFlow {
                date: date("2025-01-01"),
                amount_base: dec!(1000),
            }, // -1000 investor
            CashFlow {
                date: date("2025-06-01"),
                amount_base: dec!(-2500),
            }, // +2500 investor
        ]);
        let end = Availability::Available(dec!(-1560)); // forces a second sign change in NPV
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2025-01-01"),
            date("2026-01-01"),
        );
        // Either a single documented root or Unavailable; this case must be Unavailable.
        assert!(matches!(r, Availability::Unavailable { .. }));
    }

    #[test]
    fn money_weighted_unavailable_when_all_flows_cancel_at_every_rate() {
        // Same-day buy and sell of equal amount with zero begin/end MV:
        // investor flows are (-amount at t=0) + (+amount at t=0) = 0 for every rate.
        // NPV is identically zero across all rates — degenerate, should refuse.
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![
            CashFlow {
                date: date("2025-01-01"),
                amount_base: dec!(50000),
            }, // buy cost
            CashFlow {
                date: date("2025-01-01"),
                amount_base: dec!(-50000),
            }, // sell proceeds same day
        ]);
        let end = Availability::Available(Decimal::ZERO);
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2025-01-01"),
            date("2026-01-01"),
        );
        assert!(
            matches!(r, Availability::Unavailable { .. }),
            "degenerate all-zero NPV must be unavailable"
        );
    }
}
