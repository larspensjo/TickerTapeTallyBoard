use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::sqlite::{SqliteConnection, SqlitePool};

use crate::db::RepoError;
use crate::domain::{LedgerTransaction, TransactionKind};

const LIST_SQL: &str =
    "SELECT id, instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, \
    fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, \
    note, import_batch_id FROM transactions ORDER BY trade_date DESC, id DESC";
const FIND_SQL: &str =
    "SELECT id, instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, \
    fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, \
    note, import_batch_id FROM transactions WHERE id = ?";
const LEDGER_FOR_INSTRUMENT_SQL: &str =
    "SELECT id, instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, \
    fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, \
    note, import_batch_id FROM transactions WHERE instrument_id = ? ORDER BY trade_date, id";
const ALL_FOR_HOLDINGS_SQL: &str =
    "SELECT id, instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, \
    fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, \
    note, import_batch_id FROM transactions ORDER BY instrument_id, trade_date, id";
const LIST_FOR_BATCH_SQL: &str =
    "SELECT id, instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, \
    fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, \
    note, import_batch_id FROM transactions WHERE import_batch_id = ? ORDER BY trade_date, id";
const INSERT_SQL: &str = "INSERT INTO transactions \
             (instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, fx_rate_to_base, \
        brokerage, brokerage_currency, source_value, source_currency, note, import_batch_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?, NULL) RETURNING id, instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, \
       fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, \
       note, import_batch_id";
const INSERT_IMPORT_SQL: &str = "INSERT INTO transactions \
             (instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, fx_rate_to_base, \
        brokerage, brokerage_currency, source_value, source_currency, note, import_batch_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id, instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, \
       fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, \
       note, import_batch_id";
const INSERT_WITH_ID_SQL: &str = "INSERT INTO transactions \
             (id, instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, fx_rate_to_base, \
        brokerage, brokerage_currency, source_value, source_currency, note, import_batch_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id, instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, \
       fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, \
       note, import_batch_id";
const REPLACE_SQL: &str = "UPDATE transactions SET instrument_id = ?, type = ?, trade_date = ?, quantity = ?, \
             price = ?, dividend_per_share = ?, currency = ?, fx_rate_to_base = ?, brokerage = ?, brokerage_currency = ?, \
             note = ? WHERE id = ? RETURNING id, instrument_id, type, trade_date, quantity, price, dividend_per_share, currency, \
       fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, \
       note, import_batch_id";
const INSTRUMENT_IDS_FOR_BATCH_SQL: &str =
    "SELECT DISTINCT instrument_id FROM transactions WHERE import_batch_id = ?";
const DELETE_BATCH_SQL: &str = "DELETE FROM transactions WHERE import_batch_id = ?";
const DELETE_BY_ID_SQL: &str = "DELETE FROM transactions WHERE id = ?";

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct TransactionRow {
    pub id: i64,
    pub instrument_id: i64,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub trade_date: String,
    pub quantity: i64,
    pub price: Option<String>,
    pub dividend_per_share: Option<String>,
    pub currency: Option<String>,
    pub fx_rate_to_base: Option<String>,
    pub brokerage: Option<String>,
    pub brokerage_currency: Option<String>,
    pub source_value: Option<String>,
    pub source_currency: Option<String>,
    pub note: Option<String>,
    pub import_batch_id: Option<i64>,
}

impl TransactionRow {
    /// Convert the stored row into a pure domain transaction for derivation.
    pub fn to_ledger(&self) -> Result<LedgerTransaction, RepoError> {
        let kind = TransactionKind::from_db_str(&self.kind).ok_or_else(|| {
            RepoError::Decode(format!("unknown transaction type {:?}", self.kind))
        })?;
        let trade_date = NaiveDate::parse_from_str(&self.trade_date, "%Y-%m-%d")
            .map_err(|e| RepoError::Decode(format!("bad trade_date {:?}: {e}", self.trade_date)))?;
        Ok(LedgerTransaction {
            id: self.id,
            trade_date,
            kind,
            quantity: self.quantity,
            price: parse_decimal(self.price.as_deref())?,
            dividend_per_share: parse_decimal(self.dividend_per_share.as_deref())?,
            fx_rate_to_base: parse_decimal(self.fx_rate_to_base.as_deref())?,
            brokerage_base: parse_decimal(self.brokerage.as_deref())?.unwrap_or(Decimal::ZERO),
        })
    }
}

fn parse_decimal(value: Option<&str>) -> Result<Option<Decimal>, RepoError> {
    value
        .map(|raw| {
            Decimal::from_str(raw)
                .map_err(|e| RepoError::Decode(format!("bad decimal {raw:?}: {e}")))
        })
        .transpose()
}

/// Persistable transaction fields. `quantity` is the signed position effect.
#[derive(Clone, Debug)]
pub struct NewTransaction {
    pub instrument_id: i64,
    pub kind: TransactionKind,
    pub trade_date: NaiveDate,
    pub quantity: i64,
    pub price: Option<Decimal>,
    pub dividend_per_share: Option<Decimal>,
    pub currency: Option<String>,
    pub fx_rate_to_base: Option<Decimal>,
    pub brokerage: Option<Decimal>,
    pub note: Option<String>,
}

/// Import insert payload: editable fields plus audit/batch columns.
#[derive(Clone, Debug)]
pub struct NewImportTransaction {
    pub instrument_id: i64,
    pub kind: TransactionKind,
    pub trade_date: NaiveDate,
    pub quantity: i64,
    pub price: Option<Decimal>,
    pub dividend_per_share: Option<Decimal>,
    pub currency: Option<String>,
    pub fx_rate_to_base: Option<Decimal>,
    pub brokerage: Option<Decimal>,
    pub brokerage_currency: Option<String>,
    pub source_value: Option<Decimal>,
    pub source_currency: Option<String>,
    pub note: Option<String>,
    pub import_batch_id: i64,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<TransactionRow>, RepoError> {
    let rows = sqlx::query_as::<_, TransactionRow>(LIST_SQL)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<TransactionRow>, RepoError> {
    let row = sqlx::query_as::<_, TransactionRow>(FIND_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// All of one instrument's transactions as domain rows, ordered `(trade_date, id)`.
pub async fn ledger_for_instrument(
    pool: &SqlitePool,
    instrument_id: i64,
) -> Result<Vec<LedgerTransaction>, RepoError> {
    let rows = sqlx::query_as::<_, TransactionRow>(LEDGER_FOR_INSTRUMENT_SQL)
        .bind(instrument_id)
        .fetch_all(pool)
        .await?;
    rows.iter().map(TransactionRow::to_ledger).collect()
}

/// One instrument's stored ledger inside a caller-managed transaction.
pub async fn ledger_for_instrument_in_tx(
    conn: &mut SqliteConnection,
    instrument_id: i64,
) -> Result<Vec<LedgerTransaction>, RepoError> {
    let rows = sqlx::query_as::<_, TransactionRow>(LEDGER_FOR_INSTRUMENT_SQL)
        .bind(instrument_id)
        .fetch_all(&mut *conn)
        .await?;
    rows.iter().map(TransactionRow::to_ledger).collect()
}

/// All transactions ordered for deriving holdings across instruments in memory.
pub async fn all_for_holdings(pool: &SqlitePool) -> Result<Vec<TransactionRow>, RepoError> {
    let rows = sqlx::query_as::<_, TransactionRow>(ALL_FOR_HOLDINGS_SQL)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Current maximum transaction id, or 0 when the table is empty.
pub async fn max_id(pool: &SqlitePool) -> Result<i64, RepoError> {
    let max: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM transactions")
        .fetch_one(pool)
        .await?;
    Ok(max)
}

pub async fn insert(pool: &SqlitePool, new: &NewTransaction) -> Result<TransactionRow, RepoError> {
    let row = sqlx::query_as::<_, TransactionRow>(INSERT_SQL)
        .bind(new.instrument_id)
        .bind(new.kind.as_db_str())
        .bind(new.trade_date.format("%Y-%m-%d").to_string())
        .bind(new.quantity)
        .bind(new.price.map(|d| d.to_string()))
        .bind(new.dividend_per_share.map(|d| d.to_string()))
        .bind(new.currency.clone())
        .bind(new.fx_rate_to_base.map(|d| d.to_string()))
        .bind(new.brokerage.map(|d| d.to_string()))
        .bind(new.brokerage.map(|_| "SEK"))
        .bind(new.note.clone())
        .fetch_one(pool)
        .await?;
    Ok(row)
}

pub async fn insert_in_tx(
    conn: &mut SqliteConnection,
    new: &NewImportTransaction,
) -> Result<TransactionRow, RepoError> {
    let row = sqlx::query_as::<_, TransactionRow>(INSERT_IMPORT_SQL)
        .bind(new.instrument_id)
        .bind(new.kind.as_db_str())
        .bind(new.trade_date.format("%Y-%m-%d").to_string())
        .bind(new.quantity)
        .bind(new.price.map(|d| d.to_string()))
        .bind(new.dividend_per_share.map(|d| d.to_string()))
        .bind(new.currency.clone())
        .bind(new.fx_rate_to_base.map(|d| d.to_string()))
        .bind(new.brokerage.map(|d| d.to_string()))
        .bind(new.brokerage_currency.clone())
        .bind(new.source_value.map(|d| d.to_string()))
        .bind(new.source_currency.clone())
        .bind(new.note.clone())
        .bind(new.import_batch_id)
        .fetch_one(&mut *conn)
        .await?;
    Ok(row)
}

/// Full replacement of the editable fields. Audit/import columns are left intact.
pub async fn replace(
    pool: &SqlitePool,
    id: i64,
    new: &NewTransaction,
) -> Result<TransactionRow, RepoError> {
    let row = sqlx::query_as::<_, TransactionRow>(REPLACE_SQL)
        .bind(new.instrument_id)
        .bind(new.kind.as_db_str())
        .bind(new.trade_date.format("%Y-%m-%d").to_string())
        .bind(new.quantity)
        .bind(new.price.map(|d| d.to_string()))
        .bind(new.dividend_per_share.map(|d| d.to_string()))
        .bind(new.currency.clone())
        .bind(new.fx_rate_to_base.map(|d| d.to_string()))
        .bind(new.brokerage.map(|d| d.to_string()))
        .bind(new.brokerage.map(|_| "SEK"))
        .bind(new.note.clone())
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<u64, RepoError> {
    let result = sqlx::query("DELETE FROM transactions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Distinct instrument ids touched by one import batch.
pub async fn instrument_ids_for_batch(
    conn: &mut SqliteConnection,
    batch_id: i64,
) -> Result<Vec<i64>, RepoError> {
    let ids: Vec<(i64,)> = sqlx::query_as(INSTRUMENT_IDS_FOR_BATCH_SQL)
        .bind(batch_id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(ids.into_iter().map(|(id,)| id).collect())
}

/// Delete every transaction in a batch inside a caller-managed transaction.
pub async fn delete_batch_in_tx(
    conn: &mut SqliteConnection,
    batch_id: i64,
) -> Result<u64, RepoError> {
    let result = sqlx::query(DELETE_BATCH_SQL)
        .bind(batch_id)
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
}

/// All transactions belonging to one import batch, ordered `(trade_date, id)`.
pub async fn list_for_batch_in_tx(
    conn: &mut SqliteConnection,
    batch_id: i64,
) -> Result<Vec<TransactionRow>, RepoError> {
    let rows = sqlx::query_as::<_, TransactionRow>(LIST_FOR_BATCH_SQL)
        .bind(batch_id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows)
}

/// Delete a single transaction by id inside a caller-managed transaction.
/// Uses static SQL; callers loop over ids rather than building a dynamic IN clause.
pub async fn delete_by_id_in_tx(conn: &mut SqliteConnection, id: i64) -> Result<(), RepoError> {
    sqlx::query(DELETE_BY_ID_SQL)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Insert an imported transaction with an explicit id to preserve its same-day ordering
/// identity during a refresh. All other fields follow the same convention as `insert_in_tx`.
pub async fn insert_with_id_in_tx(
    conn: &mut SqliteConnection,
    explicit_id: i64,
    new: &NewImportTransaction,
) -> Result<TransactionRow, RepoError> {
    let row = sqlx::query_as::<_, TransactionRow>(INSERT_WITH_ID_SQL)
        .bind(explicit_id)
        .bind(new.instrument_id)
        .bind(new.kind.as_db_str())
        .bind(new.trade_date.format("%Y-%m-%d").to_string())
        .bind(new.quantity)
        .bind(new.price.map(|d| d.to_string()))
        .bind(new.dividend_per_share.map(|d| d.to_string()))
        .bind(new.currency.clone())
        .bind(new.fx_rate_to_base.map(|d| d.to_string()))
        .bind(new.brokerage.map(|d| d.to_string()))
        .bind(new.brokerage_currency.clone())
        .bind(new.source_value.map(|d| d.to_string()))
        .bind(new.source_currency.clone())
        .bind(new.note.clone())
        .bind(new.import_batch_id)
        .fetch_one(&mut *conn)
        .await?;
    Ok(row)
}
