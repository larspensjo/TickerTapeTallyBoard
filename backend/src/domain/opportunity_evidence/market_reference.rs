use rust_decimal::Decimal;

use super::{parameters::*, EvidenceReason};
use crate::domain::RecentWindow;

/// Source metadata is separate from the outcome, so adding an index source later
/// leaves consumers of the comparison state unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarketReferenceSource {
    OtherHoldingsMedian,
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
    outcome.state = if difference < -outcome.margin_percent {
        MarketState::FellMoreThanMarket
    } else if difference > outcome.margin_percent {
        MarketState::FellLessThanMarket
    } else {
        MarketState::MovedWithMarket
    };
    outcome
}
