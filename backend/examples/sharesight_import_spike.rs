use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use ticker_tape_tally_board_backend::import::sharesight::parser::{
    parse_report, sanitize_report_title, ParsedKind, ParsedReport, ParsedRow,
};

const DEFAULT_CSV_PATH: &str = "../docs/AllTradesReport_2026-06-12.csv";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse(env::args().skip(1))?;
    let bytes = fs::read(&args.csv_path)?;
    let report = parse_report(&bytes)?;
    let summary = summarize(&report, args.split_current_position);
    print!("{summary}");
    Ok(())
}

#[derive(Debug)]
struct Args {
    csv_path: PathBuf,
    split_current_position: Option<Decimal>,
}

impl Args {
    fn parse<I>(mut args: I) -> Result<Self, String>
    where
        I: Iterator<Item = String>,
    {
        let mut csv_path = PathBuf::from(DEFAULT_CSV_PATH);
        let mut split_current_position = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--csv" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--csv requires a path".to_string())?;
                    csv_path = PathBuf::from(value);
                }
                "--split-current-position" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--split-current-position requires a decimal".to_string())?;
                    split_current_position = Some(parse_decimal_arg(&value)?);
                }
                "--help" | "-h" => {
                    return Err(format!(
                        "usage: cargo run --example sharesight_import_spike -- [--csv {DEFAULT_CSV_PATH}] [--split-current-position <decimal>]"
                    ));
                }
                _ => return Err(format!("unknown argument: {arg}")),
            }
        }

        Ok(Self {
            csv_path,
            split_current_position,
        })
    }
}

fn summarize(report: &ParsedReport, split_current_position: Option<Decimal>) -> String {
    let mut output = String::new();
    let trade_count = report.rows.len();
    let markets = report
        .rows
        .iter()
        .map(|trade| trade.market.as_str())
        .collect::<BTreeSet<_>>();
    let instrument_currencies = report
        .rows
        .iter()
        .map(|trade| trade.instrument_currency.as_str())
        .collect::<BTreeSet<_>>();
    let source_columns = report
        .rows
        .iter()
        .map(|trade| trade.source_column.as_str())
        .collect::<BTreeSet<_>>();
    let brokerage_currencies = report
        .rows
        .iter()
        .map(|trade| trade.brokerage_currency.as_str())
        .collect::<BTreeSet<_>>();
    let type_counts = type_counts(&report.rows);
    let instrument_keys = instrument_key_map(&report.rows);
    let ambiguous_keys = instrument_keys
        .values()
        .filter(|identity_set| identity_set.len() > 1)
        .count();
    let duplicate_rows = duplicate_count(&report.rows);
    let partial_fill_summary = partial_fill_summary(&report.rows);
    let value_sign_mismatches = report
        .rows
        .iter()
        .filter(|trade| !value_sign_is_consistent(trade))
        .count();
    let quantity_sign_mismatches = report
        .rows
        .iter()
        .filter(|trade| !quantity_sign_is_consistent(trade))
        .count();
    let nonzero_cost_base_rows = report
        .rows
        .iter()
        .filter(|trade| !trade.cost_base_per_share_sek.is_zero())
        .count();
    let blank_comments = report
        .rows
        .iter()
        .filter(|trade| trade.comments.trim().is_empty())
        .count();
    let fx_summary = fx_model_summary(&report.rows);
    let split_summary = split_summary(&report.rows, split_current_position);
    let first_row = report
        .rows
        .iter()
        .map(|trade| trade.source_row_number)
        .min()
        .unwrap_or_default();
    let last_row = report
        .rows
        .iter()
        .map(|trade| trade.source_row_number)
        .max()
        .unwrap_or_default();

    push_line(&mut output, "# Sharesight Import Spike Summary");
    push_line(&mut output, "");
    push_line(&mut output, "## Scope");
    push_line(
        &mut output,
        &format!(
            "- Report title recognized: {}",
            sanitize_report_title(&report.metadata.title)
        ),
    );
    push_line(
        &mut output,
        &format!(
            "- Report date range: {} to {}.",
            report.metadata.date_from, report.metadata.date_to
        ),
    );
    push_line(
        &mut output,
        &format!(
            "- Header row located at CSV row {}.",
            report.header_row_number
        ),
    );
    push_line(
        &mut output,
        &format!("- Parsed {trade_count} data rows from CSV rows {first_row}-{last_row}."),
    );
    push_line(&mut output, "");
    push_line(&mut output, "## Aggregate Shape");
    push_line(
        &mut output,
        &format!("- Transaction types: {}.", format_type_counts(&type_counts)),
    );
    push_line(
        &mut output,
        &format!(
            "- Markets: {}.",
            markets.iter().copied().collect::<Vec<_>>().join(", ")
        ),
    );
    push_line(
        &mut output,
        &format!(
            "- Instrument currencies: {}.",
            instrument_currencies
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );
    push_line(
        &mut output,
        &format!(
            "- Unique Market+Code instruments: {}.",
            instrument_keys.len()
        ),
    );
    push_line(
        &mut output,
        &format!("- Market+Code identity conflicts: {ambiguous_keys}."),
    );
    push_line(
        &mut output,
        &format!(
            "- Brokerage currencies: {}.",
            brokerage_currencies
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );
    push_line(
        &mut output,
        &format!(
            "- Source/report column values: {}.",
            source_columns
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );
    push_line(&mut output, "");
    push_line(&mut output, "## Validation Findings");
    push_line(
        &mut output,
        "- Unknown transaction types: 0; all rows parsed as Buy, Sell, or Split.",
    );
    push_line(
        &mut output,
        &format!("- Value sign mismatches against type rules: {value_sign_mismatches}."),
    );
    push_line(
        &mut output,
        &format!("- Quantity sign mismatches against type rules: {quantity_sign_mismatches}."),
    );
    push_line(
        &mut output,
        &format!("- Rows with non-zero cost base per share in SEK: {nonzero_cost_base_rows}."),
    );
    push_line(&mut output, &format!("- Blank comments: {blank_comments}."));
    push_line(
        &mut output,
        &format!("- Duplicate full rows: {duplicate_rows}."),
    );
    push_line(
        &mut output,
        &format!(
            "- Same-day same-instrument same-type groups with multiple rows: {}; rows involved: {}.",
            partial_fill_summary.group_count, partial_fill_summary.row_count
        ),
    );
    push_line(&mut output, "");
    push_line(&mut output, "## FX And Value Interpretation");
    push_line(
        &mut output,
        &format!(
            "- FX rows evaluated: {} non-split rows with non-zero quantity, price, and exchange rate.",
            fx_summary.row_count
        ),
    );
    push_line(
        &mut output,
        "- Sharesight Exchange Rate is interpreted as instrument currency per SEK; the import stores the inverse as SEK per instrument currency.",
    );
    push_line(
        &mut output,
        "- Residuals under the selected interpretation are expected from exported exchange-rate rounding and Avanza's buy/sell FX spread.",
    );
    push_line(&mut output, "");
    push_line(&mut output, "### Selected Interpretation");
    push_model_line(&mut output, &fx_summary.best_model, fx_summary.row_count);
    push_line(&mut output, "");
    push_line(&mut output, "### Candidate Cross-Checks");
    push_line(
        &mut output,
        "These alternatives are printed only to show why they were rejected; large residuals here are diagnostic evidence, not import errors.",
    );
    for model in fx_summary
        .models
        .iter()
        .filter(|model| model.label != fx_summary.best_model.label)
    {
        push_line(
            &mut output,
            &format!(
                "- rejected, {}: average absolute residual {} SEK; max absolute residual {} SEK.",
                model.label,
                model.average_abs_residual(fx_summary.row_count),
                model.max_abs_residual
            ),
        );
    }
    push_line(
        &mut output,
        &format!(
            "- Best observed interpretation: {}.",
            fx_summary.best_model.label
        ),
    );
    push_line(&mut output, "");
    push_line(&mut output, "## Split Handling");
    for line in split_summary.lines {
        push_line(&mut output, &format!("- {line}"));
    }
    push_line(&mut output, "");
    push_line(&mut output, "## Privacy");
    push_line(
        &mut output,
        "- This summary intentionally omits row-level values, position sizes, instrument names, and instrument codes.",
    );

    output
}

#[derive(Debug)]
struct FxSummary {
    row_count: usize,
    models: Vec<FxModelResult>,
    best_model: FxModelResult,
}

#[derive(Clone, Debug)]
struct FxModelResult {
    label: &'static str,
    max_abs_residual: Decimal,
    total_abs_residual: Decimal,
}

fn fx_model_summary(trades: &[ParsedRow]) -> FxSummary {
    let mut models = vec![
        FxModelResult::new("Value = native gross / exchange rate"),
        FxModelResult::new("Value = native gross / exchange rate + brokerage"),
        FxModelResult::new("Value = native gross / exchange rate - brokerage"),
        FxModelResult::new("Value = native gross * exchange rate"),
        FxModelResult::new("Value = native gross * exchange rate + brokerage"),
        FxModelResult::new("Value = native gross * exchange rate - brokerage"),
    ];
    let mut row_count = 0;

    for trade in trades.iter().filter(|trade| {
        trade.kind != ParsedKind::Split
            && !trade.quantity.is_zero()
            && !trade.price.is_zero()
            && trade.exchange_rate.is_some_and(|rate| !rate.is_zero())
    }) {
        row_count += 1;
        let exchange_rate = trade
            .exchange_rate
            .expect("filtered to present exchange rate");
        let native_gross = trade.quantity * trade.price;
        let divide_base = native_gross / exchange_rate;
        let multiply_base = native_gross * exchange_rate;
        let expected_values = [
            divide_base,
            divide_base + trade.brokerage,
            divide_base - trade.brokerage,
            multiply_base,
            multiply_base + trade.brokerage,
            multiply_base - trade.brokerage,
        ];

        for (model, expected) in models.iter_mut().zip(expected_values) {
            model.record(trade.value - expected);
        }
    }

    let best_model = models
        .iter()
        .min_by(|left, right| left.total_abs_residual.cmp(&right.total_abs_residual))
        .cloned()
        .unwrap_or_else(|| FxModelResult::new("no FX model rows were available"));

    FxSummary {
        row_count,
        models,
        best_model,
    }
}

fn push_model_line(output: &mut String, model: &FxModelResult, row_count: usize) {
    push_line(
        output,
        &format!(
            "- {}: average absolute residual {} SEK; max absolute residual {} SEK.",
            model.label,
            model.average_abs_residual(row_count),
            model.max_abs_residual
        ),
    );
}

impl FxModelResult {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            max_abs_residual: Decimal::ZERO,
            total_abs_residual: Decimal::ZERO,
        }
    }

    fn record(&mut self, residual: Decimal) {
        let residual = residual.round_dp(2).abs();
        self.max_abs_residual = self.max_abs_residual.max(residual);
        self.total_abs_residual += residual;
    }

    fn average_abs_residual(&self, row_count: usize) -> Decimal {
        if row_count == 0 {
            return Decimal::ZERO;
        }

        (self.total_abs_residual / Decimal::from(row_count)).round_dp(2)
    }
}

#[derive(Debug)]
struct SplitSummary {
    lines: Vec<String>,
}

fn split_summary(trades: &[ParsedRow], current_position: Option<Decimal>) -> SplitSummary {
    let split_trades = trades
        .iter()
        .filter(|trade| trade.kind == ParsedKind::Split)
        .collect::<Vec<_>>();

    if split_trades.is_empty() {
        return SplitSummary {
            lines: vec!["No split rows were found.".to_string()],
        };
    }

    let mut lines = Vec::new();
    lines.push(format!("Split rows found: {}.", split_trades.len()));

    for split in split_trades {
        let key = (&split.market, &split.code);
        let position_before = trades
            .iter()
            .filter(|trade| {
                (&trade.market, &trade.code) == key && trade.trade_date < split.trade_date
            })
            .map(|trade| trade.quantity)
            .sum::<Decimal>();
        let final_quantity = trades
            .iter()
            .filter(|trade| (&trade.market, &trade.code) == key)
            .map(|trade| trade.quantity)
            .sum::<Decimal>();

        if position_before.is_zero() {
            lines.push("Cannot derive split ratio because pre-split position is zero.".to_string());
        } else {
            let ratio = ((position_before + split.quantity) / position_before).round_dp(8);
            lines.push(format!(
                "Delta-semantics split ratio derived from quantities: {}.",
                format_ratio(ratio)
            ));
        }

        match current_position {
            Some(current_position) if final_quantity == current_position => {
                lines.push(
                    "Provided current position matches summed quantity; delta semantics confirmed."
                        .to_string(),
                );
            }
            Some(current_position) if final_quantity - split.quantity == current_position => {
                lines.push(
                    "Provided current position matches sum minus split row; resulting-shares semantics is more likely."
                        .to_string(),
                );
            }
            Some(_) => {
                lines.push(
                    "Provided current position does not match either checked split interpretation."
                        .to_string(),
                );
            }
            None => {
                lines.push(
                    "Current Sharesight position was not provided; split invariant remains pending."
                        .to_string(),
                );
            }
        }
    }

    SplitSummary { lines }
}

fn format_ratio(ratio: Decimal) -> String {
    for denominator in 1..=20 {
        let denominator_decimal = Decimal::from(denominator);
        let numerator = ratio * denominator_decimal;
        if numerator.fract().is_zero() {
            return format!("{}/{}", numerator, denominator);
        }
    }

    ratio.to_string()
}

#[derive(Debug)]
struct PartialFillSummary {
    group_count: usize,
    row_count: usize,
}

fn partial_fill_summary(trades: &[ParsedRow]) -> PartialFillSummary {
    let mut groups = BTreeMap::<(&str, &str, NaiveDate, &'static str), usize>::new();

    for trade in trades {
        *groups
            .entry((
                trade.market.as_str(),
                trade.code.as_str(),
                trade.trade_date,
                trade.kind.as_str(),
            ))
            .or_default() += 1;
    }

    let multi_row_groups = groups
        .values()
        .filter(|count| **count > 1)
        .copied()
        .collect::<Vec<_>>();

    PartialFillSummary {
        group_count: multi_row_groups.len(),
        row_count: multi_row_groups.iter().sum(),
    }
}

fn duplicate_count(trades: &[ParsedRow]) -> usize {
    let mut counts = BTreeMap::<String, usize>::new();

    for trade in trades {
        *counts.entry(raw_signature(trade)).or_default() += 1;
    }

    counts
        .values()
        .filter(|count| **count > 1)
        .map(|count| count - 1)
        .sum()
}

fn raw_signature(trade: &ParsedRow) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        normalize_text(&trade.market),
        normalize_text(&trade.code),
        normalize_text(&trade.name),
        trade.kind.as_str(),
        trade.trade_date,
        trade.quantity,
        trade.price,
        normalize_text(&trade.instrument_currency),
        trade.cost_base_per_share_sek,
        trade.brokerage,
        normalize_text(&trade.brokerage_currency),
        trade
            .exchange_rate
            .map(|rate| rate.to_string())
            .unwrap_or_default(),
        trade.value,
        normalize_text(&trade.source_column),
        normalize_text(&trade.comments)
    )
}

fn instrument_key_map(trades: &[ParsedRow]) -> BTreeMap<(&str, &str), BTreeSet<(&str, &str)>> {
    let mut map = BTreeMap::<(&str, &str), BTreeSet<(&str, &str)>>::new();

    for trade in trades {
        map.entry((trade.market.as_str(), trade.code.as_str()))
            .or_default()
            .insert((trade.name.as_str(), trade.instrument_currency.as_str()));
    }

    map
}

fn type_counts(trades: &[ParsedRow]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();

    for trade in trades {
        *counts.entry(trade.kind.as_str()).or_default() += 1;
    }

    counts
}

fn format_type_counts(counts: &BTreeMap<&'static str, usize>) -> String {
    [ParsedKind::Buy, ParsedKind::Sell, ParsedKind::Split]
        .iter()
        .map(|kind| {
            let label = kind.as_str();
            format!(
                "{} {}",
                label,
                counts.get(label).copied().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn value_sign_is_consistent(trade: &ParsedRow) -> bool {
    match trade.kind {
        ParsedKind::Buy => trade.value > Decimal::ZERO,
        ParsedKind::Sell => trade.value < Decimal::ZERO,
        ParsedKind::Split => trade.value.is_zero(),
    }
}

fn quantity_sign_is_consistent(trade: &ParsedRow) -> bool {
    match trade.kind {
        ParsedKind::Buy => trade.quantity > Decimal::ZERO,
        ParsedKind::Sell => trade.quantity < Decimal::ZERO,
        ParsedKind::Split => trade.quantity > Decimal::ZERO,
    }
}

fn parse_decimal_arg(value: &str) -> Result<Decimal, String> {
    let normalized = normalize_decimal(value);
    if normalized.is_empty() {
        return Err("empty decimal argument".to_string());
    }
    Decimal::from_str(&normalized).map_err(|_| format!("invalid decimal argument: {value:?}"))
}

fn normalize_decimal(value: &str) -> String {
    normalize_text(value)
        .replace('\u{2212}', "-")
        .replace([','], ".")
        .replace([' ', '\u{00a0}', '\u{202f}'], "")
}

fn normalize_text(value: &str) -> &str {
    value.trim()
}

fn push_line(output: &mut String, line: &str) {
    output.push_str(line);
    output.push('\n');
}
