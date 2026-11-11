//! Shared policy values. Percentages are percentage points, not fractions.

use crate::domain::RecentWindow;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

pub const LOOKBACK_DAYS: i64 = 365;
pub const UNUSUAL_DROP_DEVIATIONS: Decimal = dec!(1.5);
pub const UNUSUAL_MIN_CLOSES: usize = 120;
pub const RANGE_MIN_CLOSES: usize = 200;
pub const GAP_LIMIT_WEEKDAYS: i64 = 5;
pub const DEEP_BELOW_HIGH_PERCENT: Decimal = dec!(20);
pub const NEAR_LOW_PERCENT: Decimal = dec!(5);
pub const REPORT_WINDOW_WIDENING_DAYS: i64 = 7;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoReportRule {
    Strict,
    Relaxed,
}
pub const NO_REPORT_RULE: NoReportRule = NoReportRule::Relaxed;
pub const RELAXED_REPORT_HORIZON_DAYS: i64 = 14;
pub const REPORT_SOON_MIN_DAYS: i64 = 0;
pub const REPORT_SOON_MAX_DAYS: i64 = 7;
pub const PROFITABLE_MARGIN_PERCENT: Decimal = dec!(0);
pub const EARNINGS_SLIDING_PERCENT: Decimal = dec!(10);
pub const EPS_ROUNDING_ALLOWANCE: Decimal = dec!(0.02);
pub const EPS_APPROXIMATE_PERCENT: Decimal = dec!(5);
pub const EPS_UNKNOWN_PERCENT: Decimal = dec!(20);
pub const MARKET_MIN_OTHER_ROWS: usize = 5;
pub const MARKET_MARGIN_7D: Decimal = dec!(3);
pub const MARKET_MARGIN_14D: Decimal = dec!(4);
pub const MARKET_MARGIN_30D: Decimal = dec!(6);
pub const STOPPED_FALLING_MIN_DAYS: i64 = 2;
pub const CALIBRATION_MIN_FETCH_DATE_OFFSET_DAYS: i64 = 1;
pub const CALIBRATION_MIN_DIPS: usize = 100;
pub const CALIBRATION_MIN_FLAGGED_DIPS: usize = 30;
pub const CALIBRATION_MIN_INSTRUMENTS: usize = 8;
pub const CALIBRATION_MIN_WEEKS: usize = 26;

pub fn market_margin(window: RecentWindow) -> Decimal {
    match window {
        RecentWindow::OneWeek => MARKET_MARGIN_7D,
        RecentWindow::TwoWeeks => MARKET_MARGIN_14D,
        RecentWindow::ThirtyDays => MARKET_MARGIN_30D,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Construct values from the actual constants, rather than keeping a second literal table.
    #[test]
    fn spike_parameters_and_domain_policy_are_identical() {
        let document = include_str!("../../../../docs/spikes/OpportunityChecksSpike.md");
        let section = document
            .split("## Initial parameters")
            .nth(1)
            .expect("parameters section");
        let actual: Vec<_> = section
            .lines()
            .skip_while(|line| !line.starts_with("| Parameter |"))
            .skip(2)
            .take_while(|line| line.starts_with('|'))
            .map(|line| {
                let cells: Vec<_> = line.split('|').map(str::trim).collect();
                (cells[1].to_owned(), cells[2].to_owned())
            })
            .collect();
        let no_report = match NO_REPORT_RULE {
            NoReportRule::Relaxed => "relaxed, because the readiness measurement showed the strict rule answers under two thirds of company rows (30d)",
            NoReportRule::Strict => "strict",
        };
        let expected = vec![
            ("Look-back length", format!("{LOOKBACK_DAYS} calendar days")),
            ("Unusual drop", format!("change ≤ −{UNUSUAL_DROP_DEVIATIONS} × sample standard deviation of overlapping same-length moves ending on or before the start close")),
            ("Unusual drop minimum history", format!("{UNUSUAL_MIN_CLOSES} closes")),
            ("1-year range minimum history", format!("{RANGE_MIN_CLOSES} closes, full-year coverage")),
            ("Gap limit", format!("more than {GAP_LIMIT_WEEKDAYS} weekdays between consecutive closes")),
            ("Deep below 1-year high", format!("at least {DEEP_BELOW_HIGH_PERCENT}% below the highest close")),
            ("Near 1-year low", format!("latest close within {NEAR_LOW_PERCENT}% above the lowest close")),
            ("Report window widening", format!("{REPORT_WINDOW_WIDENING_DAYS} calendar days before the period start")),
            ("\"No report\" rule", no_report.to_owned()),
            ("Relaxed \"no report\" horizon", format!("estimate on report-history basis ≥ {RELAXED_REPORT_HORIZON_DAYS} days ahead")),
            ("Report coming soon", format!("next report {REPORT_SOON_MIN_DAYS}-{REPORT_SOON_MAX_DAYS} days ahead (today included)")),
            ("Profitable", format!("trailing profit margin > {PROFITABLE_MARGIN_PERCENT}; otherwise loss-making (a caution, not a block)")),
            ("Earnings sliding", format!("latest quarter EPS more than {EARNINGS_SLIDING_PERCENT}% (of the year-ago magnitude) below the year-ago quarter")),
            ("EPS pair consistency", format!("{EPS_ROUNDING_ALLOWANCE} allowance; {EPS_APPROXIMATE_PERCENT}-{EPS_UNKNOWN_PERCENT}% gap → approx.; over {EPS_UNKNOWN_PERCENT}% → unknown (existing tolerances)")),
            ("Market reference minimum", format!("{MARKET_MIN_OTHER_ROWS} other held rows")),
            ("Market margin", format!("{MARKET_MARGIN_7D} / {MARKET_MARGIN_14D} / {MARKET_MARGIN_30D} percentage points for 7d / 14d / 30d")),
            ("Stopped falling", format!("latest close above the window low; low at least {STOPPED_FALLING_MIN_DAYS} trading days old")),
            ("Calibration final close", match CALIBRATION_MIN_FETCH_DATE_OFFSET_DAYS {
                1 => "a close counts as final only if its `fetched_at` UTC date is after its price date".to_owned(),
                days => format!("fetch date at least {days} days after price date"),
            }),
            ("Calibration sufficiency", format!("per window: ≥ {CALIBRATION_MIN_DIPS} dips and ≥ {CALIBRATION_MIN_FLAGGED_DIPS} flagged dips from ≥ {CALIBRATION_MIN_INSTRUMENTS} instruments over ≥ {CALIBRATION_MIN_WEEKS} distinct weeks")),
        ].into_iter().map(|(name, value)| (name.to_owned(), value)).collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}
