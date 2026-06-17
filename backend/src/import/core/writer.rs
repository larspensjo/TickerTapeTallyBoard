use std::collections::{BTreeMap, BTreeSet};

use axum::http::StatusCode;

use crate::api::ApiError;
use crate::db::import_batches;
use crate::db::instruments::{self, NewInstrument};
use crate::db::transactions::{self, NewImportTransaction};
use crate::domain;
use crate::import::core::outcome::{InstrumentKey, MappedRow};
use crate::import::now_iso8601;
use crate::state::AppState;

fn identity_conflict(isin: &str, symbol: &str) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "instrument_identity_conflict",
        format!("ISIN {isin} and symbol {symbol} resolve to different instruments"),
    )
}

async fn resolve_buy_sell_instrument(
    conn: &mut sqlx::sqlite::SqliteConnection,
    source: &str,
    key: &InstrumentKey,
) -> Result<i64, ApiError> {
    if let Some(isin) = &key.isin {
        let by_isin = instruments::find_by_isin_in_tx(conn, isin).await?;
        let by_symbol =
            instruments::find_by_exchange_symbol_in_tx(conn, &key.exchange, &key.symbol).await?;
        match (by_isin, by_symbol) {
            (Some(isin_row), Some(symbol_row)) if isin_row.id != symbol_row.id => {
                crate::engine_error!(
                    "import write_batch [{source}]: identity conflict isin={} symbol={}",
                    isin,
                    key.symbol
                );
                return Err(identity_conflict(isin, &key.symbol));
            }
            (Some(row), _) => return Ok(row.id),
            (None, Some(row)) => match row.isin.as_deref() {
                None => {
                    let row = instruments::update_isin_in_tx(conn, row.id, isin).await?;
                    return Ok(row.id);
                }
                Some(stored) if stored.eq_ignore_ascii_case(isin) => return Ok(row.id),
                Some(_) => {
                    crate::engine_error!(
                        "import write_batch [{source}]: identity conflict isin={} symbol={}",
                        isin,
                        key.symbol
                    );
                    return Err(identity_conflict(isin, &key.symbol));
                }
            },
            (None, None) => {}
        }
    }

    let (row, _created) = instruments::upsert_in_tx(
        conn,
        &NewInstrument {
            symbol: key.symbol.clone(),
            exchange: key.exchange.clone(),
            name: key.name.clone(),
            kind: "STOCK".to_string(),
            currency: key.currency.clone(),
            isin: key.isin.clone(),
        },
    )
    .await?;
    Ok(row.id)
}

async fn resolve_split_instrument(
    conn: &mut sqlx::sqlite::SqliteConnection,
    source: &str,
    key: &InstrumentKey,
    created: &BTreeMap<String, i64>,
) -> Result<i64, ApiError> {
    if let Some(id) = created.get(&key.asset_key()).copied() {
        return Ok(id);
    }
    if let Some(isin) = &key.isin {
        if let Some(existing) = instruments::find_by_isin_in_tx(conn, isin).await? {
            return Ok(existing.id);
        }
    } else if let Some(existing) =
        instruments::find_by_exchange_symbol_in_tx(conn, &key.exchange, &key.symbol).await?
    {
        return Ok(existing.id);
    }

    crate::engine_error!(
        "import write_batch [{source}]: split_without_position isin={:?} symbol={}",
        key.isin,
        key.symbol
    );
    Err(ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "split_without_position",
        "A split requires an existing position.".to_string(),
    ))
}

/// Write one atomic batch from already-mapped rows.
pub async fn write_batch(
    state: &AppState,
    source: &str,
    hash: &str,
    mapped: &[MappedRow],
) -> Result<i64, ApiError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;

    let batch_id = import_batches::insert_in_tx(&mut tx, source, &now_iso8601(), hash).await?;

    let mut created: BTreeMap<String, i64> = BTreeMap::new();
    let mut affected: BTreeSet<i64> = BTreeSet::new();
    let mut key_by_instrument_id: BTreeMap<i64, String> = BTreeMap::new();

    for row in mapped {
        if row.proposed.kind == domain::TransactionKind::Split {
            continue;
        }
        let key = row.instrument.asset_key();
        if !created.contains_key(&key) {
            let instrument_id =
                resolve_buy_sell_instrument(&mut tx, source, &row.instrument).await?;
            created.insert(key.clone(), instrument_id);
            key_by_instrument_id
                .entry(instrument_id)
                .or_insert_with(|| key.clone());
        }
    }

    for row in mapped {
        let instrument_id = if row.proposed.kind == domain::TransactionKind::Split {
            resolve_split_instrument(&mut tx, source, &row.instrument, &created).await?
        } else {
            created[&row.instrument.asset_key()]
        };
        key_by_instrument_id
            .entry(instrument_id)
            .or_insert_with(|| row.instrument.asset_key());
        affected.insert(instrument_id);

        let signed = domain::validate(&row.proposed).map_err(ApiError::from)?;
        let brokerage_currency = row.proposed.brokerage_base.map(|_| "SEK".to_string());
        transactions::insert_in_tx(
            &mut tx,
            &NewImportTransaction {
                instrument_id,
                kind: row.proposed.kind,
                trade_date: row.proposed.trade_date,
                quantity: signed,
                price: row.proposed.price,
                currency: row.proposed.currency.clone(),
                fx_rate_to_base: row.proposed.fx_rate_to_base,
                brokerage: row.proposed.brokerage_base,
                brokerage_currency,
                source_value: row.source_value,
                source_currency: row.source_currency.clone(),
                note: row.note.clone(),
                import_batch_id: batch_id,
            },
        )
        .await?;
    }

    for instrument_id in affected {
        let ledger = transactions::ledger_for_instrument_in_tx(&mut tx, instrument_id).await?;
        if let Err(error) = domain::derive_position(&ledger) {
            if let Some(asset_key) = key_by_instrument_id.get(&instrument_id) {
                crate::engine_error!(
                    "import write_batch [{source}]: derive_position failed for asset={asset_key} error={error:?}"
                );
            } else {
                crate::engine_error!(
                    "import write_batch [{source}]: derive_position failed for instrument_id={instrument_id} error={error:?}"
                );
            }
            return Err(ApiError::from(error));
        }
    }

    tx.commit()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(batch_id)
}
