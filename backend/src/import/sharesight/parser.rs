use chrono::NaiveDate;
use csv::{ReaderBuilder, StringRecord};
use rust_decimal::Decimal;
use std::str::FromStr;

const HEADER_MARKER: &str = "Market";
const REPORT_TITLE_MARKER: &str = "All Trades Report between";

/// A parsed All Trades report: metadata plus one row per data line.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedReport {
    pub metadata: ReportMetadata,
    pub header_row_number: usize,
    pub rows: Vec<ParsedRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportMetadata {
    pub title: String,
    pub date_from: NaiveDate,
    pub date_to: NaiveDate,
}

/// One faithfully parsed CSV data row. Audit-only columns are retained for diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedRow {
    pub source_row_number: usize,
    pub market: String,
    pub code: String,
    pub name: String,
    pub kind: ParsedKind,
    pub trade_date: NaiveDate,
    pub quantity: Decimal,
    pub price: Decimal,
    pub instrument_currency: String,
    pub cost_base_per_share_sek: Decimal,
    pub brokerage: Decimal,
    pub brokerage_currency: String,
    /// `None` when the Exchange Rate cell is blank; `Some` for any parsed decimal.
    pub exchange_rate: Option<Decimal>,
    pub value: Decimal,
    /// Optional report/source column retained when Sharesight includes it.
    pub source_column: String,
    pub comments: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsedKind {
    Buy,
    Sell,
    Split,
}

impl ParsedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "Buy",
            Self::Sell => "Sell",
            Self::Split => "Split",
        }
    }
}

/// A parse-stage failure with optional row context and a stable code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub row: Option<usize>,
    pub code: &'static str,
    pub message: String,
}

impl ParseError {
    pub fn header(message: impl Into<String>) -> Self {
        Self {
            row: None,
            code: "header_not_found",
            message: message.into(),
        }
    }

    pub fn row(row: usize, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            row: Some(row),
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.row {
            Some(row) => write!(f, "row {row}: {} ({})", self.message, self.code),
            None => write!(f, "{} ({})", self.message, self.code),
        }
    }
}

impl std::error::Error for ParseError {}

pub fn parse_report(bytes: &[u8]) -> Result<ParsedReport, ParseError> {
    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes);

    let mut metadata = None;
    let mut header: Option<(usize, HeaderIndexes)> = None;
    let mut data: Vec<(usize, StringRecord)> = Vec::new();

    for (zero_based, record) in reader.records().enumerate() {
        let row_number = zero_based + 1;
        let record = record
            .map_err(|e| ParseError::row(row_number, "csv_read", format!("CSV read error: {e}")))?;

        if metadata.is_none() {
            metadata = parse_metadata(&record)?;
        }
        if header.is_none() && is_header_record(&record) {
            header = Some((row_number, HeaderIndexes::parse(&record)?));
            continue;
        }
        if header.is_some() && !record_is_empty(&record) {
            data.push((row_number, record));
        }
    }

    let metadata =
        metadata.ok_or_else(|| ParseError::header("report metadata line was not found"))?;
    let (header_row_number, header) =
        header.ok_or_else(|| ParseError::header("header row was not found"))?;

    let rows = data
        .iter()
        .map(|(row_number, record)| parse_row(*row_number, record, &header))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ParsedReport {
        metadata,
        header_row_number,
        rows,
    })
}

#[derive(Debug)]
struct HeaderIndexes {
    market: usize,
    code: usize,
    name: usize,
    kind: usize,
    trade_date: usize,
    quantity: usize,
    price: usize,
    instrument_currency: usize,
    cost_base_per_share_sek: usize,
    brokerage: usize,
    brokerage_currency: usize,
    exchange_rate: usize,
    value: usize,
    source_column: Option<usize>,
    comments: usize,
}

impl HeaderIndexes {
    fn parse(record: &StringRecord) -> Result<Self, ParseError> {
        Ok(Self {
            market: require_header(record, "Market")?,
            code: require_header(record, "Code")?,
            name: require_header(record, "Name")?,
            kind: require_header(record, "Type")?,
            trade_date: require_header(record, "Date")?,
            quantity: require_header(record, "Quantity")?,
            price: require_header(record, "Price")?,
            instrument_currency: require_header(record, "Instrument Currency")?,
            cost_base_per_share_sek: require_header(record, "Cost base per share (SEK)")?,
            brokerage: require_header(record, "Brokerage")?,
            brokerage_currency: require_header(record, "Brokerage Currency")?,
            exchange_rate: require_header(record, "Exchange Rate")?,
            value: require_header(record, "Value")?,
            source_column: record.iter().position(|field| field.trim().is_empty()),
            comments: require_header(record, "Comments")?,
        })
    }
}

fn parse_metadata(record: &StringRecord) -> Result<Option<ReportMetadata>, ParseError> {
    let Some(title) = record
        .iter()
        .map(str::trim)
        .find(|field| field.contains(REPORT_TITLE_MARKER))
    else {
        return Ok(None);
    };

    let title = title
        .split_once(REPORT_TITLE_MARKER)
        .map(|(_, tail)| format!("{REPORT_TITLE_MARKER}{tail}"))
        .unwrap_or_else(|| title.to_string());

    let (_, range) = title
        .split_once(" between ")
        .ok_or_else(|| ParseError::header("metadata line did not contain a report range"))?;
    let (date_from, date_to) = range
        .split_once(" and ")
        .ok_or_else(|| ParseError::header("metadata line did not contain both range dates"))?;

    Ok(Some(ReportMetadata {
        title: title.to_string(),
        date_from: NaiveDate::parse_from_str(date_from, "%Y-%m-%d")
            .map_err(|e| ParseError::header(format!("invalid report start date: {e}")))?,
        date_to: NaiveDate::parse_from_str(date_to, "%Y-%m-%d")
            .map_err(|e| ParseError::header(format!("invalid report end date: {e}")))?,
    }))
}

fn parse_row(
    row_number: usize,
    record: &StringRecord,
    header: &HeaderIndexes,
) -> Result<ParsedRow, ParseError> {
    Ok(ParsedRow {
        source_row_number: row_number,
        market: field(record, header.market, "Market", row_number)?.to_string(),
        code: field(record, header.code, "Code", row_number)?.to_string(),
        name: field(record, header.name, "Name", row_number)?.to_string(),
        kind: parse_kind(field(record, header.kind, "Type", row_number)?, row_number)?,
        trade_date: NaiveDate::parse_from_str(
            field(record, header.trade_date, "Date", row_number)?,
            "%d/%m/%Y",
        )
        .map_err(|e| ParseError::row(row_number, "invalid_date", format!("invalid Date: {e}")))?,
        quantity: decimal_field(record, header.quantity, "Quantity", row_number)?,
        price: decimal_field(record, header.price, "Price", row_number)?,
        instrument_currency: field(
            record,
            header.instrument_currency,
            "Instrument Currency",
            row_number,
        )?
        .to_string(),
        cost_base_per_share_sek: decimal_field(
            record,
            header.cost_base_per_share_sek,
            "Cost base per share (SEK)",
            row_number,
        )?,
        brokerage: decimal_field(record, header.brokerage, "Brokerage", row_number)?,
        brokerage_currency: field(
            record,
            header.brokerage_currency,
            "Brokerage Currency",
            row_number,
        )?
        .to_string(),
        exchange_rate: optional_decimal_field(
            record,
            header.exchange_rate,
            "Exchange Rate",
            row_number,
        )?,
        value: decimal_field(record, header.value, "Value", row_number)?,
        source_column: match header.source_column {
            Some(index) => field(record, index, "source column", row_number)?.to_string(),
            None => String::new(),
        },
        comments: field(record, header.comments, "Comments", row_number)?.to_string(),
    })
}

fn parse_kind(value: &str, row_number: usize) -> Result<ParsedKind, ParseError> {
    match value.trim() {
        "Buy" => Ok(ParsedKind::Buy),
        "Sell" => Ok(ParsedKind::Sell),
        "Split" => Ok(ParsedKind::Split),
        other => Err(ParseError::row(
            row_number,
            "unknown_type",
            format!("unknown transaction type {other:?}"),
        )),
    }
}

fn decimal_field(
    record: &StringRecord,
    index: usize,
    label: &str,
    row_number: usize,
) -> Result<Decimal, ParseError> {
    let raw = field(record, index, label, row_number)?;
    let normalized = normalize_decimal(raw);
    if normalized.is_empty() {
        return Err(ParseError::row(
            row_number,
            "invalid_decimal",
            format!("empty {label}"),
        ));
    }
    Decimal::from_str(&normalized).map_err(|_| {
        ParseError::row(
            row_number,
            "invalid_decimal",
            format!("invalid {label}: {raw:?}"),
        )
    })
}

/// Blank cell -> None; a present cell that is not a decimal -> parse error.
fn optional_decimal_field(
    record: &StringRecord,
    index: usize,
    label: &str,
    row_number: usize,
) -> Result<Option<Decimal>, ParseError> {
    let raw = field(record, index, label, row_number)?;
    let normalized = normalize_decimal(raw);
    if normalized.is_empty() {
        return Ok(None);
    }
    Decimal::from_str(&normalized).map(Some).map_err(|_| {
        ParseError::row(
            row_number,
            "invalid_exchange_rate",
            format!("invalid {label}: {raw:?}"),
        )
    })
}

fn is_header_record(record: &StringRecord) -> bool {
    record.get(0).map(str::trim) == Some(HEADER_MARKER)
        && record.iter().any(|field| field.trim() == "Exchange Rate")
}

fn record_is_empty(record: &StringRecord) -> bool {
    record.iter().all(|field| field.trim().is_empty())
}

fn require_header(record: &StringRecord, name: &str) -> Result<usize, ParseError> {
    record
        .iter()
        .position(|field| field.trim() == name)
        .ok_or_else(|| ParseError::header(format!("required header {name:?} was not found")))
}

fn field<'a>(
    record: &'a StringRecord,
    index: usize,
    label: &str,
    row_number: usize,
) -> Result<&'a str, ParseError> {
    record.get(index).map(str::trim).ok_or_else(|| {
        ParseError::row(
            row_number,
            "missing_field",
            format!("missing field {label} at index {index}"),
        )
    })
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

pub fn sanitize_report_title(title: &str) -> String {
    let Some((_, report_part)) = title.split_once(" - ") else {
        return "All Trades Report".to_string();
    };

    report_part.to_string()
}

#[cfg(test)]
mod tests {
    use super::{parse_report, ParsedKind};
    use rust_decimal_macros::dec;

    const SYNTHETIC: &str = concat!(
        "Synthetic Portfolio - All Trades Report between 2025-06-12 and 2026-06-12\n",
        "\n",
        "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
        "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"9,60\",SEK,\"0,100000\",\"1\u{00A0}259,60\",All Trades,First buy\n",
        "NASDAQ,MSFT,Microsoft,Sell,13/06/2026,\u{2212}5,\"12,60\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"\u{2212}629,80\",All Trades,\n",
        "XETR,ASML,ASML Holding,Buy,14/06/2026,3,\"600,00\",EUR,\"0,00\",\"0,00\",SEK,,\"0,00\",All Trades,Missing FX\n",
        "NASDAQ,MSFT,Microsoft,Split,15/06/2026,10,\"0,00\",USD,\"0,00\",\"0,00\",SEK,,\"0,00\",All Trades,Ten for one\n",
    );

    #[test]
    fn parses_metadata_header_and_rows() {
        let report = parse_report(SYNTHETIC.as_bytes()).expect("parses");
        assert_eq!(
            report.metadata.title,
            "All Trades Report between 2025-06-12 and 2026-06-12"
        );
        assert_eq!(report.metadata.date_from.to_string(), "2025-06-12");
        assert_eq!(report.metadata.date_to.to_string(), "2026-06-12");
        assert_eq!(report.rows.len(), 4);
    }

    #[test]
    fn header_without_unnamed_source_column_parses() {
        let csv = concat!(
            "Synthetic Portfolio - All Trades Report between 2025-06-12 and 2026-06-12\n",
            "\n",
            "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,Comments\n",
            "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"9,60\",SEK,\"0,100000\",\"1\u{00A0}259,60\",First buy\n",
        );

        let report = parse_report(csv.as_bytes()).expect("parses without source column");

        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0].source_column, "");
        assert_eq!(report.rows[0].comments, "First buy");
    }

    #[test]
    fn parses_comma_decimals_nbsp_thousands_and_unicode_minus() {
        let report = parse_report(SYNTHETIC.as_bytes()).expect("parses");
        let buy = &report.rows[0];
        assert_eq!(buy.kind, ParsedKind::Buy);
        assert_eq!(buy.price, dec!(12.50));
        assert_eq!(buy.value, dec!(1259.60));
        let sell = &report.rows[1];
        assert_eq!(sell.kind, ParsedKind::Sell);
        assert_eq!(sell.quantity, dec!(-5));
        assert_eq!(sell.value, dec!(-629.80));
    }

    #[test]
    fn blank_exchange_rate_parses_as_none() {
        let report = parse_report(SYNTHETIC.as_bytes()).expect("parses");
        assert_eq!(report.rows[2].exchange_rate, None);
        assert_eq!(report.rows[0].exchange_rate, Some(dec!(0.100000)));
    }

    #[test]
    fn parses_dd_mm_yyyy_dates_and_split_row() {
        let report = parse_report(SYNTHETIC.as_bytes()).expect("parses");
        assert_eq!(report.rows[3].kind, ParsedKind::Split);
        assert_eq!(report.rows[3].trade_date.to_string(), "2026-06-15");
    }

    #[test]
    fn missing_header_is_an_error() {
        let bad = "Synthetic Portfolio - All Trades Report between 2025-06-12 and 2026-06-12\n\nno,header,here\n";
        let error = parse_report(bad.as_bytes()).expect_err("no header");
        assert_eq!(error.code, "header_not_found");
    }

    #[test]
    fn metadata_can_be_found_after_preamble_text_in_the_same_row() {
        let csv = concat!(
            "This report is provided for informational purposes only,Lars Pensjö's Portfolio - All Trades Report between 2025-06-12 and 2026-06-12,,,,,,,,,,,,,,\n",
            ",,,,,,,,,,,,,,,\n",
            "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
            "NASDAQ,MSFT,Microsoft,Buy,12/06/2025,29,\"479,66\",USD,\"0,00\",\"9,60\",SEK,\"0,1056\",\"131\u{00A0}760,06\",All Trades,\n",
        );

        let report = parse_report(csv.as_bytes()).expect("parses mixed preamble metadata");

        assert_eq!(
            report.metadata.title,
            "All Trades Report between 2025-06-12 and 2026-06-12"
        );
        assert_eq!(report.rows.len(), 1);
    }

    #[test]
    fn non_decimal_exchange_rate_is_a_parse_error() {
        let bad = SYNTHETIC.replace("\"0,100000\"", "\"abc\"");
        let error = parse_report(bad.as_bytes()).expect_err("bad rate");
        assert_eq!(error.code, "invalid_exchange_rate");
    }
}
