use super::{fundamentals_sources::code, RepoError};
use crate::domain::{fundamentals::PeriodType, report_dates::*};
use chrono::NaiveDate;
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsTicker {
    pub symbol: String,
    pub link_generation: Option<i64>,
}

/// Re-derived inside every fenced write, including listing currency and enabled
/// price mapping. The link generation matters only when that link is the source.
pub async fn current_ticker(
    conn: &mut SqliteConnection,
    id: i64,
) -> Result<Option<UsTicker>, RepoError> {
    let row = sqlx::query("SELECT i.currency, p.provider_symbol AS price_symbol, p.enabled, f.provider_symbol AS figures_symbol, f.generation, f.status FROM instruments i LEFT JOIN instrument_provider_symbols p ON p.instrument_id=i.id AND p.provider=? LEFT JOIN instrument_fundamentals_sources f ON f.instrument_id=i.id WHERE i.id=?")
        .bind(crate::providers::MarketDataProvider::Yahoo.as_str()).bind(id).fetch_optional(&mut *conn).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let status: Option<String> = row.try_get("status")?;
    if status
        .as_deref()
        .map(|s| code(s, crate::domain::fundamentals::LinkStatus::from_db_str))
        .transpose()?
        == Some(crate::domain::fundamentals::LinkStatus::NotApplicable)
    {
        return Ok(None);
    }
    let price: Option<String> = row.try_get("price_symbol")?;
    let figures: Option<String> = row.try_get("figures_symbol")?;
    let enabled: Option<bool> = row.try_get("enabled")?;
    let currency: String = row.try_get("currency")?;
    let yahoo = price.as_deref().filter(|_| enabled == Some(true));
    let generation: Option<i64> = row.try_get("generation")?;
    Ok(
        us_ticker(yahoo, figures.as_deref(), &currency).map(|symbol| UsTicker {
            symbol: symbol.to_owned(),
            link_generation: if us_ticker(yahoo, None, &currency).is_some() {
                None
            } else {
                generation
            },
        }),
    )
}
pub async fn ticker(pool: &SqlitePool, id: i64) -> Result<Option<UsTicker>, RepoError> {
    current_ticker(&mut *pool.acquire().await?, id).await
}

fn date(raw: &str) -> Result<NaiveDate, RepoError> {
    raw.parse()
        .map_err(|e| RepoError::Decode(format!("invalid report date {raw}: {e}")))
}
fn optional_date(
    row: &sqlx::sqlite::SqliteRow,
    name: &str,
) -> Result<Option<NaiveDate>, RepoError> {
    row.try_get::<Option<String>, _>(name)?
        .map(|s| date(&s))
        .transpose()
}
#[cfg(test)]
pub async fn list<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    id: i64,
) -> Result<Vec<Announcement>, RepoError> {
    read_entries(executor, id, None).await
}

/// All schedule consumers use the current identity and exclude set-aside history.
pub async fn current(pool: &SqlitePool, id: i64) -> Result<Vec<Announcement>, RepoError> {
    let mut conn = pool.acquire().await?;
    match current_ticker(&mut conn, id).await? {
        Some(ticker) => for_ticker(&mut *conn, id, &ticker).await,
        None => Ok(vec![]),
    }
}

pub async fn for_ticker<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    id: i64,
    ticker: &UsTicker,
) -> Result<Vec<Announcement>, RepoError> {
    read_entries(executor, id, Some(&ticker.symbol)).await
}

async fn read_entries<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    id: i64,
    symbol: Option<&str>,
) -> Result<Vec<Announcement>, RepoError> {
    sqlx::query("SELECT * FROM report_announcements WHERE instrument_id=? AND (? IS NULL OR (provider_symbol=? AND set_aside_at IS NULL)) ORDER BY id")
        .bind(id)
        .bind(symbol)
        .bind(symbol)
        .fetch_all(executor)
        .await?
        .into_iter()
        .map(|r| {
            Ok(Announcement {
                id: r.try_get("id")?,
                instrument_id: id,
                source: code(
                    &r.try_get::<String, _>("source")?,
                    ReportCalendarSource::from_db_str,
                )?,
                symbol: r.try_get("provider_symbol")?,
                report: AnnouncementDate {
                    date: date(&r.try_get::<String, _>("scheduled_date")?)?,
                    kind: code(
                        &r.try_get::<String, _>("kind")?,
                        AnnouncementKind::from_db_str,
                    )?,
                    fiscal_quarter: optional_date(&r, "fiscal_quarter_ending")?,
                    session: r
                        .try_get::<Option<String>, _>("session")?
                        .map(|s| code(&s, ReportSession::from_db_str))
                        .transpose()?,
                },
                first_seen: date(&r.try_get::<String, _>("first_seen_on")?)?,
                last_seen: date(&r.try_get::<String, _>("last_seen_on")?)?,
                baseline_period_end: optional_date(&r, "baseline_period_end")?,
                state: code(
                    &r.try_get::<String, _>("state")?,
                    AnnouncementState::from_db_str,
                )?,
                resolved_period_end: optional_date(&r, "resolved_period_end")?,
                superseded_by: r.try_get("superseded_by")?,
            })
        })
        .collect()
}

/// Called only inside a lease fence after rechecking the identity. History stays
/// stored; an obsolete symbol can no longer drive resolution or scan planning.
pub async fn supersede_other_symbols(
    conn: &mut SqliteConnection,
    id: i64,
    ticker: Option<&UsTicker>,
) -> Result<(), RepoError> {
    sqlx::query("UPDATE report_announcements SET state=? WHERE instrument_id=? AND state=? AND (set_aside_at IS NOT NULL OR ? IS NULL OR provider_symbol<>?)")
        .bind(AnnouncementState::Superseded.as_str()).bind(id).bind(AnnouncementState::Active.as_str())
        .bind(ticker.map(|t| &t.symbol)).bind(ticker.map(|t| &t.symbol)).execute(conn).await?;
    Ok(())
}

pub(super) async fn set_aside(
    conn: &mut SqliteConnection,
    id: i64,
    at: &str,
) -> Result<(), RepoError> {
    sqlx::query("UPDATE report_announcements SET set_aside_at=? WHERE instrument_id=? AND set_aside_at IS NULL")
        .bind(at).bind(id).execute(conn).await?;
    Ok(())
}
pub async fn lookup(
    pool: &SqlitePool,
    id: i64,
    ticker: &UsTicker,
) -> Result<LookupState, RepoError> {
    let row = sqlx::query("SELECT * FROM report_date_lookups WHERE instrument_id=? AND source=?")
        .bind(id)
        .bind(ReportCalendarSource::NasdaqUs.as_str())
        .fetch_optional(pool)
        .await?;
    let Some(r) = row else {
        return Ok(LookupState::default());
    };
    let outcome = code(
        &r.try_get::<String, _>("last_outcome")?,
        LookupOutcome::from_db_str,
    )?;
    if r.try_get::<String, _>("provider_symbol")? != ticker.symbol {
        return Ok(LookupState::default());
    }
    Ok(LookupState {
        last_lookup_day: Some(date(&r.try_get::<String, _>("last_lookup_day")?)?),
        last_success: optional_date(&r, "last_success_on")?,
        last_outcome: Some(outcome),
    })
}
pub async fn record_lookup(
    conn: &mut SqliteConnection,
    id: i64,
    ticker: &UsTicker,
    today: NaiveDate,
    outcome: LookupOutcome,
) -> Result<(), RepoError> {
    sqlx::query("INSERT INTO report_date_lookups (instrument_id, source, provider_symbol, last_lookup_day, last_success_on, last_outcome) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(instrument_id, source) DO UPDATE SET provider_symbol=excluded.provider_symbol, last_lookup_day=excluded.last_lookup_day, last_success_on=CASE WHEN excluded.last_success_on IS NOT NULL THEN excluded.last_success_on WHEN provider_symbol=excluded.provider_symbol THEN last_success_on ELSE NULL END, last_outcome=excluded.last_outcome")
        .bind(id).bind(ReportCalendarSource::NasdaqUs.as_str()).bind(&ticker.symbol).bind(today.to_string()).bind((outcome != LookupOutcome::Failed).then(|| today.to_string())).bind(outcome.as_str()).execute(conn).await?;
    Ok(())
}
pub async fn scanned_today<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    today: NaiveDate,
) -> Result<BTreeSet<NaiveDate>, RepoError> {
    sqlx::query(
        "SELECT source, calendar_date, outcome FROM report_calendar_scans WHERE scanned_on>=?",
    )
    .bind(today.to_string())
    .fetch_all(executor)
    .await?
    .into_iter()
    .map(|r| {
        code(
            &r.try_get::<String, _>("source")?,
            ReportCalendarSource::from_db_str,
        )?;
        code(
            &r.try_get::<String, _>("outcome")?,
            crate::domain::fundamentals::AttemptStatus::from_db_str,
        )?;
        date(&r.try_get::<String, _>("calendar_date")?)
    })
    .collect()
}
pub async fn record_scan(
    conn: &mut SqliteConnection,
    day: NaiveDate,
    today: NaiveDate,
    at: &str,
    outcome: crate::domain::fundamentals::AttemptStatus,
) -> Result<(), RepoError> {
    sqlx::query("INSERT INTO report_calendar_scans (source, calendar_date, scanned_at, scanned_on, outcome, last_success_at, last_success_on) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(source, calendar_date) DO UPDATE SET scanned_at=excluded.scanned_at, scanned_on=excluded.scanned_on, outcome=excluded.outcome, last_success_at=COALESCE(excluded.last_success_at, last_success_at), last_success_on=COALESCE(excluded.last_success_on, last_success_on)")
        .bind(ReportCalendarSource::NasdaqUs.as_str()).bind(day.to_string()).bind(at).bind(today.to_string()).bind(outcome.as_str())
        .bind((outcome == crate::domain::fundamentals::AttemptStatus::Succeeded).then_some(at))
        .bind((outcome == crate::domain::fundamentals::AttemptStatus::Succeeded).then(|| today.to_string())).execute(conn).await?;
    Ok(())
}
pub async fn observations<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    id: i64,
) -> Result<Vec<Observation>, RepoError> {
    Ok(
        super::fundamental_values::active_observations(executor, id, Some(PeriodType::Quarter))
            .await?
            .into_iter()
            .filter(|o| o.period_type == PeriodType::Quarter)
            .map(|o| Observation {
                period_end: o.period_end,
                first_seen: o.first_seen_on,
                absent_on: o.absent_fetch_on,
            })
            .collect(),
    )
}

/// Apply a pure plan within the caller's lease-verified transaction. Insert the
/// replacement first so every superseded entry can name its successor.
pub async fn apply(
    conn: &mut SqliteConnection,
    id: i64,
    ticker: &UsTicker,
    today: NaiveDate,
    changes: &[AnnouncementChange],
) -> Result<(), RepoError> {
    if changes.is_empty() {
        return Ok(());
    }
    let baseline = observations(&mut *conn, id)
        .await?
        .iter()
        .map(|o| o.period_end)
        .max();
    let mut inserted = changes.iter().find_map(|c| match c {
        AnnouncementChange::Seen { id, .. } => Some(*id),
        _ => None,
    });
    for change in changes {
        if let AnnouncementChange::Insert(report) = change {
            inserted = Some(sqlx::query("INSERT INTO report_announcements (instrument_id, source, provider_symbol, scheduled_date, kind, fiscal_quarter_ending, session, first_seen_on, last_seen_on, baseline_period_end, state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(id).bind(ReportCalendarSource::NasdaqUs.as_str()).bind(&ticker.symbol).bind(report.date.to_string()).bind(report.kind.as_str()).bind(report.fiscal_quarter.map(|d| d.to_string())).bind(report.session.map(ReportSession::as_str)).bind(today.to_string()).bind(today.to_string()).bind(baseline.map(|d| d.to_string())).bind(AnnouncementState::Active.as_str()).execute(&mut *conn).await?.last_insert_rowid());
        }
    }
    for change in changes {
        match change {
            AnnouncementChange::Insert(_) => {}
            AnnouncementChange::Seen { id, report } => {
                sqlx::query("UPDATE report_announcements SET kind=?, fiscal_quarter_ending=?, session=?, last_seen_on=? WHERE id=?")
                .bind(report.kind.as_str()).bind(report.fiscal_quarter.map(|d| d.to_string())).bind(report.session.map(ReportSession::as_str)).bind(today.to_string()).bind(id).execute(&mut *conn).await?;
            }
            AnnouncementChange::Supersede { id } => {
                sqlx::query("UPDATE report_announcements SET state=?, superseded_by=? WHERE id=?")
                    .bind(AnnouncementState::Superseded.as_str())
                    .bind(inserted)
                    .bind(id)
                    .execute(&mut *conn)
                    .await?;
            }
            AnnouncementChange::Withdraw { id } => {
                sqlx::query("UPDATE report_announcements SET state=? WHERE id=?")
                    .bind(AnnouncementState::Withdrawn.as_str())
                    .bind(id)
                    .execute(&mut *conn)
                    .await?;
            }
            AnnouncementChange::Resolve { id, period_end } => {
                sqlx::query(
                    "UPDATE report_announcements SET state=?, resolved_period_end=? WHERE id=?",
                )
                .bind(AnnouncementState::Resolved.as_str())
                .bind(period_end.to_string())
                .bind(id)
                .execute(&mut *conn)
                .await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
