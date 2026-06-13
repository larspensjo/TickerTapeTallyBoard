use chrono::NaiveDate;
use csv::StringRecord;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

const DEFAULT_CSV_PATH: &str = "../docs/AllTradesReport_2026-06-12.csv";
const HEADER_MARKER: &str = "Market";
const REPORT_TITLE_MARKER: &str = "All Trades Report between";

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse(env::args().skip(1))?;
    let report = parse_report(&args.csv_path)?;
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

#[derive(Debug)]
struct Report {
    metadata: ReportMetadata,
    header_row_number: usize,
    trades: Vec<Trade>,
}

#[derive(Debug)]
struct ReportMetadata {
    title: String,
    date_from: NaiveDate,
    date_to: NaiveDate,
}

#[derive(Debug, Clone)]
struct Trade {
    source_row_number: usize,
    market: String,
    code: String,
    name: String,
    transaction_type: TransactionType,
    trade_date: NaiveDate,
    quantity: Decimal,
    price: Decimal,
    instrument_currency: String,
    cost_base_per_share_sek: Decimal,
    brokerage: Decimal,
    brokerage_currency: String,
    exchange_rate: Decimal,
    value: Decimal,
    source_column: String,
    comments: String,
    raw_signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TransactionType {
    Buy,
    Sell,
    Split,
}

impl TransactionType {
    fn parse(value: &str, row_number: usize) -> Result<Self, String> {
        match value.trim() {
            "Buy" => Ok(Self::Buy),
            "Sell" => Ok(Self::Sell),
            "Split" => Ok(Self::Split),
            other => Err(format!(
                "row {row_number}: unknown transaction type {other:?}"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "Buy",
            Self::Sell => "Sell",
            Self::Split => "Split",
        }
    }
}

#[derive(Debug)]
struct HeaderIndexes {
    market: usize,
    code: usize,
    name: usize,
    transaction_type: usize,
    trade_date: usize,
    quantity: usize,
    price: usize,
    instrument_currency: usize,
    cost_base_per_share_sek: usize,
    brokerage: usize,
    brokerage_currency: usize,
    exchange_rate: usize,
    value: usize,
    source_column: usize,
    comments: usize,
}

fn parse_report(path: &PathBuf) -> Result<Report, Box<dyn Error>> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)?;

    let mut metadata = None;
    let mut header = None;
    let mut records = Vec::new();

    for (zero_based_index, record) in reader.records().enumerate() {
        let row_number = zero_based_index + 1;
        let record = record?;

        if metadata.is_none() {
            metadata = parse_metadata(&record)?;
        }

        if header.is_none() && is_header_record(&record) {
            header = Some((row_number, HeaderIndexes::parse(&record)?));
            continue;
        }

        if header.is_some() && !record_is_empty(&record) {
            records.push((row_number, record));
        }
    }

    let metadata = metadata.ok_or("report metadata line was not found")?;
    let (header_row_number, header) = header.ok_or("header row was not found")?;
    let trades = records
        .iter()
        .map(|(row_number, record)| parse_trade(*row_number, record, &header))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Report {
        metadata,
        header_row_number,
        trades,
    })
}

impl HeaderIndexes {
    fn parse(record: &StringRecord) -> Result<Self, String> {
        Ok(Self {
            market: require_header(record, "Market")?,
            code: require_header(record, "Code")?,
            name: require_header(record, "Name")?,
            transaction_type: require_header(record, "Type")?,
            trade_date: require_header(record, "Date")?,
            quantity: require_header(record, "Quantity")?,
            price: require_header(record, "Price")?,
            instrument_currency: require_header(record, "Instrument Currency")?,
            cost_base_per_share_sek: require_header(record, "Cost base per share (SEK)")?,
            brokerage: require_header(record, "Brokerage")?,
            brokerage_currency: require_header(record, "Brokerage Currency")?,
            exchange_rate: require_header(record, "Exchange Rate")?,
            value: require_header(record, "Value")?,
            source_column: record
                .iter()
                .position(|field| field.trim().is_empty())
                .ok_or_else(|| "unnamed report/source column was not found".to_string())?,
            comments: require_header(record, "Comments")?,
        })
    }
}

fn parse_metadata(record: &StringRecord) -> Result<Option<ReportMetadata>, Box<dyn Error>> {
    let Some(title) = record.get(0).map(str::trim) else {
        return Ok(None);
    };

    if !title.contains(REPORT_TITLE_MARKER) {
        return Ok(None);
    }

    let (_, range) = title
        .split_once(" between ")
        .ok_or("metadata line did not contain a report range")?;
    let (date_from, date_to) = range
        .split_once(" and ")
        .ok_or("metadata line did not contain both range dates")?;

    Ok(Some(ReportMetadata {
        title: title.to_string(),
        date_from: NaiveDate::parse_from_str(date_from, "%Y-%m-%d")?,
        date_to: NaiveDate::parse_from_str(date_to, "%Y-%m-%d")?,
    }))
}

fn parse_trade(
    source_row_number: usize,
    record: &StringRecord,
    header: &HeaderIndexes,
) -> Result<Trade, String> {
    Ok(Trade {
        source_row_number,
        market: field(record, header.market, "Market", source_row_number)?.to_string(),
        code: field(record, header.code, "Code", source_row_number)?.to_string(),
        name: field(record, header.name, "Name", source_row_number)?.to_string(),
        transaction_type: TransactionType::parse(
            field(record, header.transaction_type, "Type", source_row_number)?,
            source_row_number,
        )?,
        trade_date: NaiveDate::parse_from_str(
            field(record, header.trade_date, "Date", source_row_number)?,
            "%d/%m/%Y",
        )
        .map_err(|error| format!("row {source_row_number}: invalid Date: {error}"))?,
        quantity: parse_decimal_field(
            field(record, header.quantity, "Quantity", source_row_number)?,
            "Quantity",
            source_row_number,
        )?,
        price: parse_decimal_field(
            field(record, header.price, "Price", source_row_number)?,
            "Price",
            source_row_number,
        )?,
        instrument_currency: field(
            record,
            header.instrument_currency,
            "Instrument Currency",
            source_row_number,
        )?
        .to_string(),
        cost_base_per_share_sek: parse_decimal_field(
            field(
                record,
                header.cost_base_per_share_sek,
                "Cost base per share (SEK)",
                source_row_number,
            )?,
            "Cost base per share (SEK)",
            source_row_number,
        )?,
        brokerage: parse_decimal_field(
            field(record, header.brokerage, "Brokerage", source_row_number)?,
            "Brokerage",
            source_row_number,
        )?,
        brokerage_currency: field(
            record,
            header.brokerage_currency,
            "Brokerage Currency",
            source_row_number,
        )?
        .to_string(),
        exchange_rate: parse_decimal_field(
            field(
                record,
                header.exchange_rate,
                "Exchange Rate",
                source_row_number,
            )?,
            "Exchange Rate",
            source_row_number,
        )?,
        value: parse_decimal_field(
            field(record, header.value, "Value", source_row_number)?,
            "Value",
            source_row_number,
        )?,
        source_column: field(
            record,
            header.source_column,
            "source column",
            source_row_number,
        )?
        .to_string(),
        comments: field(record, header.comments, "Comments", source_row_number)?.to_string(),
        raw_signature: record
            .iter()
            .map(|field| normalize_text(field).to_string())
            .collect::<Vec<_>>()
            .join("|"),
    })
}

fn summarize(report: &Report, split_current_position: Option<Decimal>) -> String {
    let mut output = String::new();
    let trade_count = report.trades.len();
    let markets = report
        .trades
        .iter()
        .map(|trade| trade.market.as_str())
        .collect::<BTreeSet<_>>();
    let instrument_currencies = report
        .trades
        .iter()
        .map(|trade| trade.instrument_currency.as_str())
        .collect::<BTreeSet<_>>();
    let source_columns = report
        .trades
        .iter()
        .map(|trade| trade.source_column.as_str())
        .collect::<BTreeSet<_>>();
    let brokerage_currencies = report
        .trades
        .iter()
        .map(|trade| trade.brokerage_currency.as_str())
        .collect::<BTreeSet<_>>();
    let type_counts = type_counts(&report.trades);
    let instrument_keys = instrument_key_map(&report.trades);
    let ambiguous_keys = instrument_keys
        .values()
        .filter(|identity_set| identity_set.len() > 1)
        .count();
    let duplicate_rows = duplicate_count(&report.trades);
    let partial_fill_summary = partial_fill_summary(&report.trades);
    let value_sign_mismatches = report
        .trades
        .iter()
        .filter(|trade| !value_sign_is_consistent(trade))
        .count();
    let quantity_sign_mismatches = report
        .trades
        .iter()
        .filter(|trade| !quantity_sign_is_consistent(trade))
        .count();
    let nonzero_cost_base_rows = report
        .trades
        .iter()
        .filter(|trade| !trade.cost_base_per_share_sek.is_zero())
        .count();
    let blank_comments = report
        .trades
        .iter()
        .filter(|trade| trade.comments.trim().is_empty())
        .count();
    let fx_summary = fx_model_summary(&report.trades);
    let split_summary = split_summary(&report.trades, split_current_position);
    let first_row = report
        .trades
        .iter()
        .map(|trade| trade.source_row_number)
        .min()
        .unwrap_or_default();
    let last_row = report
        .trades
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
    for model in &fx_summary.models {
        push_line(
            &mut output,
            &format!(
                "- {}: average absolute residual {} SEK; max absolute residual {} SEK.",
                model.label,
                model.average_abs_residual(fx_summary.row_count),
                model.max_abs_residual
            ),
        );
    }
    push_line(
        &mut output,
        &format!("- Best observed interpretation: {}.", fx_summary.best_label),
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
    best_label: String,
}

#[derive(Debug)]
struct FxModelResult {
    label: &'static str,
    max_abs_residual: Decimal,
    total_abs_residual: Decimal,
}

fn fx_model_summary(trades: &[Trade]) -> FxSummary {
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
        trade.transaction_type != TransactionType::Split
            && !trade.quantity.is_zero()
            && !trade.price.is_zero()
            && !trade.exchange_rate.is_zero()
    }) {
        row_count += 1;
        let native_gross = trade.quantity * trade.price;
        let divide_base = native_gross / trade.exchange_rate;
        let multiply_base = native_gross * trade.exchange_rate;
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

    let best_label = models
        .iter()
        .min_by(|left, right| left.total_abs_residual.cmp(&right.total_abs_residual))
        .map(|model| model.label.to_string())
        .unwrap_or_else(|| "no FX model rows were available".to_string());

    FxSummary {
        row_count,
        models,
        best_label,
    }
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

fn split_summary(trades: &[Trade], current_position: Option<Decimal>) -> SplitSummary {
    let split_trades = trades
        .iter()
        .filter(|trade| trade.transaction_type == TransactionType::Split)
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

fn partial_fill_summary(trades: &[Trade]) -> PartialFillSummary {
    let mut groups: BTreeMap<(&str, &str, NaiveDate, TransactionType), usize> = BTreeMap::new();

    for trade in trades {
        *groups
            .entry((
                trade.market.as_str(),
                trade.code.as_str(),
                trade.trade_date,
                trade.transaction_type,
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

fn duplicate_count(trades: &[Trade]) -> usize {
    let mut counts = BTreeMap::<&str, usize>::new();

    for trade in trades {
        *counts.entry(&trade.raw_signature).or_default() += 1;
    }

    counts
        .values()
        .filter(|count| **count > 1)
        .map(|count| count - 1)
        .sum()
}

fn instrument_key_map(trades: &[Trade]) -> BTreeMap<(&str, &str), BTreeSet<(&str, &str)>> {
    let mut map = BTreeMap::<(&str, &str), BTreeSet<(&str, &str)>>::new();

    for trade in trades {
        map.entry((trade.market.as_str(), trade.code.as_str()))
            .or_default()
            .insert((trade.name.as_str(), trade.instrument_currency.as_str()));
    }

    map
}

fn type_counts(trades: &[Trade]) -> BTreeMap<TransactionType, usize> {
    let mut counts = BTreeMap::new();

    for trade in trades {
        *counts.entry(trade.transaction_type).or_default() += 1;
    }

    counts
}

fn format_type_counts(counts: &BTreeMap<TransactionType, usize>) -> String {
    [
        TransactionType::Buy,
        TransactionType::Sell,
        TransactionType::Split,
    ]
    .iter()
    .map(|transaction_type| {
        format!(
            "{} {}",
            transaction_type.as_str(),
            counts.get(transaction_type).copied().unwrap_or_default()
        )
    })
    .collect::<Vec<_>>()
    .join(", ")
}

fn value_sign_is_consistent(trade: &Trade) -> bool {
    match trade.transaction_type {
        TransactionType::Buy => trade.value > Decimal::ZERO,
        TransactionType::Sell => trade.value < Decimal::ZERO,
        TransactionType::Split => trade.value.is_zero(),
    }
}

fn quantity_sign_is_consistent(trade: &Trade) -> bool {
    match trade.transaction_type {
        TransactionType::Buy => trade.quantity > Decimal::ZERO,
        TransactionType::Sell => trade.quantity < Decimal::ZERO,
        TransactionType::Split => trade.quantity > Decimal::ZERO,
    }
}

fn is_header_record(record: &StringRecord) -> bool {
    record.get(0).map(str::trim) == Some(HEADER_MARKER)
        && record.iter().any(|field| field.trim() == "Exchange Rate")
}

fn record_is_empty(record: &StringRecord) -> bool {
    record.iter().all(|field| field.trim().is_empty())
}

fn require_header(record: &StringRecord, name: &str) -> Result<usize, String> {
    record
        .iter()
        .position(|field| field.trim() == name)
        .ok_or_else(|| format!("required header {name:?} was not found"))
}

fn field<'a>(
    record: &'a StringRecord,
    index: usize,
    label: &str,
    row_number: usize,
) -> Result<&'a str, String> {
    record
        .get(index)
        .map(str::trim)
        .ok_or_else(|| format!("row {row_number}: missing field {label} at index {index}"))
}

fn parse_decimal_field(value: &str, label: &str, row_number: usize) -> Result<Decimal, String> {
    parse_decimal(value).map_err(|error| format!("row {row_number}: invalid {label}: {error}"))
}

fn parse_decimal_arg(value: &str) -> Result<Decimal, String> {
    parse_decimal(value).map_err(|error| format!("invalid decimal argument: {error}"))
}

fn parse_decimal(value: &str) -> Result<Decimal, DecimalParseError> {
    let normalized = normalize_decimal(value);

    if normalized.is_empty() {
        return Err(DecimalParseError::Empty);
    }

    Decimal::from_str(&normalized).map_err(|_| DecimalParseError::Invalid(value.to_string()))
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

fn sanitize_report_title(title: &str) -> String {
    let Some((_, report_part)) = title.split_once(" - ") else {
        return "All Trades Report".to_string();
    };

    report_part.to_string()
}

fn push_line(output: &mut String, line: &str) {
    output.push_str(line);
    output.push('\n');
}

#[derive(Debug)]
enum DecimalParseError {
    Empty,
    Invalid(String),
}

impl fmt::Display for DecimalParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "empty decimal"),
            Self::Invalid(value) => write!(formatter, "{value:?} is not a decimal"),
        }
    }
}

impl Error for DecimalParseError {}
