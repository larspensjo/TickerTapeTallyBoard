//! Pure ledger domain: transaction kinds, validation, and position derivation.
//! Contains no axum, sqlx, HTTP, or provider types and performs no I/O.

#![allow(dead_code)]

mod attribution;
mod availability;
mod cash_flow;
mod conviction;
mod day_change;
mod holding_period_return;
mod market_snapshot;
mod money_weighted_return;
mod performance;
mod period_amounts;
mod position;
mod price_history;
mod price_resolution;
mod rebalance;
mod transaction;
mod valuation;
mod valuation_summary;
mod value_history;

#[allow(unused_imports)]
pub use conviction::{
    derive_targets, gap_band_status, pool_membership, ConvictionLevel, ConvictionTargetInput,
    ConvictionTargetOutput, MarketValueState, TargetField, TargetReason, TargetStatus,
};

pub use cash_flow::{actual_period_cash_flows, period_cash_flows, CashFlow};
pub use money_weighted_return::{compute_money_weighted_return, MoneyWeightedReturn};
#[allow(unused_imports)]
pub use performance::{
    apply_annualisation, compute_modified_dietz, compute_modified_dietz_denominator,
    compute_period_amounts, reconstruct_period, DisplayPercentKind, PeriodAmounts, PeriodLedger,
};
#[allow(unused_imports)]
pub use position::{
    derive_position, derive_position_performance, BaseAmount, BaseCostBasis, Position,
    PositionPerformance, RealizedGain, UnavailableReason,
};
pub use price_resolution::{
    pick_latest_on_or_before, pick_previous_before, resolve_price_series, ProviderCode,
};
#[allow(unused_imports)]
pub use rebalance::{
    build_ladder, CandidateBalance, PlannedTrade, RankBy, RebalanceCandidate, RebalanceLadder,
    RebalanceRung, RebalanceUnavailable, TradeSide, UntradedCandidate, UntradedReason,
};
#[allow(unused_imports)]
pub use transaction::{
    validate, LedgerError, LedgerTransaction, ProposedTransaction, TransactionKind, ValidationError,
};
#[allow(unused_imports)]
pub use valuation::{
    build_price_history, summarize_holdings, value_position, Availability, DataFreshness,
    FxApplied, FxCandidate, FxSnapshot, PriceCandidate, PricePoint, PriceSnapshot, ValuationReason,
    ValuationSummary, ValuedHolding,
};
pub use value_history::{build_value_history, ValueHistoryInstrument, ValueHistoryPoint};
