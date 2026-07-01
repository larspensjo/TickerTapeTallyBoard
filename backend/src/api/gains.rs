use axum::extract::{Query, State};
use axum::Json;
use chrono::{Local, NaiveDate};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

use crate::api::error::ApiError;
use crate::api::valuation::{
    load_period_inputs, load_valuation_inputs, money_string, serialize_availability,
    AvailabilityResponse, BASE_CURRENCY,
};
use crate::db::{instruments, transactions};
use crate::domain::{
    actual_period_cash_flows, compute_period_amounts, derive_position_performance,
    period_cash_flows, reconstruct_period, summarize_holdings, value_position, Availability,
    ValuationReason,
};
use crate::state::AppState;

mod performance;
use performance::{parse_method, PerformanceAccumulator, ReturnMethod};

mod rows;
use rows::{closed_gain_row, open_gain_row, serialize_reasons};

mod types;
use types::*;

#[cfg(test)]
mod tests;

fn parse_date(s: &str, field: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("invalid_date", format!("invalid {field}: {s}")))
}

pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<GainsQuery>,
) -> Result<Json<GainsResponse>, ApiError> {
    let method = parse_method(query.method.as_deref())?;
    let end_date = match &query.end_date {
        Some(s) => parse_date(s, "end_date")?,
        None => Local::now().naive_local().date(),
    };
    let start_date = match &query.start_date {
        Some(s) => {
            let d = parse_date(s, "start_date")?;
            if d > end_date {
                return Err(ApiError::bad_request(
                    "start_date_after_end_date",
                    "start_date must not be after end_date",
                ));
            }
            Some(d)
        }
        None => None,
    };

    let instruments_list = instruments::list(&state.pool).await?;
    let transaction_rows = transactions::all_for_holdings(&state.pool).await?;
    let mut ledgers: BTreeMap<i64, Vec<_>> = BTreeMap::new();
    let mut full_ledgers: BTreeMap<i64, Vec<_>> = BTreeMap::new();

    for row in &transaction_rows {
        let ledger_tx = row.to_ledger()?;
        full_ledgers
            .entry(row.instrument_id)
            .or_insert_with(Vec::new)
            .push(ledger_tx.clone());
        // Truncate ledger at end_date — transactions after end_date must not affect any result.
        if ledger_tx.trade_date <= end_date {
            ledgers
                .entry(row.instrument_id)
                .or_insert_with(Vec::new)
                .push(ledger_tx);
        }
    }

    let report_start_date = match start_date {
        Some(d) => d,
        None => ledgers
            .values()
            .flat_map(|ledger| ledger.iter().map(|tx| tx.trade_date))
            .min()
            .unwrap_or(end_date),
    };
    let has_report_start =
        start_date.is_some() || ledgers.values().any(|ledger| !ledger.is_empty());

    let mut valued_holdings = Vec::new();
    let mut gain_rows = Vec::new();
    let mut perf_accum = PerformanceAccumulator::default();

    for instrument in &instruments_list {
        let ledger = ledgers.remove(&instrument.id).unwrap_or_default();
        if ledger.is_empty() {
            continue;
        }
        let full_ledger = full_ledgers.remove(&instrument.id).unwrap_or_default();

        let performance = derive_position_performance(&ledger).map_err(|error| {
            ApiError::internal(format!(
                "inconsistent stored ledger for instrument {}: {error:?}",
                instrument.id
            ))
        })?;

        // Reconstruct period using the full ledger (including post-end_date transactions)
        // so that post_period_split_factor is correctly computed for split-adjusted cash flows.
        let period =
            reconstruct_period(&full_ledger, report_start_date, end_date).map_err(|error| {
                ApiError::internal(format!(
                    "failed to reconstruct period for instrument {}: {error:?}",
                    instrument.id
                ))
            })?;

        let has_period_exposure = period.start_position.quantity > 0
            || !period.period_transactions.is_empty()
            || period.end_position.quantity > 0;
        let performance_start_date = has_period_exposure.then_some(report_start_date);
        let mut row_income_base: Availability<Decimal> = Availability::Available(Decimal::ZERO);
        if has_period_exposure {
            let period_inputs =
                load_period_inputs(&state.pool, instrument, Some(report_start_date), end_date)
                    .await?;
            let is_sek = instrument.currency.eq_ignore_ascii_case(BASE_CURRENCY);

            let start_price = period_inputs.start_price.map(|p| p.close);
            let end_price = period_inputs.end_price.map(|p| p.close);
            let start_fx = period_inputs.start_fx.map(|f| f.rate);
            let end_fx = period_inputs.end_fx.map(|f| f.rate);

            let period_amounts =
                compute_period_amounts(&period, start_price, end_price, start_fx, end_fx, is_sek);
            row_income_base = period_amounts.income_base.clone();
            let cash_flows = period_cash_flows(&period, is_sek);
            let actual_cash_flows = actual_period_cash_flows(&period, is_sek);

            perf_accum.add(&period_amounts, &cash_flows, &actual_cash_flows);
        }

        if performance.position.quantity == 0 {
            if query.include_closed && performance.realized.sold_quantity > 0 {
                gain_rows.push(closed_gain_row(
                    instrument,
                    &performance.realized,
                    performance_start_date,
                    &row_income_base,
                )?);
            }
            continue;
        }

        let valuation_inputs = load_valuation_inputs(&state.pool, instrument, end_date).await?;

        let valued_holding = value_position(
            &performance.position,
            &instrument.currency,
            end_date,
            valuation_inputs.latest_price,
            valuation_inputs.previous_price,
            valuation_inputs.latest_fx,
            valuation_inputs.previous_fx,
        );

        valued_holdings.push(valued_holding.clone());

        gain_rows.push(open_gain_row(
            instrument,
            &valued_holding,
            &performance.realized,
            performance_start_date,
            &row_income_base,
        )?);
    }

    let summary = summarize_holdings(&valued_holdings);

    // Snapshot the accumulated amounts before consuming the accumulator.
    let perf_capital_gain = perf_accum.capital_gain;
    let perf_currency_gain = perf_accum.currency_gain;
    let perf_income_gain = perf_accum.income_gain;
    let perf_total_return = perf_accum.total_return;
    let perf_has_data = perf_accum.has_data;
    let perf_unavailable_reasons = perf_accum.unavailable_reasons.clone();
    let perf_excluded_rows = perf_accum.excluded_rows;

    let (
        capital_gain_percent,
        income_percent,
        currency_gain_percent,
        total_return_percent,
        display_percent_kind,
    ) = perf_accum.into_percents(report_start_date, end_date, method);

    let percentage_method = match method {
        ReturnMethod::Xirr => "money_weighted",
        ReturnMethod::Simple => "simple",
        ReturnMethod::ModifiedDietz => "modified_dietz",
    }
    .to_string();

    Ok(Json(GainsResponse {
        as_of_date: end_date.format("%Y-%m-%d").to_string(),
        base_currency: BASE_CURRENCY.to_string(),
        include_closed_positions: query.include_closed,
        report_period: ReportPeriodResponse {
            start_date: has_report_start.then(|| report_start_date.format("%Y-%m-%d").to_string()),
            end_date: end_date.format("%Y-%m-%d").to_string(),
        },
        percentage_method,
        display_percent_kind,
        summary: SummaryResponse {
            market_value_base: serialize_availability(&summary.market_value_base, |v| {
                money_string(*v)
            }),
            cost_basis_base: serialize_availability(&summary.cost_basis_base, |v| money_string(*v)),
            price_effect_base: serialize_availability(&summary.price_effect_base, |v| {
                money_string(*v)
            }),
            fx_effect_base: serialize_availability(&summary.fx_effect_base, |v| money_string(*v)),
            unrealized_gain_base: serialize_availability(&summary.unrealized_gain_base, |v| {
                money_string(*v)
            }),
            unrealized_gain_percent: serialize_availability(
                &summary.unrealized_gain_percent,
                |v| format!("{:.2}", v),
            ),
            day_change_base: serialize_availability(&summary.day_change_base, |v| money_string(*v)),
            day_change_percent: serialize_availability(&summary.day_change_percent, |v| {
                format!("{:.2}", v)
            }),
            excluded_rows: summary.excluded_rows,
        },
        totals: TotalsResponse {
            capital_gain_base: perf_amount_response(
                perf_has_data,
                &perf_unavailable_reasons,
                perf_capital_gain,
            ),
            capital_gain_percent,
            income_base: perf_amount_response(
                perf_has_data,
                &perf_unavailable_reasons,
                perf_income_gain,
            ),
            income_percent,
            currency_gain_base: perf_amount_response(
                perf_has_data,
                &perf_unavailable_reasons,
                perf_currency_gain,
            ),
            currency_gain_percent,
            total_return_base: perf_amount_response(
                perf_has_data,
                &perf_unavailable_reasons,
                perf_total_return,
            ),
            total_return_percent,
            excluded_rows: perf_excluded_rows,
        },
        rows: gain_rows,
    }))
}

fn perf_amount_response(
    has_data: bool,
    unavailable_reasons: &[ValuationReason],
    value: Decimal,
) -> AvailabilityResponse {
    if has_data {
        return AvailabilityResponse::Available {
            value: money_string(value),
        };
    }
    if !unavailable_reasons.is_empty() {
        return AvailabilityResponse::Unavailable {
            reasons: serialize_reasons(unavailable_reasons),
        };
    }
    AvailabilityResponse::Unavailable {
        reasons: Vec::new(),
    }
}
