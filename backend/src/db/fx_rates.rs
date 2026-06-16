use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::sqlite::SqlitePool;

use crate::db::RepoError;

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
const UPSERT_SQL: &str = "INSERT INTO fx_rates \
       (base, quote, date, rate, provider, fetched_at) \
     VALUES (?, ?, ?, ?, ?, ?) \
     ON CONFLICT (base, quote, provider, date) DO UPDATE SET \
       rate = excluded.rate, \
       fetched_at = excluded.fetched_at \
     RETURNING id, base, quote, date, rate, provider, fetched_at";

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct FxRateRow {
    pub id: i64,
    pub base: String,
    pub quote: String,
    pub date: String,
    pub rate: String,
    pub provider: String,
    pub fetched_at: String,
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
    pub provider: String,
    pub fetched_at: String,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<FxRateRow>, RepoError> {
    let rows = sqlx::query_as::<_, FxRateRow>(LIST_SQL)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<FxRateRow>, RepoError> {
    let row = sqlx::query_as::<_, FxRateRow>(FIND_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_by_key(
    pool: &SqlitePool,
    base: &str,
    quote: &str,
    provider: &str,
    date: NaiveDate,
) -> Result<Option<FxRateRow>, RepoError> {
    let row = sqlx::query_as::<_, FxRateRow>(FIND_BY_KEY_SQL)
        .bind(base)
        .bind(quote)
        .bind(provider)
        .bind(date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_latest_on_or_before(
    pool: &SqlitePool,
    base: &str,
    quote: &str,
    provider: &str,
    as_of_date: NaiveDate,
) -> Result<Option<FxRateRow>, RepoError> {
    let row = sqlx::query_as::<_, FxRateRow>(FIND_LATEST_ON_OR_BEFORE_SQL)
        .bind(base)
        .bind(quote)
        .bind(provider)
        .bind(as_of_date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_previous_before(
    pool: &SqlitePool,
    base: &str,
    quote: &str,
    provider: &str,
    before_date: NaiveDate,
) -> Result<Option<FxRateRow>, RepoError> {
    let row = sqlx::query_as::<_, FxRateRow>(FIND_PREVIOUS_BEFORE_SQL)
        .bind(base)
        .bind(quote)
        .bind(provider)
        .bind(before_date.format("%Y-%m-%d").to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn upsert(pool: &SqlitePool, new: &NewFxRate) -> Result<FxRateRow, RepoError> {
    let row = sqlx::query_as::<_, FxRateRow>(UPSERT_SQL)
        .bind(&new.base)
        .bind(&new.quote)
        .bind(new.date.format("%Y-%m-%d").to_string())
        .bind(new.rate.to_string())
        .bind(&new.provider)
        .bind(&new.fetched_at)
        .fetch_one(pool)
        .await?;
    Ok(row)
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
                provider: "FRANKFURTER".to_owned(),
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
                provider: "FRANKFURTER".to_owned(),
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
                provider: "FRANKFURTER".to_owned(),
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
            "FRANKFURTER",
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
            "FRANKFURTER",
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
            "FRANKFURTER",
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
            "FRANKFURTER",
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
            "FRANKFURTER",
            NaiveDate::from_ymd_opt(2026, 6, 9).expect("date should be valid"),
        )
        .await
        .expect("empty latest lookup should succeed");
        assert!(no_latest.is_none());

        let no_previous = find_previous_before(
            &pool,
            "USD",
            "SEK",
            "FRANKFURTER",
            NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid"),
        )
        .await
        .expect("empty previous lookup should succeed");
        assert!(no_previous.is_none());
    }

    #[tokio::test]
    async fn fx_constraints_are_enforced() {
        let pool = testing::memory_pool().await;

        let invalid_provider = sqlx::query(
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
        .expect_err("invalid provider should fail");

        assert!(is_constraint_error(&invalid_provider, "CHECK"));
    }

    fn is_constraint_error(error: &sqlx::Error, needle: &str) -> bool {
        matches!(error, sqlx::Error::Database(database_error) if database_error.message().contains(needle))
    }
}
