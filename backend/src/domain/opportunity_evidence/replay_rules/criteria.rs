//! Pre-registered decisions. Inputs are aggregate statistics already privacy-filtered.
use super::{Sufficiency, CALIBRATION_MIN_FLAGGED_DIPS, CALIBRATION_MIN_INSTRUMENTS};
use rust_decimal::Decimal;

#[derive(Clone, Copy)]
pub struct Calibration {
    pub outcome: Sufficiency,
    pub real: bool,
}
impl Calibration {
    pub fn permits_decision(self) -> bool {
        self.real && self.outcome == Sufficiency::Calibrated
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Criterion {
    Met,
    NotMet,
    Undetermined,
}
impl Criterion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Met => "met",
            Self::NotMet => "not_met",
            Self::Undetermined => "undetermined",
        }
    }
    pub fn drop_label(self) -> &'static str {
        match self {
            Self::Met => "proposal_supported_owner_decision_required",
            _ => self.as_str(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Comparison {
    pub events: usize,
    pub instruments: usize,
    /// Whole, first half, second half; differences of suppressed medians stay None.
    pub improvements: [Option<Decimal>; 3],
}

pub fn change(calibration: Calibration, sample: Comparison) -> Criterion {
    if !calibration.permits_decision() {
        return Criterion::Undetermined;
    }
    let [Some(whole), Some(first), Some(second)] = sample.improvements else {
        return Criterion::Undetermined;
    };
    if sample.events >= CALIBRATION_MIN_FLAGGED_DIPS
        && sample.instruments >= CALIBRATION_MIN_INSTRUMENTS
        && whole >= Decimal::ONE
        && first > Decimal::ZERO
        && second > Decimal::ZERO
    {
        Criterion::Met
    } else {
        Criterion::NotMet
    }
}

/// A failure in one window disproves the all-windows criterion even if another
/// window is undetermined. Only three positive findings can establish it.
pub fn all_windows(criteria: [Criterion; 3]) -> Criterion {
    if criteria.contains(&Criterion::NotMet) {
        Criterion::NotMet
    } else if criteria.contains(&Criterion::Undetermined) {
        Criterion::Undetermined
    } else {
        Criterion::Met
    }
}

pub struct GridValue {
    pub value: Decimal,
    /// Ordered 7d, 14d, 30d.
    pub windows: [Comparison; 3],
}

pub struct GridDecision {
    pub criterion: Criterion,
    pub chosen: Option<(Decimal, Decimal)>, // threshold, whole-period 30d improvement
}

pub fn choose_value(
    calibration: Calibration,
    initial: Decimal,
    grid: &[GridValue],
) -> GridDecision {
    if !calibration.permits_decision() {
        return GridDecision {
            criterion: Criterion::Undetermined,
            chosen: None,
        };
    }
    let mut chosen: Option<(Decimal, Decimal)> = None;
    let mut unknown = false;
    for candidate in grid {
        let criterion = all_windows(candidate.windows.map(|w| change(calibration, w)));
        unknown |= criterion == Criterion::Undetermined;
        if criterion != Criterion::Met {
            continue;
        }
        let improvement = candidate.windows[2].improvements[0].expect("met requires median");
        let better = chosen.is_none_or(|(value, gain)| {
            improvement > gain
                || (improvement == gain
                    && ((candidate.value - initial).abs() < (value - initial).abs()
                        || ((candidate.value - initial).abs() == (value - initial).abs()
                            && candidate.value < value)))
        });
        if better {
            chosen = Some((candidate.value, improvement));
        }
    }
    GridDecision {
        criterion: if chosen.is_some() {
            Criterion::Met
        } else if unknown {
            Criterion::Undetermined
        } else {
            Criterion::NotMet
        },
        chosen,
    }
}

/// Decisions are in the documented grid order, which breaks equal cross-grid gains.
/// Only this index is selected; all other qualifying grids are later-replay candidates.
pub fn choose_one(calibration: Calibration, grids: &[GridDecision]) -> Option<usize> {
    if !calibration.permits_decision() {
        return None;
    }
    let mut best: Option<(usize, Decimal)> = None;
    for (index, grid) in grids.iter().enumerate() {
        if let Some((_, gain)) = grid.chosen {
            if best.is_none_or(|(_, previous)| gain > previous) {
                best = Some((index, gain));
            }
        }
    }
    best.map(|(index, _)| index)
}

/// Each grid value contributes both halves, compared with dips on which THIS
/// check is computable. Missing medians cannot support a drop proposal.
pub fn drop_window(calibration: Calibration, improvements: &[[Option<Decimal>; 2]]) -> Criterion {
    if !calibration.permits_decision() || improvements.is_empty() {
        return Criterion::Undetermined;
    }
    if improvements.iter().flatten().any(Option::is_none) {
        return Criterion::Undetermined;
    }
    if improvements
        .iter()
        .flatten()
        .all(|v| v.is_some_and(|v| v <= Decimal::ZERO))
    {
        Criterion::Met
    } else {
        Criterion::NotMet
    }
}

pub fn default_index(grid: &[Decimal], initial: Decimal) -> Result<usize, &'static str> {
    grid.iter()
        .position(|v| *v == initial)
        .ok_or("Replay default threshold is outside its registered grid.")
}

#[cfg(test)]
mod tests;
