use std::collections::{BTreeMap, BTreeSet};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::db::instruments::NewInstrument;
use crate::db::transactions::NewImportTransaction;
use crate::db::{import_batches, instruments, transactions};
use crate::domain;
use crate::import::sharesight::mapper::{map_row, InstrumentKey};
use crate::import::sharesight::parser::{parse_report, ParseError, ParsedKind, ParsedReport};
use crate::import::sharesight::plan::{
    build_plan, ExistingInstrument, ImportPlan, PlanContext, RowNote,
};
use crate::import::{now_iso8601, raw_file_hash};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub metadata: Option<PreviewMetadata>,
    pub counts: PreviewCounts,
    pub new_instruments: Vec<NewInstrumentDto>,
    pub warnings: Vec<RowNoteDto>,
    pub errors: Vec<RowNoteDto>,
    pub duplicate_of_batch_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PreviewMetadata {
    pub title: String,
    pub date_from: String,
    pub date_to: String,
}

#[derive(Debug, Serialize, Default)]
pub struct PreviewCounts {
    pub rows: usize,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub new_instruments: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Serialize)]
pub struct NewInstrumentDto {
    pub exchange: String,
    pub symbol: String,
    pub name: String,
    pub currency: String,
}

#[derive(Debug, Serialize)]
pub struct RowNoteDto {
    pub row: Option<usize>,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct CommitParams {
    #[serde(default)]
    pub allow_duplicate: bool,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub batch_id: i64,
    pub counts: PreviewCounts,
}

#[derive(Debug, Serialize)]
pub struct RollbackResult {
    pub batch_id: i64,
    pub removed: u64,
}

pub async fn preview(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<Json<ImportPreview>, ApiError> {
    let hash = raw_file_hash(&bytes);
    let duplicate_of_batch_id = import_batches::find_by_hash(&state.pool, &hash)
        .await?
        .map(|batch| batch.id);

    let report = match parse_report(&bytes) {
        Ok(report) => report,
        Err(error) => return Ok(Json(parse_error_preview(error, duplicate_of_batch_id))),
    };

    let ctx = load_plan_context(&state).await?;
    let plan = build_plan(&report, &ctx);

    Ok(Json(ImportPreview {
        metadata: Some(PreviewMetadata {
            title: report.metadata.title.clone(),
            date_from: report.metadata.date_from.to_string(),
            date_to: report.metadata.date_to.to_string(),
        }),
        counts: counts_dto(&plan),
        new_instruments: plan
            .new_instruments
            .iter()
            .map(new_instrument_dto)
            .collect(),
        warnings: plan.warnings.iter().map(row_note_dto).collect(),
        errors: plan.errors.iter().map(row_note_dto).collect(),
        duplicate_of_batch_id,
    }))
}

pub async fn commit(
    State(state): State<AppState>,
    Query(params): Query<CommitParams>,
    bytes: Bytes,
) -> Result<Json<ImportResult>, ApiError> {
    let hash = raw_file_hash(&bytes);
    let report = match parse_report(&bytes) {
        Ok(report) => report,
        Err(error) => return Err(ApiError::bad_request(error.code, error.message)),
    };

    let ctx = load_plan_context(&state).await?;
    let plan = build_plan(&report, &ctx);
    if !plan.errors.is_empty() {
        let first = &plan.errors[0];
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            first.code,
            first.message.clone(),
        )
        .with_details(serde_json::json!({
            "errors": plan
                .errors
                .iter()
                .map(|error| serde_json::json!({
                    "row": error.row,
                    "code": error.code,
                    "message": error.message,
                }))
                .collect::<Vec<_>>()
        })));
    }

    if let Some(existing) = import_batches::find_by_hash(&state.pool, &hash).await? {
        if !params.allow_duplicate {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "duplicate_import",
                format!("file already imported as batch {}", existing.id),
            )
            .with_details(serde_json::json!({
                "duplicate_of_batch_id": existing.id
            })));
        }
    }

    let batch_id = write_batch(&state, &report, &hash).await?;
    Ok(Json(ImportResult {
        batch_id,
        counts: counts_dto(&plan),
    }))
}

pub async fn rollback(
    State(state): State<AppState>,
    Path(batch_id): Path<i64>,
) -> Result<Json<RollbackResult>, ApiError> {
    if import_batches::find(&state.pool, batch_id).await?.is_none() {
        return Err(ApiError::not_found("import batch", batch_id));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;

    let affected = transactions::instrument_ids_for_batch(&mut tx, batch_id).await?;
    let removed = transactions::delete_batch_in_tx(&mut tx, batch_id).await?;

    for instrument_id in affected {
        let ledger = transactions::ledger_for_instrument_in_tx(&mut tx, instrument_id).await?;
        domain::derive_position(&ledger).map_err(ApiError::from)?;
    }

    let deleted = import_batches::delete_in_tx(&mut tx, batch_id).await?;
    if deleted == 0 {
        return Err(ApiError::not_found("import batch", batch_id));
    }

    tx.commit()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(RollbackResult { batch_id, removed }))
}

fn parse_error_preview(error: ParseError, duplicate_of_batch_id: Option<i64>) -> ImportPreview {
    ImportPreview {
        metadata: None,
        counts: PreviewCounts {
            errors: 1,
            ..Default::default()
        },
        new_instruments: Vec::new(),
        warnings: Vec::new(),
        errors: vec![RowNoteDto {
            row: error.row,
            code: error.code,
            message: error.message,
        }],
        duplicate_of_batch_id,
    }
}

pub(crate) async fn load_plan_context(state: &AppState) -> Result<PlanContext, ApiError> {
    let instrument_rows = instruments::list(&state.pool).await?;
    let mut existing_ledgers = BTreeMap::new();
    let mut existing_instruments = Vec::new();

    for row in &instrument_rows {
        existing_instruments.push(ExistingInstrument {
            id: row.id,
            exchange: row.exchange.clone(),
            symbol: row.symbol.clone(),
            currency: row.currency.clone(),
        });
        existing_ledgers.insert(
            row.id,
            transactions::ledger_for_instrument(&state.pool, row.id).await?,
        );
    }

    Ok(PlanContext {
        existing_instruments,
        existing_ledgers,
        max_existing_id: transactions::max_id(&state.pool).await?,
    })
}

async fn write_batch(state: &AppState, report: &ParsedReport, hash: &str) -> Result<i64, ApiError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;

    let batch_id =
        import_batches::insert_in_tx(&mut tx, "SHARESIGHT", &now_iso8601(), hash).await?;

    let mut instrument_ids: BTreeMap<(String, String), i64> = BTreeMap::new();
    let mut affected: BTreeSet<i64> = BTreeSet::new();

    for parsed in &report.rows {
        // Re-map during the write so the transaction path never trusts preview-only state.
        let mapped = map_row(parsed).map_err(|error| {
            ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.code, error.message)
        })?;

        let key = (
            mapped.instrument.exchange.to_lowercase(),
            mapped.instrument.symbol.to_lowercase(),
        );

        let instrument_id = match instrument_ids.get(&key) {
            Some(id) => *id,
            None => {
                let (row, _created) = instruments::upsert_in_tx(
                    &mut tx,
                    &NewInstrument {
                        symbol: mapped.instrument.symbol.clone(),
                        exchange: mapped.instrument.exchange.clone(),
                        name: mapped.instrument.name.clone(),
                        kind: "STOCK".to_string(),
                        currency: mapped.instrument.currency.clone(),
                    },
                )
                .await?;
                instrument_ids.insert(key, row.id);
                row.id
            }
        };

        affected.insert(instrument_id);

        let signed = domain::validate(&mapped.proposed).map_err(ApiError::from)?;
        let brokerage_currency = mapped.proposed.brokerage_base.map(|_| "SEK".to_string());
        let source_currency =
            matches!(mapped.kind, ParsedKind::Buy | ParsedKind::Sell).then(|| "SEK".to_string());

        transactions::insert_in_tx(
            &mut tx,
            &NewImportTransaction {
                instrument_id,
                kind: mapped.proposed.kind,
                trade_date: mapped.proposed.trade_date,
                quantity: signed,
                price: mapped.proposed.price,
                currency: mapped.proposed.currency.clone(),
                fx_rate_to_base: mapped.proposed.fx_rate_to_base,
                brokerage: mapped.proposed.brokerage_base,
                brokerage_currency,
                source_value: Some(mapped.source_value),
                source_currency,
                note: non_empty(&parsed.comments),
                import_batch_id: batch_id,
            },
        )
        .await?;
    }

    for instrument_id in affected {
        let ledger = transactions::ledger_for_instrument_in_tx(&mut tx, instrument_id).await?;
        domain::derive_position(&ledger).map_err(ApiError::from)?;
    }

    tx.commit()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(batch_id)
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn counts_dto(plan: &ImportPlan) -> PreviewCounts {
    PreviewCounts {
        rows: plan.counts.rows,
        buys: plan.counts.buys,
        sells: plan.counts.sells,
        splits: plan.counts.splits,
        new_instruments: plan.counts.new_instruments,
        warnings: plan.counts.warnings,
        errors: plan.counts.errors,
    }
}

fn new_instrument_dto(key: &InstrumentKey) -> NewInstrumentDto {
    NewInstrumentDto {
        exchange: key.exchange.clone(),
        symbol: key.symbol.clone(),
        name: key.name.clone(),
        currency: key.currency.clone(),
    }
}

fn row_note_dto(note: &RowNote) -> RowNoteDto {
    RowNoteDto {
        row: note.row,
        code: note.code,
        message: note.message.clone(),
    }
}
