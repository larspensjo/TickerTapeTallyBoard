use chrono::NaiveDate;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use super::cash_flow::CashFlow;
use super::valuation::{Availability, ValuationReason};

/// Compute the Modified Dietz weighted denominator (begin_mv + Σ weight_i × cf_i).
///
/// Returns `Unavailable` when the denominator is zero or negative, or `period_days ≤ 0`.
pub fn compute_modified_dietz_denominator(
    begin_mv: Decimal,
    cash_flows: &[CashFlow],
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Availability<Decimal> {
    let period_days = (end_date - start_date).num_days();
    if period_days <= 0 {
        return Availability::Unavailable {
            reasons: vec![ValuationReason::ZeroOrInvalidPerformanceDenominator],
        };
    }
    let period_days_dec = Decimal::from(period_days);
    let weighted_flows: Decimal = cash_flows
        .iter()
        .map(|cf| {
            let remaining = (end_date - cf.date).num_days().max(0);
            let weight = Decimal::from(remaining) / period_days_dec;
            weight * cf.amount_base
        })
        .sum();
    let denominator = begin_mv + weighted_flows;
    if denominator <= Decimal::ZERO {
        return Availability::Unavailable {
            reasons: vec![ValuationReason::ZeroOrInvalidPerformanceDenominator],
        };
    }
    Availability::Available(denominator)
}

/// Compute the Modified Dietz holding-period return:
///
/// ```text
/// return = total_return / (begin_mv + Σ weight_i × cf_i)
/// ```
///
/// Returns `Unavailable` if any input is unavailable, if `period_days ≤ 0`, or if the
/// denominator is zero or negative.
pub fn compute_modified_dietz(
    begin_market_value: &Availability<Decimal>,
    total_return: &Availability<Decimal>,
    cash_flows: &Availability<Vec<CashFlow>>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Availability<Decimal> {
    let begin_mv = match begin_market_value {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => {
            return Availability::Unavailable {
                reasons: reasons.clone(),
            }
        }
    };
    let total = match total_return {
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

    let denominator =
        match compute_modified_dietz_denominator(begin_mv, flows, start_date, end_date) {
            Availability::Available(d) => d,
            unavail => return unavail,
        };

    Availability::Available(total / denominator)
}

/// Whether a performance percentage is displayed as-is or annualised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayPercentKind {
    Absolute,
    Annualised,
}

/// Convert a holding-period return to an annualised figure when the period exceeds one year.
///
/// Returns `(value, Absolute)` when:
/// - `period_days ≤ 0`
/// - `years < 1.0`
/// - `1 + hpr ≤ 0` (cannot take a root of a non-positive base)
/// - The float conversion fails
pub fn apply_annualisation(
    holding_period_return: Decimal,
    period_days: i64,
) -> (Decimal, DisplayPercentKind) {
    if period_days <= 0 {
        return (holding_period_return, DisplayPercentKind::Absolute);
    }
    let years = period_days as f64 / 365.25;
    if years < 1.0 {
        return (holding_period_return, DisplayPercentKind::Absolute);
    }
    let one_plus_r = holding_period_return + Decimal::ONE;
    if one_plus_r <= Decimal::ZERO {
        return (holding_period_return, DisplayPercentKind::Absolute);
    }
    let base: f64 = match one_plus_r.to_f64() {
        Some(v) => v,
        None => return (holding_period_return, DisplayPercentKind::Absolute),
    };
    let annualised_f64 = base.powf(1.0 / years) - 1.0;
    match rust_decimal::Decimal::from_f64_retain(annualised_f64) {
        Some(d) => (d, DisplayPercentKind::Annualised),
        None => (holding_period_return, DisplayPercentKind::Absolute),
    }
}

#[cfg(test)]
mod tests {
    use super::super::cash_flow::test_support::avail;
    use super::*;
    use rust_decimal_macros::dec;

    // ── Modified Dietz tests ──────────────────────────────────────────────────

    #[test]
    fn modified_dietz_simple_hold_no_cash_flows() {
        // 10,000 begin MV, 2,000 total return, no flows → 20%
        let begin = Availability::Available(dec!(10000));
        let total = Availability::Available(dec!(2000));
        let flows = Availability::Available(vec![]);
        let sd = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let ed = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(); // 181 days; no flows so period_days irrelevant
        let result = compute_modified_dietz(&begin, &total, &flows, sd, ed);
        assert_eq!(avail(&result), dec!(0.2));
    }

    #[test]
    fn modified_dietz_buy_at_start_full_weight() {
        // Begin MV 0, buy 10,000 at day 0 of 30-day period, end MV 12,000
        // weight = (end - start).num_days() / period_days = 30/30 = 1
        // denominator = 0 + 1*10000 = 10000; total_return = 2000; pct = 20%
        let sd = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let ed = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(); // 30 days
        let begin = Availability::Available(dec!(0));
        let total = Availability::Available(dec!(2000));
        let flows = Availability::Available(vec![CashFlow {
            date: sd,
            amount_base: dec!(10000),
        }]);
        let result = compute_modified_dietz(&begin, &total, &flows, sd, ed);
        assert_eq!(avail(&result), dec!(0.2));
    }

    #[test]
    fn modified_dietz_mid_period_flow_partial_weight() {
        // Begin MV 10,000; buy 10,000 on day 15 of 30-day period; total_return = 3,000
        // weight = 15/30 = 0.5; denominator = 10000 + 0.5*10000 = 15000; pct = 20%
        let sd = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let ed = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let mid = NaiveDate::from_ymd_opt(2026, 6, 16).unwrap(); // 15 days before end
        let begin = Availability::Available(dec!(10000));
        let total = Availability::Available(dec!(3000));
        let flows = Availability::Available(vec![CashFlow {
            date: mid,
            amount_base: dec!(10000),
        }]);
        let result = compute_modified_dietz(&begin, &total, &flows, sd, ed);
        assert_eq!(avail(&result), dec!(0.2));
    }

    #[test]
    fn modified_dietz_zero_denominator_is_unavailable() {
        let sd = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let ed = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let begin = Availability::Available(dec!(0));
        let total = Availability::Available(dec!(0));
        let flows = Availability::Available(vec![]);
        let result = compute_modified_dietz(&begin, &total, &flows, sd, ed);
        assert!(matches!(result, Availability::Unavailable { .. }));
    }

    // ── Annualisation tests ───────────────────────────────────────────────────

    #[test]
    fn annualise_over_one_year_returns_annualised() {
        // 20% over 730 days ≈ sqrt(1.2) - 1
        let (result, kind) = apply_annualisation(dec!(0.20), 730);
        assert!(matches!(kind, DisplayPercentKind::Annualised));
        let expected = rust_decimal::Decimal::from_f64_retain(1.2f64.powf(0.5) - 1.0).unwrap();
        let diff = (result - expected).abs();
        assert!(diff < dec!(0.0001), "diff too large: {diff}");
    }

    #[test]
    fn annualise_under_one_year_returns_absolute() {
        let (result, kind) = apply_annualisation(dec!(0.20), 180);
        assert!(matches!(kind, DisplayPercentKind::Absolute));
        assert_eq!(result, dec!(0.20));
    }

    #[test]
    fn annualise_negative_one_plus_return_returns_absolute_guard() {
        let (result, kind) = apply_annualisation(dec!(-1.5), 730);
        assert!(matches!(kind, DisplayPercentKind::Absolute));
        assert_eq!(result, dec!(-1.5));
    }
}
