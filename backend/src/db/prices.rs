use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::sqlite::SqlitePool;

use crate::{db::RepoError, providers::MarketDataProvider};

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
const LIST_IN_RANGE_SQL: &str =
    "SELECT id, instrument_id, provider, provider_symbol, date, close, currency, fetched_at \
    FROM prices \
    WHERE instrument_id = ? AND provider = ? \
      AND (? IS NULL OR date >= ?) \
      AND (? IS NULL OR date <= ?) \
    ORDER BY date ASC, id ASC";
const DELETE_BY_INSTRUMENT_SQL: &str = "DELETE FROM prices WHERE instrument_id = ?";
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
struct RawPriceRow {
    pub id: i64,
    pub instrument_id: i64,
    pub provider: String,
    pub provider_symbol: String,
    pub date: String,
    pub close: String,
    pub currency: String,
    pub fetched_at: String,
}

#[derive(Clone, Debug)]
pub struct PriceRow {
    pub id: i64,
    pub instrument_id: i64,
    pub provider: MarketDataProvider,
    pub provider_symbol: String,
    pub date: String,
    pub close: String,
    pub currency: String,
    pub fetched_at: String,
}

impl TryFrom<RawPriceRow> for PriceRow {
    type Error = RepoError;

    fn try_from(row: RawPriceRow) -> Result<Self, Self::Error> {
        let provider = MarketDataProvider::from_db_str(&row.provider).ok_or_else(|| {
            RepoError::Decode(format!(
                "unknown price provider {:?} in row {} for instrument {}",
                row.provider, row.id, row.instrument_id
            ))
        })?;
        Ok(Self {
            id: row.id,
            instrument_id: row.instrument_id,
            provider,
            provider_symbol: row.provider_symbol,
            date: row.date,
            close: row.close,
            currency: row.currency,
            fetched_at: row.fetched_at,
        })
    }
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
    pub provider: MarketDataProvider,
    pub provider_symbol: String,
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: String,
    pub fetched_at: String,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<PriceRow>, RepoError> {
    let rows = sqlx::query_as::<_, RawPriceRow>(LIST_SQL)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<PriceRow>, RepoError> {
    let row = sqlx::query_as::<_, RawPriceRow>(FIND_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(TryInto::try_into).transpose()
}

pub async fn find_by_key(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: MarketDataProvider,
    date: NaiveDate,
) -> Result<Option<PriceRow>, RepoError> {
    let row = sqlx::query_as::<_, RawPriceRow>(FIND_BY_KEY_SQL)
        .bind(instrument_id)
        .bind(provider.as_str())
        .bind(date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    row.map(TryInto::try_into).transpose()
}

pub async fn find_latest_on_or_before(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: MarketDataProvider,
    as_of_date: NaiveDate,
) -> Result<Option<PriceRow>, RepoError> {
    let row = sqlx::query_as::<_, RawPriceRow>(FIND_LATEST_ON_OR_BEFORE_SQL)
        .bind(instrument_id)
        .bind(provider.as_str())
        .bind(as_of_date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    row.map(TryInto::try_into).transpose()
}

pub async fn find_previous_before(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: MarketDataProvider,
    before_date: NaiveDate,
) -> Result<Option<PriceRow>, RepoError> {
    let row = sqlx::query_as::<_, RawPriceRow>(FIND_PREVIOUS_BEFORE_SQL)
        .bind(instrument_id)
        .bind(provider.as_str())
        .bind(before_date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    row.map(TryInto::try_into).transpose()
}

pub async fn list_for_instrument_in_range(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: MarketDataProvider,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Result<Vec<PriceRow>, RepoError> {
    let from = from.map(|d| d.format("%Y-%m-%d").to_string());
    let to = to.map(|d| d.format("%Y-%m-%d").to_string());
    let rows = sqlx::query_as::<_, RawPriceRow>(LIST_IN_RANGE_SQL)
        .bind(instrument_id)
        .bind(provider.as_str())
        .bind(&from)
        .bind(&from)
        .bind(&to)
        .bind(&to)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub async fn upsert(pool: &SqlitePool, new: &NewPrice) -> Result<PriceRow, RepoError> {
    let row = sqlx::query_as::<_, RawPriceRow>(UPSERT_SQL)
        .bind(new.instrument_id)
        .bind(new.provider.as_str())
        .bind(&new.provider_symbol)
        .bind(new.date.format("%Y-%m-%d").to_string())
        .bind(new.close.to_string())
        .bind(&new.currency)
        .bind(&new.fetched_at)
        .fetch_one(pool)
        .await?;
    row.try_into()
}

pub async fn delete_by_instrument_id_in_tx(
    conn: &mut sqlx::sqlite::SqliteConnection,
    instrument_id: i64,
) -> Result<u64, RepoError> {
    let result = sqlx::query(DELETE_BY_INSTRUMENT_SQL)
        .bind(instrument_id)
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
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
                provider: MarketDataProvider::Yahoo,
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
                provider: MarketDataProvider::Yahoo,
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
                provider: MarketDataProvider::Yahoo,
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
            MarketDataProvider::Yahoo,
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
            MarketDataProvider::Yahoo,
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
        )
        .await
        .expect("inclusive latest lookup should succeed")
        .expect("inclusive latest row should exist");
        assert_eq!(inclusive_latest.date, "2026-06-12");

        let prior = find_previous_before(
            &pool,
            instrument_id,
            MarketDataProvider::Yahoo,
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
        )
        .await
        .expect("previous lookup should succeed")
        .expect("prior row should exist");
        assert_eq!(prior.date, "2026-06-10");

        let same_day = find_by_key(
            &pool,
            instrument_id,
            MarketDataProvider::Yahoo,
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
            MarketDataProvider::Yahoo,
            NaiveDate::from_ymd_opt(2026, 6, 9).expect("date should be valid"),
        )
        .await
        .expect("empty latest lookup should succeed");
        assert!(no_latest.is_none());

        let no_previous = find_previous_before(
            &pool,
            instrument_id,
            MarketDataProvider::Yahoo,
            NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid"),
        )
        .await
        .expect("empty previous lookup should succeed");
        assert!(no_previous.is_none());
    }

    #[tokio::test]
    async fn list_for_instrument_in_range_orders_and_bounds_inclusively() {
        let pool = testing::memory_pool().await;
        let instrument_id = seed_instrument(&pool).await;

        for (day, close) in [(10, "10.00"), (11, "11.00"), (12, "12.00")] {
            upsert(
                &pool,
                &NewPrice {
                    instrument_id,
                    provider: MarketDataProvider::Yahoo,
                    provider_symbol: "MSFT".to_owned(),
                    date: NaiveDate::from_ymd_opt(2026, 6, day).expect("valid date"),
                    close: Decimal::from_str(close).expect("close"),
                    currency: "USD".to_owned(),
                    fetched_at: "2026-06-16T08:00:00Z".to_owned(),
                },
            )
            .await
            .expect("price upsert should succeed");
        }

        // Inclusive window 11..=12.
        let windowed = list_for_instrument_in_range(
            &pool,
            instrument_id,
            MarketDataProvider::Yahoo,
            Some(NaiveDate::from_ymd_opt(2026, 6, 11).unwrap()),
            Some(NaiveDate::from_ymd_opt(2026, 6, 12).unwrap()),
        )
        .await
        .expect("range query should succeed");
        let dates: Vec<&str> = windowed.iter().map(|r| r.date.as_str()).collect();
        assert_eq!(dates, vec!["2026-06-11", "2026-06-12"]);

        // No bounds returns full history ascending.
        let all = list_for_instrument_in_range(
            &pool,
            instrument_id,
            MarketDataProvider::Yahoo,
            None,
            None,
        )
        .await
        .expect("range query should succeed");
        let all_dates: Vec<&str> = all.iter().map(|r| r.date.as_str()).collect();
        assert_eq!(all_dates, vec!["2026-06-10", "2026-06-11", "2026-06-12"]);

        // Provider filtering: wrong provider yields nothing.
        let other = list_for_instrument_in_range(
            &pool,
            instrument_id,
            MarketDataProvider::NasdaqNordic,
            None,
            None,
        )
        .await
        .expect("range query should succeed");
        assert!(other.is_empty());

        // Open-ended `from` only.
        let from_only = list_for_instrument_in_range(
            &pool,
            instrument_id,
            MarketDataProvider::Yahoo,
            Some(NaiveDate::from_ymd_opt(2026, 6, 12).unwrap()),
            None,
        )
        .await
        .expect("range query should succeed");
        assert_eq!(from_only.len(), 1);
        assert_eq!(from_only[0].date, "2026-06-12");
    }

    #[tokio::test]
    async fn unknown_provider_is_decode_error_and_foreign_key_is_enforced() {
        let pool = testing::memory_pool().await;
        let instrument_id = seed_instrument(&pool).await;

        sqlx::query(
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
        .expect("unknown provider row inserts after CHECK removal");

        assert!(matches!(list(&pool).await, Err(RepoError::Decode(_))));

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
                isin: None,
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
