//! EPS cross-check policy shared by trailing valuations and quarter comparisons.
use super::Quarter;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

pub const EPS_ROUNDING_ALLOWANCE: Decimal = dec!(0.02);
pub const EPS_APPROXIMATE_PERCENT: Decimal = dec!(5);
pub const EPS_UNKNOWN_PERCENT: Decimal = dec!(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpsConsistencyState {
    Unavailable,
    NotCrossChecked,
    Consistent,
    Approximate,
    Inconsistent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpsConsistency {
    pub state: EpsConsistencyState,
    pub gap_percent: Option<Decimal>,
    pub compared_quarters: usize,
}

/// Compare usable quarters on their own weighted-average share bases. The caller
/// chooses the required count: three for trailing ratios, both for an EPS pair.
/// The allowance applies once to the aggregate gap. Summed absolute derived EPS
/// is the scale, so offsetting profit and loss quarters do not inflate the gap.
pub fn evaluate_eps_consistency<'a>(
    quarters: impl IntoIterator<Item = &'a Quarter>,
    minimum_pairs: usize,
) -> EpsConsistency {
    let quarters: Vec<_> = quarters.into_iter().collect();
    let pairs: Vec<_> = quarters
        .iter()
        .filter_map(|q| {
            Some((
                q.eps.value?,
                q.net_income.value? / q.shares.filter(|s| *s > Decimal::ZERO)?,
            ))
        })
        .collect();
    let mut outcome = EpsConsistency {
        state: if quarters.is_empty() || quarters.iter().any(|q| q.eps.value.is_none()) {
            EpsConsistencyState::Unavailable
        } else {
            EpsConsistencyState::NotCrossChecked
        },
        gap_percent: None,
        compared_quarters: pairs.len(),
    };
    if pairs.len() < minimum_pairs || pairs.is_empty() {
        return outcome;
    }
    let reported: Decimal = pairs.iter().map(|(eps, _)| *eps).sum();
    let derived: Decimal = pairs.iter().map(|(_, eps)| *eps).sum();
    let scale: Decimal = pairs.iter().map(|(_, eps)| eps.abs()).sum();
    let gap = if scale.is_zero() {
        Decimal::ZERO
    } else {
        ((reported - derived).abs() - EPS_ROUNDING_ALLOWANCE).max(Decimal::ZERO) / scale
    };
    outcome.gap_percent = Some(gap * Decimal::ONE_HUNDRED);
    outcome.state = if gap > EPS_UNKNOWN_PERCENT / Decimal::ONE_HUNDRED {
        EpsConsistencyState::Inconsistent
    } else if gap >= EPS_APPROXIMATE_PERCENT / Decimal::ONE_HUNDRED {
        EpsConsistencyState::Approximate
    } else {
        EpsConsistencyState::Consistent
    };
    outcome
}
