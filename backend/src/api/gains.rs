use axum::extract::{Query, State};
use axum::Json;
use chrono::{Local, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::api::error::ApiError;
use crate::api::instruments::InstrumentResponse;
use crate::api::valuation::{
    fx_snapshot_response, load_period_inputs, load_valuation_inputs, money_string,
    price_snapshot_response, serialize_availability, serialize_valuation_reason,
    AvailabilityResponse, FxSnapshotResponse, PriceSnapshotResponse, BASE_CURRENCY,
};
use crate::db::{instruments, transactions};
use crate::domain::{
    actual_period_cash_flows, apply_annualisation, compute_modified_dietz_denominator,
    compute_money_weighted_return, compute_period_amounts, derive_position_performance,
    period_cash_flows, reconstruct_period, summarize_holdings, value_position, Availability,
    BaseAmount, CashFlow, DisplayPercentKind, PeriodAmounts, RealizedGain, ValuationReason,
    ValuedHolding,
};
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReturnMethod {
    Xirr,
    Simple,
    ModifiedDietz,
}

fn parse_method(value: Option<&str>) -> Result<ReturnMethod, ApiError> {
    match value.unwrap_or("xirr") {
        "xirr" => Ok(ReturnMethod::Xirr),
        "simple" => Ok(ReturnMethod::Simple),
        "modified_dietz" => Ok(ReturnMethod::ModifiedDietz),
        other => Err(ApiError::bad_request(
            "invalid_method",
            format!("invalid method: {other}"),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct GainsQuery {
    #[serde(default)]
    include_closed: bool,
    start_date: Option<String>,
    end_date: Option<String>,
    method: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReportPeriodResponse {
    pub start_date: Option<String>,
    pub end_date: String,
}

#[derive(Debug, Serialize)]
pub struct GainsResponse {
    pub as_of_date: String,
    pub base_currency: String,
    pub include_closed_positions: bool,
    pub report_period: ReportPeriodResponse,
    pub percentage_method: String,
    pub display_percent_kind: String,
    pub summary: SummaryResponse,
    pub totals: TotalsResponse,
    pub rows: Vec<GainRow>,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub market_value_base: AvailabilityResponse,
    pub cost_basis_base: AvailabilityResponse,
    pub price_effect_base: AvailabilityResponse,
    pub fx_effect_base: AvailabilityResponse,
    pub unrealized_gain_base: AvailabilityResponse,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub day_change_base: AvailabilityResponse,
    pub day_change_percent: AvailabilityResponse,
    pub excluded_rows: usize,
}

#[derive(Debug, Serialize)]
pub struct TotalsResponse {
    pub capital_gain_base: AvailabilityResponse,
    pub capital_gain_percent: AvailabilityResponse,
    pub income_base: AvailabilityResponse,
    pub income_percent: AvailabilityResponse,
    pub currency_gain_base: AvailabilityResponse,
    pub currency_gain_percent: AvailabilityResponse,
    pub total_return_base: AvailabilityResponse,
    pub total_return_percent: AvailabilityResponse,
    pub excluded_rows: usize,
}

#[derive(Debug, Serialize)]
pub struct GainRow {
    pub instrument: InstrumentResponse,
    pub quantity: i64,
    pub cost_basis_native: String,
    pub cost_basis_base: AvailabilityResponse,
    pub performance_start_date: Option<String>,
    pub performance_denominator_base: AvailabilityResponse,
    pub capital_gain_base: AvailabilityResponse,
    pub capital_gain_percent: AvailabilityResponse,
    pub currency_gain_base: AvailabilityResponse,
    pub currency_gain_percent: AvailabilityResponse,
    pub income_base: AvailabilityResponse,
    pub total_return_base: AvailabilityResponse,
    pub total_return_percent: AvailabilityResponse,
    pub price_effect_base: AvailabilityResponse,
    pub fx_effect_base: AvailabilityResponse,
    pub latest_price: Option<PriceSnapshotResponse>,
    pub previous_price: Option<PriceSnapshotResponse>,
    pub latest_fx: Option<FxSnapshotResponse>,
    pub previous_fx: Option<FxSnapshotResponse>,
    pub market_value_native: AvailabilityResponse,
    pub market_value_base: AvailabilityResponse,
    pub proceeds_native: AvailabilityResponse,
    pub proceeds_base: AvailabilityResponse,
    pub unrealized_gain_base: AvailabilityResponse,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub realized_gain_base: AvailabilityResponse,
    pub realized_cost_basis_base: AvailabilityResponse,
    pub day_change_base: AvailabilityResponse,
    pub day_change_percent: AvailabilityResponse,
    pub reasons: Vec<String>,
    pub position_status: GainPositionStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GainPositionStatus {
    Open,
    Closed,
}

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

fn serialize_reasons(reasons: &[ValuationReason]) -> Vec<String> {
    reasons.iter().map(serialize_valuation_reason).collect()
}

fn component_percent(gain: Decimal, denominator: &Availability<Decimal>) -> AvailabilityResponse {
    match denominator {
        Availability::Available(d) => AvailabilityResponse::Available {
            value: format!("{:.2}", (gain / d) * Decimal::from(100)),
        },
        Availability::Unavailable { reasons } => AvailabilityResponse::Unavailable {
            reasons: serialize_reasons(reasons),
        },
    }
}

fn current_position_percent(
    amount: &Availability<Decimal>,
    cost_basis: &Availability<Decimal>,
) -> AvailabilityResponse {
    match (amount.as_ref(), cost_basis.as_ref()) {
        (Some(gain), Some(cb)) if !cb.is_zero() => AvailabilityResponse::Available {
            value: format!("{:.2}", (*gain / *cb) * Decimal::from(100)),
        },
        (Some(_), Some(_)) => AvailabilityResponse::Unavailable {
            reasons: serialize_reasons(&[ValuationReason::ZeroCostBasis]),
        },
        _ => AvailabilityResponse::Unavailable {
            reasons: serialize_reasons(&merge_response_reasons(&[
                amount.reasons(),
                cost_basis.reasons(),
            ])),
        },
    }
}

fn merge_response_reasons(sources: &[Vec<ValuationReason>]) -> Vec<ValuationReason> {
    let mut reasons = Vec::new();
    for source in sources {
        reasons.extend_from_slice(source);
    }
    dedup_valuation_reasons(&mut reasons);
    reasons
}

#[derive(Default)]
struct PerformanceAccumulator {
    begin_mv: Decimal,
    end_mv: Decimal,
    total_return: Decimal,
    capital_gain: Decimal,
    currency_gain: Decimal,
    income_gain: Decimal,
    cash_flows: Vec<CashFlow>,
    actual_cash_flows: Vec<CashFlow>,
    unavailable_reasons: Vec<ValuationReason>,
    excluded_rows: usize,
    has_data: bool,
}

impl PerformanceAccumulator {
    fn add(
        &mut self,
        amounts: &PeriodAmounts,
        flows: &Availability<Vec<CashFlow>>,
        actual_flows: &Availability<Vec<CashFlow>>,
    ) {
        match (
            &amounts.begin_market_value_base,
            &amounts.end_market_value_base,
            &amounts.total_return_base,
            &amounts.capital_gain_base,
            &amounts.currency_gain_base,
            &amounts.income_base,
            flows,
            actual_flows,
        ) {
            (
                Availability::Available(begin),
                Availability::Available(end),
                Availability::Available(total),
                Availability::Available(cap),
                Availability::Available(cur),
                Availability::Available(income),
                Availability::Available(cfs),
                Availability::Available(actual_cfs),
            ) => {
                self.begin_mv += begin;
                self.end_mv += end;
                self.total_return += total;
                self.capital_gain += cap;
                self.currency_gain += cur;
                self.income_gain += income;
                self.cash_flows.extend_from_slice(cfs);
                self.actual_cash_flows.extend_from_slice(actual_cfs);
                self.has_data = true;
            }
            _ => {
                self.excluded_rows += 1;
                // Collect all reasons for transparency.
                for reasons in [
                    amounts.begin_market_value_base.reasons(),
                    amounts.total_return_base.reasons(),
                    amounts.capital_gain_base.reasons(),
                    amounts.currency_gain_base.reasons(),
                    amounts.income_base.reasons(),
                    flows.reasons(),
                    actual_flows.reasons(),
                ] {
                    self.unavailable_reasons.extend(reasons);
                }
                dedup_valuation_reasons(&mut self.unavailable_reasons);
            }
        }
    }

    fn into_percents(
        self,
        start: NaiveDate,
        end: NaiveDate,
        method: ReturnMethod,
    ) -> (
        AvailabilityResponse, // capital_gain_percent
        AvailabilityResponse, // income_percent
        AvailabilityResponse, // currency_gain_percent
        AvailabilityResponse, // total_return_percent
        String,               // display_percent_kind
    ) {
        if !self.has_data {
            let u = AvailabilityResponse::Unavailable {
                reasons: serialize_reasons(&self.unavailable_reasons),
            };
            return (u.clone(), u.clone(), u.clone(), u, "absolute".to_string());
        }
        let pct100 = |x: Decimal| format!("{:.2}", (x * Decimal::from(100)).round_dp(2));
        match method {
            ReturnMethod::Xirr => {
                let mw = compute_money_weighted_return(
                    &Availability::Available(self.begin_mv),
                    &Availability::Available(self.actual_cash_flows.clone()),
                    &Availability::Available(self.end_mv),
                    start,
                    end,
                );
                let total_pct = match &mw {
                    Availability::Available(v) => v.cumulative,
                    Availability::Unavailable { reasons } => {
                        let u = AvailabilityResponse::Unavailable {
                            reasons: serialize_reasons(reasons),
                        };
                        return (
                            u.clone(),
                            u.clone(),
                            u.clone(),
                            u,
                            "money_weighted".to_string(),
                        );
                    }
                };
                let comp = |part: Decimal| -> AvailabilityResponse {
                    if self.total_return.is_zero() {
                        if self.capital_gain.is_zero()
                            && self.currency_gain.is_zero()
                            && self.income_gain.is_zero()
                        {
                            return AvailabilityResponse::Available {
                                value: "0.00".to_string(),
                            };
                        }
                        return AvailabilityResponse::Unavailable {
                            reasons: serialize_reasons(&[
                                ValuationReason::ZeroOrInvalidPerformanceDenominator,
                            ]),
                        };
                    }
                    AvailabilityResponse::Available {
                        value: pct100(total_pct * (part / self.total_return)),
                    }
                };
                (
                    comp(self.capital_gain),
                    comp(self.income_gain),
                    comp(self.currency_gain),
                    AvailabilityResponse::Available {
                        value: pct100(total_pct),
                    },
                    "money_weighted".to_string(),
                )
            }
            ReturnMethod::Simple => {
                let gross_buys: Decimal = self
                    .actual_cash_flows
                    .iter()
                    .map(|c| c.amount_base.max(Decimal::ZERO))
                    .sum();
                let denom = self.begin_mv + gross_buys;
                if denom <= Decimal::ZERO {
                    let u = AvailabilityResponse::Unavailable {
                        reasons: serialize_reasons(&[
                            ValuationReason::ZeroOrInvalidPerformanceDenominator,
                        ]),
                    };
                    return (u.clone(), u.clone(), u.clone(), u, "simple".to_string());
                }
                let comp = |part: Decimal| AvailabilityResponse::Available {
                    value: pct100(part / denom),
                };
                (
                    comp(self.capital_gain),
                    comp(self.income_gain),
                    comp(self.currency_gain),
                    comp(self.total_return),
                    "simple".to_string(),
                )
            }
            ReturnMethod::ModifiedDietz => {
                let period_days = (end - start).num_days();
                let denominator =
                    compute_modified_dietz_denominator(self.begin_mv, &self.cash_flows, start, end);
                let cap = component_percent(self.capital_gain, &denominator);
                let income = component_percent(self.income_gain, &denominator);
                let cur = component_percent(self.currency_gain, &denominator);
                match &denominator {
                    Availability::Available(d) => {
                        let (v, kind) = apply_annualisation(self.total_return / d, period_days);
                        let label = match kind {
                            DisplayPercentKind::Annualised => "annualised",
                            DisplayPercentKind::Absolute => "absolute",
                        };
                        (
                            cap,
                            income,
                            cur,
                            AvailabilityResponse::Available { value: pct100(v) },
                            label.to_string(),
                        )
                    }
                    Availability::Unavailable { reasons } => (
                        cap,
                        income,
                        cur,
                        AvailabilityResponse::Unavailable {
                            reasons: serialize_reasons(reasons),
                        },
                        "absolute".to_string(),
                    ),
                }
            }
        }
    }
}

fn open_gain_row(
    instrument: &instruments::InstrumentRow,
    valued_holding: &ValuedHolding,
    realized: &RealizedGain,
    performance_start_date: Option<NaiveDate>,
    income_base: &Availability<Decimal>,
) -> Result<GainRow, ApiError> {
    Ok(GainRow {
        instrument: InstrumentResponse::from_row(instrument)?,
        quantity: valued_holding.quantity,
        cost_basis_native: money_string(valued_holding.cost_basis_native),
        cost_basis_base: serialize_availability(&valued_holding.cost_basis_base, |v| {
            money_string(*v)
        }),
        performance_start_date: performance_start_date.map(|d| d.format("%Y-%m-%d").to_string()),
        performance_denominator_base: serialize_availability(
            &valued_holding.cost_basis_base,
            |v| money_string(*v),
        ),
        total_return_base: serialize_availability(&valued_holding.unrealized_gain_base, |v| {
            money_string(*v)
        }),
        total_return_percent: current_position_percent(
            &valued_holding.unrealized_gain_base,
            &valued_holding.cost_basis_base,
        ),
        capital_gain_base: serialize_availability(&valued_holding.price_effect_base, |v| {
            money_string(*v)
        }),
        capital_gain_percent: current_position_percent(
            &valued_holding.price_effect_base,
            &valued_holding.cost_basis_base,
        ),
        currency_gain_base: serialize_availability(&valued_holding.fx_effect_base, |v| {
            money_string(*v)
        }),
        currency_gain_percent: current_position_percent(
            &valued_holding.fx_effect_base,
            &valued_holding.cost_basis_base,
        ),
        income_base: serialize_availability(income_base, |v| money_string(*v)),
        price_effect_base: serialize_availability(&valued_holding.price_effect_base, |v| {
            money_string(*v)
        }),
        fx_effect_base: serialize_availability(&valued_holding.fx_effect_base, |v| {
            money_string(*v)
        }),
        latest_price: valued_holding
            .latest_price
            .as_ref()
            .map(price_snapshot_response),
        previous_price: valued_holding
            .previous_price
            .as_ref()
            .map(price_snapshot_response),
        latest_fx: valued_holding.latest_fx.as_ref().map(fx_snapshot_response),
        previous_fx: valued_holding
            .previous_fx
            .as_ref()
            .map(fx_snapshot_response),
        market_value_native: serialize_availability(&valued_holding.market_value_native, |v| {
            money_string(*v)
        }),
        market_value_base: serialize_availability(&valued_holding.market_value_base, |v| {
            money_string(*v)
        }),
        proceeds_native: AvailabilityResponse::Unavailable {
            reasons: Vec::new(),
        },
        proceeds_base: AvailabilityResponse::Unavailable {
            reasons: Vec::new(),
        },
        unrealized_gain_base: serialize_availability(&valued_holding.unrealized_gain_base, |v| {
            money_string(*v)
        }),
        unrealized_gain_percent: serialize_availability(
            &valued_holding.unrealized_gain_percent,
            |v| format!("{:.2}", v),
        ),
        realized_gain_base: serialize_base_amount(&realized.gain_base),
        realized_cost_basis_base: serialize_base_amount(&realized.cost_basis_base),
        day_change_base: serialize_availability(&valued_holding.day_change_base, |v| {
            money_string(*v)
        }),
        day_change_percent: serialize_availability(&valued_holding.day_change_percent, |v| {
            format!("{:.2}", v)
        }),
        reasons: valued_holding
            .reasons
            .iter()
            .map(serialize_valuation_reason)
            .collect(),
        position_status: GainPositionStatus::Open,
    })
}

fn closed_gain_row(
    instrument: &instruments::InstrumentRow,
    realized: &RealizedGain,
    performance_start_date: Option<NaiveDate>,
    income_base: &Availability<Decimal>,
) -> Result<GainRow, ApiError> {
    let cost_basis_base = base_amount_availability(&realized.cost_basis_base);
    let gain_base = base_amount_availability(&realized.gain_base);
    let gain_percent = match (gain_base.as_ref(), cost_basis_base.as_ref()) {
        (Some(gain), Some(cost_basis)) if *cost_basis != Decimal::ZERO => {
            Availability::available((*gain / *cost_basis) * Decimal::from(100))
        }
        (Some(_), Some(_)) => Availability::unavailable(ValuationReason::ZeroCostBasis),
        _ => Availability::Unavailable {
            reasons: merge_closed_reasons(&[gain_base.reasons(), cost_basis_base.reasons()]),
        },
    };
    let mut reasons = merge_closed_reasons(&[
        cost_basis_base.reasons(),
        base_amount_availability(&realized.proceeds_base).reasons(),
        base_amount_availability(&realized.price_effect_base).reasons(),
        base_amount_availability(&realized.fx_effect_base).reasons(),
        gain_base.reasons(),
        gain_percent.reasons(),
    ]);

    dedup_valuation_reasons(&mut reasons);

    Ok(GainRow {
        instrument: InstrumentResponse::from_row(instrument)?,
        quantity: 0,
        cost_basis_native: money_string(realized.cost_basis_native),
        cost_basis_base: serialize_availability(&cost_basis_base, |v| money_string(*v)),
        performance_start_date: performance_start_date.map(|d| d.format("%Y-%m-%d").to_string()),
        performance_denominator_base: serialize_availability(&cost_basis_base, |v| {
            money_string(*v)
        }),
        capital_gain_base: serialize_base_amount(&realized.price_effect_base),
        capital_gain_percent: current_position_percent(
            &base_amount_availability(&realized.price_effect_base),
            &cost_basis_base,
        ),
        currency_gain_base: serialize_base_amount(&realized.fx_effect_base),
        currency_gain_percent: current_position_percent(
            &base_amount_availability(&realized.fx_effect_base),
            &cost_basis_base,
        ),
        income_base: serialize_availability(income_base, |v| money_string(*v)),
        total_return_base: serialize_availability(&gain_base, |v| money_string(*v)),
        total_return_percent: serialize_availability(&gain_percent, |v| format!("{:.2}", v)),
        price_effect_base: serialize_base_amount(&realized.price_effect_base),
        fx_effect_base: serialize_base_amount(&realized.fx_effect_base),
        latest_price: None,
        previous_price: None,
        latest_fx: None,
        previous_fx: None,
        market_value_native: AvailabilityResponse::Available {
            value: money_string(Decimal::ZERO),
        },
        market_value_base: AvailabilityResponse::Available {
            value: money_string(Decimal::ZERO),
        },
        proceeds_native: AvailabilityResponse::Available {
            value: money_string(realized.proceeds_native),
        },
        proceeds_base: serialize_base_amount(&realized.proceeds_base),
        unrealized_gain_base: serialize_availability(&gain_base, |v| money_string(*v)),
        unrealized_gain_percent: serialize_availability(&gain_percent, |v| format!("{:.2}", v)),
        realized_gain_base: serialize_availability(&gain_base, |v| money_string(*v)),
        realized_cost_basis_base: serialize_availability(&cost_basis_base, |v| money_string(*v)),
        day_change_base: AvailabilityResponse::Unavailable {
            reasons: Vec::new(),
        },
        day_change_percent: AvailabilityResponse::Unavailable {
            reasons: Vec::new(),
        },
        reasons: reasons.iter().map(serialize_valuation_reason).collect(),
        position_status: GainPositionStatus::Closed,
    })
}

fn serialize_base_amount(value: &BaseAmount) -> AvailabilityResponse {
    let availability = base_amount_availability(value);
    serialize_availability(&availability, |v| money_string(*v))
}

fn base_amount_availability(value: &BaseAmount) -> Availability<Decimal> {
    match value {
        BaseAmount::Available(value) => Availability::available(*value),
        BaseAmount::Unavailable { .. } => Availability::unavailable(ValuationReason::MissingFx),
    }
}

fn merge_closed_reasons(sources: &[Vec<ValuationReason>]) -> Vec<ValuationReason> {
    let mut reasons = Vec::new();
    for source in sources {
        reasons.extend_from_slice(source);
    }
    dedup_valuation_reasons(&mut reasons);
    reasons
}

fn dedup_valuation_reasons(reasons: &mut Vec<ValuationReason>) {
    let mut deduped = Vec::new();
    for reason in reasons.drain(..) {
        if !deduped.contains(&reason) {
            deduped.push(reason);
        }
    }
    *reasons = deduped;
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Local};

    use crate::api::router;
    use crate::api::valuation::{BASE_CURRENCY, FX_PROVIDER, PRICE_PROVIDER};
    use crate::db::{fx_rates, prices, provider_symbols};
    use crate::import::now_iso8601;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use rust_decimal_macros::dec;
    use serde_json::json;
    use tower::ServiceExt;

    async fn send(
        state: &AppState,
        method: &str,
        uri: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request builds");
        let response = router(state.clone())
            .oneshot(request)
            .await
            .expect("request completes");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body readable");
        let value = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json body")
        };
        (status, value)
    }

    #[tokio::test]
    async fn gains_empty_portfolio() {
        let state = AppState::for_tests().await;
        let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["base_currency"], "SEK");
        assert_eq!(body["include_closed_positions"], false);
        assert_eq!(body["rows"].as_array().unwrap().len(), 0);
        assert_eq!(body["summary"]["excluded_rows"], 0);
        assert_eq!(body["totals"]["excluded_rows"], 0);
        assert_unavailable(&body["totals"]["capital_gain_base"], &[]);
        assert_unavailable(&body["totals"]["income_base"], &[]);
    }

    #[tokio::test]
    async fn gains_open_row_percent_is_current_position_not_period_hybrid() {
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
        let latest = Local::now().naive_local().date();
        let previous = latest - Duration::days(1);

        // Opening buy well before the period, then an in-period partial sell, open remainder.
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
                   "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-03-01",
                   "quantity":4,"price":"150","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;
        seed_market_data(&state, instrument_id, latest, previous).await;

        let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        let row = &body["rows"][0];
        assert_eq!(row["quantity"], 6);
        // Current-position contract: row total-return percent == unrealized percent.
        assert_eq!(row["total_return_percent"], row["unrealized_gain_percent"]);
        assert_eq!(row["total_return_base"], row["unrealized_gain_base"]);
    }

    #[tokio::test]
    async fn gains_can_include_closed_positions_with_realized_gain() {
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;

        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10","brokerage":"20"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":10,"price":"120","currency":"USD","fx_rate_to_base":"11","brokerage":"5"}),
        )
        .await;

        let (default_status, default_body) = send(&state, "GET", "/api/gains", json!({})).await;
        assert_eq!(default_status, StatusCode::OK);
        assert_eq!(default_body["rows"].as_array().expect("rows").len(), 0);

        let (status, body) = send(&state, "GET", "/api/gains?include_closed=true", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["include_closed_positions"], true);
        assert_eq!(body["rows"].as_array().expect("rows").len(), 1);
        assert_eq!(
            body["summary"]["market_value_base"]["status"],
            "unavailable"
        );
        // No report-end FX is cached for this USD closed position.
        assert_unavailable(&body["totals"]["capital_gain_base"], &["missing_end_fx"]);
        assert_unavailable(&body["totals"]["currency_gain_base"], &["missing_end_fx"]);
        assert_unavailable(&body["totals"]["total_return_base"], &["missing_end_fx"]);
        assert_unavailable_status(&body["totals"]["total_return_percent"]);
        assert_unavailable(&body["totals"]["income_base"], &["missing_end_fx"]);

        let row = &body["rows"][0];
        assert_eq!(row["instrument"]["symbol"], "MSFT");
        assert_eq!(row["position_status"], "closed");
        assert_eq!(row["quantity"], 0);
        assert_eq!(row["cost_basis_native"], "1000.00");
        assert_available(&row["cost_basis_base"], "10020.00");
        assert_available(&row["market_value_native"], "0.00");
        assert_available(&row["market_value_base"], "0.00");
        assert_available(&row["proceeds_native"], "1200.00");
        assert_available(&row["proceeds_base"], "13195.00");
        assert_available(&row["unrealized_gain_base"], "3175.00");
        assert_available(&row["unrealized_gain_percent"], "31.68");
        assert_available(&row["price_effect_base"], "2175.00");
        assert_available(&row["fx_effect_base"], "1000.00");
        assert_available(&row["capital_gain_base"], "2175.00");
        assert_available_status(&row["capital_gain_percent"]);
        assert_available(&row["currency_gain_base"], "1000.00");
        assert_available_status(&row["currency_gain_percent"]);
        assert_available(&row["total_return_base"], "3175.00");
        assert_available(&row["total_return_percent"], "31.68");
    }

    #[tokio::test]
    async fn gains_totals_include_closed_in_period_position_when_row_hidden() {
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "ERIC B", "STO", BASE_CURRENCY).await;

        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-05",
                   "quantity":100,"price":"10","currency":BASE_CURRENCY}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-20",
                   "quantity":100,"price":"11","currency":BASE_CURRENCY}),
        )
        .await;

        let (status, body) = send(
            &state,
            "GET",
            "/api/gains?start_date=2026-06-01&end_date=2026-06-30",
            json!({}),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["rows"].as_array().expect("rows").len(), 0);
        assert_available(&body["totals"]["capital_gain_base"], "100.00");
        assert_available(&body["totals"]["currency_gain_base"], "0.00");
        assert_available(&body["totals"]["total_return_base"], "100.00");
        assert_available_status(&body["totals"]["total_return_percent"]);
        assert_eq!(body["totals"]["excluded_rows"], 0);
    }

    #[tokio::test]
    async fn gains_include_closed_positions_keeps_partial_sells_in_one_open_row() {
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
        let latest = Local::now().naive_local().date();
        let previous = latest - Duration::days(1);

        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10","brokerage":"20"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":4,"price":"120","currency":"USD","fx_rate_to_base":"11","brokerage":"5"}),
        )
        .await;

        seed_market_data(&state, instrument_id, latest, previous).await;

        let (status, body) = send(&state, "GET", "/api/gains?include_closed=true", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["include_closed_positions"], true);
        assert_eq!(body["rows"].as_array().expect("rows").len(), 1);
        assert_available(&body["totals"]["capital_gain_base"], "2175.00");
        assert_available(&body["totals"]["currency_gain_base"], "1000.00");
        assert_available(&body["totals"]["total_return_base"], "3175.00");
        // Percent values vary based on today's date (days since historical buy); just check available.
        assert_available_status(&body["totals"]["capital_gain_percent"]);
        assert_available_status(&body["totals"]["currency_gain_percent"]);
        assert_available_status(&body["totals"]["total_return_percent"]);

        let rows = body["rows"].as_array().expect("rows");
        let open_row = &rows[0];
        assert_eq!(open_row["position_status"], "open");
        assert_eq!(open_row["quantity"], 6);
        assert_available_status(&open_row["performance_denominator_base"]);
        assert_available(&open_row["unrealized_gain_base"], "1908.00");
        assert_available(&open_row["capital_gain_base"], "1308.00");
        assert_available(&open_row["currency_gain_base"], "600.00");
        assert_available(&open_row["total_return_base"], "1908.00");
    }

    #[tokio::test]
    async fn gains_populated_portfolio_uses_cached_price_and_frankfurter_fx() {
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
        let latest = Local::now().naive_local().date();
        let previous = latest - Duration::days(1);
        let trade_date = (latest - Duration::days(10)).format("%Y-%m-%d").to_string();

        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":trade_date,
                   "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;

        seed_market_data(&state, instrument_id, latest, previous).await;

        let (status, body) =
            send(&state, "GET", "/api/gains?method=modified_dietz", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["base_currency"], BASE_CURRENCY);
        assert_eq!(body["rows"].as_array().expect("rows").len(), 1);

        let row = &body["rows"][0];
        assert_eq!(row["instrument"]["symbol"], "MSFT");
        assert_eq!(row["quantity"], 10);
        assert_eq!(row["performance_start_date"], trade_date);
        assert_available(&row["performance_denominator_base"], "10000.00");
        assert_eq!(row["cost_basis_native"], "1000.00");
        assert_available(&row["cost_basis_base"], "10000.00");
        assert_available(&row["price_effect_base"], "2200.00");
        assert_available(&row["fx_effect_base"], "1000.00");
        assert_eq!(row["latest_price"]["close"], "120.00");
        assert_eq!(row["latest_fx"]["rate"], "11");
        assert_eq!(row["latest_fx"]["quote"], BASE_CURRENCY);
        assert_available(&row["market_value_native"], "1200.00");
        assert_available(&row["market_value_base"], "13200.00");
        assert_unavailable(&row["proceeds_native"], &[]);
        assert_unavailable(&row["proceeds_base"], &[]);
        assert_available(&row["unrealized_gain_base"], "3200.00");
        assert_available(&row["unrealized_gain_percent"], "32.00");
        assert_available(&row["capital_gain_base"], "2200.00");
        assert_available(&row["capital_gain_percent"], "22.00");
        assert_available(&row["currency_gain_base"], "1000.00");
        assert_available(&row["currency_gain_percent"], "10.00");
        assert_available(&row["total_return_base"], "3200.00");
        assert_available(&row["total_return_percent"], "32.00");
        assert_available(&row["day_change_base"], "1650.00");
        assert_available(&row["day_change_percent"], "14.28");

        assert_available(&body["summary"]["market_value_base"], "13200.00");
        assert_available(&body["summary"]["cost_basis_base"], "10000.00");
        assert_available(&body["summary"]["price_effect_base"], "2200.00");
        assert_available(&body["summary"]["fx_effect_base"], "1000.00");
        assert_available(&body["summary"]["unrealized_gain_base"], "3200.00");
        // Modified Dietz inception mode: buy 10d ago, weight=10/10=1, denom=10000
        // capital=2200, currency=1000, total=3200 → same as cost-basis results
        assert_available(&body["totals"]["capital_gain_base"], "2200.00");
        assert_available(&body["totals"]["capital_gain_percent"], "22.00");
        assert_available(&body["totals"]["currency_gain_base"], "1000.00");
        assert_available(&body["totals"]["currency_gain_percent"], "10.00");
        assert_available(&body["totals"]["total_return_base"], "3200.00");
        assert_available(&body["totals"]["total_return_percent"], "32.00");
    }

    #[tokio::test]
    async fn gains_unavailable_attribution_serializes_reason_arrays() {
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
        let trade_date = (Local::now().naive_local().date() - Duration::days(10))
            .format("%Y-%m-%d")
            .to_string();

        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":trade_date,
                   "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;

        let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
        assert_eq!(status, StatusCode::OK);

        let row = &body["rows"][0];
        assert_unavailable(&row["price_effect_base"], &["missing_price", "missing_fx"]);
        assert_unavailable(&row["fx_effect_base"], &["missing_price", "missing_fx"]);
    }

    #[tokio::test]
    async fn gains_totals_remain_available_when_one_instrument_is_incomplete() {
        let state = AppState::for_tests().await;
        let available_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
        let incomplete_id = instrument(&state, "AAPL", "NASDAQ", "USD").await;
        let latest = Local::now().naive_local().date();
        let previous = latest - Duration::days(1);
        let trade_date = (latest - Duration::days(10)).format("%Y-%m-%d").to_string();

        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":available_id,"type":"Buy","trade_date":trade_date,
                   "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":incomplete_id,"type":"Buy","trade_date":trade_date,
                   "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;

        seed_market_data(&state, available_id, latest, previous).await;

        let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["rows"].as_array().expect("rows").len(), 2);
        assert_available(&body["totals"]["capital_gain_base"], "2200.00");
        assert_available(&body["totals"]["currency_gain_base"], "1000.00");
        assert_available(&body["totals"]["total_return_base"], "3200.00");
        assert_available_status(&body["totals"]["total_return_percent"]);
        assert_eq!(body["totals"]["excluded_rows"], 1);
    }

    #[tokio::test]
    async fn gains_all_mode_uses_one_report_start_for_row_denominators() {
        let state = AppState::for_tests().await;
        let early_id = instrument(&state, "EARLY", "STO", BASE_CURRENCY).await;
        let later_id = instrument(&state, "LATER", "STO", BASE_CURRENCY).await;

        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":early_id,"type":"Buy","trade_date":"2026-01-01",
                   "quantity":100,"price":"10","currency":BASE_CURRENCY}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":later_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":100,"price":"10","currency":BASE_CURRENCY}),
        )
        .await;

        seed_sek_prices(&state, early_id, "EARLY").await;
        seed_sek_prices(&state, later_id, "LATER").await;

        let (status, body) = send(&state, "GET", "/api/gains?end_date=2026-06-30", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["report_period"]["start_date"], "2026-01-01");

        let rows = body["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 2);
        let early_row = rows
            .iter()
            .find(|row| row["instrument"]["symbol"] == "EARLY")
            .expect("early row");
        let later_row = rows
            .iter()
            .find(|row| row["instrument"]["symbol"] == "LATER")
            .expect("later row");

        // Both rows share the same report_start_date regardless of when each was first bought.
        assert_eq!(early_row["performance_start_date"], "2026-01-01");
        assert_eq!(later_row["performance_start_date"], "2026-01-01");
        // performance_denominator_base = cost_basis_base; SEK buys with no fx_rate_to_base are unavailable.
        assert_unavailable_status(&early_row["performance_denominator_base"]);
        assert_unavailable_status(&later_row["performance_denominator_base"]);
    }

    #[tokio::test]
    async fn dividend_income_appears_in_gain_row_and_totals() {
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "ERICB", "STO", BASE_CURRENCY).await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
                   "quantity":100,"price":"10.00","currency":BASE_CURRENCY}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Dividend","trade_date":"2026-06-15",
                   "quantity":100,"price":"0.50","currency":BASE_CURRENCY}),
        )
        .await;
        seed_sek_prices(&state, instrument_id, "ERICB").await;

        // income = 100 * 0.50 * 1 = 50 SEK
        let (status, body) = send(&state, "GET", "/api/gains?end_date=2026-06-30", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        let row = &body["rows"][0];
        assert_eq!(row["income_base"]["status"], "available");
        assert_eq!(row["income_base"]["value"], "50.00");
        assert_eq!(body["totals"]["income_base"]["status"], "available");
        assert_eq!(body["totals"]["income_base"]["value"], "50.00");
    }

    async fn seed_june_fixture(state: &AppState) -> i64 {
        let instrument_id = instrument(state, "MSFT", "NASDAQ", "USD").await;
        send(
            state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
                   "quantity":100,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;
        let fetched_at = crate::import::now_iso8601();
        for (date, close) in [
            (
                chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                dec!(10),
            ),
            (
                chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
                dec!(12),
            ),
        ] {
            prices::upsert(
                &state.pool,
                &prices::NewPrice {
                    instrument_id,
                    provider: PRICE_PROVIDER.to_owned(),
                    provider_symbol: "MSFT".to_owned(),
                    date,
                    close,
                    currency: "USD".to_owned(),
                    fetched_at: fetched_at.clone(),
                },
            )
            .await
            .unwrap();
        }
        for date in [
            chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        ] {
            fx_rates::upsert(
                &state.pool,
                &fx_rates::NewFxRate {
                    base: "USD".to_owned(),
                    quote: BASE_CURRENCY.to_owned(),
                    date,
                    rate: dec!(10),
                    provider: FX_PROVIDER.to_owned(),
                    fetched_at: fetched_at.clone(),
                },
            )
            .await
            .unwrap();
        }
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "MSFT".to_owned(),
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: fetched_at.clone(),
                updated_at: fetched_at,
            },
        )
        .await
        .unwrap();
        instrument_id
    }

    #[tokio::test]
    async fn gains_with_date_range_selectable_method() {
        let state = AppState::for_tests().await;
        seed_june_fixture(&state).await;

        // default = xirr; buy on day 0 of 29-day period: cumulative = (1+annualized)^(29/365.25)-1 = 20.00%
        let (_, body) = send(
            &state,
            "GET",
            "/api/gains?start_date=2026-06-01&end_date=2026-06-30",
            json!({}),
        )
        .await;
        assert_eq!(body["percentage_method"], "money_weighted");
        assert_available(&body["totals"]["total_return_percent"], "20.00");

        // simple: 2000 / (0 + 10000) = 20.00%
        let (_, s) = send(
            &state,
            "GET",
            "/api/gains?start_date=2026-06-01&end_date=2026-06-30&method=simple",
            json!({}),
        )
        .await;
        assert_eq!(s["percentage_method"], "simple");
        assert_available(&s["totals"]["total_return_percent"], "20.00");

        // modified_dietz still available (legacy path retained)
        let (_, md) = send(
            &state,
            "GET",
            "/api/gains?start_date=2026-06-01&end_date=2026-06-30&method=modified_dietz",
            json!({}),
        )
        .await;
        assert_eq!(md["percentage_method"], "modified_dietz");
        assert_available_status(&md["totals"]["total_return_percent"]);

        // unknown method -> 400
        let (st, _) = send(&state, "GET", "/api/gains?method=bogus", json!({})).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn gains_xirr_zero_total_return_with_nonzero_components() {
        // Capital gain and currency gain offset each other to zero total return.
        // Components must be unavailable (ZeroOrInvalidPerformanceDenominator), not "0.00".
        // total_return_percent must be "0.00".
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;

        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":100,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;

        // end_mv = 100 * 10 (price) * 10 (FX) = 10000; begin_mv = 0; net_flows = 10000
        // total_return = 0; capital_gain = (10-10)*100*10 = 0 + fx contribution = (10-10)*100*10 - 10000 + 10000 = 0
        // Actually set price drop to $9 and FX rise such that total_return = 0:
        // end_mv = 100 * 9 * (10000/9/100) = 10000 — but that is hard to engineer exactly.
        // Easier: SEK instrument, buy at 10, end at 10 → total=0, capital=0, currency=0 → all zero → returns "0.00".
        // For the interesting case (nonzero components, zero total), use FX gain offsetting price loss:
        // 100 shares, buy price $10, FX 10 → begin_mv=0, net_flows=10000
        // end price $8, end FX 12.5 → end_mv = 100*8*12.5 = 10000 → total_return = 0
        // capital = (100*8 - 100*10)*12.5 - 0 = -200*12.5 = -2500
        // currency = total - capital = 0 - (-2500) = 2500

        let fetched_at = crate::import::now_iso8601();
        for (date, close) in [
            (
                chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                dec!(10),
            ),
            (
                chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
                dec!(8),
            ),
        ] {
            prices::upsert(
                &state.pool,
                &prices::NewPrice {
                    instrument_id,
                    provider: PRICE_PROVIDER.to_owned(),
                    provider_symbol: "MSFT".to_owned(),
                    date,
                    close,
                    currency: "USD".to_owned(),
                    fetched_at: fetched_at.clone(),
                },
            )
            .await
            .unwrap();
        }
        for (date, rate) in [
            (
                chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                dec!(10),
            ),
            (
                chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
                dec!(12.5),
            ),
        ] {
            fx_rates::upsert(
                &state.pool,
                &fx_rates::NewFxRate {
                    base: "USD".to_owned(),
                    quote: BASE_CURRENCY.to_owned(),
                    date,
                    rate,
                    provider: FX_PROVIDER.to_owned(),
                    fetched_at: fetched_at.clone(),
                },
            )
            .await
            .unwrap();
        }
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "MSFT".to_owned(),
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: fetched_at.clone(),
                updated_at: fetched_at,
            },
        )
        .await
        .unwrap();

        let (status, body) = send(
            &state,
            "GET",
            "/api/gains?start_date=2026-06-01&end_date=2026-06-30",
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["percentage_method"], "money_weighted");
        assert_available(&body["totals"]["total_return_base"], "0.00");
        assert_available(&body["totals"]["total_return_percent"], "0.00");
        assert_unavailable_status(&body["totals"]["capital_gain_percent"]);
        assert_unavailable_status(&body["totals"]["currency_gain_percent"]);
    }

    #[tokio::test]
    async fn gains_split_neutrality_regression() {
        // Regression: Simple and XIRR use actual_period_cash_flows (not split-adjusted).
        // reconstruct_period now receives the full ledger, so post_period_split_factor correctly
        // reflects post-end_date splits. actual_period_cash_flows is unaffected by the split
        // factor (by design), while period_cash_flows (used by Modified Dietz) is adjusted.
        // The key invariant is verified at the unit level in
        // performance::tests::actual_period_cash_flows_unaffected_by_post_period_split.
        //
        // This test confirms that when a split is recorded after end_date, both Simple and
        // Modified Dietz give the same result as when no split is recorded — because
        // actual_period_cash_flows intentionally ignores the post-period split factor.
        //
        // Setup: buy 100 shares at $10, FX=10 on Jun 1 (period start); 2:1 split on Aug 1.
        // Querying Jun 1 - Jun 30 (end_date before split): split excluded from ledger.
        // post_period_split_factor = 1; actual_cash_flows = period_cash_flows = [10000].
        // total_return = 12000 - 0 - 10000 = 2000; denom = 10000; percent = 20.00%.
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "TSLA", "NASDAQ", "USD").await;

        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":100,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Split","trade_date":"2026-08-01",
                   "quantity":100,"currency":"USD"}),
        )
        .await;

        let fetched_at = crate::import::now_iso8601();
        for (date, close) in [
            (
                chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                dec!(10),
            ),
            (
                chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
                dec!(12),
            ),
        ] {
            prices::upsert(
                &state.pool,
                &prices::NewPrice {
                    instrument_id,
                    provider: PRICE_PROVIDER.to_owned(),
                    provider_symbol: "TSLA".to_owned(),
                    date,
                    close,
                    currency: "USD".to_owned(),
                    fetched_at: fetched_at.clone(),
                },
            )
            .await
            .unwrap();
        }
        for date in [
            chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        ] {
            fx_rates::upsert(
                &state.pool,
                &fx_rates::NewFxRate {
                    base: "USD".to_owned(),
                    quote: BASE_CURRENCY.to_owned(),
                    date,
                    rate: dec!(10),
                    provider: FX_PROVIDER.to_owned(),
                    fetched_at: fetched_at.clone(),
                },
            )
            .await
            .unwrap();
        }
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "TSLA".to_owned(),
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: fetched_at.clone(),
                updated_at: fetched_at,
            },
        )
        .await
        .unwrap();

        let (_, simple_body) = send(
            &state,
            "GET",
            "/api/gains?start_date=2026-06-01&end_date=2026-06-30&method=simple",
            json!({}),
        )
        .await;
        let (_, md_body) = send(
            &state,
            "GET",
            "/api/gains?start_date=2026-06-01&end_date=2026-06-30&method=modified_dietz",
            json!({}),
        )
        .await;

        // actual_period_cash_flows does not apply the post-period split factor, so for Simple and
        // XIRR the result is unaffected by the Aug 1 split. Modified Dietz uses period_cash_flows
        // which does apply the factor, but reconstruct_period is called with the full ledger so the
        // factor is set correctly (2 here). However, the denominator scaling and the end_mv scaling
        // cancel out, giving the same total_return_percent = 2000/10000 = 20.00%.
        // The behavioural difference is verified at the unit level in
        // performance::tests::actual_period_cash_flows_unaffected_by_post_period_split.
        assert_available(&simple_body["totals"]["total_return_percent"], "20.00");
        assert_available(&md_body["totals"]["total_return_percent"], "20.00");
    }

    async fn instrument(state: &AppState, symbol: &str, exchange: &str, currency: &str) -> i64 {
        let (status, body) = send(
            state,
            "POST",
            "/api/instruments",
            json!({"symbol":symbol,"exchange":exchange,"name":symbol,"type":"Stock","currency":currency}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        body["id"].as_i64().expect("instrument id")
    }

    async fn seed_market_data(
        state: &AppState,
        instrument_id: i64,
        latest: chrono::NaiveDate,
        previous: chrono::NaiveDate,
    ) {
        let fetched_at = now_iso8601();
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "MSFT".to_owned(),
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: fetched_at.clone(),
                updated_at: fetched_at.clone(),
            },
        )
        .await
        .expect("provider symbol inserted");

        for (date, close) in [(previous, dec!(110)), (latest, dec!(120))] {
            prices::upsert(
                &state.pool,
                &prices::NewPrice {
                    instrument_id,
                    provider: PRICE_PROVIDER.to_owned(),
                    provider_symbol: "MSFT".to_owned(),
                    date,
                    close,
                    currency: "USD".to_owned(),
                    fetched_at: fetched_at.clone(),
                },
            )
            .await
            .expect("price inserted");
        }

        for (date, rate) in [(previous, dec!(10.5)), (latest, dec!(11))] {
            fx_rates::upsert(
                &state.pool,
                &fx_rates::NewFxRate {
                    base: "USD".to_owned(),
                    quote: BASE_CURRENCY.to_owned(),
                    date,
                    rate,
                    provider: FX_PROVIDER.to_owned(),
                    fetched_at: fetched_at.clone(),
                },
            )
            .await
            .expect("fx rate inserted");
        }
    }

    async fn seed_sek_prices(state: &AppState, instrument_id: i64, symbol: &str) {
        let fetched_at = now_iso8601();
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: symbol.to_owned(),
                currency: Some(BASE_CURRENCY.to_owned()),
                enabled: true,
                created_at: fetched_at.clone(),
                updated_at: fetched_at.clone(),
            },
        )
        .await
        .expect("provider symbol inserted");

        for date in [
            chrono::NaiveDate::from_ymd_opt(2026, 6, 29).unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        ] {
            prices::upsert(
                &state.pool,
                &prices::NewPrice {
                    instrument_id,
                    provider: PRICE_PROVIDER.to_owned(),
                    provider_symbol: symbol.to_owned(),
                    date,
                    close: dec!(12),
                    currency: BASE_CURRENCY.to_owned(),
                    fetched_at: fetched_at.clone(),
                },
            )
            .await
            .expect("price inserted");
        }
    }

    fn assert_available(value: &serde_json::Value, expected: &str) {
        assert_eq!(value["status"], "available");
        assert_eq!(value["value"], expected);
    }

    fn assert_unavailable(value: &serde_json::Value, expected: &[&str]) {
        assert_eq!(value["status"], "unavailable");
        let reasons = value["reasons"]
            .as_array()
            .expect("unavailable reasons array")
            .iter()
            .map(|reason| reason.as_str().expect("reason string"))
            .collect::<Vec<_>>();
        assert_eq!(reasons, expected);
    }

    fn assert_unavailable_status(value: &serde_json::Value) {
        assert_eq!(value["status"], "unavailable");
    }

    fn assert_available_status(value: &serde_json::Value) {
        assert_eq!(value["status"], "available");
    }

    #[tokio::test]
    async fn gains_rejects_malformed_start_date() {
        let state = AppState::for_tests().await;
        let (status, body) =
            send(&state, "GET", "/api/gains?start_date=not-a-date", json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_date");
    }

    #[tokio::test]
    async fn gains_rejects_start_after_end() {
        let state = AppState::for_tests().await;
        let (status, body) = send(
            &state,
            "GET",
            "/api/gains?start_date=2026-06-30&end_date=2026-06-01",
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "start_date_after_end_date");
    }

    #[tokio::test]
    async fn gains_with_end_date_uses_that_date_as_valuation_date() {
        let state = AppState::for_tests().await;
        let (status, body) = send(&state, "GET", "/api/gains?end_date=2026-01-15", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["as_of_date"], "2026-01-15");
        assert_eq!(body["report_period"]["end_date"], "2026-01-15");
    }

    #[tokio::test]
    async fn gains_with_no_dates_returns_inception_period() {
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "ERIC B", "STO", BASE_CURRENCY).await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-15",
                   "quantity":10,"price":"100","currency":BASE_CURRENCY}),
        )
        .await;

        let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["report_period"]["start_date"], "2026-01-15");
    }

    #[tokio::test]
    async fn gains_post_end_date_transaction_excluded() {
        let state = AppState::for_tests().await;
        let instrument_id = instrument(&state, "TSLA", "NASDAQ", "USD").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
                   "quantity":100,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-09-01",
                   "quantity":100,"price":"15","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;
        let (status, body) = send(&state, "GET", "/api/gains?end_date=2026-06-30", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        // Row for TSLA should show quantity 100, not 200
        let row = body["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["instrument"]["symbol"] == "TSLA")
            .unwrap();
        assert_eq!(row["quantity"], 100);
    }

    #[tokio::test]
    async fn gains_open_row_exposes_realized_gain_base() {
        let state = AppState::for_tests().await;
        let sold_id = instrument(&state, "SELLER", "STO", BASE_CURRENCY).await;
        let never_id = instrument(&state, "HOLDER", "STO", BASE_CURRENCY).await;

        // SELLER: buy 10 @100, sell 4 @150 (SEK, no fees) -> realized (150-100)*4 = 200, 6 open.
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":sold_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":sold_id,"type":"Sell","trade_date":"2026-06-05",
                   "quantity":4,"price":"150","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
        )
        .await;
        // HOLDER: buy only, never sold -> realized 0.
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":never_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":5,"price":"100","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
        )
        .await;

        let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
        assert_eq!(status, StatusCode::OK);

        let rows = body["rows"].as_array().expect("rows");
        let sold = rows
            .iter()
            .find(|r| r["instrument"]["symbol"] == "SELLER")
            .expect("seller row");
        let never = rows
            .iter()
            .find(|r| r["instrument"]["symbol"] == "HOLDER")
            .expect("holder row");

        assert_eq!(sold["position_status"], "open");
        assert_eq!(sold["quantity"], 6);
        assert_available(&sold["realized_gain_base"], "200.00");
        // Sold 4 @ cost 100 -> sold cost basis 400.00.
        assert_available(&sold["realized_cost_basis_base"], "400.00");
        assert_eq!(never["position_status"], "open");
        assert_available(&never["realized_gain_base"], "0.00");
        assert_available(&never["realized_cost_basis_base"], "0.00");
    }
}
