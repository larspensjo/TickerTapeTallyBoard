//! Pure ledger domain: transaction kinds, validation, and position derivation.
//! Contains no axum, sqlx, HTTP, or provider types and performs no I/O.

#![allow(dead_code)]

mod position;
mod transaction;

#[allow(unused_imports)]
pub use position::{derive_position, BaseCostBasis, Position, UnavailableReason};
#[allow(unused_imports)]
pub use transaction::{
    validate, LedgerError, LedgerTransaction, ProposedTransaction, TransactionKind, ValidationError,
};
