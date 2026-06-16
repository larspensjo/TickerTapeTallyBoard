use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::sqlite::SqlitePool;

use crate::db::RepoError;

const LIST_SQL: &str =
    "SELECT id, instrument_id, provider, provider_symbol, date, close, currency, fetched_at \
    FROM prices ORDER BY instrument_id, provider, date";
const FIND_SQL: &str =
    "SELECT id, instrument_id, provider, provider_symbol, date, close, currency, fetched_at \
    FROM prices WHERE id = ?";
const FIND_BY_KEY_SQL: &str =
    "SELECT id, instrument_id, provider, provider_symbol, date, close, currency, fetched_at \
    FROM prices WHERE instrument_id = ? AND provider = ? AND date = ?";
const FIND_LATEST_ON_OR_BEFORE_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, date, close, currency, fetched_at \
    FROM prices WHERE instrument_id = ? AND provider = ? AND date <= ? ORDER BY date DESC, id DESC LIMIT 1";
const FIND_PREVIOUS_BEFORE_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, date, close, currency, fetched_at \
    FROM prices WHERE instrument_id = ? AND provider = ? AND date < ? ORDER BY date DESC, id DESC LIMIT 1";
const UPSERT_SQL: &str = "INSERT INTO prices \
       (instrument_id, provider, provider_symbol, date, close, currency, fetched_at) \
     VALUES (?, ?, ?, ?, ?, ?, ?) \
     ON CONFLICT (instrument_id, provider, date) DO UPDATE SET \
       provider_symbol = excluded.provider_symbol, \
       close = excluded.close, \
       currency = excluded.currency, \
       fetched_at = excluded.fetched_at \
     RETURNING id, instrument_id, provider, provider_symbol, date, close, currency, fetched_at";

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct PriceRow {
    pub id: i64,
    pub instrument_id: i64,
    pub provider: String,
    pub provider_symbol: String,
    pub date: String,
    pub close: String,
    pub currency: String,
    pub fetched_at: String,
}

impl PriceRow {
    pub fn date_value(&self) -> Result<NaiveDate, RepoError> {
        NaiveDate::parse_from_str(&self.date, "%Y-%m-%d")
            .map_err(|error| RepoError::Decode(format!("bad price date {:?}: {error}", self.date)))
    }

    pub fn close_decimal(&self) -> Result<Decimal, RepoError> {
        Decimal::from_str(&self.close).map_err(|error| {
            RepoError::Decode(format!("bad price close {:?}: {error}", self.close))
        })
    }
}

#[derive(Clone, Debug)]
pub struct NewPrice {
    pub instrument_id: i64,
    pub provider: String,
    pub provider_symbol: String,
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: String,
    pub fetched_at: String,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<PriceRow>, RepoError> {
    let rows = sqlx::query_as::<_, PriceRow>(LIST_SQL)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<PriceRow>, RepoError> {
    let row = sqlx::query_as::<_, PriceRow>(FIND_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_by_key(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: &str,
    date: NaiveDate,
) -> Result<Option<PriceRow>, RepoError> {
    let row = sqlx::query_as::<_, PriceRow>(FIND_BY_KEY_SQL)
        .bind(instrument_id)
        .bind(provider)
        .bind(date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_latest_on_or_before(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: &str,
    as_of_date: NaiveDate,
) -> Result<Option<PriceRow>, RepoError> {
    let row = sqlx::query_as::<_, PriceRow>(FIND_LATEST_ON_OR_BEFORE_SQL)
        .bind(instrument_id)
        .bind(provider)
        .bind(as_of_date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_previous_before(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: &str,
    before_date: NaiveDate,
) -> Result<Option<PriceRow>, RepoError> {
    let row = sqlx::query_as::<_, PriceRow>(FIND_PREVIOUS_BEFORE_SQL)
        .bind(instrument_id)
        .bind(provider)
        .bind(before_date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn upsert(pool: &SqlitePool, new: &NewPrice) -> Result<PriceRow, RepoError> {
    let row = sqlx::query_as::<_, PriceRow>(UPSERT_SQL)
        .bind(new.instrument_id)
        .bind(&new.provider)
        .bind(&new.provider_symbol)
        .bind(new.date.format("%Y-%m-%d").to_string())
        .bind(new.close.to_string())
        .bind(&new.currency)
        .bind(&new.fetched_at)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::{instruments, testing};

    #[tokio::test]
    async fn price_upsert_is_idempotent_and_latest_lookup_uses_dates() {
        let pool = testing::memory_pool().await;
        let instrument_id = seed_instrument(&pool).await;

        let first = upsert(
            &pool,
            &NewPrice {
                instrument_id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "MSFT".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid"),
                close: Decimal::new(1000, 2),
                currency: "USD".to_owned(),
                fetched_at: "2026-06-16T08:00:00Z".to_owned(),
            },
        )
        .await
        .expect("first price upsert should succeed");

        let second = upsert(
            &pool,
            &NewPrice {
                instrument_id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "MSFT".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
                close: Decimal::new(1125, 2),
                currency: "USD".to_owned(),
                fetched_at: "2026-06-16T08:05:00Z".to_owned(),
            },
        )
        .await
        .expect("second price upsert should succeed");

        let updated = upsert(
            &pool,
            &NewPrice {
                instrument_id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "MSFTX".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
                close: Decimal::new(1135, 2),
                currency: "USD".to_owned(),
                fetched_at: "2026-06-16T08:10:00Z".to_owned(),
            },
        )
        .await
        .expect("updated price upsert should succeed");

        assert_ne!(first.id, second.id);
        assert_eq!(second.id, updated.id);
        assert_eq!(updated.provider_symbol, "MSFTX");
        assert_eq!(updated.close, "11.35");

        let rows = list(&pool).await.expect("list should succeed");
        assert_eq!(rows.len(), 2);

        let latest = find_latest_on_or_before(
            &pool,
            instrument_id,
            "YAHOO",
            NaiveDate::from_ymd_opt(2026, 6, 11).expect("date should be valid"),
        )
        .await
        .expect("latest lookup should succeed")
        .expect("latest row should exist");
        assert_eq!(latest.date, "2026-06-10");
        assert_eq!(latest.close, "10.00");
        assert_eq!(
            latest.date_value().expect("date should decode"),
            NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid")
        );
        assert_eq!(
            latest.close_decimal().expect("close should decode"),
            Decimal::new(1000, 2)
        );

        let inclusive_latest = find_latest_on_or_before(
            &pool,
            instrument_id,
            "YAHOO",
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
        )
        .await
        .expect("inclusive latest lookup should succeed")
        .expect("inclusive latest row should exist");
        assert_eq!(inclusive_latest.date, "2026-06-12");

        let prior = find_previous_before(
            &pool,
            instrument_id,
            "YAHOO",
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
        )
        .await
        .expect("previous lookup should succeed")
        .expect("prior row should exist");
        assert_eq!(prior.date, "2026-06-10");

        let same_day = find_by_key(
            &pool,
            instrument_id,
            "YAHOO",
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
        )
        .await
        .expect("key lookup should succeed")
        .expect("same-day row should exist");
        assert_eq!(same_day.provider_symbol, "MSFTX");
        assert_eq!(same_day.close, "11.35");

        let no_latest = find_latest_on_or_before(
            &pool,
            instrument_id,
            "YAHOO",
            NaiveDate::from_ymd_opt(2026, 6, 9).expect("date should be valid"),
        )
        .await
        .expect("empty latest lookup should succeed");
        assert!(no_latest.is_none());

        let no_previous = find_previous_before(
            &pool,
            instrument_id,
            "YAHOO",
            NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid"),
        )
        .await
        .expect("empty previous lookup should succeed");
        assert!(no_previous.is_none());
    }

    #[tokio::test]
    async fn price_constraints_are_enforced() {
        let pool = testing::memory_pool().await;
        let instrument_id = seed_instrument(&pool).await;

        let invalid_provider = sqlx::query(
            "INSERT INTO prices (instrument_id, provider, provider_symbol, date, close, currency, fetched_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(instrument_id)
        .bind("BAD")
        .bind("MSFT")
        .bind("2026-06-10")
        .bind("10.0")
        .bind("USD")
        .bind("2026-06-16T08:00:00Z")
        .execute(&pool)
        .await
        .expect_err("invalid provider should fail");

        assert!(is_constraint_error(&invalid_provider, "CHECK"));

        let missing_instrument = sqlx::query(
            "INSERT INTO prices (instrument_id, provider, provider_symbol, date, close, currency, fetched_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(999_i64)
        .bind("YAHOO")
        .bind("MSFT")
        .bind("2026-06-10")
        .bind("10.0")
        .bind("USD")
        .bind("2026-06-16T08:00:00Z")
        .execute(&pool)
        .await
        .expect_err("missing instrument should fail");

        assert!(is_constraint_error(&missing_instrument, "FOREIGN KEY"));
    }

    async fn seed_instrument(pool: &sqlx::sqlite::SqlitePool) -> i64 {
        let instrument = instruments::upsert(
            pool,
            &instruments::NewInstrument {
                symbol: "MSFT".to_owned(),
                exchange: "NASDAQ".to_owned(),
                name: "Microsoft".to_owned(),
                kind: "STOCK".to_owned(),
                currency: "USD".to_owned(),
            },
        )
        .await
        .expect("instrument upsert should succeed");

        instrument.0.id
    }

    fn is_constraint_error(error: &sqlx::Error, needle: &str) -> bool {
        matches!(error, sqlx::Error::Database(database_error) if database_error.message().contains(needle))
    }
}
