use chrono::{Duration, NaiveDate};
use rust_decimal::Decimal;

use super::{
    lookback_series::{blocked_splits, history_gaps, LookbackSeries},
    parameters::*,
    EvidenceReason, YesNoUnknown,
};
use crate::domain::{
    market_snapshot::trading_days_stale, RecentWindow, START_CLOSE_MAX_AGE_TRADING_DAYS,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnusualDropOutcome {
    pub state: YesNoUnknown,
    pub reasons: Vec<EvidenceReason>,
    pub change_percent: Decimal,
    pub adjusted_change_percent: Option<Decimal>,
    pub usual_move_percent: Option<Decimal>,
    pub threshold_percent: Option<Decimal>,
    pub closes: usize,
    pub sample_moves: usize,
}

/// The start date and displayed change come from the existing recent-price-change
/// result. Only this comparison uses the optional dividend-added-back change.
pub fn unusual_drop(
    series: &LookbackSeries,
    window: RecentWindow,
    start_close_date: NaiveDate,
    change_percent: Decimal,
    dividend_added_back_change: Option<Decimal>,
) -> UnusualDropOutcome {
    let mut outcome = UnusualDropOutcome {
        state: YesNoUnknown::Unknown,
        reasons: series.unusual_drop_reasons(),
        change_percent,
        adjusted_change_percent: dividend_added_back_change,
        usual_move_percent: None,
        threshold_percent: None,
        closes: series.closes.len(),
        sample_moves: 0,
    };
    if !outcome.reasons.is_empty() {
        return outcome;
    }
    if let Some(close) = series
        .closes
        .iter()
        .find(|close| close.close <= Decimal::ZERO)
    {
        outcome
            .reasons
            .push(EvidenceReason::NonPositiveClose { date: close.date });
        return outcome;
    }
    // Each stored close can end an overlapping move. Pair on/before the same
    // calendar-length window start, using the existing start-close tolerance.
    // No move ending after the current start close can enter the reference sample.
    let mut moves = Vec::new();
    for end in series
        .closes
        .iter()
        .take_while(|close| close.date <= start_close_date)
    {
        let target = end.date - Duration::days(window.days());
        let index = series.closes.partition_point(|close| close.date <= target);
        if index == 0 {
            continue;
        }
        let start = &series.closes[index - 1];
        if trading_days_stale(target, start.date) > START_CLOSE_MAX_AGE_TRADING_DAYS {
            continue;
        }
        let value = end
            .close
            .checked_sub(start.close)
            .and_then(|difference| difference.checked_div(start.close))
            .and_then(|ratio| ratio.checked_mul(Decimal::ONE_HUNDRED));
        let Some(value) = value else {
            outcome.reasons.push(EvidenceReason::CalculationOutOfRange);
            return outcome;
        };
        moves.push(value);
    }
    outcome.sample_moves = moves.len();
    if moves.len() < 2 {
        outcome
            .reasons
            .push(EvidenceReason::InsufficientHistoricalMoves { moves: moves.len() });
        return outcome;
    }
    let Some(deviation) = sample_standard_deviation(&moves) else {
        outcome.reasons.push(EvidenceReason::CalculationOutOfRange);
        return outcome;
    };
    outcome.usual_move_percent = Some(deviation);
    if deviation.is_zero() {
        outcome.reasons.push(EvidenceReason::NoPriceVariation);
        return outcome;
    }
    let Some(threshold) = (-UNUSUAL_DROP_DEVIATIONS).checked_mul(deviation) else {
        outcome.reasons.push(EvidenceReason::CalculationOutOfRange);
        return outcome;
    };
    outcome.threshold_percent = Some(threshold);
    outcome.state = if dividend_added_back_change.unwrap_or(change_percent) <= threshold {
        YesNoUnknown::Yes
    } else {
        YesNoUnknown::No
    };
    outcome
}

/// Decimal arithmetic throughout, including Newton's square root. Round the root
/// to 12 decimal places before comparison to make convergence/precision explicit.
fn sample_standard_deviation(values: &[Decimal]) -> Option<Decimal> {
    if values.len() < 2 {
        return None;
    }
    let count = Decimal::from(values.len());
    let sum = values
        .iter()
        .try_fold(Decimal::ZERO, |sum, value| sum.checked_add(*value))?;
    let mean = sum.checked_div(count)?;
    let squares = values.iter().try_fold(Decimal::ZERO, |sum, value| {
        let delta = value.checked_sub(mean)?;
        sum.checked_add(delta.checked_mul(delta)?)
    })?;
    let variance = squares.checked_div(count - Decimal::ONE)?;
    if variance.is_zero() {
        return Some(Decimal::ZERO);
    }
    let mut root = variance.max(Decimal::ONE);
    for _ in 0..256 {
        let next = root
            .checked_div(Decimal::TWO)?
            .checked_add(variance.checked_div(root)?.checked_div(Decimal::TWO)?)?;
        if next.round_dp(12) == root.round_dp(12) {
            return Some(next.round_dp(12));
        }
        root = next;
    }
    None
}

#[cfg(test)]
mod dispersion_tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn dispersion_is_sample_not_population_and_does_not_subtract_mean_from_the_drop() {
        assert_eq!(
            sample_standard_deviation(&[dec!(-2), dec!(0), dec!(2)]),
            Some(dec!(2))
        );
        assert_eq!(
            sample_standard_deviation(&[dec!(8), dec!(10), dec!(12)]),
            Some(dec!(2))
        );
        assert_eq!(-UNUSUAL_DROP_DEVIATIONS * dec!(2), dec!(-3));
        assert_eq!(
            sample_standard_deviation(&[dec!(1), dec!(1)]),
            Some(Decimal::ZERO)
        );
        assert_eq!(sample_standard_deviation(&[dec!(1)]), None);
        assert_eq!(
            sample_standard_deviation(&[Decimal::MIN, Decimal::MAX]),
            None
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffHighState {
    Deep,
    NotDeep,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OffHighOutcome {
    pub state: OffHighState,
    pub reasons: Vec<EvidenceReason>,
    pub closes: usize,
    pub latest_close: Option<Decimal>,
    pub high_close: Option<Decimal>,
    pub low_close: Option<Decimal>,
    pub below_high_percent: Option<Decimal>,
    pub above_low_percent: Option<Decimal>,
    pub near_low: Option<bool>,
}

pub fn off_high(series: &LookbackSeries) -> OffHighOutcome {
    let mut outcome = OffHighOutcome {
        state: OffHighState::Unknown,
        reasons: series.reasons(RANGE_MIN_CLOSES),
        closes: series.closes.len(),
        latest_close: None,
        high_close: None,
        low_close: None,
        below_high_percent: None,
        above_low_percent: None,
        near_low: None,
    };
    if !outcome.reasons.is_empty() {
        return outcome;
    }
    if let Some(close) = series
        .closes
        .iter()
        .find(|close| close.close <= Decimal::ZERO)
    {
        outcome
            .reasons
            .push(EvidenceReason::NonPositiveClose { date: close.date });
        return outcome;
    }
    let Some(latest) = series.closes.last() else {
        return outcome;
    };
    let high = series
        .closes
        .iter()
        .map(|close| close.close)
        .max()
        .unwrap_or(latest.close);
    let low = series
        .closes
        .iter()
        .map(|close| close.close)
        .min()
        .unwrap_or(latest.close);
    let below = (high - latest.close)
        .checked_div(high)
        .and_then(|value| value.checked_mul(Decimal::ONE_HUNDRED));
    let above = (latest.close - low)
        .checked_div(low)
        .and_then(|value| value.checked_mul(Decimal::ONE_HUNDRED));
    let (Some(below), Some(above)) = (below, above) else {
        outcome.reasons.push(EvidenceReason::CalculationOutOfRange);
        return outcome;
    };
    outcome.latest_close = Some(latest.close);
    outcome.high_close = Some(high);
    outcome.low_close = Some(low);
    outcome.below_high_percent = Some(below);
    outcome.above_low_percent = Some(above);
    outcome.near_low = Some(above <= NEAR_LOW_PERCENT);
    outcome.state = if below >= DEEP_BELOW_HIGH_PERCENT {
        OffHighState::Deep
    } else {
        OffHighState::NotDeep
    };
    outcome
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoppedFallingOutcome {
    pub state: YesNoUnknown,
    pub reasons: Vec<EvidenceReason>,
    pub latest_close: Option<Decimal>,
    pub low_close: Option<Decimal>,
    pub low_date: Option<NaiveDate>,
    pub low_age_trading_days: Option<i64>,
}

/// Actual close dates from the available recent change delimit the window. A
/// distant old gap/split must not invalidate this window-only display check.
pub fn stopped_falling(
    series: &LookbackSeries,
    start: NaiveDate,
    end: NaiveDate,
) -> StoppedFallingOutcome {
    let closes: Vec<_> = series
        .closes
        .iter()
        .filter(|close| close.date >= start && close.date <= end)
        .cloned()
        .collect();
    let mut outcome = StoppedFallingOutcome {
        state: YesNoUnknown::Unknown,
        reasons: Vec::new(),
        latest_close: None,
        low_close: None,
        low_date: None,
        low_age_trading_days: None,
    };
    if closes.first().map(|close| close.date) != Some(start)
        || closes.last().map(|close| close.date) != Some(end)
    {
        outcome.reasons.push(EvidenceReason::MissingWindowCloses);
        return outcome;
    }
    let dates: Vec<_> = closes.iter().map(|close| close.date).collect();
    outcome.reasons.extend(
        history_gaps(start, &dates)
            .into_iter()
            .map(|start| EvidenceReason::PriceHistoryGap { start }),
    );
    let splits: Vec<_> = series
        .split_dates
        .iter()
        .copied()
        .filter(|date| *date > start && *date <= end)
        .collect();
    outcome.reasons.extend(
        blocked_splits(&closes, &splits, series.split_adjusted_at_fetch)
            .into_iter()
            .map(|date| EvidenceReason::SplitInLookback { date }),
    );
    if let Some(close) = closes.iter().find(|close| close.close <= Decimal::ZERO) {
        outcome
            .reasons
            .push(EvidenceReason::NonPositiveClose { date: close.date });
    }
    if !outcome.reasons.is_empty() {
        return outcome;
    }
    // Most recent occurrence of a tied low: retesting the low resets its age.
    let low = closes
        .iter()
        .rev()
        .min_by_key(|close| close.close)
        .expect("window endpoints exist");
    let latest = closes.last().expect("window endpoints exist");
    let age = trading_days_stale(end, low.date);
    outcome.latest_close = Some(latest.close);
    outcome.low_close = Some(low.close);
    outcome.low_date = Some(low.date);
    outcome.low_age_trading_days = Some(age);
    outcome.state = if latest.close > low.close && age >= STOPPED_FALLING_MIN_DAYS {
        YesNoUnknown::Yes
    } else {
        YesNoUnknown::No
    };
    outcome
}
