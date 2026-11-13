//! Pure opportunity evidence. Callers supply dates and stored data; nothing here loads or logs it.
//! Evidence is requested only for rows with an available, negative recent price change.

pub mod business_check;
pub mod dividend_check;
pub mod lookback_series;
pub mod market_reference;
pub mod parameters;
pub mod price_checks;
pub mod replay_rules;
pub mod report_timing;
pub mod room_to_buy;
pub mod verdict;

use chrono::NaiveDate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceReason {
    MissingLatestClose,
    PriceHistoryGap { start: NaiveDate },
    SplitInLookback { date: NaiveDate },
    PriceHistoryTooShort { closes: usize, required: usize },
    InsufficientHistoricalMoves { moves: usize },
    NoPriceVariation,
    NonPositiveClose { date: NaiveDate },
    MissingWindowCloses,
    InsufficientMarketRows { count: usize, required: usize },
    MarketIndexUnavailable { display_name: String },
    CalculationOutOfRange,
}

impl EvidenceReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MissingLatestClose => "missing_latest_close",
            Self::PriceHistoryGap { .. } => "price_history_gap",
            Self::SplitInLookback { .. } => "split_in_lookback",
            Self::PriceHistoryTooShort { .. } => "price_history_too_short",
            Self::InsufficientHistoricalMoves { .. } => "insufficient_historical_moves",
            Self::NoPriceVariation => "no_price_variation",
            Self::NonPositiveClose { .. } => "non_positive_close",
            Self::MissingWindowCloses => "missing_window_closes",
            Self::InsufficientMarketRows { .. } => "insufficient_market_rows",
            Self::MarketIndexUnavailable { .. } => "market_index_unavailable",
            Self::CalculationOutOfRange => "calculation_out_of_range",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YesNoUnknown {
    Yes,
    No,
    Unknown,
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
