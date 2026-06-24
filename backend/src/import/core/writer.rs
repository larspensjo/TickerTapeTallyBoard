use std::collections::{BTreeMap, BTreeSet, VecDeque};

use axum::http::StatusCode;

use crate::api::ApiError;
use crate::db::import_batches;
use crate::db::instruments::{self, NewInstrument};
use crate::db::transactions::{self, NewImportTransaction, TransactionRow};
use crate::domain;
use crate::import::core::outcome::{InstrumentKey, MappedRow};
use crate::import::now_iso8601;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Canonical row key for multiset matching during refresh
// ---------------------------------------------------------------------------

/// All storable fields of an imported transaction, used to identify
/// unchanged rows across full-history refresh imports.
///
/// Decimal fields are represented as the same string produced by `.to_string()`
/// at insert time, ensuring round-trip consistency with SQLite text storage.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct CanonicalRow {
    instrument_id: i64,
    kind: String,
    trade_date: String,
    quantity: i64,
    price: Option<String>,
    dividend_per_share: Option<String>,
    currency: Option<String>,
    fx_rate_to_base: Option<String>,
    brokerage: Option<String>,
    brokerage_currency: Option<String>,
    source_value: Option<String>,
    source_currency: Option<String>,
    note: Option<String>,
}

fn canonical_from_db(row: &TransactionRow) -> CanonicalRow {
    CanonicalRow {
        instrument_id: row.instrument_id,
        kind: row.kind.clone(),
        trade_date: row.trade_date.clone(),
        quantity: row.quantity,
        price: row.price.clone(),
        dividend_per_share: row.dividend_per_share.clone(),
        currency: row.currency.clone(),
        fx_rate_to_base: row.fx_rate_to_base.clone(),
        brokerage: row.brokerage.clone(),
        brokerage_currency: row.brokerage_currency.clone(),
        source_value: row.source_value.clone(),
        source_currency: row.source_currency.clone(),
        note: row.note.clone(),
    }
}

fn canonical_from_mapped(
    row: &MappedRow,
    instrument_id: i64,
) -> Result<CanonicalRow, domain::ValidationError> {
    let signed = domain::validate(&row.proposed)?;
    let quantity_to_store = if row.proposed.kind == domain::TransactionKind::Dividend {
        row.proposed.quantity
    } else {
        signed
    };
    let brokerage_currency = row.proposed.brokerage_base.map(|_| "SEK".to_string());

    Ok(CanonicalRow {
        instrument_id,
        kind: row.proposed.kind.as_db_str().to_string(),
        trade_date: row.proposed.trade_date.format("%Y-%m-%d").to_string(),
        quantity: quantity_to_store,
        price: row.proposed.price.map(|d| d.to_string()),
        dividend_per_share: row.proposed.dividend_per_share.map(|d| d.to_string()),
        currency: row.proposed.currency.clone(),
        fx_rate_to_base: row.proposed.fx_rate_to_base.map(|d| d.to_string()),
        brokerage: row.proposed.brokerage_base.map(|d| d.to_string()),
        brokerage_currency,
        source_value: row.source_value.map(|d| d.to_string()),
        source_currency: row.source_currency.clone(),
        note: row.note.clone(),
    })
}

// ---------------------------------------------------------------------------
// Instrument resolution helpers
// ---------------------------------------------------------------------------

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
                    "import [{source}]: identity conflict isin={} symbol={}",
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
                        "import [{source}]: identity conflict isin={} symbol={}",
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
    non_split_resolved: &BTreeMap<String, i64>,
) -> Result<i64, ApiError> {
    if let Some(id) = non_split_resolved.get(&key.asset_key()).copied() {
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
        "import [{source}]: split_without_position isin={:?} symbol={}",
        key.isin,
        key.symbol
    );
    Err(ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "split_without_position",
        "A split requires an existing position.".to_string(),
    ))
}

/// Two-pass instrument resolution: non-split rows first, then splits.
///
/// Returns `(id_by_asset_key, key_by_instrument_id)`.
async fn resolve_instruments_for_mapped_rows(
    conn: &mut sqlx::sqlite::SqliteConnection,
    source: &str,
    mapped: &[MappedRow],
) -> Result<(BTreeMap<String, i64>, BTreeMap<i64, String>), ApiError> {
    let mut id_by_asset_key: BTreeMap<String, i64> = BTreeMap::new();
    let mut key_by_instrument_id: BTreeMap<i64, String> = BTreeMap::new();

    // Pass 1: resolve Buy, Sell, and Dividend instruments
    for row in mapped {
        if row.proposed.kind == domain::TransactionKind::Split {
            continue;
        }
        let key = row.instrument.asset_key();
        if !id_by_asset_key.contains_key(&key) {
            let instrument_id = resolve_buy_sell_instrument(conn, source, &row.instrument).await?;
            id_by_asset_key.insert(key.clone(), instrument_id);
            key_by_instrument_id
                .entry(instrument_id)
                .or_insert_with(|| key.clone());
        }
    }

    // Pass 2: resolve Split instruments against already-resolved or existing instruments
    for row in mapped {
        if row.proposed.kind != domain::TransactionKind::Split {
            continue;
        }
        let instrument_id =
            resolve_split_instrument(conn, source, &row.instrument, &id_by_asset_key).await?;
        let key = row.instrument.asset_key();
        id_by_asset_key.entry(key.clone()).or_insert(instrument_id);
        key_by_instrument_id
            .entry(instrument_id)
            .or_insert_with(|| key.clone());
    }

    Ok((id_by_asset_key, key_by_instrument_id))
}

/// Re-derive ledgers for every instrument in `affected_ids`.
///
/// On any `derive_position` failure, logs the error and returns a
/// conflict API error with instrument context.
async fn derive_affected_ledgers(
    conn: &mut sqlx::sqlite::SqliteConnection,
    label: &str,
    affected_ids: &BTreeSet<i64>,
    key_by_instrument_id: &BTreeMap<i64, String>,
) -> Result<(), ApiError> {
    for &instrument_id in affected_ids {
        let ledger = transactions::ledger_for_instrument_in_tx(conn, instrument_id).await?;
        if let Err(error) = domain::derive_position(&ledger) {
            let asset_key = key_by_instrument_id
                .get(&instrument_id)
                .cloned()
                .unwrap_or_else(|| instrument_id.to_string());
            crate::engine_error!(
                "import [{label}]: derive_position failed for asset={asset_key} error={error:?}"
            );
            return Err(ApiError::from(error));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Write one atomic batch from already-mapped rows (append path).
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

    let (id_by_asset_key, mut key_by_instrument_id) =
        resolve_instruments_for_mapped_rows(&mut tx, source, mapped).await?;

    let mut affected: BTreeSet<i64> = BTreeSet::new();

    for row in mapped {
        let instrument_id = id_by_asset_key[&row.instrument.asset_key()];
        key_by_instrument_id
            .entry(instrument_id)
            .or_insert_with(|| row.instrument.asset_key());
        affected.insert(instrument_id);

        let signed = domain::validate(&row.proposed).map_err(ApiError::from)?;
        let quantity_to_store = if row.proposed.kind == domain::TransactionKind::Dividend {
            row.proposed.quantity
        } else {
            signed
        };
        let brokerage_currency = row.proposed.brokerage_base.map(|_| "SEK".to_string());
        transactions::insert_in_tx(
            &mut tx,
            &NewImportTransaction {
                instrument_id,
                kind: row.proposed.kind,
                trade_date: row.proposed.trade_date,
                quantity: quantity_to_store,
                price: row.proposed.price,
                dividend_per_share: row.proposed.dividend_per_share,
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

    derive_affected_ledgers(&mut tx, source, &affected, &key_by_instrument_id).await?;

    tx.commit()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(batch_id)
}

/// Atomically replace the transaction set of an existing Avanza import batch
/// in place, preserving unchanged rows by id and updating batch metadata.
///
/// The `expected_batch_id` must be the latest live AVANZA batch; if a newer
/// AVANZA batch appeared between preview and commit, the call returns a
/// `replace_candidate_changed` conflict rather than refreshing the wrong batch.
/// Asset groups the user deselected are excluded from `mapped` but still
/// exist in the old batch. Pass their instrument IDs here so `refresh_batch`
/// leaves those rows untouched instead of deleting them.
pub async fn refresh_batch(
    state: &AppState,
    source: &str,
    expected_batch_id: i64,
    hash: &str,
    mapped: &[MappedRow],
    excluded_instrument_ids: &BTreeSet<i64>,
) -> Result<i64, ApiError> {
    // Pre-check: verify expected_batch_id is still the latest live batch for this source.
    let latest = import_batches::find_latest_by_source(&state.pool, source).await?;
    let latest_id = match latest.as_ref().map(|b| b.id) {
        Some(id) => id,
        None => {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "replace_batch_not_found",
                format!("no {source} batch found; use append to create the first"),
            ));
        }
    };
    if latest_id != expected_batch_id {
        let exists = import_batches::find(&state.pool, expected_batch_id).await?;
        let wrong_source = exists
            .as_ref()
            .is_some_and(|b| !b.source.eq_ignore_ascii_case(source));
        if exists.is_none() || wrong_source {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "replace_batch_not_found",
                format!("import batch {expected_batch_id} not found or is not {source}"),
            ));
        }
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "replace_candidate_changed",
            format!(
                "a newer {source} batch {latest_id} appeared after preview; re-preview to continue"
            ),
        )
        .with_details(serde_json::json!({
            "expected_batch_id": expected_batch_id,
            "actual_latest_id": latest_id,
        })));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;

    // Verify batch still exists and has the correct source inside the transaction
    let batch = import_batches::find_in_tx(&mut tx, expected_batch_id)
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "replace_batch_not_found",
                format!("import batch {expected_batch_id} not found"),
            )
        })?;
    if !batch.source.eq_ignore_ascii_case(source) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "replace_batch_wrong_source",
            format!(
                "batch {expected_batch_id} has source {} not {source}",
                batch.source
            ),
        ));
    }

    // Re-verify inside the transaction that expected_batch_id is still the
    // latest live batch for this source, closing the TOCTOU window between the
    // pre-check above and this transaction.
    let latest_in_tx_id = import_batches::find_latest_by_source_in_tx(&mut tx, source)
        .await?
        .map(|b| b.id);
    if latest_in_tx_id != Some(expected_batch_id) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "replace_candidate_changed",
            format!("a newer {source} batch appeared after preview; re-preview to continue"),
        )
        .with_details(serde_json::json!({
            "expected_batch_id": expected_batch_id,
            "actual_latest_id": latest_in_tx_id,
        })));
    }

    // Snapshot old batch transactions (ordered by trade_date, id)
    let old_rows = transactions::list_for_batch_in_tx(&mut tx, expected_batch_id).await?;
    let old_instrument_ids: BTreeSet<i64> = old_rows.iter().map(|r| r.instrument_id).collect();

    // Resolve instruments for new mapped rows
    let (id_by_asset_key, mut key_by_instrument_id) =
        resolve_instruments_for_mapped_rows(&mut tx, source, mapped).await?;

    // Record asset keys for old instrument ids that may not appear in new rows
    for &id in &old_instrument_ids {
        key_by_instrument_id
            .entry(id)
            .or_insert_with(|| id.to_string());
    }

    // Build canonical-key → sorted old ids (ascending) for multiset matching
    let mut old_by_canonical: BTreeMap<CanonicalRow, VecDeque<i64>> = BTreeMap::new();
    for row in &old_rows {
        let key = canonical_from_db(row);
        old_by_canonical.entry(key).or_default().push_back(row.id);
    }
    for ids in old_by_canonical.values_mut() {
        let mut v: Vec<i64> = ids.drain(..).collect();
        v.sort_unstable();
        *ids = v.into_iter().collect();
    }

    // Match new mapped rows against old canonical keys
    let mut to_insert: Vec<(&MappedRow, i64)> = Vec::new();

    for row in mapped {
        let instrument_id = id_by_asset_key[&row.instrument.asset_key()];

        let canonical = canonical_from_mapped(row, instrument_id).map_err(ApiError::from)?;

        // Only treat the new row as preserved when an unconsumed old id remains
        // for this canonical key. If the new file contains more identical
        // canonical rows than the old batch, the surplus rows are inserted.
        let preserved = old_by_canonical
            .get_mut(&canonical)
            .and_then(|ids| ids.pop_front())
            .is_some();
        if !preserved {
            to_insert.push((row, instrument_id));
        }
    }

    // Build the set of row IDs belonging to excluded instruments; these must
    // be preserved even when they have no match in the new mapped rows.
    let excluded_row_ids: BTreeSet<i64> = old_rows
        .iter()
        .filter(|r| excluded_instrument_ids.contains(&r.instrument_id))
        .map(|r| r.id)
        .collect();

    // Delete old rows that have no match in the new import, except for rows
    // belonging to excluded instruments (user deselected them; leave as-is).
    let ids_to_delete: Vec<i64> = old_by_canonical
        .values()
        .flatten()
        .copied()
        .filter(|id| !excluded_row_ids.contains(id))
        .collect();
    for id in ids_to_delete {
        transactions::delete_by_id_in_tx(&mut tx, id).await?;
    }

    // Insert genuinely new rows
    for (row, instrument_id) in to_insert {
        let signed = domain::validate(&row.proposed).map_err(ApiError::from)?;
        let quantity_to_store = if row.proposed.kind == domain::TransactionKind::Dividend {
            row.proposed.quantity
        } else {
            signed
        };
        let brokerage_currency = row.proposed.brokerage_base.map(|_| "SEK".to_string());
        transactions::insert_in_tx(
            &mut tx,
            &NewImportTransaction {
                instrument_id,
                kind: row.proposed.kind,
                trade_date: row.proposed.trade_date,
                quantity: quantity_to_store,
                price: row.proposed.price,
                dividend_per_share: row.proposed.dividend_per_share,
                currency: row.proposed.currency.clone(),
                fx_rate_to_base: row.proposed.fx_rate_to_base,
                brokerage: row.proposed.brokerage_base,
                brokerage_currency,
                source_value: row.source_value,
                source_currency: row.source_currency.clone(),
                note: row.note.clone(),
                import_batch_id: expected_batch_id,
            },
        )
        .await?;
    }

    // Update batch metadata in place (hash and timestamp)
    import_batches::update_metadata_in_tx(&mut tx, expected_batch_id, &now_iso8601(), hash).await?;

    // Re-derive ledgers for all affected instruments (union of old and new),
    // excluding instruments the user deselected — their rows were preserved
    // untouched, so their ledger state is unchanged and needs no validation.
    let new_instrument_ids: BTreeSet<i64> = id_by_asset_key.values().copied().collect();
    let all_affected: BTreeSet<i64> = old_instrument_ids
        .union(&new_instrument_ids)
        .copied()
        .filter(|id| !excluded_instrument_ids.contains(id))
        .collect();

    for &instrument_id in &all_affected {
        let ledger = transactions::ledger_for_instrument_in_tx(&mut tx, instrument_id).await?;
        if let Err(error) = domain::derive_position(&ledger) {
            let asset_key = key_by_instrument_id
                .get(&instrument_id)
                .cloned()
                .unwrap_or_else(|| instrument_id.to_string());
            crate::engine_error!(
                "import refresh_batch [{source}]: derive_position failed for asset={asset_key} error={error:?}"
            );
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "refresh_would_invalidate_ledger",
                format!(
                    "refresh would invalidate ledger for {asset_key}: {}",
                    error.code()
                ),
            )
            .with_details(serde_json::json!({
                "instrument_id": instrument_id,
                "asset_key": asset_key,
                "ledger_error": error.code(),
            })));
        }
    }

    tx.commit()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;

    Ok(expected_batch_id)
}
