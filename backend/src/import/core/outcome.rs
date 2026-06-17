use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::domain::ProposedTransaction;

/// Instrument identity + display fields from one row.
///
/// ISIN, when present, is the stable cross-source identity; otherwise the
/// `(exchange, symbol)` pair is the identity used by Sharesight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstrumentKey {
    pub exchange: String,
    pub symbol: String,
    pub name: String,
    pub currency: String,
    pub isin: Option<String>,
}

impl InstrumentKey {
    /// Stable grouping key used by the planner and writer.
    pub fn asset_key(&self) -> String {
        match &self.isin {
            Some(isin) => isin.clone(),
            None => format!(
                "{}:{}",
                self.exchange.to_lowercase(),
                self.symbol.to_lowercase()
            ),
        }
    }
}

/// A row mapped to a proposed ledger transaction plus audit/warning context.
#[derive(Clone, Debug, PartialEq)]
pub struct MappedRow {
    pub source_row_number: usize,
    pub instrument: InstrumentKey,
    pub proposed: ProposedTransaction,
    pub source_value: Option<Decimal>,
    /// Currency of `source_value`, persisted verbatim.
    pub source_currency: Option<String>,
    /// Free-text note persisted to `transactions.note`.
    pub note: Option<String>,
    /// True when a Buy/Sell had a blank or non-positive FX rate.
    pub fx_warning: bool,
}

/// A note attached to a row: a stable code plus message and optional row number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowNote {
    pub row: Option<usize>,
    pub code: &'static str,
    pub message: String,
}

/// One source row, classified into a downstream outcome.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum RowOutcome {
    Mapped(MappedRow),
    Skip {
        asset_key: Option<String>,
        note: RowNote,
    },
    Error {
        asset_key: Option<String>,
        note: RowNote,
    },
}

/// Source-row classification counts, before any planner-level filtering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceKindCounts {
    pub rows: usize,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
}

/// Minimal report header used by the shared planner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanHeader {
    pub title: String,
    pub date_from: NaiveDate,
    pub date_to: NaiveDate,
}

/// Everything a source adapter produces for the shared planner/writer.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedImport {
    pub header: PlanHeader,
    pub counts: SourceKindCounts,
    pub outcomes: Vec<RowOutcome>,
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
