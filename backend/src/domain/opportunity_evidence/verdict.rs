//! First matching rule wins. Market context and price-turn/target-gap metrics
//! deliberately have no input here. Presentation owns ordering within tiers.
use super::{
    business_check::{BusinessOutcome, BusinessState},
    price_checks::{OffHighOutcome, OffHighState, UnusualDropOutcome},
    report_timing::{
        ReportComingSoonOutcome, ReportComingSoonState, ReportInPeriodOutcome, ReportInPeriodState,
    },
    room_to_buy::RoomToBuy,
    YesNoUnknown,
};
use crate::domain::report_dates::ReportDate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceEvidence {
    Present,
    Absent,
    Undetermined,
}

pub fn price_evidence(unusual: YesNoUnknown, off_high: OffHighState) -> PriceEvidence {
    if unusual == YesNoUnknown::Yes || off_high == OffHighState::Deep {
        PriceEvidence::Present
    } else if unusual == YesNoUnknown::No && off_high == OffHighState::NotDeep {
        PriceEvidence::Absent
    } else {
        PriceEvidence::Undetermined
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecidingCheck {
    UnusualDrop,
    OffHigh,
    ReportInPeriod,
    Business,
}

/// Listed in the presentation's best-candidates-first tier order. Cautions never
/// affect the tier; displayed change, symbol and exchange are presentation ties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerdictCode {
    SentimentDipCandidate,
    PossibleCandidateReportTimingUnknown,
    NotEnoughData,
    FellForAReason,
    NormalMovement,
}

impl VerdictCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SentimentDipCandidate => "sentiment_dip_candidate",
            Self::PossibleCandidateReportTimingUnknown => {
                "possible_candidate_report_timing_unknown"
            }
            Self::NotEnoughData => "not_enough_data",
            Self::FellForAReason => "fell_for_a_reason",
            Self::NormalMovement => "normal_movement",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Caution {
    LossMaking,
    ReportComingSoon(ReportDate),
    AlreadyAtTarget,
    NoTarget,
}

#[derive(Clone, Debug)]
pub struct Verdict {
    pub code: VerdictCode,
    pub deciding: Vec<DecidingCheck>,
    pub missing: Vec<DecidingCheck>,
    pub cautions: Vec<Caution>,
}

pub fn verdict(
    unusual: &UnusualDropOutcome,
    off_high: &OffHighOutcome,
    report: &ReportInPeriodOutcome,
    business: &BusinessOutcome,
    soon: &ReportComingSoonOutcome,
    room: RoomToBuy,
) -> Verdict {
    let mut out = decide(unusual.state, off_high.state, report.state, business.state);
    if matches!(
        out.code,
        VerdictCode::SentimentDipCandidate | VerdictCode::PossibleCandidateReportTimingUnknown
    ) {
        if business.state == BusinessState::LossMaking {
            out.cautions.push(Caution::LossMaking);
        }
        if soon.state == ReportComingSoonState::Yes {
            out.cautions
                .push(Caution::ReportComingSoon(soon.report.clone()));
        }
        match room {
            RoomToBuy::AlreadyAtTarget => out.cautions.push(Caution::AlreadyAtTarget),
            RoomToBuy::NoTarget => out.cautions.push(Caution::NoTarget),
            _ => {}
        }
    }
    out
}

fn decide(
    unusual: YesNoUnknown,
    high: OffHighState,
    report: ReportInPeriodState,
    business: BusinessState,
) -> Verdict {
    use DecidingCheck::*;
    use VerdictCode::*;
    let price = price_evidence(unusual, high);
    let mut out = Verdict {
        code: NotEnoughData,
        deciding: vec![],
        missing: vec![],
        cautions: vec![],
    };
    if price == PriceEvidence::Absent {
        out.code = NormalMovement;
        out.deciding = vec![UnusualDrop, OffHigh];
        return out;
    }
    if report == ReportInPeriodState::Yes {
        out.deciding.push(ReportInPeriod);
    }
    if business == BusinessState::Weakening {
        out.deciding.push(Business);
    }
    if !out.deciding.is_empty() {
        out.code = FellForAReason;
        return out;
    }
    let usable_business = matches!(business, BusinessState::Intact | BusinessState::LossMaking);
    let both_na =
        report == ReportInPeriodState::NotApplicable && business == BusinessState::NotApplicable;
    if price == PriceEvidence::Present
        && ((report == ReportInPeriodState::No && usable_business) || both_na)
    {
        out.code = SentimentDipCandidate;
    } else if price == PriceEvidence::Present
        && report == ReportInPeriodState::Unknown
        && usable_business
    {
        out.code = PossibleCandidateReportTimingUnknown;
    }
    if out.code != NotEnoughData {
        if unusual == YesNoUnknown::Yes {
            out.deciding.push(UnusualDrop);
        }
        if high == OffHighState::Deep {
            out.deciding.push(OffHigh);
        }
        out.deciding.extend([ReportInPeriod, Business]);
        return out;
    }
    if price == PriceEvidence::Undetermined {
        if unusual == YesNoUnknown::Unknown {
            out.missing.push(UnusualDrop);
        }
        if high == OffHighState::Unknown {
            out.missing.push(OffHigh);
        }
    }
    if report == ReportInPeriodState::Unknown
        || (report == ReportInPeriodState::NotApplicable && !both_na)
    {
        out.missing.push(ReportInPeriod);
    }
    if business == BusinessState::Unknown || (business == BusinessState::NotApplicable && !both_na)
    {
        out.missing.push(Business);
    }
    out.deciding = out.missing.clone();
    out
}

#[cfg(test)]
mod tests;
