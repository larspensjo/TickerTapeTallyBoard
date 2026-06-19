//! Pure ledger domain: transaction kinds, validation, and position derivation.
//! Contains no axum, sqlx, HTTP, or provider types and performs no I/O.

#![allow(dead_code)]

mod performance;
mod position;
mod transaction;
mod valuation;

#[allow(unused_imports)]
pub use performance::{reconstruct_period, PeriodLedger};
#[allow(unused_imports)]
pub use position::{
    derive_position, derive_position_performance, BaseAmount, BaseCostBasis, Position,
    PositionPerformance, RealizedGain, UnavailableReason,
};
#[allow(unused_imports)]
pub use transaction::{
    validate, LedgerError, LedgerTransaction, ProposedTransaction, TransactionKind, ValidationError,
};
#[allow(unused_imports)]
pub use valuation::{
    summarize_holdings, value_position, Availability, DataFreshness, FxCandidate, FxSnapshot,
    PriceCandidate, PriceSnapshot, ValuationReason, ValuationSummary, ValuedHolding,
};
