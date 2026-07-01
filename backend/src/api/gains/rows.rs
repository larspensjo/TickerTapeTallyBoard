use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::api::error::ApiError;
use crate::api::instruments::InstrumentResponse;
use crate::api::valuation::{
    fx_snapshot_response, money_string, price_snapshot_response, serialize_availability,
    serialize_valuation_reason, AvailabilityResponse,
};
use crate::db::instruments;
use crate::domain::{Availability, BaseAmount, RealizedGain, ValuationReason, ValuedHolding};

use super::{GainPositionStatus, GainRow};

pub(super) fn serialize_reasons(reasons: &[ValuationReason]) -> Vec<String> {
    reasons.iter().map(serialize_valuation_reason).collect()
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

pub(super) fn open_gain_row(
    instrument: &instruments::InstrumentRow,
    valued_holding: &ValuedHolding,
    realized: &RealizedGain,
    performance_start_date: Option<NaiveDate>,
    income_base: &Availability<Decimal>,
) -> Result<GainRow, ApiError> {
    let add = |a: &Availability<Decimal>, b: &Availability<Decimal>| {
        combine_availability(a, b, |x, y| x + y)
    };
    let realized_gain = base_amount_availability(&realized.gain_base);
    let realized_cost = base_amount_availability(&realized.cost_basis_base);
    let realized_price = base_amount_availability(&realized.price_effect_base);
    let realized_fx = base_amount_availability(&realized.fx_effect_base);
    let total_gain_ex_income = add(&valued_holding.unrealized_gain_base, &realized_gain);
    let total_gain = add(&total_gain_ex_income, income_base);
    let total_cost = add(&valued_holding.cost_basis_base, &realized_cost);
    let total_price_effect = add(&valued_holding.price_effect_base, &realized_price);
    let total_fx_effect = add(&valued_holding.fx_effect_base, &realized_fx);
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
        total_return_base: serialize_availability(&total_gain, |v| money_string(*v)),
        total_return_percent: current_position_percent(&total_gain, &total_cost),
        capital_gain_base: serialize_availability(&total_price_effect, |v| money_string(*v)),
        capital_gain_percent: current_position_percent(&total_price_effect, &total_cost),
        currency_gain_base: serialize_availability(&total_fx_effect, |v| money_string(*v)),
        currency_gain_percent: current_position_percent(&total_fx_effect, &total_cost),
        income_base: serialize_availability(income_base, |v| money_string(*v)),
        price_effect_base: serialize_availability(&total_price_effect, |v| money_string(*v)),
        fx_effect_base: serialize_availability(&total_fx_effect, |v| money_string(*v)),
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
        unrealized_price_effect_base: serialize_availability(
            &valued_holding.price_effect_base,
            |v| money_string(*v),
        ),
        unrealized_fx_effect_base: serialize_availability(&valued_holding.fx_effect_base, |v| {
            money_string(*v)
        }),
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

pub(super) fn closed_gain_row(
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
        total_return_base: {
            let tr = match (&gain_base, income_base) {
                (Availability::Available(g), Availability::Available(i)) => {
                    Availability::Available(g + i)
                }
                _ => gain_base.clone(),
            };
            serialize_availability(&tr, |v| money_string(*v))
        },
        total_return_percent: {
            let tr = match (&gain_base, income_base) {
                (Availability::Available(g), Availability::Available(i)) => {
                    Availability::Available(g + i)
                }
                _ => gain_base.clone(),
            };
            current_position_percent(&tr, &cost_basis_base)
        },
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
        unrealized_price_effect_base: serialize_base_amount(&realized.price_effect_base),
        unrealized_fx_effect_base: serialize_base_amount(&realized.fx_effect_base),
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

fn combine_availability<T, U, V, F>(
    a: &Availability<T>,
    b: &Availability<U>,
    f: F,
) -> Availability<V>
where
    F: Fn(&T, &U) -> V,
{
    match (a, b) {
        (Availability::Available(x), Availability::Available(y)) => {
            Availability::Available(f(x, y))
        }
        _ => {
            let mut reasons = a.reasons();
            reasons.extend(b.reasons());
            Availability::Unavailable { reasons }
        }
    }
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

pub(super) fn dedup_valuation_reasons(reasons: &mut Vec<ValuationReason>) {
    let mut deduped = Vec::new();
    for reason in reasons.drain(..) {
        if !deduped.contains(&reason) {
            deduped.push(reason);
        }
    }
    *reasons = deduped;
}
