use std::collections::{BTreeMap, BTreeSet};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::api::instruments::ConvictionDto;
use crate::api::ApiError;
use crate::db::{import_batches, instruments, transactions};
use crate::domain::{self, ConvictionLevel, TransactionKind};
use crate::import::avanza::mapper::to_prepared as avanza_prepared;
use crate::import::avanza::parser::parse_report as parse_avanza_report;
use crate::import::core::conviction_guard::{
    closing_convicted_positions, net_quantity, predicted_quantity_append,
    predicted_quantity_replace, ClosingConvictedPosition, GuardInstrument,
};
use crate::import::core::outcome::{
    InstrumentKey, MappedRow, ParseError, PreparedImport, RowNote, RowOutcome,
};
use crate::import::core::plan::{
    build_plan, exclude_assets, known_asset_keys, AssetGroup, ExistingInstrument, ImportPlan,
    PlanContext,
};
use crate::import::core::writer::{refresh_batch, write_batch};
use crate::import::raw_file_hash;
use crate::import::sharesight::adapter::to_prepared as sharesight_prepared;
use crate::import::sharesight::parser::parse_report;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub metadata: Option<PreviewMetadata>,
    pub counts: PreviewCounts,
    pub assets: Vec<AssetGroupDto>,
    pub already_imported_assets: Vec<AssetGroupDto>,
    pub new_instruments: Vec<NewInstrumentDto>,
    pub warnings: Vec<RowNoteDto>,
    pub errors: Vec<RowNoteDto>,
    pub duplicate_of_batch_id: Option<i64>,
    /// Batch id that a subsequent Avanza refresh commit should target.
    /// Null when no prior Avanza batch exists (first import).
    pub replace_candidate_batch_id: Option<i64>,
    /// Non-blocking warning when multiple live Avanza batches exist.
    pub replace_candidate_warning: Option<String>,
    /// Convicted open positions this import would close. When non-empty the
    /// commit must carry a keep/clear choice for every listed asset key.
    pub conviction_close_positions: Vec<ConvictionClosePositionDto>,
}

#[derive(Debug, Serialize)]
pub struct ConvictionClosePositionDto {
    pub instrument_id: i64,
    pub asset_key: String,
    pub symbol: String,
    pub conviction: ConvictionDto,
}

impl ConvictionClosePositionDto {
    fn from_position(position: &ClosingConvictedPosition) -> Self {
        Self {
            instrument_id: position.instrument_id,
            asset_key: position.asset_key.clone(),
            symbol: position.symbol.clone(),
            conviction: ConvictionDto::from_level(position.conviction),
        }
    }
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
    pub already_imported_buys: usize,
    pub already_imported_sells: usize,
    pub already_imported_splits: usize,
    pub already_imported_dividends: usize,
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
    /// `replace` triggers refresh mode; `append` (or absent) uses legacy append.
    pub mode: Option<String>,
    /// Required when `mode=replace`; the batch id returned by preview.
    pub replace_batch_id: Option<i64>,
    /// Comma-separated asset keys whose conviction the user keeps despite the
    /// import closing the position.
    #[serde(default)]
    pub conviction_keep: Option<String>,
    /// Comma-separated asset keys whose conviction the user clears to `Other`
    /// as part of this commit.
    #[serde(default)]
    pub conviction_to_other: Option<String>,
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
    crate::api::reject_demo_mutation(&state)?;
    preview_source(&state, &bytes, parse_sharesight).await
}

pub async fn sharesight_commit(
    State(state): State<AppState>,
    Query(params): Query<CommitParams>,
    bytes: Bytes,
) -> Result<Json<ImportResult>, ApiError> {
    crate::api::reject_demo_mutation(&state)?;
    commit_source(&state, &bytes, "SHARESIGHT", &params, parse_sharesight).await
}

pub async fn avanza_preview(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<Json<ImportPreview>, ApiError> {
    crate::api::reject_demo_mutation(&state)?;
    avanza_preview_inner(&state, &bytes).await
}

pub async fn avanza_commit(
    State(state): State<AppState>,
    Query(params): Query<CommitParams>,
    bytes: Bytes,
) -> Result<Json<ImportResult>, ApiError> {
    crate::api::reject_demo_mutation(&state)?;
    let is_replace = params.mode.as_deref() == Some("replace");
    if is_replace {
        let replace_batch_id = params.replace_batch_id.ok_or_else(|| {
            ApiError::bad_request(
                "missing_replace_batch_id",
                "mode=replace requires replace_batch_id".to_string(),
            )
        })?;
        avanza_commit_replace(&state, &bytes, replace_batch_id, &params).await
    } else {
        commit_source(&state, &bytes, "AVANZA", &params, parse_avanza).await
    }
}

pub async fn rollback(
    State(state): State<AppState>,
    Path(batch_id): Path<i64>,
) -> Result<Json<RollbackResult>, ApiError> {
    crate::api::reject_demo_mutation(&state)?;

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

    let closing = closing_convicted(state, &ctx, &plan.new_mapped_rows, GuardMode::Append).await?;

    Ok(Json(ImportPreview {
        metadata: Some(PreviewMetadata {
            title: prepared.header.title.clone(),
            date_from: prepared.header.date_from.to_string(),
            date_to: prepared.header.date_to.to_string(),
        }),
        counts: counts_dto(&plan),
        assets: plan.assets.iter().map(asset_group_dto).collect(),
        already_imported_assets: plan
            .already_imported_assets
            .iter()
            .map(asset_group_dto)
            .collect(),
        new_instruments: plan
            .new_instruments
            .iter()
            .map(new_instrument_dto)
            .collect(),
        warnings: plan.warnings.iter().map(row_note_dto).collect(),
        errors: plan.errors.iter().map(row_note_dto).collect(),
        duplicate_of_batch_id,
        replace_candidate_batch_id: None,
        replace_candidate_warning: None,
        conviction_close_positions: closing
            .iter()
            .map(ConvictionClosePositionDto::from_position)
            .collect(),
    }))
}

/// Avanza-specific preview: enriches the standard preview with replace-candidate metadata.
async fn avanza_preview_inner(
    state: &AppState,
    bytes: &[u8],
) -> Result<Json<ImportPreview>, ApiError> {
    let hash = raw_file_hash(bytes);
    let duplicate_of_batch_id = import_batches::find_by_hash(&state.pool, &hash)
        .await?
        .map(|batch| batch.id);

    let prepared = match parse_avanza(bytes) {
        Ok(prepared) => prepared,
        Err(error) => return Ok(Json(parse_error_preview(error, duplicate_of_batch_id))),
    };

    // Look up the latest AVANZA batch for replace-candidate metadata
    let latest_avanza = import_batches::find_latest_by_source(&state.pool, "AVANZA").await?;
    let replace_candidate_batch_id = latest_avanza.as_ref().map(|b| b.id);
    let replace_candidate_warning = if replace_candidate_batch_id.is_some() {
        let count = import_batches::count_by_source(&state.pool, "AVANZA").await?;
        if count > 1 {
            replace_candidate_batch_id.map(|id| {
                format!(
                    "Multiple Avanza imports found; refreshing batch {id}, others are left untouched"
                )
            })
        } else {
            None
        }
    } else {
        None
    };

    let ctx = load_plan_context(state).await?;
    let plan = build_plan(&prepared, &ctx);

    // The Avanza UI defaults to refresh when a prior batch exists, so predict
    // against that replace candidate; otherwise this first import is an append.
    let closing = match replace_candidate_batch_id {
        Some(batch_id) => {
            let excluded = compute_excluded_instrument_ids(&ctx, &BTreeSet::new(), &plan);
            closing_convicted(
                state,
                &ctx,
                &plan.new_mapped_rows,
                GuardMode::Replace {
                    replace_batch_id: batch_id,
                    excluded_ids: &excluded,
                },
            )
            .await?
        }
        None => closing_convicted(state, &ctx, &plan.new_mapped_rows, GuardMode::Append).await?,
    };

    Ok(Json(ImportPreview {
        metadata: Some(PreviewMetadata {
            title: prepared.header.title.clone(),
            date_from: prepared.header.date_from.to_string(),
            date_to: prepared.header.date_to.to_string(),
        }),
        counts: counts_dto(&plan),
        assets: plan.assets.iter().map(asset_group_dto).collect(),
        already_imported_assets: plan
            .already_imported_assets
            .iter()
            .map(asset_group_dto)
            .collect(),
        new_instruments: plan
            .new_instruments
            .iter()
            .map(new_instrument_dto)
            .collect(),
        warnings: plan.warnings.iter().map(row_note_dto).collect(),
        errors: plan.errors.iter().map(row_note_dto).collect(),
        duplicate_of_batch_id,
        replace_candidate_batch_id,
        replace_candidate_warning,
        conviction_close_positions: closing
            .iter()
            .map(ConvictionClosePositionDto::from_position)
            .collect(),
    }))
}

/// Avanza-specific commit in replace/refresh mode.
async fn avanza_commit_replace(
    state: &AppState,
    bytes: &[u8],
    replace_batch_id: i64,
    params: &CommitParams,
) -> Result<Json<ImportResult>, ApiError> {
    let hash = raw_file_hash(bytes);
    let prepared =
        parse_avanza(bytes).map_err(|error| ApiError::bad_request(error.code, error.message))?;

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

    let mapped: Vec<MappedRow> = effective
        .outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            RowOutcome::Mapped(mapped) => Some(mapped.clone()),
            _ => None,
        })
        .collect();

    // Instruments the user deselected, or where every row is already imported:
    // their old batch rows must be preserved (not deleted) and their ledgers
    // must not be re-validated.
    let excluded_instrument_ids = compute_excluded_instrument_ids(&ctx, &exclude, &plan);

    let closing = closing_convicted(
        state,
        &ctx,
        &plan.new_mapped_rows,
        GuardMode::Replace {
            replace_batch_id,
            excluded_ids: &excluded_instrument_ids,
        },
    )
    .await?;
    let conviction_to_other_ids = resolve_conviction_choices(&closing, params)?;

    let batch_id = refresh_batch(
        state,
        "AVANZA",
        replace_batch_id,
        &hash,
        &mapped,
        &excluded_instrument_ids,
        &conviction_to_other_ids,
    )
    .await?;

    let mut warnings: Vec<RowNoteDto> = unknown
        .into_iter()
        .map(|key| RowNoteDto {
            row: None,
            code: "unknown_exclude_key",
            message: format!("exclude key {key:?} matched no asset"),
        })
        .collect();

    // Surface any plan-level fx_warning notes
    for note in &plan.warnings {
        if note.code == "missing_fx" {
            warnings.push(row_note_dto(note));
        }
    }

    Ok(Json(ImportResult {
        batch_id,
        counts: effective_counts(&plan, &plan.new_mapped_rows),
        warnings,
    }))
}

// Invariant: already-imported rows (fingerprint-matched by build_plan) are
// excluded from new_mapped_rows and therefore never written to write_batch.
// A CSV that is fully already-imported produces an empty write_batch call.
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

    if plan.new_mapped_rows.is_empty() && !plan.already_imported_assets.is_empty() {
        return Err(ApiError::bad_request(
            "nothing_to_import",
            "All rows are already imported — nothing to write.".to_string(),
        ));
    }

    if let Some(existing) = import_batches::find_by_hash(&state.pool, &hash).await? {
        if !params.allow_duplicate {
            return Err(duplicate_conflict(existing.id));
        }
    }

    let closing = closing_convicted(state, &ctx, &plan.new_mapped_rows, GuardMode::Append).await?;
    let conviction_to_other_ids = resolve_conviction_choices(&closing, params)?;

    // Use the already-filtered list from the plan so already-imported
    // rows are not written again on append.
    let batch_id = write_batch(
        state,
        source,
        &hash,
        &plan.new_mapped_rows,
        &conviction_to_other_ids,
    )
    .await?;

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
        counts: effective_counts(&plan, &plan.new_mapped_rows),
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
        already_imported_assets: Vec::new(),
        new_instruments: Vec::new(),
        warnings: Vec::new(),
        errors: vec![RowNoteDto {
            row: error.row,
            code: error.code,
            message: error.message,
        }],
        duplicate_of_batch_id,
        replace_candidate_batch_id: None,
        replace_candidate_warning: None,
        conviction_close_positions: Vec::new(),
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
        already_imported_buys: group.already_imported_buys,
        already_imported_sells: group.already_imported_sells,
        already_imported_splits: group.already_imported_splits,
        already_imported_dividends: group.already_imported_dividends,
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

fn compute_excluded_instrument_ids(
    ctx: &PlanContext,
    exclude: &BTreeSet<String>,
    plan: &ImportPlan,
) -> BTreeSet<i64> {
    ctx.existing_instruments
        .iter()
        .filter(|e| {
            let key = e.asset_key();
            // Explicitly deselected by the user
            if exclude.contains(&key) {
                return true;
            }
            // Fully already-imported: every row fingerprint-matched the DB
            if plan
                .already_imported_assets
                .iter()
                .any(|a| a.asset_key == key)
            {
                return true;
            }
            // Mixed: has new rows AND already-imported rows. The already-imported rows in
            // `mapped` consume their old counterparts via canonical matching, but old rows
            // that are NOT in the new CSV (e.g. an old buy that funded an already-imported
            // sell) would otherwise be deleted, invalidating the ledger.
            plan.assets.iter().any(|a| {
                a.asset_key == key
                    && (a.already_imported_buys
                        + a.already_imported_sells
                        + a.already_imported_splits
                        + a.already_imported_dividends)
                        > 0
            })
        })
        .map(|e| e.id)
        .collect()
}

fn row_note_dto(note: &RowNote) -> RowNoteDto {
    RowNoteDto {
        row: note.row,
        code: note.code,
        message: note.message.clone(),
    }
}

/// Which commit path the guard is predicting for.
enum GuardMode<'a> {
    /// Append: existing ledger plus the import's new rows.
    Append,
    /// Avanza refresh: for non-excluded instruments the batch's old rows are
    /// replaced by the import's rows; excluded instruments are left untouched.
    Replace {
        replace_batch_id: i64,
        excluded_ids: &'a BTreeSet<i64>,
    },
}

/// Signed quantity a mapped row contributes to a position (0 for dividends).
///
/// An invalid row counts as 0: at commit `reject_on_errors` runs first, so only
/// valid rows reach here; in preview an already-erroring row cannot be committed
/// anyway, so treating it as a non-contributor is the safe approximation.
fn mapped_row_quantity(row: &MappedRow) -> i64 {
    if row.proposed.kind == TransactionKind::Dividend {
        return 0;
    }
    domain::validate(&row.proposed).unwrap_or(0)
}

/// Detect existing convicted open positions the pending import would close.
///
/// `new_rows` is always the genuinely-new (not already-imported) mapped set,
/// `plan.new_mapped_rows`. Predicted quantity is a sum of signed non-dividend
/// quantities, which equals the derived position because buys/sells/splits are
/// additive. Refresh distinguishes non-excluded instruments (old batch replaced)
/// from excluded ones (old rows kept, so only genuinely-new rows are appended);
/// a "mixed" instrument (already-imported plus new rows) is excluded, so its new
/// sell is still counted and can close the position.
async fn closing_convicted(
    state: &AppState,
    ctx: &PlanContext,
    new_rows: &[MappedRow],
    mode: GuardMode<'_>,
) -> Result<Vec<ClosingConvictedPosition>, ApiError> {
    let conviction_by_id: BTreeMap<i64, ConvictionLevel> = instruments::list(&state.pool)
        .await?
        .iter()
        .filter_map(|row| ConvictionLevel::from_db_str(&row.conviction).map(|c| (row.id, c)))
        .collect();

    // For refresh, the batch's own non-dividend rows are the ones being replaced.
    let batch_net_by_instrument: BTreeMap<i64, i64> = match &mode {
        GuardMode::Replace {
            replace_batch_id, ..
        } => {
            let mut net = BTreeMap::new();
            for row in transactions::list_for_batch(&state.pool, *replace_batch_id).await? {
                if row.kind != "DIVIDEND" {
                    *net.entry(row.instrument_id).or_insert(0) += row.quantity;
                }
            }
            net
        }
        GuardMode::Append => BTreeMap::new(),
    };

    let mut guard_instruments = Vec::new();
    for existing in &ctx.existing_instruments {
        let ledger = ctx.existing_ledgers.get(&existing.id);
        let current_quantity = ledger.map(|l| net_quantity(l)).unwrap_or(0);

        let new_contribution: i64 = new_rows
            .iter()
            .filter(|row| existing.matches(&row.instrument))
            .map(mapped_row_quantity)
            .sum();

        let predicted_quantity = match &mode {
            GuardMode::Append => predicted_quantity_append(current_quantity, new_contribution),
            GuardMode::Replace { excluded_ids, .. } => {
                let batch_net = batch_net_by_instrument
                    .get(&existing.id)
                    .copied()
                    .unwrap_or(0);
                predicted_quantity_replace(
                    current_quantity,
                    batch_net,
                    new_contribution,
                    excluded_ids.contains(&existing.id),
                )
            }
        };

        guard_instruments.push(GuardInstrument {
            instrument_id: existing.id,
            asset_key: existing.asset_key(),
            symbol: existing.symbol.clone(),
            conviction: conviction_by_id
                .get(&existing.id)
                .copied()
                .unwrap_or(ConvictionLevel::Other),
            current_quantity,
            predicted_quantity,
        });
    }

    Ok(closing_convicted_positions(&guard_instruments))
}

fn parse_asset_key_set(raw: &Option<String>) -> BTreeSet<String> {
    raw.as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .collect()
}

/// Validate the user's keep/clear choices against the closing set and return the
/// instrument ids whose conviction should be cleared to `Other` at commit.
///
/// Every closing asset key must be covered by exactly one of keep/clear; an
/// uncovered key, or a key marked both, rejects the commit. Extra keys that no
/// longer close a position (e.g. after a re-preview) are ignored.
fn resolve_conviction_choices(
    closing: &[ClosingConvictedPosition],
    params: &CommitParams,
) -> Result<BTreeSet<i64>, ApiError> {
    if closing.is_empty() {
        return Ok(BTreeSet::new());
    }

    // Only choices for the currently-closing set matter; keys that no longer
    // close a position (e.g. after a re-preview) are ignored, including for the
    // conflict check, so a stale key cannot spuriously reject the commit.
    let required: BTreeSet<String> = closing
        .iter()
        .map(|position| position.asset_key.clone())
        .collect();
    let keep: BTreeSet<String> = parse_asset_key_set(&params.conviction_keep)
        .intersection(&required)
        .cloned()
        .collect();
    let to_other: BTreeSet<String> = parse_asset_key_set(&params.conviction_to_other)
        .intersection(&required)
        .cloned()
        .collect();

    let conflicting: Vec<String> = keep.intersection(&to_other).cloned().collect();
    if !conflicting.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "conviction_choice_conflict",
            "an asset key was marked both keep and change-to-other".to_string(),
        )
        .with_details(serde_json::json!({ "asset_keys": conflicting })));
    }

    let covered: BTreeSet<String> = keep.union(&to_other).cloned().collect();
    let missing: Vec<serde_json::Value> = closing
        .iter()
        .filter(|position| !covered.contains(&position.asset_key))
        .map(|position| {
            serde_json::json!({
                "instrument_id": position.instrument_id,
                "asset_key": position.asset_key,
                "symbol": position.symbol,
            })
        })
        .collect();
    if !missing.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "conviction_choice_required",
            "closing convicted positions require a keep or change-to-other choice".to_string(),
        )
        .with_details(serde_json::json!({ "closing_positions": missing })));
    }

    Ok(closing
        .iter()
        .filter(|position| to_other.contains(&position.asset_key))
        .map(|position| position.instrument_id)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{compute_excluded_instrument_ids, effective_counts, AssetGroup, ImportPlan};
    use crate::domain::{ProposedTransaction, TransactionKind};
    use crate::import::core::outcome::InstrumentKey;
    use crate::import::core::outcome::MappedRow;
    use crate::import::core::plan::{ExistingInstrument, PlanContext, PlanCounts};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use std::collections::{BTreeMap, BTreeSet};

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
                price: None,
                dividend_per_share: Some(dec!(7.5)),
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
                already_imported_buys: 0,
                already_imported_sells: 0,
                already_imported_splits: 0,
                already_imported_dividends: 0,
                default_selected: true,
                skipped_reason: None,
                warnings: Vec::new(),
                errors: Vec::new(),
                is_new_instrument: false,
            }],
            already_imported_assets: Vec::new(),
            new_mapped_rows: Vec::new(),
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

    #[test]
    fn already_imported_assets_are_added_to_excluded_instrument_ids() {
        let ctx = PlanContext {
            existing_instruments: vec![ExistingInstrument {
                id: 42,
                exchange: "AVANZA".into(),
                symbol: "US0231351067".into(),
                currency: "USD".into(),
                isin: Some("US0231351067".into()),
            }],
            existing_ledgers: BTreeMap::new(),
            max_existing_id: 0,
        };
        let exclude: BTreeSet<String> = BTreeSet::new();
        let plan = ImportPlan {
            counts: PlanCounts::default(),
            new_instruments: Vec::new(),
            assets: Vec::new(),
            already_imported_assets: vec![AssetGroup {
                asset_key: "US0231351067".into(),
                name: "Some Stock".into(),
                currency: "USD".into(),
                buys: 0,
                sells: 0,
                splits: 0,
                dividends: 0,
                already_imported_buys: 0,
                already_imported_sells: 1,
                already_imported_splits: 0,
                already_imported_dividends: 0,
                default_selected: true,
                skipped_reason: None,
                warnings: Vec::new(),
                errors: Vec::new(),
                is_new_instrument: false,
            }],
            new_mapped_rows: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
        };

        let excluded = compute_excluded_instrument_ids(&ctx, &exclude, &plan);
        assert!(
            excluded.contains(&42),
            "instrument in already_imported_assets must be excluded to protect its old batch rows"
        );
    }

    #[test]
    fn mixed_asset_with_already_imported_rows_is_added_to_excluded_instrument_ids() {
        // Rocket Lab scenario: old batch has buy_old + sell; new CSV shows 1 new buy + same
        // sell (already-imported). The already-imported sell consumes the old sell via
        // canonical matching. buy_old has no match and would be deleted, making the sell
        // invalid. Protection must extend to mixed assets that have already-imported row counts.
        let ctx = PlanContext {
            existing_instruments: vec![ExistingInstrument {
                id: 11,
                exchange: "AVANZA".into(),
                symbol: "US7731211089".into(),
                currency: "USD".into(),
                isin: Some("US7731211089".into()),
            }],
            existing_ledgers: BTreeMap::new(),
            max_existing_id: 0,
        };
        let exclude: BTreeSet<String> = BTreeSet::new();
        // Mixed asset: 1 new buy, 1 already-imported sell (still in the new CSV)
        let plan = ImportPlan {
            counts: PlanCounts::default(),
            new_instruments: Vec::new(),
            assets: vec![AssetGroup {
                asset_key: "US7731211089".into(),
                name: "Rocket Lab".into(),
                currency: "USD".into(),
                buys: 1,
                sells: 0,
                splits: 0,
                dividends: 0,
                already_imported_buys: 0,
                already_imported_sells: 1,
                already_imported_splits: 0,
                already_imported_dividends: 0,
                default_selected: true,
                skipped_reason: None,
                warnings: Vec::new(),
                errors: Vec::new(),
                is_new_instrument: false,
            }],
            already_imported_assets: Vec::new(),
            new_mapped_rows: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
        };

        let excluded = compute_excluded_instrument_ids(&ctx, &exclude, &plan);
        assert!(
            excluded.contains(&11),
            "mixed asset with already-imported rows must be excluded to protect old batch rows \
             that the already-imported sell depends on"
        );
    }

    #[test]
    fn explicitly_excluded_asset_is_also_in_excluded_instrument_ids() {
        let ctx = PlanContext {
            existing_instruments: vec![ExistingInstrument {
                id: 7,
                exchange: "AVANZA".into(),
                symbol: "SE0000108656".into(),
                currency: "SEK".into(),
                isin: Some("SE0000108656".into()),
            }],
            existing_ledgers: BTreeMap::new(),
            max_existing_id: 0,
        };
        let mut exclude: BTreeSet<String> = BTreeSet::new();
        exclude.insert("SE0000108656".into());
        let plan = ImportPlan {
            counts: PlanCounts::default(),
            new_instruments: Vec::new(),
            assets: Vec::new(),
            already_imported_assets: Vec::new(),
            new_mapped_rows: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
        };

        let excluded = compute_excluded_instrument_ids(&ctx, &exclude, &plan);
        assert!(
            excluded.contains(&7),
            "explicitly excluded asset must remain in excluded_instrument_ids"
        );
    }
}
