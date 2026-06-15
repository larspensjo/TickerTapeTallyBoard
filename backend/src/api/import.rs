use std::collections::BTreeMap;

use axum::{body::Bytes, extract::State, Json};
use serde::Serialize;

use crate::api::error::ApiError;
use crate::db::{import_batches, instruments, transactions};
use crate::import::raw_file_hash;
use crate::import::sharesight::mapper::InstrumentKey;
use crate::import::sharesight::parser::{parse_report, ParseError};
use crate::import::sharesight::plan::{
    build_plan, ExistingInstrument, ImportPlan, PlanContext, RowNote,
};
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
