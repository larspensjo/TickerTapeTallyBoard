use chrono::NaiveDate;
use std::collections::BTreeSet;
use ticker_tape_tally_board_backend::{
    api::CompanyFigureInputs,
    domain::{
        company_figures::EpsConsistencyState,
        opportunity_evidence::business_check::{business_check, BusinessState},
    },
};

pub struct Business {
    pub state: &'static str,
    pub reasons: BTreeSet<&'static str>,
    pub eps: &'static str,
}

fn eps_label(state: EpsConsistencyState) -> &'static str {
    match state {
        EpsConsistencyState::Unavailable => "pair_unavailable",
        EpsConsistencyState::NotCrossChecked => "not_cross_checked",
        EpsConsistencyState::Consistent => "consistent",
        EpsConsistencyState::Approximate => "approximate",
        EpsConsistencyState::Inconsistent => "unknown_inconsistent",
    }
}

#[cfg(test)]
pub fn eps_pair(
    pair: [&ticker_tape_tally_board_backend::domain::company_figures::Quarter; 2],
) -> &'static str {
    eps_label(
        ticker_tape_tally_board_backend::domain::company_figures::evaluate_eps_consistency(pair, 2)
            .state,
    )
}

/// Preserve the measurement's category names while delegating the pure rules.
pub fn evaluate(today: NaiveDate, loaded: &CompanyFigureInputs) -> Business {
    let out = business_check(today, &loaded.figures, &loaded.source, &loaded.next_report);
    Business {
        state: match out.state {
            BusinessState::Intact => "intact",
            BusinessState::Weakening => "weakening",
            BusinessState::LossMaking => "loss_making_only",
            BusinessState::Unknown | BusinessState::NotApplicable => "unknown",
        },
        reasons: out.reasons.iter().map(|r| r.code).collect(),
        eps: eps_label(out.eps_consistency.state),
    }
}
