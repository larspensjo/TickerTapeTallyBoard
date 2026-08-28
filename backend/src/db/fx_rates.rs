use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::sqlite::SqlitePool;

use crate::{db::RepoError, providers::FxProvider};

const LIST_SQL: &str = "SELECT id, base, quote, date, rate, provider, fetched_at \
    FROM fx_rates ORDER BY base, quote, provider, date";
const FIND_SQL: &str = "SELECT id, base, quote, date, rate, provider, fetched_at \
    FROM fx_rates WHERE id = ?";
const FIND_BY_KEY_SQL: &str = "SELECT id, base, quote, date, rate, provider, fetched_at \
    FROM fx_rates WHERE base = ? AND quote = ? AND provider = ? AND date = ?";
const FIND_LATEST_ON_OR_BEFORE_SQL: &str = "SELECT id, base, quote, date, rate, provider, fetched_at \
    FROM fx_rates WHERE base = ? AND quote = ? AND provider = ? AND date <= ? ORDER BY date DESC, id DESC LIMIT 1";
const FIND_PREVIOUS_BEFORE_SQL: &str = "SELECT id, base, quote, date, rate, provider, fetched_at \
    FROM fx_rates WHERE base = ? AND quote = ? AND provider = ? AND date < ? ORDER BY date DESC, id DESC LIMIT 1";
const LIST_FOR_PAIR_SQL: &str = "SELECT id, base, quote, date, rate, provider, fetched_at \
    FROM fx_rates WHERE base = ? AND quote = ? AND provider = ? ORDER BY date ASC, id ASC";
const UPSERT_SQL: &str = "INSERT INTO fx_rates \
       (base, quote, date, rate, provider, fetched_at) \
     VALUES (?, ?, ?, ?, ?, ?) \
     ON CONFLICT (base, quote, provider, date) DO UPDATE SET \
       rate = excluded.rate, \
       fetched_at = excluded.fetched_at \
     RETURNING id, base, quote, date, rate, provider, fetched_at";

#[derive(Clone, Debug, sqlx::FromRow)]
struct RawFxRateRow {
    pub id: i64,
    pub base: String,
    pub quote: String,
    pub date: String,
    pub rate: String,
    pub provider: String,
    pub fetched_at: String,
}

#[derive(Clone, Debug)]
pub struct FxRateRow {
    pub id: i64,
    pub base: String,
    pub quote: String,
    pub date: String,
    pub rate: String,
    pub provider: FxProvider,
    pub fetched_at: String,
}

impl TryFrom<RawFxRateRow> for FxRateRow {
    type Error = RepoError;

    fn try_from(row: RawFxRateRow) -> Result<Self, Self::Error> {
        let provider = FxProvider::from_db_str(&row.provider).ok_or_else(|| {
            RepoError::Decode(format!(
                "unknown fx provider {:?} in row {} for {}/{}",
                row.provider, row.id, row.base, row.quote
            ))
        })?;
        Ok(Self {
            id: row.id,
            base: row.base,
            quote: row.quote,
            date: row.date,
            rate: row.rate,
            provider,
            fetched_at: row.fetched_at,
        })
    }
}

impl FxRateRow {
    pub fn date_value(&self) -> Result<NaiveDate, RepoError> {
        NaiveDate::parse_from_str(&self.date, "%Y-%m-%d")
            .map_err(|error| RepoError::Decode(format!("bad fx date {:?}: {error}", self.date)))
    }

    pub fn rate_decimal(&self) -> Result<Decimal, RepoError> {
        Decimal::from_str(&self.rate)
            .map_err(|error| RepoError::Decode(format!("bad fx rate {:?}: {error}", self.rate)))
    }
}

#[derive(Clone, Debug)]
pub struct NewFxRate {
    pub base: String,
    pub quote: String,
    pub date: NaiveDate,
    pub rate: Decimal,
    pub provider: FxProvider,
    pub fetched_at: String,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<FxRateRow>, RepoError> {
    let rows = sqlx::query_as::<_, RawFxRateRow>(LIST_SQL)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<FxRateRow>, RepoError> {
    let row = sqlx::query_as::<_, RawFxRateRow>(FIND_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(TryInto::try_into).transpose()
}

pub async fn find_by_key(
    pool: &SqlitePool,
    base: &str,
    quote: &str,
    provider: FxProvider,
    date: NaiveDate,
) -> Result<Option<FxRateRow>, RepoError> {
    let row = sqlx::query_as::<_, RawFxRateRow>(FIND_BY_KEY_SQL)
        .bind(base)
        .bind(quote)
        .bind(provider.as_str())
        .bind(date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    row.map(TryInto::try_into).transpose()
}

pub async fn find_latest_on_or_before(
    pool: &SqlitePool,
    base: &str,
    quote: &str,
    provider: FxProvider,
    as_of_date: NaiveDate,
) -> Result<Option<FxRateRow>, RepoError> {
    let row = sqlx::query_as::<_, RawFxRateRow>(FIND_LATEST_ON_OR_BEFORE_SQL)
        .bind(base)
        .bind(quote)
        .bind(provider.as_str())
        .bind(as_of_date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    row.map(TryInto::try_into).transpose()
}

pub async fn find_previous_before(
    pool: &SqlitePool,
    base: &str,
    quote: &str,
    provider: FxProvider,
    before_date: NaiveDate,
) -> Result<Option<FxRateRow>, RepoError> {
    let row = sqlx::query_as::<_, RawFxRateRow>(FIND_PREVIOUS_BEFORE_SQL)
        .bind(base)
        .bind(quote)
        .bind(provider.as_str())
        .bind(before_date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    row.map(TryInto::try_into).transpose()
}

pub async fn list_for_pair(
    pool: &SqlitePool,
    base: &str,
    quote: &str,
    provider: FxProvider,
) -> Result<Vec<FxRateRow>, RepoError> {
    let rows = sqlx::query_as::<_, RawFxRateRow>(LIST_FOR_PAIR_SQL)
        .bind(base)
        .bind(quote)
        .bind(provider.as_str())
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub async fn upsert(pool: &SqlitePool, new: &NewFxRate) -> Result<FxRateRow, RepoError> {
    let row = sqlx::query_as::<_, RawFxRateRow>(UPSERT_SQL)
        .bind(&new.base)
        .bind(&new.quote)
        .bind(new.date.format("%Y-%m-%d").to_string())
        .bind(new.rate.to_string())
        .bind(new.provider.as_str())
        .bind(&new.fetched_at)
        .fetch_one(pool)
        .await?;
    row.try_into()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::testing;

    #[tokio::test]
    async fn fx_upsert_is_idempotent_and_latest_lookup_uses_dates() {
        let pool = testing::memory_pool().await;

        let first = upsert(
            &pool,
            &NewFxRate {
                base: "USD".to_owned(),
                quote: "SEK".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid"),
                rate: Decimal::new(1005, 2),
                provider: FxProvider::Frankfurter,
                fetched_at: "2026-06-16T08:00:00Z".to_owned(),
            },
        )
        .await
        .expect("first fx upsert should succeed");

        let second = upsert(
            &pool,
            &NewFxRate {
                base: "USD".to_owned(),
                quote: "SEK".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
                rate: Decimal::new(1012, 2),
                provider: FxProvider::Frankfurter,
                fetched_at: "2026-06-16T08:05:00Z".to_owned(),
            },
        )
        .await
        .expect("second fx upsert should succeed");

        let updated = upsert(
            &pool,
            &NewFxRate {
                base: "USD".to_owned(),
                quote: "SEK".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
                rate: Decimal::new(1022, 2),
                provider: FxProvider::Frankfurter,
                fetched_at: "2026-06-16T08:10:00Z".to_owned(),
            },
        )
        .await
        .expect("updated fx upsert should succeed");

        assert_ne!(first.id, second.id);
        assert_eq!(second.id, updated.id);
        assert_eq!(updated.rate, "10.22");

        let rows = list(&pool).await.expect("list should succeed");
        assert_eq!(rows.len(), 2);

        let found = find(&pool, updated.id)
            .await
            .expect("find should succeed")
            .expect("row should exist");
        assert_eq!(found.rate, "10.22");

        let latest = find_latest_on_or_before(
            &pool,
            "USD",
            "SEK",
            FxProvider::Frankfurter,
            NaiveDate::from_ymd_opt(2026, 6, 11).expect("date should be valid"),
        )
        .await
        .expect("latest lookup should succeed")
        .expect("latest row should exist");
        assert_eq!(latest.date, "2026-06-10");
        assert_eq!(latest.rate, "10.05");
        assert_eq!(
            latest.date_value().expect("date should decode"),
            NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid")
        );
        assert_eq!(
            latest.rate_decimal().expect("rate should decode"),
            Decimal::new(1005, 2)
        );

        let inclusive_latest = find_latest_on_or_before(
            &pool,
            "USD",
            "SEK",
            FxProvider::Frankfurter,
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
        )
        .await
        .expect("inclusive latest lookup should succeed")
        .expect("inclusive latest row should exist");
        assert_eq!(inclusive_latest.date, "2026-06-12");

        let prior = find_previous_before(
            &pool,
            "USD",
            "SEK",
            FxProvider::Frankfurter,
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
        )
        .await
        .expect("previous lookup should succeed")
        .expect("prior row should exist");
        assert_eq!(prior.date, "2026-06-10");

        let same_day = find_by_key(
            &pool,
            "USD",
            "SEK",
            FxProvider::Frankfurter,
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
        )
        .await
        .expect("key lookup should succeed")
        .expect("same-day row should exist");
        assert_eq!(same_day.rate, "10.22");

        let no_latest = find_latest_on_or_before(
            &pool,
            "USD",
            "SEK",
            FxProvider::Frankfurter,
            NaiveDate::from_ymd_opt(2026, 6, 9).expect("date should be valid"),
        )
        .await
        .expect("empty latest lookup should succeed");
        assert!(no_latest.is_none());

        let no_previous = find_previous_before(
            &pool,
            "USD",
            "SEK",
            FxProvider::Frankfurter,
            NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid"),
        )
        .await
        .expect("empty previous lookup should succeed");
        assert!(no_previous.is_none());
    }

    #[tokio::test]
    async fn list_for_pair_returns_full_history_ascending() {
        let pool = testing::memory_pool().await;

        for (day, rate) in [(8, "9.00"), (10, "10.00"), (12, "11.00")] {
            upsert(
                &pool,
                &NewFxRate {
                    base: "USD".to_owned(),
                    quote: "SEK".to_owned(),
                    date: NaiveDate::from_ymd_opt(2026, 6, day).expect("valid date"),
                    rate: Decimal::from_str(rate).expect("rate"),
                    provider: FxProvider::Frankfurter,
                    fetched_at: "2026-06-16T08:00:00Z".to_owned(),
                },
            )
            .await
            .expect("fx upsert should succeed");
        }

        let rows = list_for_pair(&pool, "USD", "SEK", FxProvider::Frankfurter)
            .await
            .expect("pair query should succeed");
        let dates: Vec<&str> = rows.iter().map(|r| r.date.as_str()).collect();
        assert_eq!(dates, vec!["2026-06-08", "2026-06-10", "2026-06-12"]);

        // Wrong pair direction yields nothing.
        let reversed = list_for_pair(&pool, "SEK", "USD", FxProvider::Frankfurter)
            .await
            .expect("pair query should succeed");
        assert!(reversed.is_empty());
    }

    #[tokio::test]
    async fn unknown_provider_is_decode_error() {
        let pool = testing::memory_pool().await;

        sqlx::query(
            "INSERT INTO fx_rates (base, quote, date, rate, provider, fetched_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind("USD")
        .bind("SEK")
        .bind("2026-06-10")
        .bind("10.0")
        .bind("BAD")
        .bind("2026-06-16T08:00:00Z")
        .execute(&pool)
        .await
        .expect("unknown provider row inserts after CHECK removal");

        assert!(matches!(list(&pool).await, Err(RepoError::Decode(_))));
    }
}
