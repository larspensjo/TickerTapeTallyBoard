use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::domain::{self, LedgerTransaction, TransactionKind};
use crate::import::core::outcome::{InstrumentKey, MappedRow, PreparedImport, RowNote, RowOutcome};

const RECONCILIATION_FLOOR_SEK: Decimal = dec!(300);
const RECONCILIATION_RATE: Decimal = dec!(0.01);

/// DB-derived context the pure planner needs.
#[derive(Clone, Debug, Default)]
pub struct PlanContext {
    pub existing_instruments: Vec<ExistingInstrument>,
    pub existing_ledgers: BTreeMap<i64, Vec<LedgerTransaction>>,
    pub max_existing_id: i64,
}

#[derive(Clone, Debug)]
pub struct ExistingInstrument {
    pub id: i64,
    pub exchange: String,
    pub symbol: String,
    pub currency: String,
    pub isin: Option<String>,
}

impl ExistingInstrument {
    fn matches(&self, key: &InstrumentKey) -> bool {
        match (&self.isin, &key.isin) {
            (Some(existing), Some(requested)) => existing.eq_ignore_ascii_case(requested),
            _ => {
                self.exchange.eq_ignore_ascii_case(&key.exchange)
                    && self.symbol.eq_ignore_ascii_case(&key.symbol)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPlan {
    pub counts: PlanCounts,
    pub new_instruments: Vec<InstrumentKey>,
    pub assets: Vec<AssetGroup>,
    pub warnings: Vec<RowNote>,
    pub errors: Vec<RowNote>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanCounts {
    pub rows: usize,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
    pub new_instruments: usize,
    pub skipped: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetGroup {
    pub asset_key: String,
    pub name: String,
    pub currency: String,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
    pub default_selected: bool,
    pub skipped_reason: Option<String>,
    pub warnings: Vec<RowNote>,
    pub errors: Vec<RowNote>,
    pub is_new_instrument: bool,
}

pub fn build_plan(prepared: &PreparedImport, ctx: &PlanContext) -> ImportPlan {
    let mut warnings: Vec<RowNote> = Vec::new();
    let mut errors: Vec<RowNote> = Vec::new();
    let mut new_instruments: Vec<InstrumentKey> = Vec::new();
    let mut mapped_rows: Vec<MappedRow> = Vec::new();

    let mut asset_order: Vec<String> = Vec::new();
    let mut assets: BTreeMap<String, AssetGroup> = BTreeMap::new();
    let mut ledgers: BTreeMap<String, Vec<LedgerTransaction>> = BTreeMap::new();
    let mut seeded: BTreeSet<String> = BTreeSet::new();
    let mut skipped = 0usize;

    for (index, outcome) in prepared.outcomes.iter().enumerate() {
        match outcome {
            RowOutcome::Mapped(mapped) => {
                mapped_rows.push(mapped.clone());
                process_mapped(
                    index,
                    mapped,
                    ctx,
                    &mut ledgers,
                    &mut seeded,
                    &mut new_instruments,
                    &mut warnings,
                    &mut errors,
                    &mut asset_order,
                    &mut assets,
                );
            }
            RowOutcome::Skip { asset_key, note } => {
                skipped += 1;
                warnings.push(note.clone());
                if let Some(key) = asset_key {
                    let group = asset_group_mut(&mut assets, &mut asset_order, key, None, None);
                    group.warnings.push(note.clone());
                }
            }
            RowOutcome::Error { asset_key, note } => {
                errors.push(note.clone());
                if let Some(key) = asset_key {
                    let group = asset_group_mut(&mut assets, &mut asset_order, key, None, None);
                    group.errors.push(note.clone());
                }
            }
        }
    }

    duplicate_row_warnings(&mapped_rows, &mut warnings);
    ledger_errors(&mut ledgers, ctx, prepared, &mut errors);
    attach_asset_notes(&mut assets, &warnings, &errors, &mapped_rows);

    for group in assets.values_mut() {
        if group.buys + group.sells + group.splits == 0
            && (!group.warnings.is_empty() || !group.errors.is_empty())
        {
            group.skipped_reason = Some("no writable rows (all skipped)".to_string());
            group.default_selected = false;
        }
    }

    let counts = PlanCounts {
        rows: prepared.counts.rows,
        buys: prepared.counts.buys,
        sells: prepared.counts.sells,
        splits: prepared.counts.splits,
        dividends: prepared.counts.dividends,
        new_instruments: new_instruments.len(),
        skipped,
        warnings: warnings.len(),
        errors: errors.len(),
    };

    let assets = asset_order
        .into_iter()
        .filter_map(|key| assets.remove(&key))
        .collect();

    ImportPlan {
        counts,
        new_instruments,
        assets,
        warnings,
        errors,
    }
}

/// Remove every outcome for any asset whose key is in `exclude`.
///
/// Mapped rows are filtered by their instrument asset key, while Skip/Error
/// outcomes are filtered by their carried asset key when present. Rows without
/// an asset key stay in place because the caller cannot reliably exclude them.
pub fn exclude_assets(prepared: &PreparedImport, exclude: &BTreeSet<String>) -> PreparedImport {
    let outcomes = prepared
        .outcomes
        .iter()
        .filter(|outcome| match outcome {
            RowOutcome::Mapped(mapped) => !exclude.contains(&mapped.instrument.asset_key()),
            RowOutcome::Skip { asset_key, .. } | RowOutcome::Error { asset_key, .. } => {
                asset_key.as_ref().is_none_or(|key| !exclude.contains(key))
            }
        })
        .cloned()
        .collect();

    PreparedImport {
        header: prepared.header.clone(),
        counts: prepared.counts,
        outcomes,
    }
}

/// Asset keys named by the import, including keys that only appear on a Skip or
/// Error outcome. This lets commit validate `exclude` against the full set of
/// assets the file knows about.
pub fn known_asset_keys(prepared: &PreparedImport) -> BTreeSet<String> {
    prepared
        .outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            RowOutcome::Mapped(mapped) => Some(mapped.instrument.asset_key()),
            RowOutcome::Skip { asset_key, .. } | RowOutcome::Error { asset_key, .. } => {
                asset_key.clone()
            }
        })
        .collect()
}

fn asset_group_mut<'a>(
    assets: &'a mut BTreeMap<String, AssetGroup>,
    asset_order: &mut Vec<String>,
    key: &str,
    name: Option<&str>,
    currency: Option<&str>,
) -> &'a mut AssetGroup {
    let group = assets.entry(key.to_string()).or_insert_with(|| {
        asset_order.push(key.to_string());
        AssetGroup {
            asset_key: key.to_string(),
            name: key.to_string(),
            currency: String::new(),
            buys: 0,
            sells: 0,
            splits: 0,
            dividends: 0,
            default_selected: true,
            skipped_reason: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            is_new_instrument: false,
        }
    });

    if let Some(name) = name.filter(|value| !value.trim().is_empty()) {
        group.name = name.to_string();
    }
    if let Some(currency) = currency.filter(|value| !value.trim().is_empty()) {
        group.currency = currency.to_string();
    }

    group
}

#[allow(clippy::too_many_arguments)]
fn process_mapped(
    index: usize,
    mapped: &MappedRow,
    ctx: &PlanContext,
    ledgers: &mut BTreeMap<String, Vec<LedgerTransaction>>,
    seeded: &mut BTreeSet<String>,
    new_instruments: &mut Vec<InstrumentKey>,
    warnings: &mut Vec<RowNote>,
    errors: &mut Vec<RowNote>,
    asset_order: &mut Vec<String>,
    assets: &mut BTreeMap<String, AssetGroup>,
) {
    let key = mapped.instrument.asset_key();
    let group = asset_group_mut(
        assets,
        asset_order,
        &key,
        Some(&mapped.instrument.name),
        Some(&mapped.instrument.currency),
    );

    match mapped.proposed.kind {
        TransactionKind::Buy => group.buys += 1,
        TransactionKind::Sell => group.sells += 1,
        TransactionKind::Split => group.splits += 1,
        TransactionKind::Dividend => group.dividends += 1,
    }

    if let Some(existing) = ctx
        .existing_instruments
        .iter()
        .find(|instrument| instrument.matches(&mapped.instrument))
    {
        if !existing
            .currency
            .eq_ignore_ascii_case(&mapped.instrument.currency)
            && !mapped.instrument.currency.trim().is_empty()
        {
            errors.push(RowNote {
                row: Some(mapped.source_row_number),
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
        if mapped.proposed.kind != TransactionKind::Split
            && !new_instruments.contains(&mapped.instrument)
        {
            new_instruments.push(mapped.instrument.clone());
            group.is_new_instrument = true;
        }
    }

    match domain::validate(&mapped.proposed) {
        Ok(signed) => {
            let provisional_id = ctx.max_existing_id + 1 + index as i64;
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
            if mapped.fx_warning {
                warnings.push(RowNote {
                    row: Some(mapped.source_row_number),
                    code: "missing_fx",
                    message: "Exchange Rate blank or non-positive; SEK base unavailable"
                        .to_string(),
                });
            }
            reconciliation_warning(signed, mapped, warnings);
        }
        Err(validation) => errors.push(RowNote {
            row: Some(mapped.source_row_number),
            code: validation.code(),
            message: validation.message().to_string(),
        }),
    }
}

fn reconciliation_warning(signed: i64, mapped: &MappedRow, warnings: &mut Vec<RowNote>) {
    if !matches!(
        mapped.proposed.kind,
        TransactionKind::Buy | TransactionKind::Sell
    ) {
        return;
    }
    let (Some(fx), Some(price), Some(source_value)) = (
        mapped.proposed.fx_rate_to_base,
        mapped.proposed.price,
        mapped.source_value,
    ) else {
        return;
    };

    let signed_native_gross = Decimal::from(signed) * price;
    let brokerage = mapped.proposed.brokerage_base.unwrap_or(Decimal::ZERO);
    let derived = signed_native_gross * fx + brokerage;
    let residual = (source_value - derived).abs();
    let threshold = reconciliation_threshold(source_value);
    if residual > threshold {
        warnings.push(RowNote {
            row: Some(mapped.source_row_number),
            code: "reconciliation_residual",
            message: format!(
                "derived SEK off by {} (> {})",
                residual.round_dp(2),
                threshold.round_dp(2)
            ),
        });
    }
}

fn reconciliation_threshold(source_value: Decimal) -> Decimal {
    let proportional = RECONCILIATION_RATE * source_value.abs();
    proportional.max(RECONCILIATION_FLOOR_SEK)
}

fn duplicate_row_warnings(mapped: &[MappedRow], warnings: &mut Vec<RowNote>) {
    type DuplicateKey = (
        String,
        &'static str,
        String,
        i64,
        Option<Decimal>,
        Option<Decimal>,
    );

    let mut groups: BTreeMap<DuplicateKey, Vec<usize>> = BTreeMap::new();
    for row in mapped {
        groups
            .entry((
                row.instrument.asset_key(),
                row.proposed.kind.as_db_str(),
                row.proposed.trade_date.to_string(),
                row.proposed.quantity,
                row.proposed.price,
                row.source_value,
            ))
            .or_default()
            .push(row.source_row_number);
    }

    for rows in groups.values().filter(|rows| rows.len() > 1) {
        warnings.push(RowNote {
            row: rows.first().copied(),
            code: "duplicate_row",
            message: format!("identical row appears {} times", rows.len()),
        });
    }
}

fn ledger_errors(
    ledgers: &mut BTreeMap<String, Vec<LedgerTransaction>>,
    ctx: &PlanContext,
    prepared: &PreparedImport,
    errors: &mut Vec<RowNote>,
) {
    for ledger in ledgers.values_mut() {
        ledger.sort_by_key(|tx| (tx.trade_date, tx.id));
        if let Err(ledger_error) = domain::derive_position(ledger) {
            let id = ledger_error.transaction_id();
            let row = if id > ctx.max_existing_id {
                prepared
                    .outcomes
                    .get((id - ctx.max_existing_id - 1) as usize)
                    .and_then(|outcome| match outcome {
                        RowOutcome::Mapped(mapped) => Some(mapped.source_row_number),
                        _ => None,
                    })
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
}

fn attach_asset_notes(
    assets: &mut BTreeMap<String, AssetGroup>,
    warnings: &[RowNote],
    errors: &[RowNote],
    mapped: &[MappedRow],
) {
    let row_to_asset: BTreeMap<usize, String> = mapped
        .iter()
        .map(|mapped| (mapped.source_row_number, mapped.instrument.asset_key()))
        .collect();

    for note in warnings {
        if let Some(asset_key) = note.row.and_then(|row| row_to_asset.get(&row)) {
            if let Some(group) = assets.get_mut(asset_key) {
                group.warnings.push(note.clone());
            }
        }
    }

    for note in errors {
        if let Some(asset_key) = note.row.and_then(|row| row_to_asset.get(&row)) {
            if let Some(group) = assets.get_mut(asset_key) {
                group.errors.push(note.clone());
            }
        }
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
        SellMissingPrice { .. } => "A sell requires a native price.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_plan, ExistingInstrument, PlanContext};
    use crate::domain::{LedgerTransaction, TransactionKind};
    use crate::import::core::outcome::RowOutcome;
    use crate::import::sharesight::adapter::to_prepared;
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
        let report = parse_report(csv.as_bytes()).expect("parses");
        let prepared = to_prepared(&report);
        build_plan(&prepared, &ctx)
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
                isin: None,
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
                isin: None,
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

    #[test]
    fn sharesight_adapter_turns_report_into_prepared_import() {
        let report = parse_report(FRESH.as_bytes()).expect("parse");
        let prepared = to_prepared(&report);
        assert_eq!(prepared.counts.rows, 2);
        assert!(prepared
            .outcomes
            .iter()
            .all(|outcome| matches!(outcome, RowOutcome::Mapped(_))));
        assert_eq!(prepared.counts.dividends, 0);
        assert_eq!(
            prepared.header.title,
            "All Trades Report between 2025-06-12 and 2026-06-12"
        );
    }
}
