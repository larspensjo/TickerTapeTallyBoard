use chrono::{DateTime, Duration, NaiveDate, Utc};
use rust_decimal::Decimal;

use super::{parameters::*, EvidenceReason};
use crate::domain::{market_snapshot::trading_days_stale, ProviderCode};

/// A stored close with provider identity and fetch instant already decoded by the loader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryClose {
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: String,
    pub provider: ProviderCode,
    pub fetched_at: DateTime<Utc>,
}

/// The loader supplies the effective latest close (including its source), rather than
/// inferring precedence from all stored sources. Ledger and provider split dates are merged.
#[derive(Clone, Debug)]
pub struct LookbackSeries {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub provider: Option<ProviderCode>,
    /// Caller-supplied fact for the selected source, independent of its identity.
    pub split_adjusted_at_fetch: bool,
    pub latest_date: Option<NaiveDate>,
    pub closes: Vec<HistoryClose>,
    pub split_dates: Vec<NaiveDate>,
    pub excluded_source_closes: usize,
    pub excluded_currency_closes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryTopUpNeed {
    Short { closes: usize, required: usize },
    Gap { start: NaiveDate },
    SplitStale { date: NaiveDate },
    SplitAdjustmentUnverified { date: NaiveDate },
}

pub fn prepare_lookback(
    today: NaiveDate,
    instrument_currency: &str,
    latest: Option<&HistoryClose>,
    stored: &[HistoryClose],
    ledger_splits: &[NaiveDate],
    provider_splits: &[NaiveDate],
    split_adjusted_at_fetch: bool,
) -> LookbackSeries {
    let start = today - Duration::days(LOOKBACK_DAYS);
    let provider = latest
        .filter(|close| close.date <= today)
        .map(|close| close.provider.clone());
    let mut series = LookbackSeries {
        start,
        end: today,
        provider,
        split_adjusted_at_fetch,
        latest_date: latest
            .filter(|close| close.date <= today)
            .map(|close| close.date),
        closes: Vec::new(),
        split_dates: Vec::new(),
        excluded_source_closes: 0,
        excluded_currency_closes: 0,
    };
    for close in stored
        .iter()
        .filter(|close| close.date >= start && close.date <= today)
    {
        if Some(&close.provider) != series.provider.as_ref() {
            series.excluded_source_closes += 1;
        } else if !close.currency.eq_ignore_ascii_case(instrument_currency) {
            series.excluded_currency_closes += 1;
        } else {
            series.closes.push(close.clone());
        }
    }
    series.closes.sort_by_key(|close| close.date);
    series.split_dates = ledger_splits
        .iter()
        .chain(provider_splits)
        .copied()
        .filter(|date| *date >= start && *date <= today)
        .collect();
    series.split_dates.sort_unstable();
    series.split_dates.dedup();
    series
}

/// Missing weekdays strictly between closes. Reuses the application's weekday
/// approximation; the right endpoint is an observed close, not a missing day.
pub fn missing_weekdays(left: NaiveDate, right: NaiveDate) -> i64 {
    right
        .pred_opt()
        .map_or(0, |before_right| trading_days_stale(before_right, left))
}

/// Dates must be ascending. Coverage starts at the look-back boundary, so a late
/// first close is a gap too. Holidays receive no special calendar treatment.
pub fn history_gaps(start: NaiveDate, dates: &[NaiveDate]) -> Vec<NaiveDate> {
    let mut previous = start;
    let mut gaps = Vec::new();
    for date in dates {
        if missing_weekdays(previous, *date) > GAP_LIMIT_WEEKDAYS {
            gaps.push(previous);
        }
        previous = *date;
    }
    gaps
}

/// UTC fetch date must be strictly after every effective split date. Empty history
/// is handled by the short-history rule, as in the readiness measurement.
pub fn blocked_splits(
    closes: &[HistoryClose],
    splits: &[NaiveDate],
    split_adjusted_at_fetch: bool,
) -> Vec<NaiveDate> {
    splits
        .iter()
        .copied()
        .filter(|split| {
            closes.iter().any(|close| {
                !split_adjusted_at_fetch
                    || split_fetch_is_stale(close.fetched_at.date_naive(), *split)
            })
        })
        .collect()
}

/// Fetch dates are UTC dates. A fetch on the split date is not a restatement.
/// Kept separate so loaders can evaluate fetch dates lazily without decoding prices.
pub fn split_fetch_is_stale(fetched_on: NaiveDate, split: NaiveDate) -> bool {
    fetched_on <= split
}

pub fn short_history_reason(closes: usize, required: usize) -> Option<EvidenceReason> {
    (closes < required).then_some(EvidenceReason::PriceHistoryTooShort { closes, required })
}

impl LookbackSeries {
    pub fn reasons(&self, minimum_closes: usize) -> Vec<EvidenceReason> {
        let mut reasons = Vec::new();
        if self.provider.is_none() || self.closes.last().map(|close| close.date) != self.latest_date
        {
            reasons.push(EvidenceReason::MissingLatestClose);
        }
        reasons.extend(
            history_gaps(self.start, &self.dates())
                .into_iter()
                .map(|start| EvidenceReason::PriceHistoryGap { start }),
        );
        reasons.extend(
            blocked_splits(
                &self.closes,
                &self.split_dates,
                self.split_adjusted_at_fetch,
            )
            .into_iter()
            .map(|date| EvidenceReason::SplitInLookback { date }),
        );
        reasons.extend(short_history_reason(self.closes.len(), minimum_closes));
        reasons
    }

    pub fn top_up_needs(&self) -> Vec<HistoryTopUpNeed> {
        let mut needs = Vec::new();
        if short_history_reason(self.closes.len(), RANGE_MIN_CLOSES).is_some() {
            needs.push(HistoryTopUpNeed::Short {
                closes: self.closes.len(),
                required: RANGE_MIN_CLOSES,
            });
        }
        needs.extend(
            history_gaps(self.start, &self.dates())
                .into_iter()
                .map(|start| HistoryTopUpNeed::Gap { start }),
        );
        needs.extend(
            blocked_splits(
                &self.closes,
                &self.split_dates,
                self.split_adjusted_at_fetch,
            )
            .into_iter()
            .map(|date| {
                if self.split_adjusted_at_fetch {
                    HistoryTopUpNeed::SplitStale { date }
                } else {
                    HistoryTopUpNeed::SplitAdjustmentUnverified { date }
                }
            }),
        );
        needs
    }

    fn dates(&self) -> Vec<NaiveDate> {
        self.closes.iter().map(|close| close.date).collect()
    }
}
