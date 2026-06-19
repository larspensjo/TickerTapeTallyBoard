use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::position::{derive_position, Position};
use super::transaction::{LedgerError, LedgerTransaction, TransactionKind};

pub struct PeriodLedger {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub start_position: Position,
    pub period_transactions: Vec<LedgerTransaction>,
    pub end_position: Position,
    pub in_period_split_factor: Decimal,
    pub post_period_split_factor: Decimal,
}

pub fn reconstruct_period(
    all_transactions: &[LedgerTransaction],
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<PeriodLedger, LedgerError> {
    let pre: Vec<_> = all_transactions
        .iter()
        .filter(|t| t.trade_date < start_date)
        .cloned()
        .collect();
    let period: Vec<_> = all_transactions
        .iter()
        .filter(|t| t.trade_date >= start_date && t.trade_date <= end_date)
        .cloned()
        .collect();
    let through_end: Vec<_> = all_transactions
        .iter()
        .filter(|t| t.trade_date <= end_date)
        .cloned()
        .collect();
    let post: Vec<_> = all_transactions
        .iter()
        .filter(|t| t.trade_date > end_date)
        .cloned()
        .collect();

    let start_position = derive_position(&pre)?;
    let end_position = derive_position(&through_end)?;
    // Pass opening quantity so split_factor sees the pre-existing shares.
    let in_period_split_factor = split_factor(&period, start_position.quantity)?;
    let post_period_split_factor = split_factor(&post, end_position.quantity)?;

    Ok(PeriodLedger {
        start_date,
        end_date,
        start_position,
        period_transactions: period,
        end_position,
        in_period_split_factor,
        post_period_split_factor,
    })
}

/// Computes the cumulative split factor for a slice of transactions,
/// given the quantity already held before the first transaction in the slice.
/// For each Split with delta d when running quantity is q: factor *= (q + d) / q.
fn split_factor(
    transactions: &[LedgerTransaction],
    opening_qty: i64,
) -> Result<Decimal, LedgerError> {
    let mut factor = Decimal::ONE;
    let mut running_qty: i64 = opening_qty;
    for tx in transactions {
        match tx.kind {
            TransactionKind::Buy => running_qty += tx.quantity,
            TransactionKind::Sell => running_qty += tx.quantity, // tx.quantity is negative for sells
            TransactionKind::Split => {
                if running_qty > 0 {
                    let after = running_qty + tx.quantity;
                    factor *= Decimal::from(after) / Decimal::from(running_qty);
                    running_qty = after;
                }
            }
            TransactionKind::Dividend => {}
        }
    }
    Ok(factor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn buy(id: i64, d: &str, qty: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date(d),
            kind: TransactionKind::Buy,
            quantity: qty,
            price: Some(dec!(10)),
            fx_rate_to_base: Some(dec!(10)),
            brokerage_base: Decimal::ZERO,
        }
    }

    fn split_tx(id: i64, d: &str, delta: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date(d),
            kind: TransactionKind::Split,
            quantity: delta,
            price: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    #[test]
    fn buy_before_start_is_in_start_position() {
        let txs = vec![buy(1, "2026-01-01", 100)];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.start_position.quantity, 100);
        assert_eq!(p.period_transactions.len(), 0);
        assert_eq!(p.end_position.quantity, 100);
    }

    #[test]
    fn buy_on_start_date_is_period_flow_not_start_position() {
        let txs = vec![buy(1, "2026-06-01", 100)];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.start_position.quantity, 0);
        assert_eq!(p.period_transactions.len(), 1);
        assert_eq!(p.end_position.quantity, 100);
    }

    #[test]
    fn buy_after_end_excluded_from_end_position() {
        let txs = vec![buy(1, "2026-07-01", 100)];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.start_position.quantity, 0);
        assert_eq!(p.period_transactions.len(), 0);
        assert_eq!(p.end_position.quantity, 0);
    }

    #[test]
    fn in_period_split_factor_for_2_to_1_split() {
        // 100 shares held before start, 2:1 split mid-period:
        // factor = (100 + 100) / 100 = 2
        let txs = vec![buy(1, "2026-01-01", 100), split_tx(2, "2026-06-15", 100)];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.start_position.quantity, 100);
        assert_eq!(p.end_position.quantity, 200);
        assert_eq!(p.in_period_split_factor, dec!(2));
        assert_eq!(p.post_period_split_factor, dec!(1));
    }

    #[test]
    fn post_period_split_factor_for_split_after_end() {
        // 100 shares held, 2:1 split after end_date:
        // post factor = (100 + 100) / 100 = 2
        let txs = vec![buy(1, "2026-01-01", 100), split_tx(2, "2026-08-01", 100)];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.end_position.quantity, 100);
        assert_eq!(p.in_period_split_factor, dec!(1));
        assert_eq!(p.post_period_split_factor, dec!(2));
    }

    #[test]
    fn in_period_split_with_zero_start_position_is_factor_one() {
        // No pre-period shares; in-period buy then split.
        // split_factor(&period, opening_qty=0): split at running_qty=100 → factor = 200/100 = 2.
        // But in_period_split_factor is called with opening_qty = start_position.quantity = 0.
        // The split happens after the buy, so running_qty at split = 100.
        let txs = vec![buy(1, "2026-06-05", 100), split_tx(2, "2026-06-15", 100)];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.in_period_split_factor, dec!(2));
    }
}
