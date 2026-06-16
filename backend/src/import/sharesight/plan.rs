use std::collections::BTreeMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::domain::{self, LedgerTransaction};
use crate::import::sharesight::mapper::{map_row, InstrumentKey};
use crate::import::sharesight::parser::{ParsedKind, ParsedReport};

/// Reconciliation tolerance: warn when residual exceeds max(floor, rate * |value|).
const RECONCILIATION_FLOOR_SEK: Decimal = dec!(300);
const RECONCILIATION_RATE: Decimal = dec!(0.01);

/// Context the pure planner needs but cannot read itself (DB-derived).
#[derive(Clone, Debug, Default)]
pub struct PlanContext {
    /// Existing instruments keyed by `(exchange, symbol)` lowercased.
    pub existing_instruments: Vec<ExistingInstrument>,
    /// Stored ledger per existing instrument id.
    pub existing_ledgers: BTreeMap<i64, Vec<LedgerTransaction>>,
    /// Current `MAX(transactions.id)`, or 0 when the table is empty.
    pub max_existing_id: i64,
}

#[derive(Clone, Debug)]
pub struct ExistingInstrument {
    pub id: i64,
    pub exchange: String,
    pub symbol: String,
    pub currency: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPlan {
    pub counts: PlanCounts,
    pub new_instruments: Vec<InstrumentKey>,
    pub warnings: Vec<RowNote>,
    pub errors: Vec<RowNote>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanCounts {
    pub rows: usize,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub new_instruments: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowNote {
    pub row: Option<usize>,
    pub code: &'static str,
    pub message: String,
}

pub fn build_plan(report: &ParsedReport, ctx: &PlanContext) -> ImportPlan {
    let mut warnings: Vec<RowNote> = Vec::new();
    let mut errors: Vec<RowNote> = Vec::new();
    let mut new_instruments: Vec<InstrumentKey> = Vec::new();

    let mut ledgers: BTreeMap<(String, String), Vec<LedgerTransaction>> = BTreeMap::new();
    let mut seeded: std::collections::BTreeSet<(String, String)> = Default::default();

    let mut counts = PlanCounts {
        rows: report.rows.len(),
        ..Default::default()
    };

    for (i, parsed) in report.rows.iter().enumerate() {
        match parsed.kind {
            ParsedKind::Buy => counts.buys += 1,
            ParsedKind::Sell => counts.sells += 1,
            ParsedKind::Split => counts.splits += 1,
        }

        let mapped = match map_row(parsed) {
            Ok(mapped) => mapped,
            Err(err) => {
                errors.push(RowNote {
                    row: Some(err.row),
                    code: err.code,
                    message: err.message,
                });
                continue;
            }
        };

        let key = (
            mapped.instrument.exchange.to_lowercase(),
            mapped.instrument.symbol.to_lowercase(),
        );

        if let Some(existing) = ctx.existing_instruments.iter().find(|e| {
            e.exchange.eq_ignore_ascii_case(&mapped.instrument.exchange)
                && e.symbol.eq_ignore_ascii_case(&mapped.instrument.symbol)
        }) {
            if !existing
                .currency
                .eq_ignore_ascii_case(&mapped.instrument.currency)
            {
                errors.push(RowNote {
                    row: Some(parsed.source_row_number),
                    code: "currency_mismatch",
                    message: format!(
                        "row currency {} differs from stored {}",
                        mapped.instrument.currency, existing.currency
                    ),
                });
            }
            if seeded.insert(key.clone()) {
                ledgers.insert(
                    key.clone(),
                    ctx.existing_ledgers
                        .get(&existing.id)
                        .cloned()
                        .unwrap_or_default(),
                );
            }
        } else {
            seeded.insert(key.clone());
            ledgers.entry(key.clone()).or_default();
            if !new_instruments.contains(&mapped.instrument) {
                new_instruments.push(mapped.instrument.clone());
            }
        }

        let signed = match domain::validate(&mapped.proposed) {
            Ok(signed) => {
                let provisional_id = ctx.max_existing_id + 1 + i as i64;
                ledgers
                    .entry(key.clone())
                    .or_default()
                    .push(LedgerTransaction {
                        id: provisional_id,
                        trade_date: mapped.proposed.trade_date,
                        kind: mapped.proposed.kind,
                        quantity: signed,
                        price: mapped.proposed.price,
                        fx_rate_to_base: mapped.proposed.fx_rate_to_base,
                        brokerage_base: mapped.proposed.brokerage_base.unwrap_or(Decimal::ZERO),
                    });
                Some(signed)
            }
            Err(validation) => {
                errors.push(RowNote {
                    row: Some(parsed.source_row_number),
                    code: validation.code(),
                    message: validation.message().to_string(),
                });
                None
            }
        };

        if mapped.fx_warning {
            warnings.push(RowNote {
                row: Some(parsed.source_row_number),
                code: "missing_fx",
                message: "Exchange Rate blank or non-positive; SEK base unavailable".to_string(),
            });
        }

        if let Some(signed) = signed {
            if !matches!(mapped.kind, ParsedKind::Buy | ParsedKind::Sell) {
                continue;
            }
            if let (Some(fx), Some(price)) =
                (mapped.proposed.fx_rate_to_base, mapped.proposed.price)
            {
                let signed_native_gross = Decimal::from(signed) * price;
                let brokerage = mapped.proposed.brokerage_base.unwrap_or(Decimal::ZERO);
                let derived = signed_native_gross * fx + brokerage;
                let residual = (mapped.source_value - derived).abs();
                let threshold = reconciliation_threshold(mapped.source_value);
                if residual > threshold {
                    warnings.push(RowNote {
                        row: Some(parsed.source_row_number),
                        code: "reconciliation_residual",
                        message: format!(
                            "derived SEK off by {} (> {})",
                            residual.round_dp(2),
                            threshold.round_dp(2)
                        ),
                    });
                }
            }
        }
    }

    // Flag only byte-equivalent rows (same instrument, date, direction, and
    // identical quantity/price/value) as likely duplicate export lines.
    // Distinct same-day fills differ in quantity or price and stay silent.
    type DuplicateKey = (
        String,
        String,
        String,
        &'static str,
        Decimal,
        Decimal,
        Decimal,
    );
    let mut groups: BTreeMap<DuplicateKey, Vec<usize>> = BTreeMap::new();
    for parsed in &report.rows {
        groups
            .entry((
                parsed.market.to_lowercase(),
                parsed.code.to_lowercase(),
                parsed.trade_date.to_string(),
                parsed.kind.as_str(),
                parsed.quantity,
                parsed.price,
                parsed.value,
            ))
            .or_default()
            .push(parsed.source_row_number);
    }
    for rows in groups.values().filter(|rows| rows.len() > 1) {
        warnings.push(RowNote {
            row: rows.first().copied(),
            code: "duplicate_row",
            message: format!("identical row appears {} times", rows.len()),
        });
    }

    for ledger in ledgers.values_mut() {
        ledger.sort_by_key(|tx| (tx.trade_date, tx.id));
        if let Err(ledger_error) = domain::derive_position(ledger) {
            let id = ledger_error.transaction_id();
            let row = if id > ctx.max_existing_id {
                report
                    .rows
                    .get((id - ctx.max_existing_id - 1) as usize)
                    .map(|r| r.source_row_number)
            } else {
                None
            };
            errors.push(RowNote {
                row,
                code: ledger_error.code(),
                message: ledger_message(ledger_error),
            });
        }
    }

    counts.new_instruments = new_instruments.len();
    counts.warnings = warnings.len();
    counts.errors = errors.len();

    ImportPlan {
        counts,
        new_instruments,
        warnings,
        errors,
    }
}

fn reconciliation_threshold(source_value: Decimal) -> Decimal {
    let proportional = RECONCILIATION_RATE * source_value.abs();
    if proportional > RECONCILIATION_FLOOR_SEK {
        proportional
    } else {
        RECONCILIATION_FLOOR_SEK
    }
}

fn ledger_message(error: crate::domain::LedgerError) -> String {
    use crate::domain::LedgerError::*;
    match error {
        SellExceedsPosition {
            available,
            requested,
            ..
        } => format!("Sell of {requested} exceeds available position of {available}."),
        SplitWithoutPosition { .. } => "A split requires an existing position.".to_string(),
        SplitDrivesNonPositive {
            resulting_quantity, ..
        } => format!("Split would drive the position to {resulting_quantity}."),
        BuyMissingPrice { .. } => "A buy requires a native price.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_plan, ExistingInstrument, PlanContext};
    use crate::domain::{LedgerTransaction, TransactionKind};
    use crate::import::sharesight::parser::parse_report;
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use std::collections::BTreeMap;

    const FRESH: &str = concat!(
        "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
        "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
        "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"9,60\",SEK,\"0,100000\",\"1259,60\",All Trades,\n",
        "NASDAQ,MSFT,Microsoft,Sell,13/06/2026,\u{2212}4,\"12,60\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"\u{2212}504,00\",All Trades,\n",
    );

    fn plan_for(csv: &str, ctx: PlanContext) -> super::ImportPlan {
        build_plan(&parse_report(csv.as_bytes()).expect("parses"), &ctx)
    }

    #[test]
    fn fresh_portfolio_counts_new_instrument_and_no_errors() {
        let plan = plan_for(FRESH, PlanContext::default());
        assert_eq!(plan.counts.rows, 2);
        assert_eq!(plan.counts.buys, 1);
        assert_eq!(plan.counts.sells, 1);
        assert_eq!(plan.counts.new_instruments, 1);
        assert_eq!(plan.counts.errors, 0);
    }

    #[test]
    fn oversell_is_a_hard_error() {
        let oversell = FRESH.replace("Buy,12/06/2026,10", "Buy,12/06/2026,2");
        let plan = plan_for(&oversell, PlanContext::default());
        assert!(plan
            .errors
            .iter()
            .any(|e| e.code == "sell_exceeds_position"));
    }

    #[test]
    fn currency_mismatch_against_existing_instrument_is_an_error() {
        let ctx = PlanContext {
            existing_instruments: vec![ExistingInstrument {
                id: 1,
                exchange: "NASDAQ".into(),
                symbol: "MSFT".into(),
                currency: "EUR".into(),
            }],
            existing_ledgers: BTreeMap::new(),
            max_existing_id: 0,
        };
        let plan = plan_for(FRESH, ctx);
        assert!(plan.errors.iter().any(|e| e.code == "currency_mismatch"));
        assert_eq!(plan.counts.new_instruments, 0);
    }

    #[test]
    fn provisional_ids_sort_imported_rows_after_existing_same_day_rows() {
        let existing = LedgerTransaction {
            id: 5,
            trade_date: NaiveDate::from_ymd_opt(2026, 6, 12).unwrap(),
            kind: TransactionKind::Buy,
            quantity: 4,
            price: Some(dec!(10)),
            fx_rate_to_base: Some(dec!(1)),
            brokerage_base: dec!(0),
        };
        let mut ledgers = BTreeMap::new();
        ledgers.insert(1, vec![existing]);
        let csv = concat!(
            "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
            "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
            "NASDAQ,MSFT,Microsoft,Sell,12/06/2026,\u{2212}4,\"10,00\",USD,\"0,00\",\"0,00\",SEK,\"1,000000\",\"\u{2212}40,00\",All Trades,\n",
        );
        let ctx = PlanContext {
            existing_instruments: vec![ExistingInstrument {
                id: 1,
                exchange: "NASDAQ".into(),
                symbol: "MSFT".into(),
                currency: "USD".into(),
            }],
            existing_ledgers: ledgers,
            max_existing_id: 5,
        };
        let plan = plan_for(csv, ctx);
        assert_eq!(
            plan.counts.errors, 0,
            "imported sell after existing buy must validate"
        );
    }

    #[test]
    fn reconciliation_residual_over_threshold_warns() {
        let off = FRESH.replace("\"1259,60\"", "\"9999,00\"");
        let plan = plan_for(&off, PlanContext::default());
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.code == "reconciliation_residual"));
    }

    #[test]
    fn identical_rows_warn_as_duplicate() {
        let csv = concat!(
            "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
            "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
            "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"1250,00\",All Trades,\n",
            "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"1250,00\",All Trades,\n",
        );
        let plan = plan_for(csv, PlanContext::default());
        assert!(plan
            .warnings
            .iter()
            .any(|w| { w.code == "duplicate_row" && w.row == Some(3) && w.message.contains("2") }));
    }

    #[test]
    fn distinct_same_day_fills_do_not_warn() {
        // Two same-day same-direction buys at different quantity/price are
        // legitimate partial fills, not duplicates: no warning expected.
        let csv = concat!(
            "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
            "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
            "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"1250,00\",All Trades,\n",
            "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,2,\"12,55\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"251,00\",All Trades,\n",
        );
        let plan = plan_for(csv, PlanContext::default());
        assert!(
            !plan.warnings.iter().any(|w| w.code == "duplicate_row"),
            "distinct fills must not be flagged as duplicates"
        );
    }
}
