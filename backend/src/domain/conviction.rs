//! Pure conviction-target derivation.
//!
//! Conviction is user-managed portfolio metadata (see the Conviction Targets
//! design note). This module turns the current open-position pool into per-asset
//! target values, signed gaps, and display statuses. It contains no I/O, no
//! serialization, and no HTTP types: callers feed it the already-derived market
//! values and format the resulting `Decimal`s at the edge.
//!
//! Targets are relative to the *whole* eligible pool, so the entire input set
//! must be passed in one call; a single edit can move every eligible target.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Display tolerance band, in percent, around a target value. Within `±5%` a
/// holding reads as on target. The band is display-only in V1.
const TOLERANCE_PERCENT: Decimal = dec!(5);

/// Conviction level for one instrument. `Other` has no target and never enters
/// the target pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvictionLevel {
    Other,
    Low,
    Medium,
    High,
}

impl ConvictionLevel {
    /// The canonical DB/storage string.
    pub fn db_str(self) -> &'static str {
        match self {
            Self::Other => "OTHER",
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        }
    }

    /// Parse the canonical DB/storage string.
    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "OTHER" => Some(Self::Other),
            "LOW" => Some(Self::Low),
            "MEDIUM" => Some(Self::Medium),
            "HIGH" => Some(Self::High),
            _ => None,
        }
    }

    /// Relative target weight. `Other` has no weight and is excluded from the
    /// target denominator; Low/Medium/High are 1/2/4.
    pub fn weight(self) -> Option<Decimal> {
        match self {
            Self::Other => None,
            Self::Low => Some(dec!(1)),
            Self::Medium => Some(dec!(2)),
            Self::High => Some(dec!(4)),
        }
    }
}

/// The current market-value state of a holding, as seen by target derivation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketValueState {
    /// Valuation is available; carries the current market value in base currency.
    Available(Decimal),
    /// Valuation is present but unavailable (e.g. missing price or FX).
    Unavailable,
    /// No valuation at all because price mapping is disabled for the instrument.
    MappingDisabled,
}

/// One holding's input to target derivation.
///
/// Eligibility is fully determined by `conviction` and `market_value`: a holding
/// enters the pool only with a Low/Medium/High conviction and an available,
/// strictly positive market value. Open quantity is therefore not needed here;
/// a non-positive value (including a zero-priced or short position) is excluded
/// via `current_value_not_positive`.
#[derive(Clone, Debug)]
pub struct ConvictionTargetInput {
    pub instrument_id: i64,
    pub conviction: ConvictionLevel,
    pub market_value: MarketValueState,
}

/// Shared pool-membership predicate for target and rebalance planning.
///
/// Returns the conviction weight and market value when the holding belongs in
/// the eligible pool; otherwise `None`.
pub fn pool_membership(
    conviction: ConvictionLevel,
    market_value: MarketValueState,
) -> Option<(Decimal, Decimal)> {
    let weight = conviction.weight()?;

    match market_value {
        MarketValueState::Available(value) if value > Decimal::ZERO => Some((weight, value)),
        _ => None,
    }
}

/// Classify a signed display-gap percent (`value − target`, as percent of
/// target) against the shared ±5% tolerance band.
pub fn gap_band_status(gap_percent: Decimal) -> TargetStatus {
    if gap_percent < -TOLERANCE_PERCENT {
        TargetStatus::Below
    } else if gap_percent > TOLERANCE_PERCENT {
        TargetStatus::Above
    } else {
        TargetStatus::OnTarget
    }
}

/// Overall display status for a holding's target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetStatus {
    Below,
    OnTarget,
    Above,
    NoTarget,
    ExcludedUnavailable,
    Unavailable,
}

impl TargetStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Below => "below",
            Self::OnTarget => "on_target",
            Self::Above => "above",
            Self::NoTarget => "no_target",
            Self::ExcludedUnavailable => "excluded_unavailable",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Why a target field is unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetReason {
    /// Conviction is `Other`, so the holding has no target by design.
    NoTarget,
    /// The eligible target pool has no members.
    TargetPoolEmpty,
    /// The eligible target pool sums to zero value.
    TargetPoolZero,
    /// Convicted, but valuation is present-but-unavailable.
    ValuationUnavailable,
    /// Convicted, but price mapping is disabled so there is no valuation.
    PriceMappingDisabled,
    /// Convicted, valuation available, but the current value is not positive.
    CurrentValueNotPositive,
}

impl TargetReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoTarget => "no_target",
            Self::TargetPoolEmpty => "target_pool_empty",
            Self::TargetPoolZero => "target_pool_zero",
            Self::ValuationUnavailable => "valuation_unavailable",
            Self::PriceMappingDisabled => "price_mapping_disabled",
            Self::CurrentValueNotPositive => "current_value_not_positive",
        }
    }
}

/// An availability wrapper for a derived target `Decimal`.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetField {
    Available(Decimal),
    Unavailable(Vec<TargetReason>),
}

/// The derived target for one holding.
#[derive(Clone, Debug)]
pub struct ConvictionTargetOutput {
    pub instrument_id: i64,
    pub conviction: ConvictionLevel,
    pub status: TargetStatus,
    /// Target value in base currency.
    pub target_value: TargetField,
    /// Signed gap = current value − target value. Positive is above target
    /// (overweight); negative is below target (underweight).
    pub target_gap: TargetField,
    /// Signed gap as a percent of the target value.
    pub target_gap_percent: TargetField,
}

impl ConvictionTargetOutput {
    fn excluded(input: &ConvictionTargetInput, reason: TargetReason, status: TargetStatus) -> Self {
        Self {
            instrument_id: input.instrument_id,
            conviction: input.conviction,
            status,
            target_value: TargetField::Unavailable(vec![reason]),
            target_gap: TargetField::Unavailable(vec![reason]),
            target_gap_percent: TargetField::Unavailable(vec![reason]),
        }
    }
}

/// Derive targets for the whole pool at once.
///
/// The eligible pool is the set of Low/Medium/High holdings with an available,
/// strictly positive current value. Each eligible holding's target is
/// `pool_value * weight / total_weight`; its gap is `current − target`.
pub fn derive_targets(inputs: &[ConvictionTargetInput]) -> Vec<ConvictionTargetOutput> {
    let mut pool_value = Decimal::ZERO;
    let mut total_weight = Decimal::ZERO;
    for input in inputs {
        if let Some((weight, value)) = pool_membership(input.conviction, input.market_value) {
            pool_value += value;
            total_weight += weight;
        }
    }

    inputs
        .iter()
        .map(|input| derive_one(input, pool_value, total_weight))
        .collect()
}

fn derive_one(
    input: &ConvictionTargetInput,
    pool_value: Decimal,
    total_weight: Decimal,
) -> ConvictionTargetOutput {
    let Some(weight) = input.conviction.weight() else {
        return ConvictionTargetOutput::excluded(
            input,
            TargetReason::NoTarget,
            TargetStatus::NoTarget,
        );
    };

    let current_value = match input.market_value {
        MarketValueState::MappingDisabled => {
            return ConvictionTargetOutput::excluded(
                input,
                TargetReason::PriceMappingDisabled,
                TargetStatus::ExcludedUnavailable,
            );
        }
        MarketValueState::Unavailable => {
            return ConvictionTargetOutput::excluded(
                input,
                TargetReason::ValuationUnavailable,
                TargetStatus::ExcludedUnavailable,
            );
        }
        MarketValueState::Available(value) if value <= Decimal::ZERO => {
            return ConvictionTargetOutput::excluded(
                input,
                TargetReason::CurrentValueNotPositive,
                TargetStatus::ExcludedUnavailable,
            );
        }
        MarketValueState::Available(value) => value,
    };

    // An eligible holding is always in the pool, so these guard against a
    // degenerate pool defensively; they are not reachable for this holding.
    if total_weight <= Decimal::ZERO {
        return ConvictionTargetOutput::excluded(
            input,
            TargetReason::TargetPoolEmpty,
            TargetStatus::Unavailable,
        );
    }
    if pool_value <= Decimal::ZERO {
        return ConvictionTargetOutput::excluded(
            input,
            TargetReason::TargetPoolZero,
            TargetStatus::Unavailable,
        );
    }

    let target_value = pool_value * weight / total_weight;
    let target_gap = current_value - target_value;
    let target_gap_percent = target_gap / target_value * dec!(100);

    let status = gap_band_status(target_gap_percent);

    ConvictionTargetOutput {
        instrument_id: input.instrument_id,
        conviction: input.conviction,
        status,
        target_value: TargetField::Available(target_value),
        target_gap: TargetField::Available(target_gap),
        target_gap_percent: TargetField::Available(target_gap_percent),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        derive_targets, ConvictionLevel, ConvictionTargetInput, MarketValueState, TargetField,
        TargetReason, TargetStatus,
    };
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn input(
        id: i64,
        conviction: ConvictionLevel,
        value: MarketValueState,
    ) -> ConvictionTargetInput {
        ConvictionTargetInput {
            instrument_id: id,
            conviction,
            market_value: value,
        }
    }

    fn available(value: Decimal) -> MarketValueState {
        MarketValueState::Available(value)
    }

    fn target_value(field: &TargetField) -> Decimal {
        match field {
            TargetField::Available(value) => *value,
            TargetField::Unavailable(reasons) => {
                panic!("expected available target, got {reasons:?}")
            }
        }
    }

    #[test]
    fn design_example_targets_match_note() {
        let outputs = derive_targets(&[
            input(1, ConvictionLevel::Low, available(dec!(100000))),
            input(2, ConvictionLevel::Medium, available(dec!(300000))),
            input(3, ConvictionLevel::High, available(dec!(300000))),
            input(4, ConvictionLevel::Other, available(dec!(500000))),
        ]);

        assert_eq!(target_value(&outputs[0].target_value), dec!(100000));
        assert_eq!(target_value(&outputs[1].target_value), dec!(200000));
        assert_eq!(target_value(&outputs[2].target_value), dec!(400000));

        assert_eq!(outputs[0].status, TargetStatus::OnTarget);
        assert_eq!(outputs[1].status, TargetStatus::Above);
        assert_eq!(outputs[2].status, TargetStatus::Below);
        assert_eq!(outputs[3].status, TargetStatus::NoTarget);
        assert_eq!(
            outputs[3].target_value,
            TargetField::Unavailable(vec![TargetReason::NoTarget])
        );
    }

    #[test]
    fn eligible_gaps_sum_to_zero() {
        let outputs = derive_targets(&[
            input(1, ConvictionLevel::Low, available(dec!(100000))),
            input(2, ConvictionLevel::Medium, available(dec!(300000))),
            input(3, ConvictionLevel::High, available(dec!(300000))),
            input(4, ConvictionLevel::Other, available(dec!(500000))),
        ]);

        let sum: Decimal = outputs
            .iter()
            .filter_map(|output| match &output.target_gap {
                TargetField::Available(value) => Some(*value),
                TargetField::Unavailable(_) => None,
            })
            .sum();
        assert_eq!(sum, Decimal::ZERO);
    }

    #[test]
    fn tolerance_boundary_is_inclusive_on_target() {
        // Two equal-weight Low holdings; each targets the pool average of 100.
        let outputs = derive_targets(&[
            input(1, ConvictionLevel::Low, available(dec!(105))),
            input(2, ConvictionLevel::Low, available(dec!(95))),
        ]);
        // +5% and -5% both land exactly on the band edge → on target.
        assert_eq!(outputs[0].status, TargetStatus::OnTarget);
        assert_eq!(outputs[1].status, TargetStatus::OnTarget);
    }

    #[test]
    fn just_outside_tolerance_is_above_or_below() {
        let outputs = derive_targets(&[
            input(1, ConvictionLevel::Low, available(dec!(106))),
            input(2, ConvictionLevel::Low, available(dec!(94))),
        ]);
        // target 100 each; +6% → above, -6% → below.
        assert_eq!(outputs[0].status, TargetStatus::Above);
        assert_eq!(outputs[1].status, TargetStatus::Below);
        assert_eq!(target_value(&outputs[0].target_gap), dec!(6));
        assert_eq!(target_value(&outputs[1].target_gap), dec!(-6));
    }

    #[test]
    fn signed_gap_and_percent_semantics() {
        let outputs = derive_targets(&[
            input(1, ConvictionLevel::Low, available(dec!(106))),
            input(2, ConvictionLevel::Low, available(dec!(94))),
        ]);
        // Positive gap → above target; percent has matching sign.
        assert_eq!(target_value(&outputs[0].target_gap_percent), dec!(6));
        assert_eq!(target_value(&outputs[1].target_gap_percent), dec!(-6));
    }

    #[test]
    fn unavailable_valuation_excludes_but_keeps_conviction() {
        let outputs = derive_targets(&[
            input(1, ConvictionLevel::High, MarketValueState::Unavailable),
            input(2, ConvictionLevel::Low, available(dec!(100))),
        ]);
        assert_eq!(outputs[0].status, TargetStatus::ExcludedUnavailable);
        assert_eq!(outputs[0].conviction, ConvictionLevel::High);
        assert_eq!(
            outputs[0].target_value,
            TargetField::Unavailable(vec![TargetReason::ValuationUnavailable])
        );
        // The other holding is still the sole eligible member of the pool.
        assert_eq!(outputs[1].status, TargetStatus::OnTarget);
    }

    #[test]
    fn mapping_disabled_valuation_is_excluded() {
        let outputs = derive_targets(&[input(
            1,
            ConvictionLevel::Medium,
            MarketValueState::MappingDisabled,
        )]);
        assert_eq!(outputs[0].status, TargetStatus::ExcludedUnavailable);
        assert_eq!(
            outputs[0].target_value,
            TargetField::Unavailable(vec![TargetReason::PriceMappingDisabled])
        );
    }

    #[test]
    fn zero_or_negative_value_is_excluded() {
        let outputs = derive_targets(&[
            input(1, ConvictionLevel::Low, available(dec!(0))),
            input(2, ConvictionLevel::Low, available(dec!(-500))),
        ]);
        for output in &outputs {
            assert_eq!(output.status, TargetStatus::ExcludedUnavailable);
            assert_eq!(
                output.target_value,
                TargetField::Unavailable(vec![TargetReason::CurrentValueNotPositive])
            );
        }
    }

    #[test]
    fn empty_pool_yields_no_targets_without_treating_missing_as_zero() {
        // Only Other plus a convicted-but-unavailable holding: no eligible pool.
        let outputs = derive_targets(&[
            input(1, ConvictionLevel::Other, available(dec!(500000))),
            input(2, ConvictionLevel::High, MarketValueState::Unavailable),
        ]);
        assert_eq!(outputs[0].status, TargetStatus::NoTarget);
        assert_eq!(outputs[1].status, TargetStatus::ExcludedUnavailable);
    }

    #[test]
    fn single_eligible_asset_targets_full_pool_and_is_on_target() {
        let outputs = derive_targets(&[
            input(1, ConvictionLevel::High, available(dec!(250000))),
            input(2, ConvictionLevel::Other, available(dec!(999999))),
        ]);
        assert_eq!(target_value(&outputs[0].target_value), dec!(250000));
        assert_eq!(target_value(&outputs[0].target_gap), Decimal::ZERO);
        assert_eq!(outputs[0].status, TargetStatus::OnTarget);
    }

    #[test]
    fn db_string_round_trips() {
        for level in [
            ConvictionLevel::Other,
            ConvictionLevel::Low,
            ConvictionLevel::Medium,
            ConvictionLevel::High,
        ] {
            assert_eq!(ConvictionLevel::from_db_str(level.db_str()), Some(level));
        }
        assert_eq!(ConvictionLevel::from_db_str("BOGUS"), None);
    }

    #[test]
    fn gap_band_status_matches_derive_targets_band() {
        use super::gap_band_status;

        assert_eq!(gap_band_status(dec!(-5.01)), TargetStatus::Below);
        assert_eq!(gap_band_status(dec!(-5)), TargetStatus::OnTarget);
        assert_eq!(gap_band_status(dec!(0)), TargetStatus::OnTarget);
        assert_eq!(gap_band_status(dec!(5)), TargetStatus::OnTarget);
        assert_eq!(gap_band_status(dec!(5.01)), TargetStatus::Above);
    }
}
