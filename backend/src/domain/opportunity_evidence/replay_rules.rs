//! Pure eligibility rules for offline replay. No application request path uses these.
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;

use super::{
    lookback_series::{
        blocked_splits, internal_history_gaps, prepare_lookback, HistoryClose, LookbackSeries,
    },
    parameters::*,
    price_checks::{self, OffHighOutcome, StoppedFallingOutcome, UnusualDropOutcome},
};
use crate::domain::{
    recent_price_change, DataFreshness, PriceCandidate, PriceSnapshot, RecentPriceChange,
    RecentWindow,
};

pub mod criteria;

pub struct CheckHistory<'a> {
    pub currency: &'a str,
    /// Caller-owned source capabilities; no provider knowledge enters the domain.
    pub split_adjusted_at_fetch: &'a dyn Fn(&str) -> bool,
    pub stored: &'a [HistoryClose],
    pub effective_finals: &'a [HistoryClose],
    pub ledger_splits: &'a [NaiveDate],
    pub provider_splits: &'a [NaiveDate],
}

pub struct ReplayChecks {
    pub series: LookbackSeries,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub change: Decimal,
    pub unusual: UnusualDropOutcome,
    pub off_high: OffHighOutcome,
    pub stopped: StoppedFallingOutcome,
}

/// The caller's effective final series retains the app's provider tie precedence.
/// Every price used by a check is bounded here, including latest/start selection.
pub fn checks_as_of(
    history: &CheckHistory<'_>,
    as_of: NaiveDate,
    window: RecentWindow,
) -> Option<ReplayChecks> {
    let stored = final_closes_as_of(history.stored, as_of);
    let finals = final_closes_as_of(history.effective_finals, as_of);
    let latest = finals
        .iter()
        .filter(|c| c.date <= as_of)
        .max_by_key(|c| c.date)?;
    let start = finals
        .iter()
        .filter(|c| c.date <= window.start_date(as_of))
        .max_by_key(|c| c.date);
    let end_price = PriceSnapshot {
        date: latest.date,
        close: latest.close,
        currency: latest.currency.clone(),
        source: latest.provider.clone(),
        freshness: DataFreshness::Fresh,
    };
    let start_price = start.map(|c| PriceCandidate {
        date: c.date,
        close: c.close,
        currency: c.currency.clone(),
        source: c.provider.clone(),
    });
    let RecentPriceChange::Available {
        start_date,
        end_date,
        change_percent,
        ..
    } = recent_price_change(
        window.start_date(as_of),
        Some(&end_price),
        start_price.as_ref(),
        history.ledger_splits,
    )
    else {
        return None;
    };
    let series = prepare_lookback(
        as_of,
        history.currency,
        Some(latest),
        &stored,
        history.ledger_splits,
        history.provider_splits,
        (history.split_adjusted_at_fetch)(latest.provider.as_str()),
    );
    let unusual = price_checks::unusual_drop(&series, window, start_date, change_percent, None);
    let off_high = price_checks::off_high(&series);
    let stopped = price_checks::stopped_falling(&series, start_date, end_date);
    Some(ReplayChecks {
        series,
        start: start_date,
        end: end_date,
        change: change_percent,
        unusual,
        off_high,
        stopped,
    })
}

pub fn is_final(close: &HistoryClose) -> bool {
    (close.fetched_at.date_naive() - close.date).num_days()
        >= CALIBRATION_MIN_FETCH_DATE_OFFSET_DAYS
}

/// Fetch instants can be later restatements; only price dates delimit as-of history.
/// Non-final bars disappear, allowing the ordinary history gap checks to see holes.
pub fn final_closes_as_of(stored: &[HistoryClose], as_of: NaiveDate) -> Vec<HistoryClose> {
    stored
        .iter()
        .filter(|c| c.date <= as_of && is_final(c))
        .cloned()
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Exclusion {
    NonFinal,
    Source,
    Currency,
    Gap,
    Split,
    Incomplete,
    BenchmarkUnavailable,
    InvalidClose,
}

impl Exclusion {
    pub const ALL: [Self; 8] = [
        Self::NonFinal,
        Self::Source,
        Self::Currency,
        Self::Gap,
        Self::Split,
        Self::Incomplete,
        Self::BenchmarkUnavailable,
        Self::InvalidClose,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NonFinal => "non_final",
            Self::Source => "source",
            Self::Currency => "currency",
            Self::Gap => "gap",
            Self::Split => "split",
            Self::Incomplete => "incomplete",
            Self::BenchmarkUnavailable => "benchmark_unavailable",
            Self::InvalidClose => "invalid_close_or_arithmetic",
        }
    }
}

/// `sessions` is the date-ascending, one-bar-per-date effective stored series,
/// BEFORE finality/source/currency filtering. h is counted in stored sessions.
/// All applicable reasons are returned, once each. Invalid interior bars are missing
/// for gap detection; an invalid endpoint always excludes the horizon.
pub fn forward_return(
    sessions: &[HistoryClose],
    start_close: &HistoryClose,
    horizon: usize,
    currency: &str,
    splits: &[NaiveDate],
    split_adjusted_at_fetch: &dyn Fn(&str) -> bool,
) -> Result<Decimal, Vec<Exclusion>> {
    let start = start_close.date;
    let source = &start_close.provider;
    let Ok(index) = sessions.binary_search_by_key(&start, |c| c.date) else {
        return Err(vec![Exclusion::Incomplete]);
    };
    let Some(end_index) = index.checked_add(horizon).filter(|i| *i < sessions.len()) else {
        return Err(vec![Exclusion::Incomplete]);
    };
    let mut closes = sessions[index..=end_index].to_vec();
    // The as-of endpoint can be a final lower-priority source when that day's
    // preferred bar is provisional. Start at the actual selected close.
    closes[0] = start_close.clone();
    let end = &sessions[end_index];
    let mut reasons = Vec::new();
    let mut dates = Vec::new();
    for (offset, close) in closes.iter().enumerate() {
        let endpoint = offset == 0 || offset == horizon;
        let final_close = is_final(close);
        let same_source = &close.provider == source;
        let same_currency = close.currency.eq_ignore_ascii_case(currency);
        if endpoint {
            if !final_close {
                reasons.push(Exclusion::NonFinal);
            }
            if !same_source {
                reasons.push(Exclusion::Source);
            }
            if !same_currency {
                reasons.push(Exclusion::Currency);
            }
            if close.close <= Decimal::ZERO {
                reasons.push(Exclusion::InvalidClose);
            }
        }
        if final_close && same_source && same_currency && close.close > Decimal::ZERO {
            dates.push(close.date);
        }
    }
    // Include the endpoints as boundaries even if invalid, so interior holes count.
    if dates.first() != Some(&start) {
        dates.insert(0, start);
    }
    if dates.last() != Some(&end.date) {
        dates.push(end.date);
    }
    if !internal_history_gaps(&dates).is_empty() {
        reasons.push(Exclusion::Gap);
    }
    let horizon_splits: Vec<_> = splits
        .iter()
        .copied()
        .filter(|s| *s > start && *s <= end.date)
        .collect();
    let adjusted = closes
        .iter()
        .all(|c| c.provider == *source && split_adjusted_at_fetch(c.provider.as_str()));
    if !blocked_splits(&closes, &horizon_splits, adjusted).is_empty() {
        reasons.push(Exclusion::Split);
    }
    // A selected endpoint beyond the latest final stored close is incomplete too.
    if sessions
        .iter()
        .rev()
        .find(|c| is_final(c))
        .is_none_or(|c| c.date < end.date)
    {
        reasons.push(Exclusion::Incomplete);
    }
    reasons.sort_unstable();
    reasons.dedup();
    if !reasons.is_empty() {
        return Err(reasons);
    }
    end.close
        .checked_sub(closes[0].close)
        .and_then(|delta| delta.checked_div(closes[0].close))
        .and_then(|r| r.checked_mul(Decimal::ONE_HUNDRED))
        .ok_or_else(|| vec![Exclusion::InvalidClose])
}

/// Exact median; even populations average the two middle observations.
pub fn median(values: &[Decimal]) -> Option<Decimal> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        let lower = sorted[middle - 1];
        let upper = sorted[middle];
        // Preserve precision for small values; only use the equivalent midpoint
        // form when the sum of two large same-sign observations overflows.
        match lower.checked_add(upper) {
            Some(sum) => sum.checked_div(Decimal::TWO),
            None => lower.checked_add(upper.checked_sub(lower)?.checked_div(Decimal::TWO)?),
        }
    } else {
        Some(sorted[middle])
    }
}

/// Caller excludes the subject, and supplies only other instruments held on the
/// replay date. None is an unusable own forward return, never an imputed zero.
pub fn benchmark(others: impl IntoIterator<Item = Option<Decimal>>) -> Result<Decimal, Exclusion> {
    let values: Vec<_> = others.into_iter().flatten().collect();
    if values.len() < MARKET_MIN_OTHER_ROWS {
        return Err(Exclusion::BenchmarkUnavailable);
    }
    median(&values).ok_or(Exclusion::InvalidClose)
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SampleSize {
    pub dips: usize,
    pub flagged: usize,
    pub flagged_instruments: usize,
    pub flagged_weeks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sufficiency {
    Calibrated,
    InsufficientHistory,
}
impl Sufficiency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Calibrated => "Calibrated",
            Self::InsufficientHistory => "Insufficient history",
        }
    }
}

/// Counts include only events with usable primary (20-session) excess returns.
pub fn sufficiency(size: SampleSize) -> Sufficiency {
    if size.dips >= CALIBRATION_MIN_DIPS
        && size.flagged >= CALIBRATION_MIN_FLAGGED_DIPS
        && size.flagged_instruments >= CALIBRATION_MIN_INSTRUMENTS
        && size.flagged_weeks >= CALIBRATION_MIN_WEEKS
    {
        Sufficiency::Calibrated
    } else {
        Sufficiency::InsufficientHistory
    }
}

pub fn week(date: NaiveDate) -> (i32, u32) {
    let week = date.iso_week();
    (week.year(), week.week())
}

#[cfg(test)]
mod tests;
