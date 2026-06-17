use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::domain::{ProposedTransaction, TransactionKind};
use crate::import::core::outcome::{InstrumentKey, MappedRow};
use crate::import::sharesight::parser::{ParsedKind, ParsedRow};

/// A mapping-stage failure (parse-level errors are handled by the parser).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapError {
    pub row: usize,
    pub code: &'static str,
    pub message: String,
}

pub fn map_row(row: &ParsedRow) -> Result<MappedRow, MapError> {
    let instrument = InstrumentKey {
        exchange: row.market.trim().to_string(),
        symbol: row.code.trim().to_string(),
        name: row.name.trim().to_string(),
        currency: row.instrument_currency.trim().to_string(),
        isin: None,
    };

    let kind = match row.kind {
        ParsedKind::Buy => TransactionKind::Buy,
        ParsedKind::Sell => TransactionKind::Sell,
        ParsedKind::Split => TransactionKind::Split,
    };

    let proposed = match row.kind {
        ParsedKind::Buy | ParsedKind::Sell => {
            let magnitude = integral_magnitude(row)?;
            let fx_rate_to_base = invert_fx(row.exchange_rate);
            let fx_warning = fx_rate_to_base.is_none();
            let brokerage_base = sek_brokerage(row)?;
            return Ok(MappedRow {
                source_row_number: row.source_row_number,
                instrument,
                proposed: ProposedTransaction {
                    kind,
                    trade_date: row.trade_date,
                    quantity: magnitude,
                    price: Some(row.price),
                    currency: Some(row.instrument_currency.trim().to_string()),
                    fx_rate_to_base,
                    brokerage_base,
                },
                source_value: Some(row.value),
                source_currency: Some("SEK".to_string()),
                note: non_empty_comment(row),
                fx_warning,
            });
        }
        ParsedKind::Split => ProposedTransaction {
            kind,
            trade_date: row.trade_date,
            quantity: integral_signed(row)?,
            price: None,
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
        },
    };

    Ok(MappedRow {
        source_row_number: row.source_row_number,
        instrument,
        proposed,
        source_value: Some(row.value),
        source_currency: None,
        note: non_empty_comment(row),
        fx_warning: false,
    })
}

/// Quantity must be integral; the magnitude (absolute value) is passed to the
/// domain validator for Buy/Sell, which re-signs it from the row's kind.
fn integral_magnitude(row: &ParsedRow) -> Result<i64, MapError> {
    Ok(integral_signed(row)?.abs())
}

/// Quantity must be integral; returns the signed value. Splits keep the sign so
/// a reverse-split (negative delta) is preserved; Buy/Sell take `.abs()` of this.
fn integral_signed(row: &ParsedRow) -> Result<i64, MapError> {
    if !row.quantity.fract().is_zero() {
        return Err(MapError {
            row: row.source_row_number,
            code: "non_integer_quantity",
            message: format!("quantity {} is not an integer", row.quantity),
        });
    }
    row.quantity.to_i64().ok_or(MapError {
        row: row.source_row_number,
        code: "non_integer_quantity",
        message: format!("quantity {} does not fit in i64", row.quantity),
    })
}

/// `fx_rate_to_base = 1 / Exchange Rate`, only for a present positive rate.
fn invert_fx(exchange_rate: Option<Decimal>) -> Option<Decimal> {
    match exchange_rate {
        Some(rate) if rate > Decimal::ZERO => Some(Decimal::ONE / rate),
        _ => None,
    }
}

/// SEK brokerage as an optional fee; non-zero non-SEK brokerage is a hard error.
fn sek_brokerage(row: &ParsedRow) -> Result<Option<Decimal>, MapError> {
    if row.brokerage.is_zero() {
        return Ok(None);
    }
    if !row.brokerage_currency.trim().eq_ignore_ascii_case("SEK") {
        return Err(MapError {
            row: row.source_row_number,
            code: "non_sek_brokerage",
            message: format!(
                "non-zero brokerage in {} is not SEK",
                row.brokerage_currency.trim()
            ),
        });
    }
    Ok(Some(row.brokerage))
}

/// The trimmed `Comments` cell as an optional note.
fn non_empty_comment(row: &ParsedRow) -> Option<String> {
    let trimmed = row.comments.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{map_row, MapError};
    use crate::domain::TransactionKind;
    use crate::import::sharesight::parser::{ParsedKind, ParsedRow};
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn row(kind: ParsedKind) -> ParsedRow {
        ParsedRow {
            source_row_number: 1,
            market: "NASDAQ".into(),
            code: "MSFT".into(),
            name: "Microsoft".into(),
            kind,
            trade_date: NaiveDate::from_ymd_opt(2026, 6, 12).unwrap(),
            quantity: dec!(10),
            price: dec!(12.50),
            instrument_currency: "USD".into(),
            cost_base_per_share_sek: dec!(0),
            brokerage: dec!(9.60),
            brokerage_currency: "SEK".into(),
            exchange_rate: Some(dec!(0.100000)),
            value: dec!(1259.60),
            source_column: "All Trades".into(),
            comments: String::new(),
        }
    }

    #[test]
    fn buy_inverts_fx_keeps_sek_brokerage_and_source_value() {
        let mapped = map_row(&row(ParsedKind::Buy)).expect("maps");
        assert_eq!(mapped.instrument.exchange, "NASDAQ");
        assert_eq!(mapped.instrument.symbol, "MSFT");
        assert_eq!(mapped.instrument.currency, "USD");
        assert_eq!(mapped.proposed.kind, TransactionKind::Buy);
        assert_eq!(mapped.proposed.quantity, 10);
        assert_eq!(mapped.proposed.price, Some(dec!(12.50)));
        assert_eq!(mapped.proposed.fx_rate_to_base, Some(dec!(10)));
        assert_eq!(mapped.proposed.brokerage_base, Some(dec!(9.60)));
        assert_eq!(mapped.source_value, Some(dec!(1259.60)));
        assert!(!mapped.fx_warning);
    }

    #[test]
    fn sell_passes_absolute_magnitude() {
        let mut r = row(ParsedKind::Sell);
        r.quantity = dec!(-5);
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.kind, TransactionKind::Sell);
        assert_eq!(mapped.proposed.quantity, 5);
    }

    #[test]
    fn blank_or_zero_fx_maps_to_none_with_warning() {
        let mut r = row(ParsedKind::Buy);
        r.exchange_rate = None;
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.fx_rate_to_base, None);
        assert!(mapped.fx_warning);

        r.exchange_rate = Some(dec!(0));
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.fx_rate_to_base, None);
        assert!(mapped.fx_warning);
    }

    #[test]
    fn zero_brokerage_stores_no_fee_and_ignores_currency_label() {
        let mut r = row(ParsedKind::Buy);
        r.brokerage = dec!(0);
        r.brokerage_currency = "USD".into();
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.brokerage_base, None);
    }

    #[test]
    fn non_zero_non_sek_brokerage_is_a_hard_error() {
        let mut r = row(ParsedKind::Buy);
        r.brokerage_currency = "USD".into();
        assert_eq!(map_row(&r).unwrap_err().code, "non_sek_brokerage");
    }

    #[test]
    fn non_integer_quantity_is_a_hard_error() {
        let mut r = row(ParsedKind::Buy);
        r.quantity = dec!(1.5);
        assert_eq!(map_row(&r).unwrap_err().code, "non_integer_quantity");
    }

    #[test]
    fn split_carries_only_quantity_delta() {
        let r = row(ParsedKind::Split);
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.kind, TransactionKind::Split);
        assert_eq!(mapped.proposed.quantity, 10);
        assert_eq!(mapped.proposed.price, None);
        assert_eq!(mapped.proposed.currency, None);
        assert_eq!(mapped.proposed.fx_rate_to_base, None);
        assert_eq!(mapped.proposed.brokerage_base, None);
        let _ = MapError {
            row: 1,
            code: "x",
            message: String::new(),
        };
        let _ = Decimal::ZERO;
    }

    #[test]
    fn reverse_split_preserves_negative_delta() {
        let mut r = row(ParsedKind::Split);
        r.quantity = dec!(-9);
        let mapped = map_row(&r).expect("maps");
        assert_eq!(mapped.proposed.kind, TransactionKind::Split);
        assert_eq!(mapped.proposed.quantity, -9);
    }
}
