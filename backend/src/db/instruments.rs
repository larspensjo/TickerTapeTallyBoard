use sqlx::sqlite::SqlitePool;

use crate::db::RepoError;

const COLUMNS: &str = "id, symbol, exchange, name, type, currency";

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct InstrumentRow {
    pub id: i64,
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub currency: String,
}

/// Fields for creating an instrument. `kind` is the DB string (e.g. "STOCK").
#[derive(Clone, Debug)]
pub struct NewInstrument {
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    pub kind: String,
    pub currency: String,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<InstrumentRow>, RepoError> {
    let rows = sqlx::query_as::<_, InstrumentRow>(&format!(
        "SELECT {COLUMNS} FROM instruments ORDER BY exchange, symbol"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(&format!(
        "SELECT {COLUMNS} FROM instruments WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn find_by_exchange_symbol(
    pool: &SqlitePool,
    exchange: &str,
    symbol: &str,
) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(&format!(
        "SELECT {COLUMNS} FROM instruments WHERE exchange = ? AND symbol = ?"
    ))
    .bind(exchange)
    .bind(symbol)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Upsert-like on `(exchange, symbol)`: returns the existing row (`created=false`)
/// or inserts a new one (`created=true`). Existing rows are returned unchanged.
pub async fn upsert(
    pool: &SqlitePool,
    new: &NewInstrument,
) -> Result<(InstrumentRow, bool), RepoError> {
    if let Some(existing) = find_by_exchange_symbol(pool, &new.exchange, &new.symbol).await? {
        return Ok((existing, false));
    }

    let inserted = sqlx::query_as::<_, InstrumentRow>(&format!(
        "INSERT INTO instruments (symbol, exchange, name, type, currency) \
         VALUES (?, ?, ?, ?, ?) RETURNING {COLUMNS}"
    ))
    .bind(&new.symbol)
    .bind(&new.exchange)
    .bind(&new.name)
    .bind(&new.kind)
    .bind(&new.currency)
    .fetch_one(pool)
    .await;

    match inserted {
        Ok(row) => Ok((row, true)),
        // A concurrent insert won the UNIQUE race; return the now-existing row.
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            let existing = find_by_exchange_symbol(pool, &new.exchange, &new.symbol)
                .await?
                .ok_or_else(|| {
                    RepoError::Decode("instrument vanished after unique violation".to_owned())
                })?;
            Ok((existing, false))
        }
        Err(error) => Err(RepoError::Sqlx(error)),
    }
}
