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
            dividend_per_share: None,
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
            price: None,
            dividend_per_share: Some(native_price_per_share),
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
            dividend_per_share: None,
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
mod tests;
