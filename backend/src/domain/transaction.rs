use chrono::NaiveDate;
use rust_decimal::Decimal;

/// The kind of a ledger transaction. PascalCase on the wire, UPPERCASE in the DB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionKind {
    Buy,
    Sell,
    Split,
    Dividend,
}

impl TransactionKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
            Self::Split => "SPLIT",
            Self::Dividend => "DIVIDEND",
        }
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "BUY" => Some(Self::Buy),
            "SELL" => Some(Self::Sell),
            "SPLIT" => Some(Self::Split),
            "DIVIDEND" => Some(Self::Dividend),
            _ => None,
        }
    }
}

/// A client-proposed transaction before stateless validation.
///
/// `quantity` is the positive magnitude for Buy/Sell and a signed non-zero
/// delta for Split.
#[derive(Clone, Debug, PartialEq)]
pub struct ProposedTransaction {
    pub kind: TransactionKind,
    pub trade_date: NaiveDate,
    pub quantity: i64,
    pub price: Option<Decimal>,
    pub currency: Option<String>,
    pub fx_rate_to_base: Option<Decimal>,
    pub brokerage_base: Option<Decimal>,
}

/// A validated ledger row used as pure input to derivation.
///
/// `quantity` is the signed position effect (Buy > 0, Sell < 0, Split = signed
/// delta).
#[derive(Clone, Debug, PartialEq)]
pub struct LedgerTransaction {
    pub id: i64,
    pub trade_date: NaiveDate,
    pub kind: TransactionKind,
    pub quantity: i64,
    pub price: Option<Decimal>,
    pub fx_rate_to_base: Option<Decimal>,
    pub brokerage_base: Decimal,
}

/// Stateless, field-level validation errors. Each maps to a stable API code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    QuantityMustBePositive,
    PriceRequired,
    PriceMustBePositive,
    CurrencyRequired,
    FxRateMustBePositive,
    BrokerageMustNotBeNegative,
    SplitQuantityMustBeNonZero,
    SplitMustNotCarryCostInputs,
    DividendMustNotCarryBrokerage,
}

impl ValidationError {
    pub fn code(self) -> &'static str {
        match self {
            Self::QuantityMustBePositive => "quantity_must_be_positive",
            Self::PriceRequired => "price_required",
            Self::PriceMustBePositive => "price_must_be_positive",
            Self::CurrencyRequired => "currency_required",
            Self::FxRateMustBePositive => "fx_rate_must_be_positive",
            Self::BrokerageMustNotBeNegative => "brokerage_must_not_be_negative",
            Self::SplitQuantityMustBeNonZero => "split_quantity_must_be_non_zero",
            Self::SplitMustNotCarryCostInputs => "split_must_not_carry_cost_inputs",
            Self::DividendMustNotCarryBrokerage => "dividend_must_not_carry_brokerage",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::QuantityMustBePositive => "Buy and Sell quantity must be a positive integer.",
            Self::PriceRequired => "Buy and Sell require a native price.",
            Self::PriceMustBePositive => "Buy and Sell price must be greater than zero.",
            Self::CurrencyRequired => "Buy and Sell require a native currency.",
            Self::FxRateMustBePositive => "FX rate to base must be greater than zero when present.",
            Self::BrokerageMustNotBeNegative => "Brokerage must not be negative when present.",
            Self::SplitQuantityMustBeNonZero => "Split requires a non-zero quantity delta.",
            Self::SplitMustNotCarryCostInputs => {
                "Split must not carry price, currency, FX, or brokerage."
            }
            Self::DividendMustNotCarryBrokerage => "Dividend must not carry brokerage.",
        }
    }
}

/// Stateful derivation errors (depend on prior ledger state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerError {
    SellExceedsPosition {
        transaction_id: i64,
        available: i64,
        requested: i64,
    },
    SplitWithoutPosition {
        transaction_id: i64,
    },
    SplitDrivesNonPositive {
        transaction_id: i64,
        resulting_quantity: i64,
    },
    BuyMissingPrice {
        transaction_id: i64,
    },
    SellMissingPrice {
        transaction_id: i64,
    },
}

impl LedgerError {
    pub fn code(self) -> &'static str {
        match self {
            Self::SellExceedsPosition { .. } => "sell_exceeds_position",
            Self::SplitWithoutPosition { .. } => "split_without_position",
            Self::SplitDrivesNonPositive { .. } => "split_drives_non_positive",
            Self::BuyMissingPrice { .. } => "buy_missing_price",
            Self::SellMissingPrice { .. } => "sell_missing_price",
        }
    }

    pub fn transaction_id(self) -> i64 {
        match self {
            Self::SellExceedsPosition { transaction_id, .. }
            | Self::SplitWithoutPosition { transaction_id }
            | Self::SplitDrivesNonPositive { transaction_id, .. }
            | Self::BuyMissingPrice { transaction_id }
            | Self::SellMissingPrice { transaction_id } => transaction_id,
        }
    }
}

/// Validate a proposed transaction's type-specific field rules.
///
/// On success returns the signed position effect for `quantity`.
pub fn validate(proposed: &ProposedTransaction) -> Result<i64, ValidationError> {
    match proposed.kind {
        TransactionKind::Buy | TransactionKind::Sell => {
            if proposed.quantity <= 0 {
                return Err(ValidationError::QuantityMustBePositive);
            }
            match proposed.price {
                None => return Err(ValidationError::PriceRequired),
                Some(price) if price <= Decimal::ZERO => {
                    return Err(ValidationError::PriceMustBePositive);
                }
                Some(_) => {}
            }

            if proposed
                .currency
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            {
                return Err(ValidationError::CurrencyRequired);
            }

            if proposed
                .fx_rate_to_base
                .is_some_and(|fx| fx <= Decimal::ZERO)
            {
                return Err(ValidationError::FxRateMustBePositive);
            }

            if proposed
                .brokerage_base
                .is_some_and(|brokerage| brokerage < Decimal::ZERO)
            {
                return Err(ValidationError::BrokerageMustNotBeNegative);
            }

            Ok(if proposed.kind == TransactionKind::Sell {
                -proposed.quantity
            } else {
                proposed.quantity
            })
        }
        TransactionKind::Split => {
            if proposed.quantity == 0 {
                return Err(ValidationError::SplitQuantityMustBeNonZero);
            }
            if proposed.price.is_some()
                || proposed.currency.is_some()
                || proposed.fx_rate_to_base.is_some()
                || proposed.brokerage_base.is_some()
            {
                return Err(ValidationError::SplitMustNotCarryCostInputs);
            }
            Ok(proposed.quantity)
        }
        TransactionKind::Dividend => {
            if proposed.quantity <= 0 {
                return Err(ValidationError::QuantityMustBePositive);
            }
            match proposed.price {
                None => return Err(ValidationError::PriceRequired),
                Some(price) if price <= Decimal::ZERO => {
                    return Err(ValidationError::PriceMustBePositive);
                }
                Some(_) => {}
            }
            if proposed
                .currency
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            {
                return Err(ValidationError::CurrencyRequired);
            }
            if proposed
                .fx_rate_to_base
                .is_some_and(|fx| fx <= Decimal::ZERO)
            {
                return Err(ValidationError::FxRateMustBePositive);
            }
            if proposed.brokerage_base.is_some_and(|b| b != Decimal::ZERO) {
                return Err(ValidationError::DividendMustNotCarryBrokerage);
            }
            Ok(0) // dividend has no position quantity effect
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{validate, ProposedTransaction, TransactionKind, ValidationError};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 12).expect("valid date")
    }

    fn buy() -> ProposedTransaction {
        ProposedTransaction {
            kind: TransactionKind::Buy,
            trade_date: date(),
            quantity: 10,
            price: Some(dec!(12.50)),
            currency: Some("USD".to_owned()),
            fx_rate_to_base: Some(dec!(10.0)),
            brokerage_base: Some(dec!(9.60)),
        }
    }

    #[test]
    fn buy_returns_positive_signed_quantity() {
        assert_eq!(validate(&buy()), Ok(10));
    }

    #[test]
    fn sell_returns_negative_signed_quantity() {
        let sell = ProposedTransaction {
            kind: TransactionKind::Sell,
            ..buy()
        };
        assert_eq!(validate(&sell), Ok(-10));
    }

    #[test]
    fn buy_without_price_is_rejected() {
        let proposed = ProposedTransaction {
            price: None,
            ..buy()
        };
        assert_eq!(validate(&proposed), Err(ValidationError::PriceRequired));
    }

    #[test]
    fn buy_without_currency_is_rejected() {
        let proposed = ProposedTransaction {
            currency: None,
            ..buy()
        };
        assert_eq!(validate(&proposed), Err(ValidationError::CurrencyRequired));
    }

    #[test]
    fn buy_with_non_positive_quantity_is_rejected() {
        let proposed = ProposedTransaction {
            quantity: 0,
            ..buy()
        };
        assert_eq!(
            validate(&proposed),
            Err(ValidationError::QuantityMustBePositive)
        );
    }

    #[test]
    fn buy_with_non_positive_price_is_rejected() {
        let zero = ProposedTransaction {
            price: Some(dec!(0)),
            ..buy()
        };
        assert_eq!(validate(&zero), Err(ValidationError::PriceMustBePositive));

        let negative = ProposedTransaction {
            price: Some(dec!(-1)),
            ..buy()
        };
        assert_eq!(
            validate(&negative),
            Err(ValidationError::PriceMustBePositive)
        );
    }

    #[test]
    fn buy_with_non_positive_fx_is_rejected() {
        let proposed = ProposedTransaction {
            fx_rate_to_base: Some(dec!(0)),
            ..buy()
        };
        assert_eq!(
            validate(&proposed),
            Err(ValidationError::FxRateMustBePositive)
        );
    }

    #[test]
    fn buy_with_negative_brokerage_is_rejected() {
        let proposed = ProposedTransaction {
            brokerage_base: Some(dec!(-0.01)),
            ..buy()
        };
        assert_eq!(
            validate(&proposed),
            Err(ValidationError::BrokerageMustNotBeNegative)
        );
    }

    #[test]
    fn split_returns_signed_delta() {
        let split = ProposedTransaction {
            kind: TransactionKind::Split,
            quantity: 8,
            price: None,
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
            ..buy()
        };
        assert_eq!(validate(&split), Ok(8));
    }

    #[test]
    fn split_with_zero_quantity_is_rejected() {
        let split = ProposedTransaction {
            kind: TransactionKind::Split,
            quantity: 0,
            price: None,
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
            ..buy()
        };
        assert_eq!(
            validate(&split),
            Err(ValidationError::SplitQuantityMustBeNonZero)
        );
    }

    #[test]
    fn split_carrying_cost_inputs_is_rejected() {
        let split = ProposedTransaction {
            kind: TransactionKind::Split,
            quantity: 8,
            price: Some(dec!(1.0)),
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
            ..buy()
        };
        assert_eq!(
            validate(&split),
            Err(ValidationError::SplitMustNotCarryCostInputs)
        );
    }

    #[test]
    fn dividend_with_valid_fields_succeeds() {
        let d = ProposedTransaction {
            kind: TransactionKind::Dividend,
            trade_date: date(),
            quantity: 10,
            price: Some(dec!(0.50)),
            currency: Some("USD".to_owned()),
            fx_rate_to_base: Some(dec!(10.5)),
            brokerage_base: None,
        };
        assert_eq!(validate(&d), Ok(0));
    }

    #[test]
    fn dividend_without_quantity_is_rejected() {
        let d = ProposedTransaction {
            kind: TransactionKind::Dividend,
            quantity: 0,
            price: Some(dec!(0.50)),
            currency: Some("USD".to_owned()),
            fx_rate_to_base: None,
            brokerage_base: None,
            trade_date: date(),
        };
        assert_eq!(validate(&d), Err(ValidationError::QuantityMustBePositive));
    }

    #[test]
    fn dividend_without_price_is_rejected() {
        let d = ProposedTransaction {
            kind: TransactionKind::Dividend,
            quantity: 10,
            price: None,
            currency: Some("USD".to_owned()),
            fx_rate_to_base: None,
            brokerage_base: None,
            trade_date: date(),
        };
        assert_eq!(validate(&d), Err(ValidationError::PriceRequired));
    }

    #[test]
    fn dividend_without_currency_is_rejected() {
        let d = ProposedTransaction {
            kind: TransactionKind::Dividend,
            quantity: 10,
            price: Some(dec!(0.50)),
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
            trade_date: date(),
        };
        assert_eq!(validate(&d), Err(ValidationError::CurrencyRequired));
    }

    #[test]
    fn dividend_with_brokerage_is_rejected() {
        let d = ProposedTransaction {
            kind: TransactionKind::Dividend,
            quantity: 10,
            price: Some(dec!(0.50)),
            currency: Some("USD".to_owned()),
            fx_rate_to_base: None,
            brokerage_base: Some(dec!(5.00)),
            trade_date: date(),
        };
        assert_eq!(
            validate(&d),
            Err(ValidationError::DividendMustNotCarryBrokerage)
        );
    }

    #[test]
    fn db_string_round_trips() {
        for kind in [
            TransactionKind::Buy,
            TransactionKind::Sell,
            TransactionKind::Split,
            TransactionKind::Dividend,
        ] {
            assert_eq!(TransactionKind::from_db_str(kind.as_db_str()), Some(kind));
        }
        assert_eq!(TransactionKind::from_db_str("GIFT"), None);
    }
}
