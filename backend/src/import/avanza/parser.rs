use chrono::NaiveDate;
use csv::{ReaderBuilder, StringRecord};
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::import::core::outcome::ParseError;
use crate::import::text::normalize_decimal;

const HEADER_MARKER: &str = "Datum";

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedAvanzaReport {
    pub rows: Vec<ParsedAvanzaRow>,
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvanzaKind {
    Buy,
    Sell,
    Dividend,
    Split,
    /// Any other "Typ av transaktion"; carried so the mapper can skip-warn.
    Unsupported,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedAvanzaRow {
    pub source_row_number: usize,
    pub trade_date: NaiveDate,
    pub raw_kind: String,
    pub kind: AvanzaKind,
    pub name: String,
    pub quantity: Decimal,
    pub price: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub transaction_currency: String,
    pub brokerage: Option<Decimal>,
    pub fx_rate: Option<Decimal>,
    pub instrument_currency: String,
    pub isin: String,
}

#[derive(Debug)]
struct Header {
    datum: usize,
    kind: usize,
    name: usize,
    quantity: usize,
    price: usize,
    amount: usize,
    transaction_currency: usize,
    brokerage: usize,
    fx_rate: usize,
    instrument_currency: usize,
    isin: usize,
}

pub fn parse_report(bytes: &[u8]) -> Result<ParsedAvanzaReport, ParseError> {
    let mut reader = ReaderBuilder::new()
        .delimiter(b';')
        .has_headers(true)
        .flexible(true)
        .from_reader(bytes);

    let header_record = reader
        .headers()
        .map_err(|e| ParseError::header(format!("CSV header read error: {e}")))?;
    let header = parse_header(header_record)?;

    let mut rows = Vec::new();
    let mut min_date: Option<NaiveDate> = None;
    let mut max_date: Option<NaiveDate> = None;

    for (zero_based, record) in reader.records().enumerate() {
        let row_number = zero_based + 2;
        let record = record
            .map_err(|e| ParseError::row(row_number, "csv_read", format!("CSV read error: {e}")))?;
        if record_is_empty(&record) {
            continue;
        }

        let row = parse_row(row_number, &record, &header)?;
        min_date = Some(match min_date {
            Some(current) => current.min(row.trade_date),
            None => row.trade_date,
        });
        max_date = Some(match max_date {
            Some(current) => current.max(row.trade_date),
            None => row.trade_date,
        });
        rows.push(row);
    }

    Ok(ParsedAvanzaReport {
        rows,
        date_from: min_date,
        date_to: max_date,
    })
}

fn parse_header(record: &StringRecord) -> Result<Header, ParseError> {
    if trim_bom(record.get(0).unwrap_or_default()).trim() != HEADER_MARKER {
        return Err(ParseError::header(
            "not an Avanza AllTradesReport (missing Datum header)",
        ));
    }

    let required = |name: &str| {
        record
            .iter()
            .position(|field| trim_bom(field).trim() == name)
            .ok_or_else(|| ParseError::header(format!("required column {name:?} not found")))
    };

    Ok(Header {
        datum: required("Datum")?,
        kind: required("Typ av transaktion")?,
        name: required("Värdepapper/beskrivning")?,
        quantity: required("Antal")?,
        price: required("Kurs")?,
        amount: required("Belopp")?,
        transaction_currency: required("Transaktionsvaluta")?,
        brokerage: required("Courtage")?,
        fx_rate: required("Valutakurs")?,
        instrument_currency: required("Instrumentvaluta")?,
        isin: required("ISIN")?,
    })
}

fn parse_row(
    row_number: usize,
    record: &StringRecord,
    header: &Header,
) -> Result<ParsedAvanzaRow, ParseError> {
    let trade_date = parse_date(
        field(record, header.datum, "Datum", row_number)?,
        row_number,
    )?;
    let raw_kind = field(record, header.kind, "Typ av transaktion", row_number)?.to_string();

    Ok(ParsedAvanzaRow {
        source_row_number: row_number,
        trade_date,
        raw_kind: raw_kind.clone(),
        kind: classify(&raw_kind),
        name: field(record, header.name, "Värdepapper/beskrivning", row_number)?.to_string(),
        quantity: decimal_field(record, header.quantity, "Antal", row_number)?,
        price: optional_decimal_field(record, header.price, "Kurs", row_number)?,
        amount: optional_decimal_field(record, header.amount, "Belopp", row_number)?,
        transaction_currency: field(
            record,
            header.transaction_currency,
            "Transaktionsvaluta",
            row_number,
        )?
        .to_string(),
        brokerage: optional_decimal_field(record, header.brokerage, "Courtage", row_number)?,
        fx_rate: optional_decimal_field(record, header.fx_rate, "Valutakurs", row_number)?,
        instrument_currency: field(
            record,
            header.instrument_currency,
            "Instrumentvaluta",
            row_number,
        )?
        .to_string(),
        isin: field(record, header.isin, "ISIN", row_number)?.to_string(),
    })
}

fn classify(raw: &str) -> AvanzaKind {
    match raw.trim() {
        "Köp" => AvanzaKind::Buy,
        "Sälj" => AvanzaKind::Sell,
        "Utdelning" => AvanzaKind::Dividend,
        "Split värdepapper" => AvanzaKind::Split,
        _ => AvanzaKind::Unsupported,
    }
}

fn parse_date(raw: &str, row_number: usize) -> Result<NaiveDate, ParseError> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|e| ParseError::row(row_number, "invalid_date", format!("invalid Datum: {e}")))
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
            "invalid_decimal",
            format!("invalid {label}: {raw:?}"),
        )
    })
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

fn record_is_empty(record: &StringRecord) -> bool {
    record.iter().all(|field| field.trim().is_empty())
}

fn trim_bom(value: &str) -> &str {
    value.trim_start_matches('\u{feff}')
}

#[cfg(test)]
mod tests {
    use super::{parse_report, AvanzaKind};
    use rust_decimal_macros::dec;

    const SAMPLE: &str = concat!(
        "Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n",
        "2026-06-10;ISK;Köp;Apple Inc;5;200,00;-10000,00;SEK;9,00;9,50;USD;US0378331005;\n",
        "2026-06-09;ISK;Sälj;Apple Inc;-2;210,00;3990,00;SEK;9,00;9,50;USD;US0378331005;120,00\n",
        "2026-06-08;ISK;Utdelning;Apple Inc;0;;50,00;SEK;;9,50;USD;US0378331005;\n",
        "2026-06-07;ISK;Köp;Volvo B;1,5;250,00;375,00;SEK;0,00;;SEK;SE0000115446;\n",
        "2026-06-06;ISK;Köp;ASML;1;800,00;-800,00;EUR;;;EUR;NL0010273215;\n",
        "2026-06-05;ISK;Övrigt;Cash;0;;100,00;SEK;;;SEK;;\n",
    );

    #[test]
    fn parses_kinds_decimals_and_dates() {
        let report = parse_report(SAMPLE.as_bytes()).expect("parses");

        assert_eq!(report.rows.len(), 6);
        assert_eq!(report.rows[0].source_row_number, 2);
        assert_eq!(report.rows[0].kind, AvanzaKind::Buy);
        assert_eq!(report.rows[0].price, Some(dec!(200.00)));
        assert_eq!(report.rows[0].quantity, dec!(5));
        assert_eq!(report.rows[1].kind, AvanzaKind::Sell);
        assert_eq!(report.rows[2].kind, AvanzaKind::Dividend);
        assert_eq!(report.rows[3].quantity, dec!(1.5));
        assert_eq!(report.rows[3].fx_rate, None);
        assert_eq!(report.rows[3].instrument_currency, "SEK");
        assert_eq!(report.rows[4].fx_rate, None);
        assert_eq!(report.rows[4].brokerage, None);
        assert_eq!(report.rows[5].kind, AvanzaKind::Unsupported);
        assert_eq!(report.date_from.unwrap().to_string(), "2026-06-05");
        assert_eq!(report.date_to.unwrap().to_string(), "2026-06-10");
    }

    #[test]
    fn missing_header_is_a_parse_error() {
        let bad = "Konto;Typ av transaktion\nISK;Köp\n";
        let error = parse_report(bad.as_bytes()).expect_err("missing header");

        assert_eq!(error.code, "header_not_found");
    }

    #[test]
    fn utf8_bom_header_parses() {
        let csv = concat!(
            "\u{feff}Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n",
            "2026-06-01;ISK;Köp;Example;1;10,00;-10,00;SEK;;;SEK;SE0000000001;\n",
        );

        let report = parse_report(csv.as_bytes()).expect("BOM header parses");

        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0].trade_date.to_string(), "2026-06-01");
    }

    #[test]
    fn invalid_date_has_stable_error_code() {
        let bad = SAMPLE.replace("2026-06-10", "2026/06/10");
        let error = parse_report(bad.as_bytes()).expect_err("bad date");

        assert_eq!(error.code, "invalid_date");
        assert_eq!(error.row, Some(2));
    }

    #[test]
    fn non_decimal_values_are_rejected() {
        let bad = SAMPLE.replace("200,00", "abc");
        let error = parse_report(bad.as_bytes()).expect_err("bad decimal");

        assert_eq!(error.code, "invalid_decimal");
        assert_eq!(error.row, Some(2));
    }

    #[test]
    fn blank_required_quantity_is_rejected() {
        let bad = SAMPLE.replace(";5;200,00;", ";;200,00;");
        let error = parse_report(bad.as_bytes()).expect_err("blank quantity");

        assert_eq!(error.code, "invalid_decimal");
        assert_eq!(error.row, Some(2));
    }

    #[test]
    fn short_rows_are_rejected() {
        let csv = concat!(
            "Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n",
            "2026-06-01;ISK;Köp;Example;1;10,00\n",
        );
        let error = parse_report(csv.as_bytes()).expect_err("short row");

        assert_eq!(error.code, "missing_field");
        assert_eq!(error.row, Some(2));
    }

    #[test]
    fn header_only_file_parses_as_empty_report() {
        let report = parse_report(
            "Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n"
                .as_bytes(),
        )
        .expect("header-only report parses");

        assert!(report.rows.is_empty());
        assert_eq!(report.date_from, None);
        assert_eq!(report.date_to, None);
    }

    #[test]
    fn blank_optional_cells_are_none() {
        let csv = concat!(
            "Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n",
            "2026-06-01;ISK;Köp;Example;1;10,00;-10,00;SEK;;;SEK;SE0000000001;\n",
        );

        let report = parse_report(csv.as_bytes()).expect("parses");
        let row = &report.rows[0];

        assert_eq!(row.price, Some(dec!(10.00)));
        assert_eq!(row.amount, Some(dec!(-10.00)));
        assert_eq!(row.brokerage, None);
        assert_eq!(row.fx_rate, None);
    }

    #[test]
    fn file_order_is_preserved() {
        let report = parse_report(SAMPLE.as_bytes()).expect("parses");
        let rows: Vec<_> = report
            .rows
            .iter()
            .map(|row| row.source_row_number)
            .collect();

        assert_eq!(rows, vec![2, 3, 4, 5, 6, 7]);
    }
}
