use rust_decimal::Decimal;

use crate::domain::transaction::{LedgerError, LedgerTransaction, TransactionKind};

/// A derived open position for a single instrument.
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    pub quantity: i64,
    /// Sum native gross of the open shares, in the instrument's currency.
    pub cost_basis_native: Decimal,
    pub base: BaseCostBasis,
}

/// SEK cost-basis state.
#[derive(Clone, Debug, PartialEq)]
pub enum BaseCostBasis {
    Available {
        cost_basis_base: Decimal,
        fee_component_base: Decimal,
    },
    Unavailable {
        reasons: Vec<UnavailableReason>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    MissingFx { transaction_id: i64 },
}

impl Position {
    fn empty() -> Self {
        Self {
            quantity: 0,
            cost_basis_native: Decimal::ZERO,
            base: BaseCostBasis::Available {
                cost_basis_base: Decimal::ZERO,
                fee_component_base: Decimal::ZERO,
            },
        }
    }

    pub fn average_cost_native(&self) -> Option<Decimal> {
        (self.quantity > 0).then(|| self.cost_basis_native / Decimal::from(self.quantity))
    }

    pub fn average_cost_base(&self) -> Option<Decimal> {
        match &self.base {
            BaseCostBasis::Available {
                cost_basis_base, ..
            } if self.quantity > 0 => Some(*cost_basis_base / Decimal::from(self.quantity)),
            _ => None,
        }
    }
}

/// Derive a position by folding `(trade_date, id)`-ordered transactions.
///
/// Callers must pass transactions already sorted by `(trade_date, id)`.
pub fn derive_position(transactions: &[LedgerTransaction]) -> Result<Position, LedgerError> {
    debug_assert!(
        transactions
            .windows(2)
            .all(|pair| { (pair[0].trade_date, pair[0].id) <= (pair[1].trade_date, pair[1].id) }),
        "derive_position requires transactions sorted by (trade_date, id)"
    );

    let mut position = Position::empty();
    for transaction in transactions {
        apply(&mut position, transaction)?;
    }
    Ok(position)
}

fn apply(position: &mut Position, tx: &LedgerTransaction) -> Result<(), LedgerError> {
    match tx.kind {
        TransactionKind::Buy => apply_buy(position, tx),
        TransactionKind::Sell => apply_sell(position, tx),
        TransactionKind::Split => apply_split(position, tx),
        TransactionKind::Dividend => Ok(()),
    }
}

fn apply_buy(position: &mut Position, tx: &LedgerTransaction) -> Result<(), LedgerError> {
    let price = tx.price.ok_or(LedgerError::BuyMissingPrice {
        transaction_id: tx.id,
    })?;
    let native_gross = price * Decimal::from(tx.quantity);
    position.cost_basis_native += native_gross;

    let mut missing_fx = false;
    match tx.fx_rate_to_base {
        Some(fx) => {
            if let BaseCostBasis::Available {
                cost_basis_base,
                fee_component_base,
            } = &mut position.base
            {
                *cost_basis_base += native_gross * fx + tx.brokerage_base;
                *fee_component_base += tx.brokerage_base;
            }
        }
        None => {
            missing_fx = true;
            if matches!(position.base, BaseCostBasis::Available { .. }) {
                position.base = BaseCostBasis::Unavailable {
                    reasons: Vec::new(),
                };
            }
        }
    }

    if missing_fx {
        if let BaseCostBasis::Unavailable { reasons } = &mut position.base {
            reasons.push(UnavailableReason::MissingFx {
                transaction_id: tx.id,
            });
        }
    }

    position.quantity += tx.quantity;
    Ok(())
}

fn apply_sell(position: &mut Position, tx: &LedgerTransaction) -> Result<(), LedgerError> {
    let sell_qty = -tx.quantity;
    if sell_qty > position.quantity {
        return Err(LedgerError::SellExceedsPosition {
            transaction_id: tx.id,
            available: position.quantity,
            requested: sell_qty,
        });
    }

    let remaining = position.quantity - sell_qty;
    if remaining == 0 {
        *position = Position::empty();
        return Ok(());
    }

    let ratio = Decimal::from(remaining) / Decimal::from(position.quantity);
    position.cost_basis_native *= ratio;
    if let BaseCostBasis::Available {
        cost_basis_base,
        fee_component_base,
    } = &mut position.base
    {
        *cost_basis_base *= ratio;
        *fee_component_base *= ratio;
    }
    position.quantity = remaining;
    Ok(())
}

fn apply_split(position: &mut Position, tx: &LedgerTransaction) -> Result<(), LedgerError> {
    if position.quantity == 0 {
        return Err(LedgerError::SplitWithoutPosition {
            transaction_id: tx.id,
        });
    }

    let resulting = position.quantity + tx.quantity;
    if resulting <= 0 {
        return Err(LedgerError::SplitDrivesNonPositive {
            transaction_id: tx.id,
            resulting_quantity: resulting,
        });
    }

    position.quantity = resulting;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{derive_position, BaseCostBasis, UnavailableReason};
    use crate::domain::transaction::{LedgerError, LedgerTransaction, TransactionKind};
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    fn buy(
        id: i64,
        date: NaiveDate,
        qty: i64,
        price: Decimal,
        fx: Option<Decimal>,
    ) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Buy,
            quantity: qty,
            price: Some(price),
            fx_rate_to_base: fx,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn sell(id: i64, date: NaiveDate, qty: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Sell,
            quantity: -qty,
            price: Some(dec!(1)),
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn split(id: i64, date: NaiveDate, delta: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Split,
            quantity: delta,
            price: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    #[test]
    fn single_buy_sets_native_and_base_cost() {
        let mut tx = buy(1, d(2026, 6, 12), 10, dec!(12.50), Some(dec!(10.0)));
        tx.brokerage_base = dec!(9.60);

        let position = derive_position(&[tx]).expect("derives");

        assert_eq!(position.quantity, 10);
        assert_eq!(position.cost_basis_native, dec!(125.00));
        assert_eq!(position.average_cost_native(), Some(dec!(12.50)));
        match position.base {
            BaseCostBasis::Available {
                cost_basis_base,
                fee_component_base,
            } => {
                assert_eq!(cost_basis_base, dec!(1259.60));
                assert_eq!(fee_component_base, dec!(9.60));
            }
            BaseCostBasis::Unavailable { .. } => panic!("base should be available"),
        }
    }

    #[test]
    fn weighted_average_blends_two_buys() {
        let first = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));
        let second = buy(2, d(2026, 6, 2), 10, dec!(200), Some(dec!(1)));

        let position = derive_position(&[first, second]).expect("derives");

        assert_eq!(position.quantity, 20);
        assert_eq!(position.average_cost_native(), Some(dec!(150)));
    }

    #[test]
    fn sell_keeps_average_and_reduces_components() {
        let first = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(2)));
        let part = sell(2, d(2026, 6, 2), 4);

        let position = derive_position(&[first, part]).expect("derives");

        assert_eq!(position.quantity, 6);
        assert_eq!(position.average_cost_native(), Some(dec!(100)));
        assert_eq!(position.average_cost_base(), Some(dec!(200)));
    }

    #[test]
    fn same_day_buy_then_sell_orders_by_id() {
        let b = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));
        let s = sell(2, d(2026, 6, 1), 4);

        let position = derive_position(&[b, s]).expect("derives");
        assert_eq!(position.quantity, 6);
    }

    #[test]
    fn same_day_sell_before_buy_is_rejected() {
        let s = sell(1, d(2026, 6, 1), 4);
        let b = buy(2, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));

        let error = derive_position(&[s, b]).expect_err("sell before any buy fails");
        assert!(matches!(error, LedgerError::SellExceedsPosition { .. }));
    }

    #[test]
    fn sell_below_zero_is_rejected() {
        let b = buy(1, d(2026, 6, 1), 3, dec!(100), Some(dec!(1)));
        let s = sell(2, d(2026, 6, 2), 4);

        let error = derive_position(&[b, s]).expect_err("oversell fails");
        assert_eq!(
            error,
            LedgerError::SellExceedsPosition {
                transaction_id: 2,
                available: 3,
                requested: 4,
            }
        );
    }

    #[test]
    fn missing_fx_makes_base_unavailable_but_native_stays() {
        let known = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));
        let unknown = buy(2, d(2026, 6, 2), 10, dec!(200), None);

        let position = derive_position(&[known, unknown]).expect("derives");

        assert_eq!(position.quantity, 20);
        assert_eq!(position.average_cost_native(), Some(dec!(150)));
        assert_eq!(position.average_cost_base(), None);
        match position.base {
            BaseCostBasis::Unavailable { reasons } => {
                assert_eq!(
                    reasons,
                    vec![UnavailableReason::MissingFx { transaction_id: 2 }]
                );
            }
            BaseCostBasis::Available { .. } => panic!("base should be unavailable"),
        }
    }

    #[test]
    fn closing_and_reopening_recovers_base_availability() {
        let contaminated = buy(1, d(2026, 6, 1), 10, dec!(100), None);
        let close = sell(2, d(2026, 6, 2), 10);
        let reopen = buy(3, d(2026, 6, 3), 5, dec!(100), Some(dec!(2)));

        let position = derive_position(&[contaminated, close, reopen]).expect("derives");

        assert_eq!(position.quantity, 5);
        assert_eq!(position.average_cost_base(), Some(dec!(200)));
    }

    #[test]
    fn split_rescales_average_without_changing_cost_basis() {
        let b = buy(1, d(2026, 6, 1), 10, dec!(120), Some(dec!(1)));
        let s = split(2, d(2026, 6, 2), 10);

        let position = derive_position(&[b, s]).expect("derives");

        assert_eq!(position.quantity, 20);
        assert_eq!(position.cost_basis_native, dec!(1200));
        assert_eq!(position.average_cost_native(), Some(dec!(60)));
    }

    #[test]
    fn split_without_position_is_rejected() {
        let s = split(1, d(2026, 6, 1), 10);
        let error = derive_position(&[s]).expect_err("split needs a position");
        assert_eq!(
            error,
            LedgerError::SplitWithoutPosition { transaction_id: 1 }
        );
    }

    #[test]
    fn split_driving_non_positive_is_rejected() {
        let b = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));
        let s = split(2, d(2026, 6, 2), -10);
        let error = derive_position(&[b, s]).expect_err("split to zero fails");
        assert_eq!(
            error,
            LedgerError::SplitDrivesNonPositive {
                transaction_id: 2,
                resulting_quantity: 0,
            }
        );
    }

    #[test]
    fn dividend_row_is_a_position_no_op() {
        let b = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)));
        let dividend = LedgerTransaction {
            id: 2,
            trade_date: d(2026, 6, 2),
            kind: TransactionKind::Dividend,
            quantity: 0,
            price: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        };

        let position = derive_position(&[b, dividend]).expect("derives");
        assert_eq!(position.quantity, 10);
    }

    #[test]
    fn average_base_handles_repeating_decimal() {
        let mut b = buy(1, d(2026, 6, 1), 3, dec!(1), Some(dec!(1)));
        b.brokerage_base = dec!(7);

        let position = derive_position(&[b]).expect("derives");
        let avg = position.average_cost_base().expect("available");

        assert_eq!(avg.round_dp(2), dec!(3.33));
        assert!(avg > dec!(3.33));
        assert!(avg < dec!(3.34));
    }
}
