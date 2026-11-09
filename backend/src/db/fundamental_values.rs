use super::{
    fundamentals_sources::{
        self, AttemptStatus, GenerationWrite, LinkDefinition, LinkOrigin, LinkToken, SourceLink,
    },
    RepoError,
};
use crate::domain::fundamentals::{FundamentalValue, SplitEvent};
pub use crate::domain::fundamentals::{FundamentalsSource, Metric, PeriodType};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::{
    sqlite::{SqliteConnection, SqlitePool},
    Row,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct StoredValue {
    pub input: FundamentalValue,
    pub source: FundamentalsSource,
    pub provider_symbol: String,
    pub first_seen_at: String,
    pub updated_at: String,
}
#[derive(Clone, Debug)]
pub struct PeriodObservation {
    pub period_type: PeriodType,
    pub period_end: NaiveDate,
    pub provider_symbol: String,
    pub first_seen_on: NaiveDate,
    pub absent_fetch_on: Option<NaiveDate>,
}
fn date(raw: &str) -> Result<NaiveDate, RepoError> {
    raw.parse()
        .map_err(|e| RepoError::Decode(format!("invalid fundamentals date {raw}: {e}")))
}

/// Winner is the latest active revision across retained same-company symbols.
pub async fn active_values(
    pool: &SqlitePool,
    instrument_id: i64,
) -> Result<Vec<StoredValue>, RepoError> {
    let rows = sqlx::query("SELECT * FROM fundamental_values WHERE instrument_id=? AND set_aside_at IS NULL ORDER BY updated_at, id").bind(instrument_id).fetch_all(pool).await?;
    let mut winners = BTreeMap::new();
    for row in rows {
        let period: String = row.try_get("period_type")?;
        let end: String = row.try_get("period_end")?;
        let metric: String = row.try_get("metric")?;
        let value: String = row.try_get("value")?;
        let stored = StoredValue {
            input: FundamentalValue {
                period_type: fundamentals_sources::code(&period, PeriodType::from_db_str)?,
                period_end: date(&end)?,
                metric: fundamentals_sources::code(&metric, Metric::from_db_str)?,
                value: value
                    .parse::<Decimal>()
                    .map_err(|e| RepoError::Decode(e.to_string()))?,
                currency: row.try_get("currency")?,
            },
            source: fundamentals_sources::code(
                &row.try_get::<String, _>("source")?,
                FundamentalsSource::from_db_str,
            )?,
            provider_symbol: row.try_get("provider_symbol")?,
            first_seen_at: row.try_get("first_seen_at")?,
            updated_at: row.try_get("updated_at")?,
        };
        winners.insert((period, end, metric), stored);
    }
    Ok(winners.into_values().collect())
}

pub async fn has_active_values<'e, E>(executor: E, instrument_id: i64) -> Result<bool, RepoError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM fundamental_values WHERE instrument_id=? AND set_aside_at IS NULL)").bind(instrument_id).fetch_one(executor).await?)
}

pub async fn has_set_aside_values(
    pool: &SqlitePool,
    instrument_id: i64,
) -> Result<bool, RepoError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM fundamental_values WHERE instrument_id=? AND set_aside_at IS NOT NULL)")
        .bind(instrument_id).fetch_one(pool).await?)
}

pub async fn active_observations<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    instrument_id: i64,
    period_type: Option<PeriodType>,
) -> Result<Vec<PeriodObservation>, RepoError> {
    let rows = sqlx::query("SELECT * FROM fundamental_period_observations WHERE instrument_id=? AND set_aside_at IS NULL AND (? IS NULL OR period_type=?) ORDER BY first_seen_on, id").bind(instrument_id).bind(period_type.map(PeriodType::as_str)).bind(period_type.map(PeriodType::as_str)).fetch_all(executor).await?;
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for row in rows {
        let period: String = row.try_get("period_type")?;
        let end: String = row.try_get("period_end")?;
        let period_type = fundamentals_sources::code(&period, PeriodType::from_db_str)?;
        if !seen.insert((period, end.clone())) {
            continue;
        }
        result.push(PeriodObservation {
            period_type,
            period_end: date(&end)?,
            provider_symbol: row.try_get("provider_symbol")?,
            first_seen_on: date(&row.try_get::<String, _>("first_seen_on")?)?,
            absent_fetch_on: row
                .try_get::<Option<String>, _>("absent_fetch_on")?
                .map(|s| date(&s))
                .transpose()?,
        });
    }
    Ok(result)
}

pub async fn report_currencies(
    pool: &SqlitePool,
    instrument_id: i64,
) -> Result<BTreeSet<String>, RepoError> {
    Ok(active_values(pool, instrument_id)
        .await?
        .into_iter()
        .filter(|v| {
            !matches!(
                v.input.metric,
                Metric::MarketCap | Metric::DilutedShares | Metric::OrdinaryShares
            )
        })
        .filter_map(|v| v.input.currency)
        .collect())
}
pub async fn active_splits(
    pool: &SqlitePool,
    instrument_id: i64,
) -> Result<Vec<SplitEvent>, RepoError> {
    let rows = sqlx::query("SELECT split_date, numerator, denominator FROM fundamental_split_events WHERE instrument_id=? AND set_aside_at IS NULL ORDER BY split_date").bind(instrument_id).fetch_all(pool).await?;
    rows.into_iter()
        .map(|r| {
            Ok(SplitEvent {
                date: date(&r.try_get::<String, _>("split_date")?)?,
                numerator: r
                    .try_get::<String, _>("numerator")?
                    .parse()
                    .map_err(|e: rust_decimal::Error| RepoError::Decode(e.to_string()))?,
                denominator: r
                    .try_get::<String, _>("denominator")?
                    .parse()
                    .map_err(|e: rust_decimal::Error| RepoError::Decode(e.to_string()))?,
            })
        })
        .collect()
}

pub struct FetchWrite {
    pub instrument_id: i64,
    pub expected: Option<LinkToken>,
    pub discovery: Option<LinkDefinition>,
    pub verified: Option<LinkDefinition>,
    pub values: Vec<FundamentalValue>,
    pub splits: Vec<SplitEvent>,
    pub at: String,
    pub seen_on: NaiveDate,
    pub previous_success_on: Option<NaiveDate>,
    pub status: AttemptStatus,
    pub reason: Option<String>,
}

/// The caller must hold the refresh lease through fenced_write. All derived
/// rows and bookkeeping share this generation check and transaction.
pub async fn write_fetch(
    conn: &mut SqliteConnection,
    write: &FetchWrite,
) -> Result<GenerationWrite<SourceLink>, RepoError> {
    if !fundamentals_sources::matches(conn, write.instrument_id, write.expected.as_ref()).await? {
        return Ok(GenerationWrite::Superseded);
    }
    let old = fundamentals_sources::find(&mut *conn, write.instrument_id).await?;
    if write.discovery.is_some()
        && old
            .as_ref()
            .is_some_and(|s| s.definition.origin == LinkOrigin::Manual)
    {
        return Ok(GenerationWrite::Superseded);
    }
    let link = if let Some(definition) = &write.discovery {
        fundamentals_sources::replace(conn, write.instrument_id, definition).await?
    } else {
        old.clone()
            .ok_or_else(|| RepoError::Decode("fetch without a link".to_owned()))?
    };
    // Verification may enrich an existing link, but must never replace its identity.
    if let Some(verified) = &write.verified {
        if verified.source != link.definition.source
            || verified.symbol != link.definition.symbol
            || verified.origin != link.definition.origin
        {
            return Err(RepoError::Decode(
                "verification changed link identity".to_owned(),
            ));
        }
        if verified != &link.definition {
            sqlx::query("UPDATE instrument_fundamentals_sources SET security_type=?, classification=?, status=?, generation=generation+1 WHERE instrument_id=?")
                .bind(&verified.security_type).bind(verified.classification.as_str()).bind(verified.status.as_str()).bind(write.instrument_id).execute(&mut *conn).await?;
        }
    }
    let symbol = link.definition.symbol.as_deref();
    if let (Some(symbol), Some(source)) = (symbol, link.definition.source) {
        let day = write.seen_on.to_string();
        let absent_day = write
            .previous_success_on
            .filter(|_| write.discovery.is_none())
            .map(|date| date.to_string());
        for value in &write.values {
            sqlx::query("INSERT INTO fundamental_values (instrument_id, source, provider_symbol, period_type, period_end, metric, value, currency, first_seen_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(instrument_id, source, provider_symbol, period_type, period_end, metric) WHERE set_aside_at IS NULL DO UPDATE SET value=excluded.value, currency=excluded.currency, updated_at=excluded.updated_at")
                .bind(write.instrument_id).bind(source.as_str()).bind(symbol).bind(value.period_type.as_str()).bind(value.period_end.to_string()).bind(value.metric.as_str()).bind(value.value.to_string()).bind(&value.currency).bind(&write.at).bind(&write.at).execute(&mut *conn).await?;
            if value.period_type == PeriodType::Quarter {
                sqlx::query("INSERT INTO fundamental_period_observations (instrument_id, period_type, period_end, provider_symbol, first_seen_on, absent_fetch_on) SELECT ?, ?, ?, ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM fundamental_period_observations WHERE instrument_id=? AND period_type=? AND period_end=? AND set_aside_at IS NULL) ON CONFLICT DO NOTHING")
                    .bind(write.instrument_id).bind(value.period_type.as_str()).bind(value.period_end.to_string()).bind(symbol).bind(&day).bind(&absent_day)
                    .bind(write.instrument_id).bind(value.period_type.as_str()).bind(value.period_end.to_string()).execute(&mut *conn).await?;
            }
        }
        for split in &write.splits {
            sqlx::query("INSERT INTO fundamental_split_events (instrument_id, provider_symbol, split_date, numerator, denominator, updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(instrument_id, provider_symbol, split_date) WHERE set_aside_at IS NULL DO UPDATE SET numerator=excluded.numerator, denominator=excluded.denominator, updated_at=excluded.updated_at")
                .bind(write.instrument_id).bind(symbol).bind(split.date.to_string()).bind(split.numerator.to_string()).bind(split.denominator.to_string()).bind(&write.at).execute(&mut *conn).await?;
        }
    }
    sqlx::query("UPDATE instrument_fundamentals_sources SET last_attempt_at=?, last_attempt_status=?, last_attempt_reason=?, last_success_at=CASE WHEN ? THEN ? ELSE last_success_at END, consecutive_empty=CASE WHEN ? THEN consecutive_empty+1 WHEN ? THEN 0 ELSE consecutive_empty END WHERE instrument_id=?")
        .bind(&write.at).bind(write.status.as_str()).bind(&write.reason).bind(write.status != AttemptStatus::Failed).bind(&write.at).bind(write.status == AttemptStatus::Empty).bind(write.status == AttemptStatus::Succeeded).bind(write.instrument_id).execute(&mut *conn).await?;
    Ok(GenerationWrite::Applied(
        fundamentals_sources::find(&mut *conn, write.instrument_id)
            .await?
            .expect("written link"),
    ))
}

pub(super) async fn set_aside(
    conn: &mut SqliteConnection,
    instrument_id: i64,
    at: &str,
) -> Result<(), RepoError> {
    for statement in [
        "UPDATE fundamental_values SET set_aside_at=?, set_aside_reason='wrong company' WHERE instrument_id=? AND set_aside_at IS NULL",
        "UPDATE fundamental_period_observations SET set_aside_at=?, set_aside_reason='wrong company' WHERE instrument_id=? AND set_aside_at IS NULL",
        "UPDATE fundamental_split_events SET set_aside_at=?, set_aside_reason='wrong company' WHERE instrument_id=? AND set_aside_at IS NULL",
    ] { sqlx::query(statement).bind(at).bind(instrument_id).execute(&mut *conn).await?; }
    Ok(())
}

#[cfg(test)]
mod tests;

/// Explicit whole-instrument deletion follows the existing cache cleanup
/// contract. Link correction uses set_aside instead and never calls this.
pub async fn delete_by_instrument_in_tx(
    conn: &mut SqliteConnection,
    instrument_id: i64,
) -> Result<(), RepoError> {
    for sql in [
        "DELETE FROM fundamental_values WHERE instrument_id=?",
        "DELETE FROM fundamental_period_observations WHERE instrument_id=?",
        "DELETE FROM fundamental_split_events WHERE instrument_id=?",
        "DELETE FROM instrument_fundamentals_sources WHERE instrument_id=?",
    ] {
        sqlx::query(sql)
            .bind(instrument_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
