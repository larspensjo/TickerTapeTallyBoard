use sqlx::sqlite::{SqliteConnection, SqlitePool};

use crate::db::RepoError;

const LIST_SQL: &str = "SELECT id, symbol, exchange, name, type, currency, isin \
    FROM instruments ORDER BY exchange, symbol";
const FIND_SQL: &str =
    "SELECT id, symbol, exchange, name, type, currency, isin FROM instruments WHERE id = ?";
const FIND_BY_EXCHANGE_SYMBOL_SQL: &str =
    "SELECT id, symbol, exchange, name, type, currency, isin FROM instruments WHERE exchange = ? AND symbol = ?";
const FIND_BY_ISIN_SQL: &str =
    "SELECT id, symbol, exchange, name, type, currency, isin FROM instruments WHERE isin = ?";
const INSERT_SQL: &str = "INSERT INTO instruments (symbol, exchange, name, type, currency, isin) \
     VALUES (?, ?, ?, ?, ?, ?) RETURNING id, symbol, exchange, name, type, currency, isin";
const UPDATE_ISIN_SQL: &str = "UPDATE instruments SET isin = ? WHERE id = ? \
     RETURNING id, symbol, exchange, name, type, currency, isin";

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct InstrumentRow {
    pub id: i64,
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub currency: String,
    pub isin: Option<String>,
}

/// Fields for creating an instrument. `kind` is the DB string (e.g. "STOCK").
#[derive(Clone, Debug)]
pub struct NewInstrument {
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    pub kind: String,
    pub currency: String,
    pub isin: Option<String>,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<InstrumentRow>, RepoError> {
    let rows = sqlx::query_as::<_, InstrumentRow>(LIST_SQL)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(FIND_SQL)
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
    let row = sqlx::query_as::<_, InstrumentRow>(FIND_BY_EXCHANGE_SYMBOL_SQL)
        .bind(exchange)
        .bind(symbol)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_by_exchange_symbol_in_tx(
    conn: &mut SqliteConnection,
    exchange: &str,
    symbol: &str,
) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(FIND_BY_EXCHANGE_SYMBOL_SQL)
        .bind(exchange)
        .bind(symbol)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row)
}

pub async fn find_by_isin(
    pool: &SqlitePool,
    isin: &str,
) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(FIND_BY_ISIN_SQL)
        .bind(isin)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_by_isin_in_tx(
    conn: &mut SqliteConnection,
    isin: &str,
) -> Result<Option<InstrumentRow>, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(FIND_BY_ISIN_SQL)
        .bind(isin)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row)
}

/// Upsert-like on `(exchange, symbol)`: returns the existing row (`created=false`)
/// or inserts a new one (`created=true`). Existing rows are returned unchanged,
/// except that a missing ISIN on a matching row is backfilled when the caller
/// supplies one.
pub async fn upsert(
    pool: &SqlitePool,
    new: &NewInstrument,
) -> Result<(InstrumentRow, bool), RepoError> {
    let by_isin = match new.isin.as_deref() {
        Some(isin) => find_by_isin(pool, isin).await?,
        None => None,
    };
    let by_symbol = find_by_exchange_symbol(pool, &new.exchange, &new.symbol).await?;

    match resolve_existing(new, by_isin, by_symbol)? {
        UpsertDecision::Existing(row) => Ok((row, false)),
        UpsertDecision::BackfillIsin(id) => {
            let row = update_isin(pool, id, new.isin.as_deref().expect("isin required")).await?;
            Ok((row, false))
        }
        UpsertDecision::Insert => match insert(
            pool,
            &new.symbol,
            &new.exchange,
            &new.name,
            &new.kind,
            &new.currency,
            new.isin.as_deref(),
        )
        .await
        {
            Ok(row) => Ok((row, true)),
            Err(error) if is_unique_violation(&error) => {
                // Retry both identities: either the symbol unique index or the
                // partial ISIN unique index can be the one we raced on.
                let by_isin = match new.isin.as_deref() {
                    Some(isin) => find_by_isin(pool, isin).await?,
                    None => None,
                };
                let by_symbol = find_by_exchange_symbol(pool, &new.exchange, &new.symbol).await?;
                match resolve_existing(new, by_isin, by_symbol)? {
                    UpsertDecision::Existing(row) => Ok((row, false)),
                    UpsertDecision::BackfillIsin(id) => {
                        let row =
                            update_isin(pool, id, new.isin.as_deref().expect("isin required"))
                                .await?;
                        Ok((row, false))
                    }
                    UpsertDecision::Insert => Err(RepoError::Decode(
                        "instrument unique violation but no matching row was found".to_owned(),
                    )),
                }
            }
            Err(error) => Err(error),
        },
    }
}

/// Upsert on `(exchange, symbol)` inside a caller-managed transaction.
pub async fn upsert_in_tx(
    conn: &mut SqliteConnection,
    new: &NewInstrument,
) -> Result<(InstrumentRow, bool), RepoError> {
    let by_isin = match new.isin.as_deref() {
        Some(isin) => find_by_isin_in_tx(conn, isin).await?,
        None => None,
    };
    let by_symbol = find_by_exchange_symbol_in_tx(conn, &new.exchange, &new.symbol).await?;

    match resolve_existing(new, by_isin, by_symbol)? {
        UpsertDecision::Existing(row) => Ok((row, false)),
        UpsertDecision::BackfillIsin(id) => {
            let row =
                update_isin_in_tx(conn, id, new.isin.as_deref().expect("isin required")).await?;
            Ok((row, false))
        }
        UpsertDecision::Insert => match insert_in_tx(
            conn,
            &new.symbol,
            &new.exchange,
            &new.name,
            &new.kind,
            &new.currency,
            new.isin.as_deref(),
        )
        .await
        {
            Ok(row) => Ok((row, true)),
            Err(error) if is_unique_violation(&error) => {
                // Retry both identities: either the symbol unique index or the
                // partial ISIN unique index can be the one we raced on.
                let by_isin = match new.isin.as_deref() {
                    Some(isin) => find_by_isin_in_tx(conn, isin).await?,
                    None => None,
                };
                let by_symbol =
                    find_by_exchange_symbol_in_tx(conn, &new.exchange, &new.symbol).await?;
                match resolve_existing(new, by_isin, by_symbol)? {
                    UpsertDecision::Existing(row) => Ok((row, false)),
                    UpsertDecision::BackfillIsin(id) => {
                        let row = update_isin_in_tx(
                            conn,
                            id,
                            new.isin.as_deref().expect("isin required"),
                        )
                        .await?;
                        Ok((row, false))
                    }
                    UpsertDecision::Insert => Err(RepoError::Decode(
                        "instrument unique violation but no matching row was found".to_owned(),
                    )),
                }
            }
            Err(error) => Err(error),
        },
    }
}

fn is_unique_violation(error: &RepoError) -> bool {
    matches!(
        error,
        RepoError::Sqlx(sqlx::Error::Database(database_error))
            if database_error.is_unique_violation()
    )
}

fn identity_conflict(exchange: &str, symbol: &str, stored: &str, requested: &str) -> RepoError {
    RepoError::Decode(format!(
        "instrument identity conflict for {exchange}/{symbol}: stored isin {stored} differs from requested {requested}"
    ))
}

enum UpsertDecision {
    Existing(InstrumentRow),
    BackfillIsin(i64),
    Insert,
}

fn resolve_existing(
    new: &NewInstrument,
    by_isin: Option<InstrumentRow>,
    by_symbol: Option<InstrumentRow>,
) -> Result<UpsertDecision, RepoError> {
    if let Some(by_isin) = by_isin {
        if let Some(by_symbol) = by_symbol {
            if by_isin.id != by_symbol.id {
                return Err(RepoError::Decode(format!(
                    "instrument identity conflict for {}/{}: ISIN lookup id {} differs from symbol lookup id {}",
                    new.exchange, new.symbol, by_isin.id, by_symbol.id
                )));
            }
        }
        return Ok(UpsertDecision::Existing(by_isin));
    }

    if let Some(existing) = by_symbol {
        match new.isin.as_deref() {
            Some(requested) => match existing.isin.as_deref() {
                None => return Ok(UpsertDecision::BackfillIsin(existing.id)),
                Some(stored) if stored == requested => {
                    return Ok(UpsertDecision::Existing(existing))
                }
                Some(stored) => {
                    return Err(identity_conflict(
                        &new.exchange,
                        &new.symbol,
                        stored,
                        requested,
                    ))
                }
            },
            None => return Ok(UpsertDecision::Existing(existing)),
        }
    }

    Ok(UpsertDecision::Insert)
}

async fn insert(
    pool: &SqlitePool,
    symbol: &str,
    exchange: &str,
    name: &str,
    kind: &str,
    currency: &str,
    isin: Option<&str>,
) -> Result<InstrumentRow, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(INSERT_SQL)
        .bind(symbol)
        .bind(exchange)
        .bind(name)
        .bind(kind)
        .bind(currency)
        .bind(isin)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

async fn insert_in_tx(
    conn: &mut SqliteConnection,
    symbol: &str,
    exchange: &str,
    name: &str,
    kind: &str,
    currency: &str,
    isin: Option<&str>,
) -> Result<InstrumentRow, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(INSERT_SQL)
        .bind(symbol)
        .bind(exchange)
        .bind(name)
        .bind(kind)
        .bind(currency)
        .bind(isin)
        .fetch_one(&mut *conn)
        .await?;
    Ok(row)
}

async fn update_isin(pool: &SqlitePool, id: i64, isin: &str) -> Result<InstrumentRow, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(UPDATE_ISIN_SQL)
        .bind(isin)
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

async fn update_isin_in_tx(
    conn: &mut SqliteConnection,
    id: i64,
    isin: &str,
) -> Result<InstrumentRow, RepoError> {
    let row = sqlx::query_as::<_, InstrumentRow>(UPDATE_ISIN_SQL)
        .bind(isin)
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::{find_by_isin, upsert, upsert_in_tx, NewInstrument};
    use crate::db::memory_pool;

    fn avanza(isin: &str) -> NewInstrument {
        NewInstrument {
            symbol: isin.to_owned(),
            exchange: "AVANZA".to_owned(),
            name: "Example".to_owned(),
            kind: "STOCK".to_owned(),
            currency: "SEK".to_owned(),
            isin: Some(isin.to_owned()),
        }
    }

    #[tokio::test]
    async fn upsert_backfills_missing_isin_on_existing_symbol_match() {
        let pool = memory_pool().await.expect("pool");
        sqlx::query(
            "INSERT INTO instruments (symbol, exchange, name, type, currency) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("US1234567890")
        .bind("AVANZA")
        .bind("Example")
        .bind("STOCK")
        .bind("SEK")
        .execute(&pool)
        .await
        .expect("seed instrument");

        let (row, created) = upsert(&pool, &avanza("US1234567890"))
            .await
            .expect("upsert");

        assert!(!created);
        assert_eq!(row.isin.as_deref(), Some("US1234567890"));

        let found = find_by_isin(&pool, "US1234567890")
            .await
            .expect("find")
            .expect("instrument");
        assert_eq!(found.id, row.id);
        assert_eq!(found.isin.as_deref(), Some("US1234567890"));
    }

    #[tokio::test]
    async fn upsert_in_tx_and_find_by_isin_in_tx_round_trip() {
        let pool = memory_pool().await.expect("pool");
        let mut tx = pool.begin().await.expect("tx");

        let (row, created) = upsert_in_tx(&mut tx, &avanza("US0987654321"))
            .await
            .expect("upsert");
        assert!(created);
        assert_eq!(row.isin.as_deref(), Some("US0987654321"));

        let found = super::find_by_isin_in_tx(&mut tx, "US0987654321")
            .await
            .expect("find")
            .expect("instrument");
        assert_eq!(found.id, row.id);

        tx.commit().await.expect("commit");
    }

    #[tokio::test]
    async fn multiple_null_isin_instruments_coexist() {
        let pool = memory_pool().await.expect("pool");
        for symbol in ["MSFT", "AAPL"] {
            upsert(
                &pool,
                &NewInstrument {
                    symbol: symbol.to_owned(),
                    exchange: "NASDAQ".to_owned(),
                    name: symbol.to_owned(),
                    kind: "STOCK".to_owned(),
                    currency: "USD".to_owned(),
                    isin: None,
                },
            )
            .await
            .expect("null-isin upsert should succeed");
        }
    }
}
