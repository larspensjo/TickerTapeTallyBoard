use super::performance::split_factor;
use super::{
    derive_position, FxCandidate, LedgerError, LedgerTransaction, PriceCandidate, TransactionKind,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct ValueHistoryInstrument {
    pub native_currency: String,
    pub ledger: Vec<LedgerTransaction>,
    pub prices: Vec<PriceCandidate>,
    pub fx_rates: Vec<FxCandidate>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValueHistoryPoint {
    pub date: NaiveDate,
    pub value_base: Decimal,
    pub invested_base: Option<Decimal>,
    pub incomplete: bool,
    pub included_count: usize,
    pub excluded_count: usize,
}

struct InvestedCashFlow {
    date: NaiveDate,
    delta: Decimal,
}

/// Collect SEK cash-flow deltas from Buy/Sell trades and the earliest date from
/// which invested capital becomes unavailable because a non-SEK trade lacks FX.
/// Buys add cash out (price·qty·fx + brokerage); sells subtract cash returned
/// (price·|qty|·fx − brokerage). Splits and dividends are ignored.
fn invested_cash_flows(
    instruments: &[ValueHistoryInstrument],
) -> Result<(Vec<InvestedCashFlow>, Option<NaiveDate>), LedgerError> {
    let mut events = Vec::new();
    let mut unavailable_from: Option<NaiveDate> = None;
    let mark_unavailable = |date: NaiveDate, slot: &mut Option<NaiveDate>| {
        *slot = Some(match *slot {
            Some(current) => current.min(date),
            None => date,
        });
    };

    for inst in instruments {
        let is_base = inst.native_currency.eq_ignore_ascii_case("SEK");
        for tx in &inst.ledger {
            let (price, signed_qty) = match tx.kind {
                TransactionKind::Buy => (
                    tx.price.ok_or(LedgerError::BuyMissingPrice {
                        transaction_id: tx.id,
                    })?,
                    Decimal::from(tx.quantity),
                ),
                TransactionKind::Sell => (
                    tx.price.ok_or(LedgerError::SellMissingPrice {
                        transaction_id: tx.id,
                    })?,
                    // Sell quantity is negative; cash returned reduces invested.
                    Decimal::from(tx.quantity),
                ),
                TransactionKind::Split | TransactionKind::Dividend => continue,
            };

            let fx = if is_base {
                Some(Decimal::ONE)
            } else {
                tx.fx_rate_to_base
            };
            let Some(fx) = fx else {
                mark_unavailable(tx.trade_date, &mut unavailable_from);
                continue;
            };

            // Both buys and sells *add* brokerage_base. Buy: +(price·qty·fx) +
            // brokerage raises net invested. Sell: signed_qty is negative, so
            // price·signed_qty·fx is already the (negative) cash returned;
            // adding brokerage reduces that cash returned, i.e. keeps net
            // invested higher. Matches the authoritative formula above:
            //   −(price·|qty|·fx − brokerage) = price·signed_qty·fx + brokerage.
            let delta = price * signed_qty * fx + tx.brokerage_base;
            events.push(InvestedCashFlow {
                date: tx.trade_date,
                delta,
            });
        }
    }

    events.sort_by_key(|event| event.date);
    Ok((events, unavailable_from))
}

/// Build the portfolio value series in SEK from a union of price and FX dates.
///
/// The date spine starts at the first Buy and includes every supplied price or
/// FX date in the optional window. For each date, open positions are derived
/// from ledger rows up to that date, future splits are applied so historic
/// quantities line up with split-adjusted price history, and the latest price
/// and FX rows on or before the date are carried forward. Instruments missing
/// either input are counted as excluded; dates where every open position is
/// excluded are omitted so the chart never shows a spurious zero portfolio
/// value. Non-SEK FX rows must be for `native_currency -> SEK`; mismatched rows
/// are ignored to keep this pure helper safe even if callers forget to prefilter.
pub fn build_value_history(
    instruments: &[ValueHistoryInstrument],
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Result<Vec<ValueHistoryPoint>, LedgerError> {
    let first_buy = instruments
        .iter()
        .flat_map(|inst| inst.ledger.iter())
        .filter(|tx| tx.kind == TransactionKind::Buy)
        .map(|tx| tx.trade_date)
        .min();
    let Some(first_buy) = first_buy else {
        return Ok(Vec::new());
    };

    let (cash_flows, invested_unavailable_from) = invested_cash_flows(instruments)?;
    let mut next_flow = 0usize;
    let mut invested_total = Decimal::ZERO;

    let mut spine: BTreeSet<NaiveDate> = BTreeSet::new();
    for inst in instruments {
        for price in &inst.prices {
            spine.insert(price.date);
        }
        for fx in &inst.fx_rates {
            spine.insert(fx.date);
        }
    }

    let mut points = Vec::new();
    for date in spine {
        if date < first_buy {
            continue;
        }
        if from.is_some_and(|from| date < from) || to.is_some_and(|to| date > to) {
            continue;
        }

        let mut value_base = Decimal::ZERO;
        let mut included_count = 0usize;
        let mut excluded_count = 0usize;

        for inst in instruments {
            let active: Vec<LedgerTransaction> = inst
                .ledger
                .iter()
                .filter(|tx| tx.trade_date <= date)
                .cloned()
                .collect();
            let position = derive_position(&active)?;
            if position.quantity == 0 {
                continue;
            }

            let future: Vec<LedgerTransaction> = inst
                .ledger
                .iter()
                .filter(|tx| tx.trade_date > date)
                .cloned()
                .collect();
            let factor = split_factor(&future, position.quantity)?;
            let adjusted_qty = Decimal::from(position.quantity) * factor;

            let close = inst
                .prices
                .iter()
                .rfind(|price| {
                    price.date <= date && price.currency.eq_ignore_ascii_case(&inst.native_currency)
                })
                .map(|price| price.close);

            let rate = if inst.native_currency.eq_ignore_ascii_case("SEK") {
                Some(Decimal::ONE)
            } else {
                inst.fx_rates
                    .iter()
                    .rfind(|fx| {
                        fx.date <= date
                            && fx.base.eq_ignore_ascii_case(&inst.native_currency)
                            && fx.quote.eq_ignore_ascii_case("SEK")
                    })
                    .map(|fx| fx.rate)
            };

            match (close, rate) {
                (Some(close), Some(rate)) => {
                    value_base += adjusted_qty * close * rate;
                    included_count += 1;
                }
                _ => excluded_count += 1,
            }
        }

        if included_count == 0 {
            continue;
        }

        while next_flow < cash_flows.len() && cash_flows[next_flow].date <= date {
            invested_total += cash_flows[next_flow].delta;
            next_flow += 1;
        }
        let invested_base = match invested_unavailable_from {
            Some(unavailable) if date >= unavailable => None,
            _ => Some(invested_total),
        };

        points.push(ValueHistoryPoint {
            date,
            value_base,
            invested_base,
            incomplete: excluded_count > 0,
            included_count,
            excluded_count,
        });
    }

    Ok(points)
}

#[cfg(test)]
mod tests {
    use super::{build_value_history, FxCandidate, PriceCandidate, ValueHistoryInstrument};
    use crate::domain::{LedgerTransaction, ProviderCode, TransactionKind};
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
        _currency: &str,
    ) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Buy,
            quantity: qty,
            price: Some(price),
            dividend_per_share: None,
            fx_rate_to_base: fx,
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
            dividend_per_share: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn price(date: NaiveDate, close: Decimal, currency: &str) -> PriceCandidate {
        PriceCandidate {
            date,
            close,
            currency: currency.to_owned(),
            source: ProviderCode::new("PRIMARY"),
        }
    }

    fn fx(date: NaiveDate, rate: Decimal, base: &str, quote: &str) -> FxCandidate {
        FxCandidate {
            date,
            rate,
            base: base.to_owned(),
            quote: quote.to_owned(),
        }
    }

    fn vh_instrument(
        currency: &str,
        ledger: Vec<LedgerTransaction>,
        prices: Vec<PriceCandidate>,
        fx_rates: Vec<FxCandidate>,
    ) -> ValueHistoryInstrument {
        ValueHistoryInstrument {
            native_currency: currency.to_owned(),
            ledger,
            prices,
            fx_rates,
        }
    }

    fn ledger_buy(id: i64, date: NaiveDate, qty: i64, price: Decimal) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Buy,
            quantity: qty,
            price: Some(price),
            dividend_per_share: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn ledger_sell(id: i64, date: NaiveDate, qty: i64, price: Decimal) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Sell,
            quantity: -qty,
            price: Some(price),
            dividend_per_share: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn ledger_buy_fx(
        id: i64,
        date: NaiveDate,
        qty: i64,
        price: Decimal,
        fx_rate_to_base: Option<Decimal>,
    ) -> LedgerTransaction {
        LedgerTransaction {
            fx_rate_to_base,
            ..ledger_buy(id, date, qty, price)
        }
    }

    #[test]
    fn invested_capital_tracks_buy_cost_including_brokerage() {
        // SEK instrument: buy 10 @ 100 with 9 SEK brokerage on 2026-01-02.
        let mut buy = ledger_buy(1, d(2026, 1, 2), 10, dec!(100));
        buy.brokerage_base = dec!(9);
        let inst = ValueHistoryInstrument {
            native_currency: "SEK".to_string(),
            ledger: vec![buy],
            prices: vec![price(d(2026, 1, 2), dec!(100), "SEK")],
            fx_rates: vec![],
        };
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert_eq!(points.len(), 1);
        // 10*100 + 9 = 1009
        assert_eq!(points[0].invested_base, Some(dec!(1009)));
    }

    #[test]
    fn invested_capital_drops_by_sell_proceeds_net_of_brokerage() {
        // Buy 10 @ 100 (2026-01-02), sell 4 @ 150 with 5 SEK brokerage (2026-01-05).
        let buy = ledger_buy(1, d(2026, 1, 2), 10, dec!(100));
        let mut sell = ledger_sell(2, d(2026, 1, 5), 4, dec!(150));
        sell.brokerage_base = dec!(5);
        let inst = ValueHistoryInstrument {
            native_currency: "SEK".to_string(),
            ledger: vec![buy, sell],
            prices: vec![
                price(d(2026, 1, 2), dec!(100), "SEK"),
                price(d(2026, 1, 5), dec!(150), "SEK"),
            ],
            fx_rates: vec![],
        };
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        // Day 1: invested = 1000. Day 2: 1000 - (4*150 - 5) = 1000 - 595 = 405.
        assert_eq!(points[0].invested_base, Some(dec!(1000)));
        assert_eq!(points[1].invested_base, Some(dec!(405)));
    }

    #[test]
    fn invested_capital_uses_trade_time_fx_for_non_sek() {
        // USD instrument: buy 10 @ 100 USD at fx 10 on 2026-01-02.
        let buy = ledger_buy_fx(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)));
        let inst = ValueHistoryInstrument {
            native_currency: "USD".to_string(),
            ledger: vec![buy],
            prices: vec![price(d(2026, 1, 2), dec!(100), "USD")],
            fx_rates: vec![fx(d(2026, 1, 2), dec!(10), "USD", "SEK")],
        };
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        // 10*100*10 = 10000, no brokerage.
        assert_eq!(points[0].invested_base, Some(dec!(10000)));
    }

    #[test]
    fn invested_capital_unavailable_when_non_sek_trade_lacks_fx() {
        // USD buy missing fx_rate_to_base => invested unavailable from that date.
        let buy = ledger_buy_fx(1, d(2026, 1, 2), 10, dec!(100), None);
        let inst = ValueHistoryInstrument {
            native_currency: "USD".to_string(),
            ledger: vec![buy],
            prices: vec![price(d(2026, 1, 2), dec!(100), "USD")],
            fx_rates: vec![fx(d(2026, 1, 2), dec!(10), "USD", "SEK")],
        };
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert_eq!(points[0].invested_base, None);
    }

    #[test]
    fn value_history_sek_single_holding_uses_price_dates() {
        let inst = vh_instrument(
            "SEK",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
            vec![
                price(d(2026, 1, 2), dec!(100), "SEK"),
                price(d(2026, 1, 5), dec!(110), "SEK"),
            ],
            vec![],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].date, d(2026, 1, 2));
        assert_eq!(points[0].value_base, dec!(1000));
        assert_eq!(points[0].included_count, 1);
        assert!(!points[0].incomplete);
        assert_eq!(points[1].value_base, dec!(1100));
    }

    #[test]
    fn value_history_carries_price_and_fx_forward() {
        let inst = vh_instrument(
            "USD",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)), "USD")],
            vec![price(d(2026, 1, 2), dec!(100), "USD")],
            vec![
                fx(d(2026, 1, 2), dec!(10), "USD", "SEK"),
                fx(d(2026, 1, 6), dec!(11), "USD", "SEK"),
            ],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].date, d(2026, 1, 2));
        assert_eq!(points[0].value_base, dec!(10000));
        assert_eq!(points[1].date, d(2026, 1, 6));
        assert_eq!(points[1].value_base, dec!(11000));
    }

    #[test]
    fn value_history_ignores_fx_for_other_pairs() {
        let inst = vh_instrument(
            "USD",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)), "USD")],
            vec![price(d(2026, 1, 2), dec!(100), "USD")],
            vec![
                fx(d(2026, 1, 2), dec!(10), "EUR", "SEK"),
                fx(d(2026, 1, 3), dec!(11), "USD", "NOK"),
            ],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert!(points.is_empty());
    }

    #[test]
    fn value_history_split_adjusts_pre_split_points() {
        let inst = vh_instrument(
            "SEK",
            vec![
                buy(1, d(2026, 1, 2), 10, dec!(120), Some(dec!(1)), "SEK"),
                split(2, d(2026, 1, 10), 10),
            ],
            vec![
                price(d(2026, 1, 5), dec!(60), "SEK"),
                price(d(2026, 1, 12), dec!(60), "SEK"),
            ],
            vec![],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        let p5 = points
            .iter()
            .find(|p| p.date == d(2026, 1, 5))
            .expect("1/5");
        assert_eq!(p5.value_base, dec!(1200));
        let p12 = points
            .iter()
            .find(|p| p.date == d(2026, 1, 12))
            .expect("1/12");
        assert_eq!(p12.value_base, dec!(1200));
    }

    #[test]
    fn value_history_excludes_instrument_with_disabled_mapping() {
        let inst = vh_instrument(
            "SEK",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
            vec![],
            vec![],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert!(points.is_empty());
    }

    #[test]
    fn value_history_marks_incomplete_and_omits_all_excluded_dates() {
        let present = vh_instrument(
            "SEK",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
            vec![price(d(2026, 1, 2), dec!(100), "SEK")],
            vec![],
        );
        let absent = vh_instrument(
            "SEK",
            vec![buy(2, d(2026, 1, 2), 5, dec!(50), Some(dec!(1)), "SEK")],
            vec![price(d(2026, 1, 9), dec!(50), "SEK")],
            vec![],
        );
        let points =
            build_value_history(&[present, absent], None, None).expect("derivable ledgers");
        let p2 = points
            .iter()
            .find(|p| p.date == d(2026, 1, 2))
            .expect("1/2");
        assert_eq!(p2.value_base, dec!(1000));
        assert_eq!(p2.included_count, 1);
        assert_eq!(p2.excluded_count, 1);
        assert!(p2.incomplete);
        let p9 = points
            .iter()
            .find(|p| p.date == d(2026, 1, 9))
            .expect("1/9");
        assert_eq!(p9.included_count, 2);
        assert!(!p9.incomplete);
    }

    #[test]
    fn value_history_omits_points_where_every_position_is_excluded() {
        let inst = vh_instrument(
            "USD",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)), "USD")],
            vec![price(d(2026, 1, 2), dec!(100), "USD")],
            vec![],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert!(points.is_empty());
    }

    #[test]
    fn value_history_empty_when_no_buy_yet() {
        let points = build_value_history(&[], None, None).expect("no ledger is Ok(empty)");
        assert!(points.is_empty());
    }

    #[test]
    fn value_history_windows_with_from_and_to() {
        let inst = vh_instrument(
            "SEK",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
            vec![
                price(d(2026, 1, 2), dec!(100), "SEK"),
                price(d(2026, 1, 5), dec!(110), "SEK"),
                price(d(2026, 1, 9), dec!(120), "SEK"),
            ],
            vec![],
        );
        let points = build_value_history(&[inst], Some(d(2026, 1, 5)), Some(d(2026, 1, 5)))
            .expect("derivable ledger");
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].date, d(2026, 1, 5));
        assert_eq!(points[0].value_base, dec!(1100));
    }
}
