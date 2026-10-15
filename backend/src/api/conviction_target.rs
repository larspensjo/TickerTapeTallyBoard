use rust_decimal::Decimal;
use serde::Serialize;

use crate::api::instruments::ConvictionDto;
use crate::api::valuation::{money_string, serialize_valuation_reason, AvailabilityResponse};
use crate::domain::{ConvictionTargetOutput, TargetField, ValuationReason};

/// Conviction target derived for the full eligible pool. The `status` and the
/// availability of each field come from the pure `domain::conviction` module;
/// only formatting happens here. Current value is intentionally not duplicated —
/// consumers read the same holding's `valuation.market_value_base`.
#[derive(Debug, Serialize)]
pub struct ConvictionTargetResponse {
    pub conviction: ConvictionDto,
    pub status: &'static str,
    pub target_value_base: AvailabilityResponse,
    pub target_gap_base: AvailabilityResponse,
    pub target_gap_percent: AvailabilityResponse,
}

fn target_field_response(
    field: &TargetField,
    valuation_reasons: &[String],
    format: impl Fn(Decimal) -> String,
) -> AvailabilityResponse {
    match field {
        TargetField::Available(value) => AvailabilityResponse::Available {
            value: format(*value),
        },
        TargetField::Unavailable(reasons) => {
            // Retain the target-specific reason (e.g. `valuation_unavailable`)
            // and append the underlying market-value reasons so the target
            // tooltip is as actionable as the valuation field beside it.
            let mut codes: Vec<String> = reasons
                .iter()
                .map(|reason| reason.as_str().to_owned())
                .collect();
            codes.extend(valuation_reasons.iter().cloned());
            AvailabilityResponse::Unavailable { reasons: codes }
        }
    }
}

pub(crate) fn build_conviction_target(
    output: &ConvictionTargetOutput,
    market_value_reasons: &[ValuationReason],
) -> ConvictionTargetResponse {
    // Only present when the market value is present-but-unavailable, which the
    // domain maps to `TargetReason::ValuationUnavailable`; empty otherwise.
    let valuation_reasons: Vec<String> = market_value_reasons
        .iter()
        .map(serialize_valuation_reason)
        .collect();
    ConvictionTargetResponse {
        conviction: ConvictionDto::from_level(output.conviction),
        status: output.status.as_str(),
        target_value_base: target_field_response(
            &output.target_value,
            &valuation_reasons,
            money_string,
        ),
        target_gap_base: target_field_response(
            &output.target_gap,
            &valuation_reasons,
            money_string,
        ),
        target_gap_percent: target_field_response(
            &output.target_gap_percent,
            &valuation_reasons,
            |value| format!("{value:.2}"),
        ),
    }
}
