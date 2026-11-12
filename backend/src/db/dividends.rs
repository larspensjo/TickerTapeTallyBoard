//! Dividend facts for a single price source. Mutations must share the closes' fenced write.
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::{SqliteConnection, SqlitePool};

use super::RepoError;
use crate::{
    domain::opportunity_evidence::dividend_check::{DividendCoverage, DividendEvent},
    providers::{DividendCapture, MarketDataProvider},
};

pub struct StoredDividends {
    pub events: Vec<DividendEvent>,
    pub coverage: Vec<DividendCoverage>,
    pub source_reports_dividends: bool,
}

#[derive(sqlx::FromRow)]
struct CoverageRow {
    start_date: String,
    end_date: String,
    outcome: String,
    reason: Option<String>,
}

fn date(value: &str) -> Result<NaiveDate, RepoError> {
    value
        .parse()
        .map_err(|error| RepoError::Decode(format!("dividend date {value}: {error}")))
}

/// Select the same source as the caller's close series; never combine providers.
pub async fn read(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: MarketDataProvider,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<StoredDividends, RepoError> {
    // Keep the events and coverage from one database snapshot.
    let mut tx = pool.begin().await?;
    let rows: Vec<(String, String, String)> = sqlx::query_as("SELECT ex_date, amount, currency FROM dividend_events WHERE instrument_id=? AND provider=? AND ex_date>=? AND ex_date<=? ORDER BY ex_date")
        .bind(instrument_id).bind(provider.as_str()).bind(start.to_string()).bind(end.to_string()).fetch_all(&mut *tx).await?;
    let events = rows
        .into_iter()
        .map(|(ex_date, amount, currency)| {
            Ok(DividendEvent {
                ex_date: date(&ex_date)?,
                amount: amount
                    .parse::<Decimal>()
                    .map_err(|error| RepoError::Decode(format!("dividend amount: {error}")))?,
                currency,
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;
    let rows: Vec<CoverageRow> = sqlx::query_as("SELECT c.start_date, c.end_date, c.outcome, c.reason FROM dividend_coverage c JOIN instrument_provider_symbols m ON m.id=c.mapping_id WHERE m.instrument_id=? AND m.provider=? AND c.start_date<=? AND c.end_date>=? ORDER BY c.start_date")
        .bind(instrument_id).bind(provider.as_str()).bind(end.to_string()).bind(start.to_string()).fetch_all(&mut *tx).await?;
    let coverage = rows
        .into_iter()
        .map(|row| {
            Ok(DividendCoverage {
                start: date(&row.start_date)?,
                end: date(&row.end_date)?,
                valid: row.outcome == "VALID",
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;
    tx.commit().await?;
    Ok(StoredDividends {
        events,
        coverage,
        source_reports_dividends: provider.reports_dividends(),
    })
}

pub async fn capture(
    conn: &mut SqliteConnection,
    instrument_id: i64,
    provider: MarketDataProvider,
    requested: std::ops::RangeInclusive<NaiveDate>,
    latest_close: Option<NaiveDate>,
    capture: &DividendCapture,
) -> Result<(), RepoError> {
    if matches!(capture, DividendCapture::NotReported) {
        return Ok(());
    }
    let (start, end) = (*requested.start(), *requested.end());
    let mapping_id: i64 = sqlx::query_scalar(
        "SELECT id FROM instrument_provider_symbols WHERE instrument_id=? AND provider=?",
    )
    .bind(instrument_id)
    .bind(provider.as_str())
    .fetch_one(&mut *conn)
    .await?;
    let (outcome, reason, coverage_end) = match capture {
        DividendCapture::Valid(events) => {
            sqlx::query("DELETE FROM dividend_events WHERE instrument_id=? AND provider=? AND ex_date>=? AND ex_date<=?")
                .bind(instrument_id).bind(provider.as_str()).bind(start.to_string()).bind(end.to_string()).execute(&mut *conn).await?;
            for event in events
                .iter()
                .filter(|event| event.ex_date >= start && event.ex_date <= end)
            {
                sqlx::query("INSERT INTO dividend_events (instrument_id, provider, ex_date, amount, currency) VALUES (?, ?, ?, ?, ?)")
                    .bind(instrument_id).bind(provider.as_str()).bind(event.ex_date.to_string()).bind(event.amount.to_string()).bind(&event.currency).execute(&mut *conn).await?;
            }
            // Event replacement uses the requested range, but absence is established
            // only through the latest close returned by this response.
            let Some(end) = latest_close
                .map(|latest| latest.min(end))
                .filter(|end| *end >= start)
            else {
                return Ok(());
            };
            // Subtract the successfully captured interval from every outstanding failure.
            let failures: Vec<CoverageRow> = sqlx::query_as("SELECT start_date, end_date, outcome, reason FROM dividend_coverage WHERE mapping_id=? AND outcome<>'VALID' AND start_date<=? AND end_date>=?")
                .bind(mapping_id).bind(end.to_string()).bind(start.to_string()).fetch_all(&mut *conn).await?;
            sqlx::query("DELETE FROM dividend_coverage WHERE mapping_id=? AND outcome<>'VALID' AND start_date<=? AND end_date>=?")
                .bind(mapping_id).bind(end.to_string()).bind(start.to_string()).execute(&mut *conn).await?;
            for failure in failures {
                let left = date(&failure.start_date)?;
                let right = date(&failure.end_date)?;
                if left < start {
                    insert_range(
                        conn,
                        mapping_id,
                        left,
                        start.pred_opt().expect("earlier failure date exists"),
                        &failure.outcome,
                        failure.reason.as_deref(),
                    )
                    .await?;
                }
                if right > end {
                    insert_range(
                        conn,
                        mapping_id,
                        end.succ_opt().expect("later failure date exists"),
                        right,
                        &failure.outcome,
                        failure.reason.as_deref(),
                    )
                    .await?;
                }
            }
            ("VALID", None, end)
        }
        DividendCapture::Invalid { reason } => ("INVALID", Some(reason.as_str()), end),
        DividendCapture::NotReported => unreachable!(),
    };
    insert_range(conn, mapping_id, start, coverage_end, outcome, reason).await
}

async fn insert_range(
    conn: &mut SqliteConnection,
    mapping_id: i64,
    mut start: NaiveDate,
    mut end: NaiveDate,
    outcome: &str,
    reason: Option<&str>,
) -> Result<(), RepoError> {
    // Expand to the connected union, including adjacency. Repeat because an
    // expanded range can reach an older range that the first query did not see.
    // Outcomes stay separate: failures never erase older valid coverage.
    loop {
        let ranges: Vec<CoverageRow> = sqlx::query_as("SELECT start_date, end_date, outcome, reason FROM dividend_coverage WHERE mapping_id=? AND outcome=? AND start_date<=? AND end_date>=?")
            .bind(mapping_id).bind(outcome)
            .bind(end.succ_opt().unwrap_or(end).to_string())
            .bind(start.pred_opt().unwrap_or(start).to_string())
            .fetch_all(&mut *conn).await?;
        if ranges.is_empty() {
            break;
        }
        for range in ranges {
            start = start.min(date(&range.start_date)?);
            end = end.max(date(&range.end_date)?);
            sqlx::query("DELETE FROM dividend_coverage WHERE mapping_id=? AND start_date=? AND end_date=? AND outcome=?")
                .bind(mapping_id).bind(&range.start_date).bind(&range.end_date).bind(outcome)
                .execute(&mut *conn).await?;
        }
    }
    // For merged failures retain the most recently recorded reason; the outcome
    // and covered days, rather than the reason, determine domain evidence.
    sqlx::query("INSERT INTO dividend_coverage (mapping_id, start_date, end_date, outcome, reason) VALUES (?, ?, ?, ?, ?) ON CONFLICT (mapping_id, start_date, end_date, outcome) DO UPDATE SET reason=excluded.reason")
        .bind(mapping_id).bind(start.to_string()).bind(end.to_string()).bind(outcome).bind(reason).execute(conn).await?;
    Ok(())
}
