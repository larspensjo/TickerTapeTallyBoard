//! Pure detection of positions an import would close while a Low/Medium/High
//! conviction is still stored.
//!
//! Conviction is user-managed metadata and must never be silently reset. When a
//! commit would drive a currently-open convicted position to zero, the import
//! flow blocks until the user chooses to keep the conviction or clear it to
//! `Other`. This module only decides *which* instruments are in that state; the
//! API layer computes the current/predicted quantities and applies the choice.

use std::collections::BTreeSet;

use crate::domain::{ConvictionLevel, LedgerTransaction, TransactionKind};

/// Net signed quantity of a ledger.
///
/// Buys, sells, and split deltas are additive on quantity, so the final
/// position is their sum regardless of order. Dividend rows carry a raw share
/// count that does not change the position and are excluded.
pub fn net_quantity(ledger: &[LedgerTransaction]) -> i64 {
    ledger
        .iter()
        .filter(|tx| tx.kind != TransactionKind::Dividend)
        .map(|tx| tx.quantity)
        .sum()
}

/// Predicted position after an append commit: the current position plus the
/// genuinely-new rows.
pub fn predicted_quantity_append(current: i64, new_contribution: i64) -> i64 {
    current + new_contribution
}

/// Predicted position after an Avanza refresh commit.
///
/// A non-excluded instrument has its old batch rows replaced, so its batch net
/// is removed before adding the new rows. An excluded instrument keeps its old
/// batch rows (deselected, fully-already-imported, or mixed), so refresh only
/// appends its genuinely-new rows — the batch net is not removed. In both cases
/// `new_contribution` is the sum of the genuinely-new rows for the instrument.
pub fn predicted_quantity_replace(
    current: i64,
    batch_net: i64,
    new_contribution: i64,
    excluded: bool,
) -> i64 {
    let removed = if excluded { 0 } else { batch_net };
    current - removed + new_contribution
}

/// One existing instrument, reduced to the inputs the guard needs.
#[derive(Clone, Debug)]
pub struct GuardInstrument {
    pub instrument_id: i64,
    pub asset_key: String,
    pub symbol: String,
    pub conviction: ConvictionLevel,
    /// Net position today, before the pending commit.
    pub current_quantity: i64,
    /// Net position the pending commit would leave.
    pub predicted_quantity: i64,
}

/// A currently-open convicted position the pending import would close.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosingConvictedPosition {
    pub instrument_id: i64,
    pub asset_key: String,
    pub symbol: String,
    pub conviction: ConvictionLevel,
}

/// Instruments whose open Low/Medium/High position the import drives to zero or
/// below. `Other` never blocks (no target, nothing to preserve), and an
/// already-closed position (current quantity not positive) never blocks.
pub fn closing_convicted_positions(
    instruments: &[GuardInstrument],
) -> Vec<ClosingConvictedPosition> {
    instruments
        .iter()
        .filter(|instrument| {
            instrument.conviction.weight().is_some()
                && instrument.current_quantity > 0
                && instrument.predicted_quantity <= 0
        })
        .map(|instrument| ClosingConvictedPosition {
            instrument_id: instrument.instrument_id,
            asset_key: instrument.asset_key.clone(),
            symbol: instrument.symbol.clone(),
            conviction: instrument.conviction,
        })
        .collect()
}

/// Asset keys of a closing set, used to validate the user's per-instrument
/// keep/clear choices at commit time.
pub fn closing_asset_keys(positions: &[ClosingConvictedPosition]) -> BTreeSet<String> {
    positions
        .iter()
        .map(|position| position.asset_key.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{closing_convicted_positions, net_quantity, GuardInstrument};
    use crate::domain::{ConvictionLevel, LedgerTransaction, TransactionKind};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    fn tx(id: i64, kind: TransactionKind, quantity: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            kind,
            quantity,
            price: Some(dec!(10)),
            dividend_per_share: None,
            fx_rate_to_base: Some(dec!(1)),
            brokerage_base: dec!(0),
        }
    }

    #[test]
    fn net_quantity_ignores_dividends() {
        let ledger = vec![
            tx(1, TransactionKind::Buy, 10),
            tx(2, TransactionKind::Dividend, 10),
            tx(3, TransactionKind::Sell, -4),
        ];
        assert_eq!(net_quantity(&ledger), 6);
    }

    fn guard(
        id: i64,
        conviction: ConvictionLevel,
        current: i64,
        predicted: i64,
    ) -> GuardInstrument {
        GuardInstrument {
            instrument_id: id,
            asset_key: format!("key{id}"),
            symbol: format!("SYM{id}"),
            conviction,
            current_quantity: current,
            predicted_quantity: predicted,
        }
    }

    #[test]
    fn flags_only_convicted_positions_going_to_zero() {
        let closing = closing_convicted_positions(&[
            // Convicted, open, driven to zero -> flagged.
            guard(1, ConvictionLevel::High, 10, 0),
            // Convicted, open, driven negative -> flagged.
            guard(2, ConvictionLevel::Low, 5, -1),
            // Convicted but stays open -> not flagged.
            guard(3, ConvictionLevel::Medium, 10, 4),
            // Other never blocks.
            guard(4, ConvictionLevel::Other, 10, 0),
            // Already closed (not positive now) never blocks.
            guard(5, ConvictionLevel::High, 0, 0),
        ]);

        let ids: Vec<i64> = closing.iter().map(|p| p.instrument_id).collect();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(closing[0].conviction, ConvictionLevel::High);
    }

    #[test]
    fn unchanged_position_is_not_flagged() {
        let closing = closing_convicted_positions(&[guard(1, ConvictionLevel::High, 10, 10)]);
        assert!(closing.is_empty());
    }

    #[test]
    fn append_prediction_adds_new_rows() {
        assert_eq!(super::predicted_quantity_append(5, -5), 0);
        assert_eq!(super::predicted_quantity_append(5, 3), 8);
    }

    #[test]
    fn replace_prediction_removes_batch_net_only_when_not_excluded() {
        // Non-excluded: old batch rows are replaced by the new content.
        // buy 5 in the batch, new content is buy 5 + sell 5 -> net 0.
        assert_eq!(super::predicted_quantity_replace(5, 5, 0, false), 0);

        // Mixed instrument is excluded (old rows kept), so refresh only appends
        // its genuinely-new rows: buy 5 today, refresh adds a new sell 5 -> 0.
        assert_eq!(super::predicted_quantity_replace(5, 5, -5, true), 0);

        // Deselected instrument: excluded, no new rows -> unchanged.
        assert_eq!(super::predicted_quantity_replace(5, 5, 0, true), 5);
    }
}
