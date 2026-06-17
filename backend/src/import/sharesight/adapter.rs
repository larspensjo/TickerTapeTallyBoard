use crate::import::core::outcome::{
    PlanHeader, PreparedImport, RowNote, RowOutcome, SourceKindCounts,
};
use crate::import::sharesight::mapper::map_row;
use crate::import::sharesight::parser::{ParsedKind, ParsedReport};

/// Turn a parsed Sharesight report into source-neutral outcomes for the planner.
pub fn to_prepared(report: &ParsedReport) -> PreparedImport {
    let mut counts = SourceKindCounts {
        rows: report.rows.len(),
        ..Default::default()
    };
    let mut outcomes = Vec::with_capacity(report.rows.len());

    for parsed in &report.rows {
        match parsed.kind {
            ParsedKind::Buy => counts.buys += 1,
            ParsedKind::Sell => counts.sells += 1,
            ParsedKind::Split => counts.splits += 1,
        }

        match map_row(parsed) {
            Ok(mapped) => outcomes.push(RowOutcome::Mapped(mapped)),
            Err(err) => outcomes.push(RowOutcome::Error {
                asset_key: Some(format!(
                    "{}:{}",
                    parsed.market.trim().to_lowercase(),
                    parsed.code.trim().to_lowercase()
                )),
                note: RowNote {
                    row: Some(err.row),
                    code: err.code,
                    message: err.message,
                },
            }),
        }
    }

    PreparedImport {
        header: PlanHeader {
            title: report.metadata.title.clone(),
            date_from: report.metadata.date_from,
            date_to: report.metadata.date_to,
        },
        counts,
        outcomes,
    }
}
