use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::{parameters::*, EvidenceReason};
use crate::domain::{
    market_snapshot::trading_days_stale, RecentWindow, START_CLOSE_MAX_AGE_TRADING_DAYS,
};

/// Reference identity and actual paired dates travel with the comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarketReferenceSource {
    OtherHoldingsMedian,
    MarketIndex {
        code: String,
        display_name: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketState {
    FellMoreThanMarket,
    MovedWithMarket,
    FellLessThanMarket,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketOutcome {
    pub state: MarketState,
    pub reasons: Vec<EvidenceReason>,
    pub source: MarketReferenceSource,
    pub change_percent: Decimal,
    pub reference_percent: Option<Decimal>,
    pub difference_percent: Option<Decimal>,
    pub margin_percent: Decimal,
    pub count: usize,
}

/// Rows are held rows with at most one available change each, all in this window.
/// Identity exclusion happens here so the asset never participates in its own median.
pub fn market_reference(
    instrument_id: i64,
    change_percent: Decimal,
    window: RecentWindow,
    held_changes: &[(i64, Option<Decimal>)],
) -> MarketOutcome {
    let mut changes: Vec<_> = held_changes
        .iter()
        .filter(|(id, _)| *id != instrument_id)
        .filter_map(|(_, change)| *change)
        .collect();
    changes.sort_unstable();
    let count = changes.len();
    let mut outcome = MarketOutcome {
        state: MarketState::Unknown,
        reasons: Vec::new(),
        source: MarketReferenceSource::OtherHoldingsMedian,
        change_percent,
        reference_percent: None,
        difference_percent: None,
        margin_percent: market_margin(window),
        count,
    };
    if count < MARKET_MIN_OTHER_ROWS {
        outcome
            .reasons
            .push(EvidenceReason::InsufficientMarketRows {
                count,
                required: MARKET_MIN_OTHER_ROWS,
            });
        return outcome;
    }
    let middle = count / 2;
    let median = if count.is_multiple_of(2) {
        changes[middle - 1]
            .checked_add(changes[middle])
            .and_then(|sum| sum.checked_div(Decimal::TWO))
    } else {
        Some(changes[middle])
    };
    let Some(median) = median else {
        outcome.reasons.push(EvidenceReason::CalculationOutOfRange);
        return outcome;
    };
    outcome.reference_percent = Some(median);
    let Some(difference) = change_percent.checked_sub(median) else {
        outcome.reasons.push(EvidenceReason::CalculationOutOfRange);
        return outcome;
    };
    outcome.difference_percent = Some(difference);
    outcome.state = comparison_state(difference, outcome.margin_percent);
    outcome
}

/// Plain stored facts supplied by the caller; listing selection belongs outside this module.
pub struct IndexReference<'a> {
    pub code: &'a str,
    pub display_name: &'a str,
    pub closes: &'a [(NaiveDate, Decimal)],
}

pub fn market_reference_with_index(
    instrument_id: i64,
    change_percent: Decimal,
    window: RecentWindow,
    held_changes: &[(i64, Option<Decimal>)],
    dates: (NaiveDate, NaiveDate),
    index: Option<IndexReference<'_>>,
) -> MarketOutcome {
    if let Some(index) = index {
        if let Some((start, end, reference, difference)) =
            index_comparison(&index, dates, change_percent)
        {
            let margin = market_margin(window);
            return MarketOutcome {
                state: comparison_state(difference, margin),
                reasons: vec![],
                source: MarketReferenceSource::MarketIndex {
                    code: index.code.into(),
                    display_name: index.display_name.into(),
                    start_date: start,
                    end_date: end,
                },
                change_percent,
                reference_percent: Some(reference),
                difference_percent: Some(difference),
                margin_percent: margin,
                count: held_changes
                    .iter()
                    .filter(|(id, change)| *id != instrument_id && change.is_some())
                    .count(),
            };
        }
        let mut median = market_reference(instrument_id, change_percent, window, held_changes);
        median.reasons.push(EvidenceReason::MarketIndexUnavailable {
            display_name: index.display_name.into(),
        });
        return median;
    }
    market_reference(instrument_id, change_percent, window, held_changes)
}

fn comparison_state(difference: Decimal, margin: Decimal) -> MarketState {
    if difference < -margin {
        MarketState::FellMoreThanMarket
    } else if difference > margin {
        MarketState::FellLessThanMarket
    } else {
        MarketState::MovedWithMarket
    }
}

fn index_comparison(
    index: &IndexReference<'_>,
    dates: (NaiveDate, NaiveDate),
    change: Decimal,
) -> Option<(NaiveDate, NaiveDate, Decimal, Decimal)> {
    let pair = |date| {
        index
            .closes
            .iter()
            .filter(|(day, _)| *day <= date)
            .max_by_key(|(day, _)| *day)
            .filter(|(day, close)| {
                trading_days_stale(date, *day) <= START_CLOSE_MAX_AGE_TRADING_DAYS
                    && *close > Decimal::ZERO
            })
    };
    let (start_date, start) = pair(dates.0)?;
    let (end_date, end) = pair(dates.1)?;
    if *end_date <= dates.0 {
        return None;
    }
    let reference = end
        .checked_sub(*start)?
        .checked_div(*start)?
        .checked_mul(Decimal::ONE_HUNDRED)?;
    let difference = change.checked_sub(reference)?;
    Some((*start_date, *end_date, reference, difference))
}

#[cfg(test)]
mod tests;
