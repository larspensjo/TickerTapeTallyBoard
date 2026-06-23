use std::collections::{BTreeMap, BTreeSet};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::db::{import_batches, instruments, transactions};
use crate::domain;
use crate::import::avanza::mapper::to_prepared as avanza_prepared;
use crate::import::avanza::parser::parse_report as parse_avanza_report;
use crate::import::core::outcome::{
    InstrumentKey, MappedRow, ParseError, PreparedImport, RowNote, RowOutcome,
};
use crate::import::core::plan::{
    build_plan, exclude_assets, known_asset_keys, AssetGroup, ExistingInstrument, ImportPlan,
    PlanContext,
};
use crate::import::core::writer::write_batch;
use crate::import::raw_file_hash;
use crate::import::sharesight::adapter::to_prepared as sharesight_prepared;
use crate::import::sharesight::parser::parse_report;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub metadata: Option<PreviewMetadata>,
    pub counts: PreviewCounts,
    pub assets: Vec<AssetGroupDto>,
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
    pub dividends: usize,
    pub new_instruments: usize,
    pub skipped: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Serialize)]
pub struct AssetGroupDto {
    pub asset_key: String,
    pub name: String,
    pub currency: String,
    pub buys: usize,
    pub sells: usize,
    pub splits: usize,
    pub dividends: usize,
    pub default_selected: bool,
    pub skipped_reason: Option<String>,
    pub warnings: Vec<RowNoteDto>,
    pub errors: Vec<RowNoteDto>,
    pub is_new_instrument: bool,
}

#[derive(Debug, Serialize)]
pub struct NewInstrumentDto {
    pub exchange: String,
    pub symbol: String,
    pub name: String,
    pub currency: String,
    pub isin: Option<String>,
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
    #[serde(default)]
    pub exclude: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub batch_id: i64,
    pub counts: PreviewCounts,
    pub warnings: Vec<RowNoteDto>,
}

#[derive(Debug, Serialize)]
pub struct RollbackResult {
    pub batch_id: i64,
    pub removed: u64,
}

pub async fn sharesight_preview(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<Json<ImportPreview>, ApiError> {
    preview_source(&state, &bytes, parse_sharesight).await
}

pub async fn sharesight_commit(
    State(state): State<AppState>,
    Query(params): Query<CommitParams>,
    bytes: Bytes,
) -> Result<Json<ImportResult>, ApiError> {
    commit_source(&state, &bytes, "SHARESIGHT", &params, parse_sharesight).await
}

pub async fn avanza_preview(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<Json<ImportPreview>, ApiError> {
    preview_source(&state, &bytes, parse_avanza).await
}

pub async fn avanza_commit(
    State(state): State<AppState>,
    Query(params): Query<CommitParams>,
    bytes: Bytes,
) -> Result<Json<ImportResult>, ApiError> {
    commit_source(&state, &bytes, "AVANZA", &params, parse_avanza).await
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

fn parse_sharesight(bytes: &[u8]) -> Result<PreparedImport, ParseError> {
    parse_report(bytes).map(|report| sharesight_prepared(&report))
}

fn parse_avanza(bytes: &[u8]) -> Result<PreparedImport, ParseError> {
    parse_avanza_report(bytes).map(|report| avanza_prepared(&report))
}

async fn preview_source(
    state: &AppState,
    bytes: &[u8],
    parse: fn(&[u8]) -> Result<PreparedImport, ParseError>,
) -> Result<Json<ImportPreview>, ApiError> {
    let hash = raw_file_hash(bytes);
    let duplicate_of_batch_id = import_batches::find_by_hash(&state.pool, &hash)
        .await?
        .map(|batch| batch.id);

    let prepared = match parse(bytes) {
        Ok(prepared) => prepared,
        Err(error) => return Ok(Json(parse_error_preview(error, duplicate_of_batch_id))),
    };

    let ctx = load_plan_context(state).await?;
    let plan = build_plan(&prepared, &ctx);

    Ok(Json(ImportPreview {
        metadata: Some(PreviewMetadata {
            title: prepared.header.title.clone(),
            date_from: prepared.header.date_from.to_string(),
            date_to: prepared.header.date_to.to_string(),
        }),
        counts: counts_dto(&plan),
        assets: plan.assets.iter().map(asset_group_dto).collect(),
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

async fn commit_source(
    state: &AppState,
    bytes: &[u8],
    source: &str,
    params: &CommitParams,
    parse: fn(&[u8]) -> Result<PreparedImport, ParseError>,
) -> Result<Json<ImportResult>, ApiError> {
    let hash = raw_file_hash(bytes);
    let prepared =
        parse(bytes).map_err(|error| ApiError::bad_request(error.code, error.message))?;
    let exclude: BTreeSet<String> = params
        .exclude
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .collect();

    let known = known_asset_keys(&prepared);
    let unknown: Vec<String> = exclude.difference(&known).cloned().collect();

    let effective = exclude_assets(&prepared, &exclude);
    let ctx = load_plan_context(state).await?;
    let plan = build_plan(&effective, &ctx);
    reject_on_errors(&plan)?;

    if let Some(existing) = import_batches::find_by_hash(&state.pool, &hash).await? {
        if !params.allow_duplicate {
            return Err(duplicate_conflict(existing.id));
        }
    }

    let mapped: Vec<MappedRow> = effective
        .outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            RowOutcome::Mapped(mapped) => Some(mapped.clone()),
            _ => None,
        })
        .collect();
    let batch_id = write_batch(state, source, &hash, &mapped).await?;

    let warnings = unknown
        .into_iter()
        .map(|key| RowNoteDto {
            row: None,
            code: "unknown_exclude_key",
            message: format!("exclude key {key:?} matched no asset"),
        })
        .collect();

    Ok(Json(ImportResult {
        batch_id,
        counts: effective_counts(&plan, &mapped),
        warnings,
    }))
}

fn parse_error_preview(error: ParseError, duplicate_of_batch_id: Option<i64>) -> ImportPreview {
    ImportPreview {
        metadata: None,
        counts: PreviewCounts {
            errors: 1,
            ..Default::default()
        },
        assets: Vec::new(),
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
            isin: row.isin.clone(),
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

fn reject_on_errors(plan: &ImportPlan) -> Result<(), ApiError> {
    if plan.errors.is_empty() {
        return Ok(());
    }

    let first = &plan.errors[0];
    Err(ApiError::new(
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
    })))
}

fn duplicate_conflict(batch_id: i64) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "duplicate_import",
        format!("file already imported as batch {batch_id}"),
    )
    .with_details(serde_json::json!({
        "duplicate_of_batch_id": batch_id
    }))
}

fn counts_dto(plan: &ImportPlan) -> PreviewCounts {
    PreviewCounts {
        rows: plan.counts.rows,
        buys: plan.counts.buys,
        sells: plan.counts.sells,
        splits: plan.counts.splits,
        dividends: plan.counts.dividends,
        new_instruments: plan.counts.new_instruments,
        skipped: plan.counts.skipped,
        warnings: plan.counts.warnings,
        errors: plan.counts.errors,
    }
}

fn effective_counts(plan: &ImportPlan, mapped: &[MappedRow]) -> PreviewCounts {
    let mut counts = counts_dto(plan);
    counts.rows = mapped.len();
    counts.buys = kind_count(mapped, domain::TransactionKind::Buy);
    counts.sells = kind_count(mapped, domain::TransactionKind::Sell);
    counts.splits = kind_count(mapped, domain::TransactionKind::Split);
    counts.dividends = kind_count(mapped, domain::TransactionKind::Dividend);
    counts
}

fn kind_count(mapped: &[MappedRow], kind: domain::TransactionKind) -> usize {
    mapped
        .iter()
        .filter(|row| row.proposed.kind == kind)
        .count()
}

fn asset_group_dto(group: &AssetGroup) -> AssetGroupDto {
    AssetGroupDto {
        asset_key: group.asset_key.clone(),
        name: group.name.clone(),
        currency: group.currency.clone(),
        buys: group.buys,
        sells: group.sells,
        splits: group.splits,
        dividends: group.dividends,
        default_selected: group.default_selected,
        skipped_reason: group.skipped_reason.clone(),
        warnings: group.warnings.iter().map(row_note_dto).collect(),
        errors: group.errors.iter().map(row_note_dto).collect(),
        is_new_instrument: group.is_new_instrument,
    }
}

fn new_instrument_dto(key: &InstrumentKey) -> NewInstrumentDto {
    NewInstrumentDto {
        exchange: key.exchange.clone(),
        symbol: key.symbol.clone(),
        name: key.name.clone(),
        currency: key.currency.clone(),
        isin: key.isin.clone(),
    }
}

fn row_note_dto(note: &RowNote) -> RowNoteDto {
    RowNoteDto {
        row: note.row,
        code: note.code,
        message: note.message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{effective_counts, AssetGroup, ImportPlan};
    use crate::import::core::plan::PlanCounts;
    use crate::import::core::outcome::MappedRow;
    use crate::domain::{ProposedTransaction, TransactionKind};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use crate::import::core::outcome::InstrumentKey;

    fn dummy_instrument() -> InstrumentKey {
        InstrumentKey {
            exchange: "AVANZA".to_string(),
            symbol: "TEST".to_string(),
            name: "Test".to_string(),
            currency: "SEK".to_string(),
            isin: Some("SE0000000001".to_string()),
        }
    }

    fn dividend_row() -> MappedRow {
        MappedRow {
            source_row_number: 1,
            instrument: dummy_instrument(),
            proposed: ProposedTransaction {
                kind: TransactionKind::Dividend,
                trade_date: NaiveDate::from_ymd_opt(2026, 5, 20).unwrap(),
                quantity: 5,
                price: Some(dec!(7.5)),
                currency: Some("SEK".to_string()),
                fx_rate_to_base: Some(dec!(1)),
                brokerage_base: None,
            },
            source_value: Some(dec!(37.5)),
            source_currency: Some("SEK".to_string()),
            note: None,
            fx_warning: false,
        }
    }

    #[test]
    fn effective_counts_dividends_come_from_mapped_rows() {
        let plan = ImportPlan {
            counts: PlanCounts {
                rows: 2,
                dividends: 2,
                ..Default::default()
            },
            new_instruments: Vec::new(),
            assets: vec![AssetGroup {
                asset_key: "retained".to_string(),
                name: "Retained".to_string(),
                currency: "SEK".to_string(),
                buys: 0,
                sells: 0,
                splits: 0,
                dividends: 1,
                default_selected: true,
                skipped_reason: None,
                warnings: Vec::new(),
                errors: Vec::new(),
                is_new_instrument: false,
            }],
            warnings: Vec::new(),
            errors: Vec::new(),
        };

        // No dividend in mapped rows → count is 0
        let counts_no_dividend = effective_counts(&plan, &[]);
        assert_eq!(counts_no_dividend.rows, 0);
        assert_eq!(counts_no_dividend.dividends, 0);

        // Dividend in mapped rows → count is 1
        let mapped = vec![dividend_row()];
        let counts_with_dividend = effective_counts(&plan, &mapped);
        assert_eq!(counts_with_dividend.rows, 1);
        assert_eq!(counts_with_dividend.dividends, 1);
    }
}
