//! Avanza to PreparedImport mapping.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::domain::{ProposedTransaction, TransactionKind};
use crate::import::avanza::parser::{AvanzaKind, ParsedAvanzaReport, ParsedAvanzaRow};
use crate::import::core::outcome::{
    InstrumentKey, MappedRow, PlanHeader, PreparedImport, RowNote, RowOutcome, SourceKindCounts,
};

const TITLE: &str = "Avanza All Trades";

/// Build position events (sorted by (date, source_row)) from all buy/sell/split rows
/// so dividend mapping can derive eligible share counts deterministically.
fn build_position_events(
    rows: &[ParsedAvanzaRow],
) -> BTreeMap<String, Vec<(NaiveDate, usize, i64)>> {
    let mut by_isin: BTreeMap<String, Vec<(NaiveDate, usize, i64)>> = BTreeMap::new();

    // Buy and sell events directly
    for row in rows {
        if row.isin.trim().is_empty() {
            continue;
        }
        match row.kind {
            AvanzaKind::Buy => {
                if let Some(q) = row.quantity.to_i64() {
                    if q > 0 {
                        by_isin.entry(row.isin.clone()).or_default().push((
                            row.trade_date,
                            row.source_row_number,
                            q,
                        ));
                    }
                }
            }
            AvanzaKind::Sell => {
                if let Some(q) = row.quantity.abs().to_i64() {
                    if q > 0 {
                        by_isin.entry(row.isin.clone()).or_default().push((
                            row.trade_date,
                            row.source_row_number,
                            -q,
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    // Net split events grouped by (date, isin), same netting as the main split mapper
    let mut split_groups: BTreeMap<(NaiveDate, String), (Decimal, usize)> = BTreeMap::new();
    for row in rows {
        if row.isin.trim().is_empty() || row.kind != AvanzaKind::Split {
            continue;
        }
        let entry = split_groups
            .entry((row.trade_date, row.isin.clone()))
            .or_insert((Decimal::ZERO, row.source_row_number));
        entry.0 += row.quantity;
        entry.1 = entry.1.min(row.source_row_number);
    }
    for ((date, isin), (net, first_row)) in split_groups {
        if let Some(delta) = net.to_i64() {
            if delta != 0 {
                by_isin
                    .entry(isin)
                    .or_default()
                    .push((date, first_row, delta));
            }
        }
    }

    for events in by_isin.values_mut() {
        events.sort_by_key(|(d, row, _)| (*d, *row));
    }

    by_isin
}

fn eligible_quantity_at(
    isin: &str,
    date: NaiveDate,
    source_row: usize,
    position_events: &BTreeMap<String, Vec<(NaiveDate, usize, i64)>>,
) -> i64 {
    position_events
        .get(isin)
        .map(|events| {
            events
                .iter()
                .filter(|(d, row, _)| (*d, *row) <= (date, source_row))
                .map(|(_, _, delta)| delta)
                .sum::<i64>()
        })
        .unwrap_or(0)
}

pub fn to_prepared(report: &ParsedAvanzaReport) -> PreparedImport {
    let mut counts = SourceKindCounts {
        rows: report.rows.len(),
        ..Default::default()
    };
    let mut outcomes: Vec<RowOutcome> = Vec::new();

    let mut instrument_by_isin: BTreeMap<String, InstrumentKey> = BTreeMap::new();
    for row in &report.rows {
        if matches!(row.kind, AvanzaKind::Buy | AvanzaKind::Sell) && !row.isin.trim().is_empty() {
            instrument_by_isin
                .entry(row.isin.clone())
                .or_insert_with(|| buy_sell_instrument(row));
        }
    }

    let position_events = build_position_events(&report.rows);

    let mut split_groups: BTreeMap<(NaiveDate, String), SplitGroup> = BTreeMap::new();

    for row in &report.rows {
        match row.kind {
            AvanzaKind::Buy => {
                counts.buys += 1;
                outcomes.push(map_buy_sell(row, TransactionKind::Buy));
            }
            AvanzaKind::Sell => {
                counts.sells += 1;
                outcomes.push(map_buy_sell(row, TransactionKind::Sell));
            }
            AvanzaKind::Dividend => {
                counts.dividends += 1;
                outcomes.push(map_dividend(row, &position_events, &instrument_by_isin));
            }
            AvanzaKind::Split => {
                counts.splits += 1;
                if row.isin.trim().is_empty() {
                    outcomes.push(RowOutcome::Skip {
                        asset_key: None,
                        note: RowNote {
                            row: Some(row.source_row_number),
                            code: "missing_isin",
                            message: format!("split row for {} has no ISIN", row.name),
                        },
                    });
                    continue;
                }
                let entry = split_groups
                    .entry((row.trade_date, row.isin.clone()))
                    .or_insert_with(|| SplitGroup::new(row));
                entry.net += row.quantity;
                entry.first_row = entry.first_row.min(row.source_row_number);
            }
            AvanzaKind::Unsupported => {
                outcomes.push(RowOutcome::Skip {
                    asset_key: asset_key_of(&row.isin),
                    note: RowNote {
                        row: Some(row.source_row_number),
                        code: "unsupported_type",
                        message: format!("transaction type {:?} is not supported", row.raw_kind),
                    },
                });
            }
        }
    }

    for ((trade_date, isin), group) in split_groups {
        outcomes.push(map_split(
            trade_date,
            &isin,
            group.net,
            group.first_row,
            &group.name,
            &instrument_by_isin,
        ));
    }

    PreparedImport {
        header: PlanHeader {
            title: TITLE.to_string(),
            date_from: report.date_from.unwrap_or_else(trade_date_fallback),
            date_to: report.date_to.unwrap_or_else(trade_date_fallback),
        },
        counts,
        outcomes,
    }
}

fn trade_date_fallback() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch date")
}

fn buy_sell_instrument(row: &ParsedAvanzaRow) -> InstrumentKey {
    InstrumentKey {
        exchange: "AVANZA".to_string(),
        symbol: row.isin.clone(),
        name: row.name.clone(),
        currency: row.instrument_currency.clone(),
        isin: Some(row.isin.clone()),
    }
}

fn asset_key_of(isin: &str) -> Option<String> {
    let trimmed = isin.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn map_buy_sell(row: &ParsedAvanzaRow, kind: TransactionKind) -> RowOutcome {
    if !row.quantity.fract().is_zero() {
        return RowOutcome::Excluded {
            asset_key: asset_key_of(&row.isin),
            name: Some(row.name.clone()),
            currency: Some(row.instrument_currency.clone()),
            note: RowNote {
                row: Some(row.source_row_number),
                code: "fractional_fund_excluded",
                message: format!(
                    "{} has fractional quantity {}; fund transactions are excluded",
                    row.name, row.quantity
                ),
            },
        };
    }

    let Some(magnitude) = row.quantity.abs().to_i64() else {
        return RowOutcome::Skip {
            asset_key: asset_key_of(&row.isin),
            note: RowNote {
                row: Some(row.source_row_number),
                code: "non_integer_quantity",
                message: format!("quantity {} does not fit in i64", row.quantity),
            },
        };
    };

    let (fx_rate_to_base, fx_warning) = match row.fx_rate {
        Some(rate) if rate > Decimal::ZERO => (Some(rate), false),
        _ if row.instrument_currency.eq_ignore_ascii_case("SEK") => (Some(Decimal::ONE), false),
        _ => (None, true),
    };

    let brokerage_base = match row.brokerage {
        Some(brokerage) if brokerage > Decimal::ZERO => Some(brokerage),
        _ => None,
    };

    RowOutcome::Mapped(MappedRow {
        source_row_number: row.source_row_number,
        instrument: buy_sell_instrument(row),
        proposed: ProposedTransaction {
            kind,
            trade_date: row.trade_date,
            quantity: magnitude,
            price: row.price,
            currency: Some(row.instrument_currency.clone()),
            fx_rate_to_base,
            brokerage_base,
        },
        source_value: row.amount.map(|amount| -amount),
        source_currency: Some(row.transaction_currency.clone()),
        note: None,
        fx_warning,
    })
}

fn map_dividend(
    row: &ParsedAvanzaRow,
    position_events: &BTreeMap<String, Vec<(NaiveDate, usize, i64)>>,
    instrument_by_isin: &BTreeMap<String, InstrumentKey>,
) -> RowOutcome {
    if row.isin.trim().is_empty() {
        return RowOutcome::Skip {
            asset_key: None,
            note: RowNote {
                row: Some(row.source_row_number),
                code: "missing_isin",
                message: format!("dividend for {} has no ISIN", row.name),
            },
        };
    }

    let Some(cash_amount) = row.amount else {
        return RowOutcome::Skip {
            asset_key: asset_key_of(&row.isin),
            note: RowNote {
                row: Some(row.source_row_number),
                code: "missing_amount",
                message: format!("dividend for {} has no cash amount", row.name),
            },
        };
    };

    if row.instrument_currency.trim().is_empty() {
        return RowOutcome::Skip {
            asset_key: asset_key_of(&row.isin),
            note: RowNote {
                row: Some(row.source_row_number),
                code: "missing_currency",
                message: format!("dividend for {} has no instrument currency", row.name),
            },
        };
    }

    let eligible_qty = match positive_integer_quantity(row) {
        Ok(Some(quantity)) => quantity,
        Ok(None) => eligible_quantity_at(
            &row.isin,
            row.trade_date,
            row.source_row_number,
            position_events,
        ),
        Err(note) => {
            return RowOutcome::Skip {
                asset_key: asset_key_of(&row.isin),
                note,
            };
        }
    };

    if eligible_qty <= 0 {
        return RowOutcome::Skip {
            asset_key: asset_key_of(&row.isin),
            note: RowNote {
                row: Some(row.source_row_number),
                code: "non_positive_eligible_quantity",
                message: format!(
                    "dividend for {} has no positive eligible quantity at {}",
                    row.name, row.trade_date
                ),
            },
        };
    }

    let native_price_per_share = if let Some(kurs) = row.price.filter(|k| *k > Decimal::ZERO) {
        kurs
    } else if let Some(fx) = dividend_fx_rate(row, cash_amount, eligible_qty, None).0 {
        match derive_dividend_per_share(row, cash_amount, eligible_qty, fx) {
            Ok(per_share) => per_share,
            Err(note) => {
                return RowOutcome::Skip {
                    asset_key: asset_key_of(&row.isin),
                    note,
                };
            }
        }
    } else {
        return missing_dividend_fx(row);
    };

    let (fx_rate_to_base, fx_warning) =
        dividend_fx_rate(row, cash_amount, eligible_qty, Some(native_price_per_share));

    let instrument = instrument_by_isin
        .get(&row.isin)
        .cloned()
        .unwrap_or_else(|| InstrumentKey {
            exchange: "AVANZA".to_string(),
            symbol: row.isin.clone(),
            name: row.name.clone(),
            currency: row.instrument_currency.clone(),
            isin: Some(row.isin.clone()),
        });

    RowOutcome::Mapped(MappedRow {
        source_row_number: row.source_row_number,
        instrument,
        proposed: ProposedTransaction {
            kind: TransactionKind::Dividend,
            trade_date: row.trade_date,
            quantity: eligible_qty,
            price: Some(native_price_per_share),
            currency: Some(row.instrument_currency.clone()),
            fx_rate_to_base,
            brokerage_base: None,
        },
        source_value: row.amount.map(|a| a.abs()),
        source_currency: Some(row.transaction_currency.clone()),
        note: None,
        fx_warning,
    })
}

fn positive_integer_quantity(row: &ParsedAvanzaRow) -> Result<Option<i64>, RowNote> {
    if row.quantity <= Decimal::ZERO {
        return Ok(None);
    }
    if !row.quantity.fract().is_zero() {
        return Err(RowNote {
            row: Some(row.source_row_number),
            code: "non_integer_quantity",
            message: format!(
                "dividend quantity {} for {} is not an integer",
                row.quantity, row.name
            ),
        });
    }
    row.quantity.to_i64().map(Some).ok_or_else(|| RowNote {
        row: Some(row.source_row_number),
        code: "non_integer_quantity",
        message: format!(
            "dividend quantity {} for {} does not fit in i64",
            row.quantity, row.name
        ),
    })
}

fn dividend_fx_rate(
    row: &ParsedAvanzaRow,
    cash_amount: Decimal,
    eligible_qty: i64,
    native_price_per_share: Option<Decimal>,
) -> (Option<Decimal>, bool) {
    match row.fx_rate {
        Some(rate) if rate > Decimal::ZERO => (Some(rate), false),
        _ if row.instrument_currency.eq_ignore_ascii_case("SEK") => (Some(Decimal::ONE), false),
        _ => derive_dividend_fx_from_source(row, cash_amount, eligible_qty, native_price_per_share)
            .map_or((None, true), |fx| (Some(fx), false)),
    }
}

fn derive_dividend_fx_from_source(
    row: &ParsedAvanzaRow,
    cash_amount: Decimal,
    eligible_qty: i64,
    native_price_per_share: Option<Decimal>,
) -> Option<Decimal> {
    if !row.transaction_currency.eq_ignore_ascii_case("SEK") {
        return None;
    }
    let price = native_price_per_share.or(row.price)?;
    if eligible_qty <= 0 || price <= Decimal::ZERO || cash_amount.is_zero() {
        return None;
    }
    let native_total = Decimal::from(eligible_qty) * price;
    (native_total > Decimal::ZERO).then(|| cash_amount.abs() / native_total)
}

fn derive_dividend_per_share(
    row: &ParsedAvanzaRow,
    cash_amount: Decimal,
    eligible_qty: i64,
    fx: Decimal,
) -> Result<Decimal, RowNote> {
    let native_total = cash_amount.abs() / fx;
    let per_share = native_total / Decimal::from(eligible_qty);
    if per_share <= Decimal::ZERO {
        return Err(RowNote {
            row: Some(row.source_row_number),
            code: "non_positive_dividend",
            message: format!(
                "derived per-share dividend for {} is not positive",
                row.name
            ),
        });
    }
    Ok(per_share)
}

fn missing_dividend_fx(row: &ParsedAvanzaRow) -> RowOutcome {
    RowOutcome::Skip {
        asset_key: asset_key_of(&row.isin),
        note: RowNote {
            row: Some(row.source_row_number),
            code: "missing_fx_for_derivation",
            message: format!(
                "dividend for {} has no native price and no FX rate; cannot derive per-share amount",
                row.name
            ),
        },
    }
}

fn map_split(
    trade_date: NaiveDate,
    isin: &str,
    net: Decimal,
    row: usize,
    name: &str,
    instrument_by_isin: &BTreeMap<String, InstrumentKey>,
) -> RowOutcome {
    if net.is_zero() {
        return RowOutcome::Skip {
            asset_key: asset_key_of(isin),
            note: RowNote {
                row: Some(row),
                code: "split_zero_net",
                message: format!("net split delta for {name} is zero"),
            },
        };
    }

    if !net.fract().is_zero() {
        return RowOutcome::Skip {
            asset_key: asset_key_of(isin),
            note: RowNote {
                row: Some(row),
                code: "non_integer_quantity",
                message: format!("net split delta {net} is not an integer"),
            },
        };
    }

    let Some(delta) = net.to_i64() else {
        return RowOutcome::Skip {
            asset_key: asset_key_of(isin),
            note: RowNote {
                row: Some(row),
                code: "non_integer_quantity",
                message: format!("net split delta {net} does not fit in i64"),
            },
        };
    };

    let instrument = instrument_by_isin
        .get(isin)
        .cloned()
        .unwrap_or_else(|| InstrumentKey {
            exchange: "AVANZA".to_string(),
            symbol: isin.to_string(),
            name: name.to_string(),
            currency: String::new(),
            isin: Some(isin.to_string()),
        });

    RowOutcome::Mapped(MappedRow {
        source_row_number: row,
        instrument,
        proposed: ProposedTransaction {
            kind: TransactionKind::Split,
            trade_date,
            quantity: delta,
            price: None,
            currency: None,
            fx_rate_to_base: None,
            brokerage_base: None,
        },
        source_value: None,
        source_currency: None,
        note: None,
        fx_warning: false,
    })
}

#[derive(Clone, Debug)]
struct SplitGroup {
    net: Decimal,
    first_row: usize,
    name: String,
}

impl SplitGroup {
    fn new(row: &ParsedAvanzaRow) -> Self {
        Self {
            net: Decimal::ZERO,
            first_row: row.source_row_number,
            name: row.name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::core::outcome::RowOutcome;
    use crate::import::core::plan::{build_plan, PlanContext};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    struct RowSpec<'a> {
        source_row_number: usize,
        trade_date: NaiveDate,
        raw_kind: &'a str,
        name: &'a str,
        quantity: Decimal,
        price: Option<Decimal>,
        amount: Option<Decimal>,
        transaction_currency: &'a str,
        brokerage: Option<Decimal>,
        fx_rate: Option<Decimal>,
        instrument_currency: &'a str,
        isin: &'a str,
    }

    fn row(spec: RowSpec<'_>) -> ParsedAvanzaRow {
        ParsedAvanzaRow {
            source_row_number: spec.source_row_number,
            trade_date: spec.trade_date,
            raw_kind: spec.raw_kind.to_string(),
            kind: match spec.raw_kind {
                "Köp" => AvanzaKind::Buy,
                "Sälj" => AvanzaKind::Sell,
                "Utdelning" => AvanzaKind::Dividend,
                "Split värdepapper" => AvanzaKind::Split,
                _ => AvanzaKind::Unsupported,
            },
            name: spec.name.to_string(),
            quantity: spec.quantity,
            price: spec.price,
            amount: spec.amount,
            transaction_currency: spec.transaction_currency.to_string(),
            brokerage: spec.brokerage,
            fx_rate: spec.fx_rate,
            instrument_currency: spec.instrument_currency.to_string(),
            isin: spec.isin.to_string(),
        }
    }

    fn report(rows: Vec<ParsedAvanzaRow>) -> ParsedAvanzaReport {
        ParsedAvanzaReport {
            rows,
            date_from: Some(date(2026, 5, 1)),
            date_to: Some(date(2026, 6, 2)),
        }
    }

    fn mapped(prepared: &PreparedImport) -> Vec<&MappedRow> {
        prepared
            .outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                RowOutcome::Mapped(mapped) => Some(mapped),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn buy_sell_rows_keep_fx_and_source_currency() {
        let prepared = to_prepared(&report(vec![row(RowSpec {
            source_row_number: 2,
            trade_date: date(2026, 6, 1),
            raw_kind: "Köp",
            name: "ServiceNow",
            quantity: dec!(10),
            price: Some(dec!(900)),
            amount: Some(dec!(-94500)),
            transaction_currency: "SEK",
            brokerage: Some(dec!(9)),
            fx_rate: Some(dec!(10.50)),
            instrument_currency: "USD",
            isin: "US81762P1021",
        })]));
        let rows = mapped(&prepared);

        assert_eq!(prepared.counts.rows, 1);
        assert_eq!(prepared.counts.buys, 1);
        assert_eq!(prepared.counts.sells, 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].proposed.kind, TransactionKind::Buy);
        assert_eq!(rows[0].proposed.fx_rate_to_base, Some(dec!(10.50)));
        assert_eq!(rows[0].source_value, Some(dec!(94500)));
        assert_eq!(rows[0].source_currency.as_deref(), Some("SEK"));
        assert!(!rows[0].fx_warning);
        assert_eq!(rows[0].proposed.currency.as_deref(), Some("USD"));
    }

    #[test]
    fn unsettled_buy_uses_native_currency_and_warns_about_fx() {
        let prepared = to_prepared(&report(vec![row(RowSpec {
            source_row_number: 2,
            trade_date: date(2026, 5, 1),
            raw_kind: "Köp",
            name: "ASML Holding",
            quantity: dec!(1),
            price: Some(dec!(800)),
            amount: Some(dec!(-800)),
            transaction_currency: "EUR",
            brokerage: None,
            fx_rate: None,
            instrument_currency: "EUR",
            isin: "NL0010273215",
        })]));
        let rows = mapped(&prepared);

        assert_eq!(rows[0].proposed.fx_rate_to_base, None);
        assert!(rows[0].fx_warning);
        assert_eq!(rows[0].source_currency.as_deref(), Some("EUR"));
    }

    #[test]
    fn sek_instrument_with_blank_fx_gets_one_without_warning() {
        let prepared = to_prepared(&report(vec![row(RowSpec {
            source_row_number: 2,
            trade_date: date(2026, 5, 1),
            raw_kind: "Köp",
            name: "Volvo B",
            quantity: dec!(3),
            price: Some(dec!(250)),
            amount: Some(dec!(-750)),
            transaction_currency: "SEK",
            brokerage: Some(dec!(0)),
            fx_rate: None,
            instrument_currency: "SEK",
            isin: "SE0000115446",
        })]));
        let rows = mapped(&prepared);

        assert_eq!(rows[0].proposed.fx_rate_to_base, Some(Decimal::ONE));
        assert!(!rows[0].fx_warning);
    }

    #[test]
    fn dividend_with_prior_buys_maps_to_dividend_row() {
        let prepared = to_prepared(&report(vec![
            row(RowSpec {
                source_row_number: 2,
                trade_date: date(2026, 5, 10),
                raw_kind: "Köp",
                name: "Apple Inc",
                quantity: dec!(5),
                price: Some(dec!(200)),
                amount: Some(dec!(-9459)),
                transaction_currency: "SEK",
                brokerage: Some(dec!(9)),
                fx_rate: Some(dec!(9.45)),
                instrument_currency: "USD",
                isin: "US0378331005",
            }),
            row(RowSpec {
                source_row_number: 3,
                trade_date: date(2026, 5, 15),
                raw_kind: "Sälj",
                name: "Apple Inc",
                quantity: dec!(-2),
                price: Some(dec!(210)),
                amount: Some(dec!(3960)),
                transaction_currency: "SEK",
                brokerage: Some(dec!(9)),
                fx_rate: Some(dec!(9.45)),
                instrument_currency: "USD",
                isin: "US0378331005",
            }),
            row(RowSpec {
                source_row_number: 4,
                trade_date: date(2026, 5, 20),
                raw_kind: "Utdelning",
                name: "Apple Inc",
                quantity: dec!(0),
                price: None,
                amount: Some(dec!(120)),
                transaction_currency: "SEK",
                brokerage: None,
                fx_rate: Some(dec!(9.40)),
                instrument_currency: "USD",
                isin: "US0378331005",
            }),
        ]));

        let rows = mapped(&prepared);
        // buy + sell + dividend (mapped)
        assert_eq!(rows.len(), 3);

        let dividend = rows
            .iter()
            .find(|r| r.proposed.kind == TransactionKind::Dividend)
            .expect("dividend row");
        // eligible_qty = 5 - 2 = 3
        assert_eq!(dividend.proposed.quantity, 3);
        assert_eq!(dividend.proposed.currency.as_deref(), Some("USD"));
        assert_eq!(dividend.proposed.fx_rate_to_base, Some(dec!(9.40)));
        // per_share = (120 / 9.40) / 3 ≈ 4.255319...
        let expected = (dec!(120) / dec!(9.40)) / dec!(3);
        assert_eq!(dividend.proposed.price, Some(expected));
        assert_eq!(dividend.source_value, Some(dec!(120)));
        assert_eq!(dividend.source_currency.as_deref(), Some("SEK"));
        assert!(!dividend.fx_warning);
    }

    #[test]
    fn dividend_with_native_kurs_uses_kurs_directly() {
        let prepared = to_prepared(&report(vec![
            row(RowSpec {
                source_row_number: 2,
                trade_date: date(2026, 5, 1),
                raw_kind: "Köp",
                name: "Volvo B",
                quantity: dec!(10),
                price: Some(dec!(250)),
                amount: Some(dec!(-2500)),
                transaction_currency: "SEK",
                brokerage: None,
                fx_rate: None,
                instrument_currency: "SEK",
                isin: "SE0000115446",
            }),
            row(RowSpec {
                source_row_number: 3,
                trade_date: date(2026, 5, 20),
                raw_kind: "Utdelning",
                name: "Volvo B",
                quantity: dec!(0),
                price: Some(dec!(7.5)), // native per-share from Avanza
                amount: Some(dec!(75)),
                transaction_currency: "SEK",
                brokerage: None,
                fx_rate: None,
                instrument_currency: "SEK",
                isin: "SE0000115446",
            }),
        ]));

        let rows = mapped(&prepared);
        let dividend = rows
            .iter()
            .find(|r| r.proposed.kind == TransactionKind::Dividend)
            .expect("dividend row");
        assert_eq!(dividend.proposed.quantity, 10);
        assert_eq!(dividend.proposed.price, Some(dec!(7.5)));
        assert_eq!(dividend.proposed.fx_rate_to_base, Some(Decimal::ONE));
        assert!(!dividend.fx_warning);
    }

    #[test]
    fn dividend_foreign_missing_fx_warns_when_native_kurs_present() {
        let prepared = to_prepared(&report(vec![
            row(RowSpec {
                source_row_number: 2,
                trade_date: date(2026, 5, 1),
                raw_kind: "Köp",
                name: "ASML",
                quantity: dec!(4),
                price: Some(dec!(800)),
                amount: Some(dec!(-800)),
                transaction_currency: "EUR",
                brokerage: None,
                fx_rate: None,
                instrument_currency: "EUR",
                isin: "NL0010273215",
            }),
            row(RowSpec {
                source_row_number: 3,
                trade_date: date(2026, 5, 20),
                raw_kind: "Utdelning",
                name: "ASML",
                quantity: dec!(0),
                price: Some(dec!(6.40)), // native per-share, FX missing
                amount: Some(dec!(25)),
                transaction_currency: "EUR",
                brokerage: None,
                fx_rate: None, // missing FX
                instrument_currency: "EUR",
                isin: "NL0010273215",
            }),
        ]));

        let rows = mapped(&prepared);
        let dividend = rows
            .iter()
            .find(|r| r.proposed.kind == TransactionKind::Dividend)
            .expect("dividend row");
        assert_eq!(dividend.proposed.quantity, 4);
        assert_eq!(dividend.proposed.price, Some(dec!(6.40)));
        assert_eq!(dividend.proposed.fx_rate_to_base, None);
        assert!(dividend.fx_warning);
    }

    #[test]
    fn dividend_blank_fx_derives_rate_from_sek_amount_and_native_kurs() {
        let prepared = to_prepared(&report(vec![
            row(RowSpec {
                source_row_number: 2,
                trade_date: date(2025, 9, 8),
                raw_kind: "Köp",
                name: "Vistra",
                quantity: dec!(72),
                price: Some(dec!(188.385556)),
                amount: Some(dec!(-127253.55)),
                transaction_currency: "SEK",
                brokerage: Some(dec!(87.74)),
                fx_rate: Some(dec!(9.37541)),
                instrument_currency: "USD",
                isin: "US92840M1027",
            }),
            row(RowSpec {
                source_row_number: 3,
                trade_date: date(2025, 12, 30),
                raw_kind: "Sälj",
                name: "Vistra",
                quantity: dec!(-72),
                price: Some(dec!(161.670417)),
                amount: Some(dec!(106748.07)),
                transaction_currency: "SEK",
                brokerage: Some(dec!(73.69)),
                fx_rate: Some(dec!(9.176915)),
                instrument_currency: "USD",
                isin: "US92840M1027",
            }),
            row(RowSpec {
                source_row_number: 4,
                trade_date: date(2026, 1, 2),
                raw_kind: "Utdelning",
                name: "Vistra",
                quantity: dec!(72),
                price: Some(dec!(0.227)),
                amount: Some(dec!(150.02)),
                transaction_currency: "SEK",
                brokerage: None,
                fx_rate: None,
                instrument_currency: "USD",
                isin: "US92840M1027",
            }),
        ]));

        let dividend = mapped(&prepared)
            .into_iter()
            .find(|r| r.proposed.kind == TransactionKind::Dividend)
            .expect("dividend row");

        assert_eq!(dividend.proposed.quantity, 72);
        assert_eq!(dividend.proposed.price, Some(dec!(0.227)));
        assert_eq!(
            dividend.proposed.fx_rate_to_base,
            Some(dec!(150.02) / (dec!(72) * dec!(0.227)))
        );
        assert!(!dividend.fx_warning);
    }

    #[test]
    fn dividend_without_prior_buys_is_skipped() {
        let prepared = to_prepared(&report(vec![row(RowSpec {
            source_row_number: 2,
            trade_date: date(2026, 5, 20),
            raw_kind: "Utdelning",
            name: "Apple Inc",
            quantity: dec!(0),
            price: None,
            amount: Some(dec!(120)),
            transaction_currency: "SEK",
            brokerage: None,
            fx_rate: Some(dec!(9.40)),
            instrument_currency: "USD",
            isin: "US0378331005",
        })]));

        assert_eq!(prepared.counts.dividends, 1);
        assert!(matches!(
            prepared.outcomes[0],
            RowOutcome::Skip {
                ref note,
                asset_key: Some(_)
            } if note.code == "non_positive_eligible_quantity"
        ));
    }

    #[test]
    fn dividend_missing_fx_and_no_kurs_is_skipped() {
        let prepared = to_prepared(&report(vec![
            row(RowSpec {
                source_row_number: 2,
                trade_date: date(2026, 5, 1),
                raw_kind: "Köp",
                name: "Apple Inc",
                quantity: dec!(5),
                price: Some(dec!(200)),
                amount: Some(dec!(-200)),
                transaction_currency: "EUR",
                brokerage: None,
                fx_rate: None,
                instrument_currency: "EUR",
                isin: "US0378331005",
            }),
            row(RowSpec {
                source_row_number: 3,
                trade_date: date(2026, 5, 20),
                raw_kind: "Utdelning",
                name: "Apple Inc",
                quantity: dec!(0),
                price: None, // no native kurs
                amount: Some(dec!(100)),
                transaction_currency: "SEK",
                brokerage: None,
                fx_rate: None, // no FX
                instrument_currency: "EUR",
                isin: "US0378331005",
            }),
        ]));

        assert!(prepared
            .outcomes
            .iter()
            .any(|o| matches!(o, RowOutcome::Skip { note, .. } if note.code == "missing_fx_for_derivation")));
    }

    #[test]
    fn dividend_rows_counted_not_skipped_as_deferred() {
        // Ensures the old "dividend_deferred" code is no longer used when there are prior buys.
        let prepared = to_prepared(&report(vec![
            row(RowSpec {
                source_row_number: 2,
                trade_date: date(2026, 5, 1),
                raw_kind: "Köp",
                name: "Apple Inc",
                quantity: dec!(5),
                price: Some(dec!(200)),
                amount: Some(dec!(-9459)),
                transaction_currency: "SEK",
                brokerage: None,
                fx_rate: Some(dec!(9.45)),
                instrument_currency: "USD",
                isin: "US0378331005",
            }),
            row(RowSpec {
                source_row_number: 3,
                trade_date: date(2026, 5, 20),
                raw_kind: "Utdelning",
                name: "Apple Inc",
                quantity: dec!(0),
                price: None,
                amount: Some(dec!(120)),
                transaction_currency: "SEK",
                brokerage: None,
                fx_rate: Some(dec!(9.40)),
                instrument_currency: "USD",
                isin: "US0378331005",
            }),
        ]));

        assert_eq!(prepared.counts.dividends, 1);
        // Should be Mapped, not a dividend_deferred skip
        assert!(!prepared.outcomes.iter().any(|o| matches!(
            o,
            RowOutcome::Skip { note, .. } if note.code == "dividend_deferred"
        )));
        assert!(prepared.outcomes.iter().any(|o| matches!(
            o,
            RowOutcome::Mapped(m) if m.proposed.kind == TransactionKind::Dividend
        )));
    }

    #[test]
    fn unsupported_rows_are_skipped() {
        let prepared = to_prepared(&report(vec![row(RowSpec {
            source_row_number: 2,
            trade_date: date(2026, 5, 5),
            raw_kind: "Övrigt",
            name: "Cash",
            quantity: dec!(0),
            price: None,
            amount: Some(dec!(100)),
            transaction_currency: "SEK",
            brokerage: None,
            fx_rate: None,
            instrument_currency: "SEK",
            isin: "",
        })]));

        assert!(matches!(
            prepared.outcomes[0],
            RowOutcome::Skip {
                ref note,
                asset_key: None
            } if note.code == "unsupported_type"
        ));
    }

    #[test]
    fn fractional_rows_are_excluded_without_warning_noise() {
        let prepared = to_prepared(&report(vec![row(RowSpec {
            source_row_number: 2,
            trade_date: date(2026, 6, 7),
            raw_kind: "Köp",
            name: "Spiltan Aktiefond Investmentbolag",
            quantity: dec!(1.5),
            price: Some(dec!(250)),
            amount: Some(dec!(375)),
            transaction_currency: "SEK",
            brokerage: Some(dec!(0)),
            fx_rate: None,
            instrument_currency: "SEK",
            isin: "SE0004297927",
        })]));

        assert!(matches!(
            prepared.outcomes[0],
            RowOutcome::Excluded {
                ref note,
                asset_key: Some(_)
                , ..
            } if note.code == "fractional_fund_excluded"
        ));

        let plan = build_plan(&prepared, &PlanContext::default());
        assert_eq!(plan.counts.skipped, 1);
        assert_eq!(plan.counts.warnings, 0);
        let asset = plan.assets.first().expect("excluded fund asset");
        assert!(!asset.default_selected);
        assert_eq!(asset.warnings.len(), 0);
        assert_eq!(
            asset.skipped_reason.as_deref(),
            Some("Spiltan Aktiefond Investmentbolag has fractional quantity 1.5; fund transactions are excluded")
        );
    }

    #[test]
    fn split_rows_are_netted_to_one_delta() {
        let prepared = to_prepared(&report(vec![
            row(RowSpec {
                source_row_number: 10,
                trade_date: date(2026, 6, 2),
                raw_kind: "Split värdepapper",
                name: "ServiceNow",
                quantity: dec!(-14),
                price: None,
                amount: None,
                transaction_currency: "",
                brokerage: None,
                fx_rate: None,
                instrument_currency: "",
                isin: "US81762P1021",
            }),
            row(RowSpec {
                source_row_number: 11,
                trade_date: date(2026, 6, 2),
                raw_kind: "Split värdepapper",
                name: "ServiceNow",
                quantity: dec!(70),
                price: None,
                amount: None,
                transaction_currency: "",
                brokerage: None,
                fx_rate: None,
                instrument_currency: "",
                isin: "US81762P1021",
            }),
        ]));

        let splits: Vec<_> = prepared
            .outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                RowOutcome::Mapped(mapped) if mapped.proposed.kind == TransactionKind::Split => {
                    Some(mapped)
                }
                _ => None,
            })
            .collect();

        assert_eq!(splits.len(), 1);
        assert_eq!(splits[0].proposed.quantity, 56);
        assert_eq!(splits[0].instrument.currency, "");
    }

    #[test]
    fn split_without_same_file_buy_sell_keeps_empty_currency() {
        let prepared = to_prepared(&report(vec![row(RowSpec {
            source_row_number: 10,
            trade_date: date(2026, 6, 2),
            raw_kind: "Split värdepapper",
            name: "Orphan",
            quantity: dec!(5),
            price: None,
            amount: None,
            transaction_currency: "",
            brokerage: None,
            fx_rate: None,
            instrument_currency: "",
            isin: "XS9999999999",
        })]));

        let split = prepared
            .outcomes
            .iter()
            .find_map(|outcome| match outcome {
                RowOutcome::Mapped(mapped) if mapped.proposed.kind == TransactionKind::Split => {
                    Some(mapped)
                }
                _ => None,
            })
            .expect("split row");

        assert_eq!(split.instrument.currency, "");
        assert_eq!(split.instrument.name, "Orphan");
    }

    #[test]
    fn split_without_isin_is_skipped_without_creating_blank_identity() {
        let prepared = to_prepared(&report(vec![row(RowSpec {
            source_row_number: 10,
            trade_date: date(2026, 6, 2),
            raw_kind: "Split värdepapper",
            name: "No ISIN",
            quantity: dec!(5),
            price: None,
            amount: None,
            transaction_currency: "",
            brokerage: None,
            fx_rate: None,
            instrument_currency: "",
            isin: "",
        })]));

        assert!(matches!(
            prepared.outcomes[0],
            RowOutcome::Skip {
                ref note,
                asset_key: None
            } if note.code == "missing_isin"
        ));
    }

    #[test]
    fn settled_avanza_buy_sell_do_not_reconcile_with_sign_residuals() {
        let prepared = to_prepared(&report(vec![
            row(RowSpec {
                source_row_number: 2,
                trade_date: date(2026, 6, 1),
                raw_kind: "Köp",
                name: "ServiceNow",
                quantity: dec!(10),
                price: Some(dec!(900)),
                amount: Some(dec!(-94509)),
                transaction_currency: "SEK",
                brokerage: Some(dec!(9)),
                fx_rate: Some(dec!(10.50)),
                instrument_currency: "USD",
                isin: "US81762P1021",
            }),
            row(RowSpec {
                source_row_number: 3,
                trade_date: date(2026, 6, 2),
                raw_kind: "Sälj",
                name: "ServiceNow",
                quantity: dec!(-2),
                price: Some(dec!(950)),
                amount: Some(dec!(19941)),
                transaction_currency: "SEK",
                brokerage: Some(dec!(9)),
                fx_rate: Some(dec!(10.50)),
                instrument_currency: "USD",
                isin: "US81762P1021",
            }),
        ]));

        let plan = build_plan(&prepared, &PlanContext::default());

        assert!(
            !plan
                .warnings
                .iter()
                .any(|warning| warning.code == "reconciliation_residual"),
            "Avanza Belopp sign must be normalized before shared reconciliation"
        );
        assert_eq!(plan.counts.errors, 0);
    }

    #[test]
    fn settled_avanza_mismatched_amount_still_reconciles_as_warning() {
        let prepared = to_prepared(&report(vec![row(RowSpec {
            source_row_number: 2,
            trade_date: date(2026, 6, 1),
            raw_kind: "Köp",
            name: "ServiceNow",
            quantity: dec!(10),
            price: Some(dec!(900)),
            amount: Some(dec!(-1000)),
            transaction_currency: "SEK",
            brokerage: Some(dec!(9)),
            fx_rate: Some(dec!(10.50)),
            instrument_currency: "USD",
            isin: "US81762P1021",
        })]));

        let plan = build_plan(&prepared, &PlanContext::default());

        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.code == "reconciliation_residual"));
    }

    #[test]
    fn dividend_excludes_same_day_later_buy_in_eligible_quantity() {
        // A buy on the same date as the dividend, but appearing later in the
        // source file, must not inflate the dividend's eligible share count.
        let prepared = to_prepared(&report(vec![
            row(RowSpec {
                source_row_number: 2,
                trade_date: date(2026, 5, 1),
                raw_kind: "Köp",
                name: "Apple Inc",
                quantity: dec!(5),
                price: Some(dec!(200)),
                amount: Some(dec!(-9459)),
                transaction_currency: "SEK",
                brokerage: None,
                fx_rate: Some(dec!(9.45)),
                instrument_currency: "USD",
                isin: "US0378331005",
            }),
            row(RowSpec {
                source_row_number: 3,
                trade_date: date(2026, 5, 20),
                raw_kind: "Utdelning",
                name: "Apple Inc",
                quantity: dec!(0),
                price: None,
                amount: Some(dec!(120)),
                transaction_currency: "SEK",
                brokerage: None,
                fx_rate: Some(dec!(9.40)),
                instrument_currency: "USD",
                isin: "US0378331005",
            }),
            row(RowSpec {
                source_row_number: 4,
                trade_date: date(2026, 5, 20),
                raw_kind: "Köp",
                name: "Apple Inc",
                quantity: dec!(10),
                price: Some(dec!(210)),
                amount: Some(dec!(-19845)),
                transaction_currency: "SEK",
                brokerage: None,
                fx_rate: Some(dec!(9.45)),
                instrument_currency: "USD",
                isin: "US0378331005",
            }),
        ]));

        let rows = mapped(&prepared);
        let dividend = rows
            .iter()
            .find(|r| r.proposed.kind == TransactionKind::Dividend)
            .expect("dividend row");
        // Only the prior buy of 5 counts; the same-day later buy of 10 is excluded.
        assert_eq!(dividend.proposed.quantity, 5);
    }
}
