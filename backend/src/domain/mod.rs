//! Pure ledger domain: transaction kinds, validation, and position derivation.
//! Contains no axum, sqlx, HTTP, or provider types and performs no I/O.

#![allow(dead_code)]

mod conviction;
mod performance;
mod position;
mod rebalance;
mod transaction;
mod valuation;

#[allow(unused_imports)]
pub use conviction::{
    derive_targets, gap_band_status, pool_membership, ConvictionLevel, ConvictionTargetInput,
    ConvictionTargetOutput, MarketValueState, TargetField, TargetReason, TargetStatus,
};

#[allow(unused_imports)]
pub use performance::{
    actual_period_cash_flows, apply_annualisation, compute_modified_dietz,
    compute_modified_dietz_denominator, compute_money_weighted_return, compute_period_amounts,
    period_cash_flows, reconstruct_period, CashFlow, DisplayPercentKind, MoneyWeightedReturn,
    PeriodAmounts, PeriodLedger,
};
#[allow(unused_imports)]
pub use position::{
    derive_position, derive_position_performance, BaseAmount, BaseCostBasis, Position,
    PositionPerformance, RealizedGain, UnavailableReason,
};
#[allow(unused_imports)]
pub use rebalance::{
    build_ladder, CandidateBalance, PlannedTrade, RebalanceCandidate, RebalanceLadder,
    RebalanceRung, RebalanceUnavailable, TradeSide, UntradedCandidate, UntradedReason,
};
#[allow(unused_imports)]
pub use transaction::{
    validate, LedgerError, LedgerTransaction, ProposedTransaction, TransactionKind, ValidationError,
};
#[allow(unused_imports)]
pub use valuation::{
    build_price_history, build_value_history, summarize_holdings, value_position, Availability,
    DataFreshness, FxApplied, FxCandidate, FxSnapshot, PriceCandidate, PricePoint, PriceSnapshot,
    ValuationReason, ValuationSummary, ValueHistoryInstrument, ValueHistoryPoint, ValuedHolding,
};
