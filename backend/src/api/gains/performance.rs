use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::api::error::ApiError;
use crate::api::valuation::AvailabilityResponse;
use crate::domain::{
    apply_annualisation, compute_modified_dietz_denominator, compute_money_weighted_return,
    Availability, CashFlow, DisplayPercentKind, PeriodAmounts, ValuationReason,
};

use super::rows::{dedup_valuation_reasons, serialize_reasons};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReturnMethod {
    Xirr,
    Simple,
    ModifiedDietz,
}

pub(super) fn parse_method(value: Option<&str>) -> Result<ReturnMethod, ApiError> {
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

#[derive(Default)]
pub(super) struct PerformanceAccumulator {
    begin_mv: Decimal,
    end_mv: Decimal,
    pub(super) total_return: Decimal,
    pub(super) capital_gain: Decimal,
    pub(super) currency_gain: Decimal,
    pub(super) income_gain: Decimal,
    cash_flows: Vec<CashFlow>,
    actual_cash_flows: Vec<CashFlow>,
    pub(super) unavailable_reasons: Vec<ValuationReason>,
    pub(super) excluded_rows: usize,
    pub(super) has_data: bool,
}

impl PerformanceAccumulator {
    pub(super) fn add(
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

    pub(super) fn into_percents(
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
