use super::parameters::{EARNINGS_SLIDING_PERCENT, PROFITABLE_MARGIN_PERCENT};
use crate::domain::{
    company_figures::{
        evaluate_eps_consistency, CompanyFigures, EpsConsistency, EpsConsistencyState, Figure,
        Reason, SourceState,
    },
    fundamentals::Classification,
    report_dates::ReportDate,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusinessState {
    Intact,
    Weakening,
    LossMaking,
    Unknown,
    NotApplicable,
}

#[derive(Clone, Debug)]
pub struct BusinessSubtest {
    pub passes: Option<bool>,
    /// Margin and sales growth are fractions; EPS retains its reported units.
    pub value: Option<Decimal>,
    pub year_ago_value: Option<Decimal>,
    pub approximate: bool,
    pub reasons: Vec<Reason>,
}

impl BusinessSubtest {
    fn from_figures(current: &Figure, previous: Option<&Figure>, passes: Option<bool>) -> Self {
        let mut reasons = current.reasons.clone();
        if let Some(previous) = previous {
            for r in &previous.reasons {
                if !reasons.contains(r) {
                    reasons.push(r.clone());
                }
            }
        }
        Self {
            passes,
            value: current.value,
            year_ago_value: previous.and_then(|f| f.value),
            approximate: current.certainty
                == crate::domain::company_figures::Certainty::Approximate
                || previous.is_some_and(|f| {
                    f.certainty == crate::domain::company_figures::Certainty::Approximate
                }),
            reasons,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BusinessOutcome {
    pub state: BusinessState,
    pub reasons: Vec<Reason>,
    pub profitable: BusinessSubtest,
    pub sales_not_shrinking: BusinessSubtest,
    pub earnings_not_sliding: BusinessSubtest,
    pub eps_consistency: EpsConsistency,
}

pub fn earnings_not_sliding(current: Decimal, prior: Decimal) -> bool {
    current >= prior - (EARNINGS_SLIDING_PERCENT / Decimal::ONE_HUNDRED) * prior.abs()
        && !(prior > Decimal::ZERO && current < Decimal::ZERO)
}

pub fn business_check(
    today: NaiveDate,
    figures: &CompanyFigures,
    source: &SourceState,
    next: &ReportDate,
) -> BusinessOutcome {
    let grid = &figures.quarters;
    let pair = (grid.len() >= 5).then(|| [&grid[grid.len() - 1], &grid[grid.len() - 5]]);
    let missing = Figure::unavailable("year_ago_quarter_missing");
    let (eps, old_eps) = match pair {
        Some([latest, prior]) => (&latest.eps, &prior.eps),
        None => (&missing, &missing),
    };
    let consistency = evaluate_eps_consistency(pair.into_iter().flatten(), 2);
    let mut out = BusinessOutcome {
        state: BusinessState::Unknown,
        reasons: vec![],
        profitable: BusinessSubtest::from_figures(
            &figures.profit_margin,
            None,
            figures
                .profit_margin
                .value
                .map(|m| m > PROFITABLE_MARGIN_PERCENT / Decimal::ONE_HUNDRED),
        ),
        // Company figures already derives latest-quarter growth against the
        // year-ago quarter, with its currency and availability rules.
        sales_not_shrinking: BusinessSubtest::from_figures(
            &figures.sales_growth,
            None,
            figures
                .sales_growth
                .value
                .map(|growth| growth >= Decimal::ZERO),
        ),
        earnings_not_sliding: BusinessSubtest::from_figures(
            eps,
            Some(old_eps),
            eps.value
                .zip(old_eps.value)
                .map(|(current, prior)| earnings_not_sliding(current, prior)),
        ),
        eps_consistency: consistency,
    };
    if let Some(code) = source.not_applicable() {
        out.state = BusinessState::NotApplicable;
        out.reasons.push(Reason::new(code));
        return out;
    }
    if let Some(reason) = source.blocking_reason(!grid.is_empty()) {
        out.reasons.push(reason);
    } else if source.classification == Classification::Unknown {
        out.reasons.push(Reason::new("security_type_unknown"));
    }
    if next.date.is_some_and(|date| date < today) {
        out.reasons.push(Reason::new("expected_report_passed"));
    }
    if [&figures.profit_margin, &figures.sales_growth]
        .iter()
        .any(|f| f.reasons.iter().any(|r| r.code == "newer_report_expected"))
    {
        out.reasons.push(Reason::new("newer_report_expected"));
    }
    if pair.is_some_and(|[latest, prior]| latest.currency != prior.currency) {
        out.reasons.push(Reason::new("report_currency_changed"));
    }
    match out.eps_consistency.state {
        EpsConsistencyState::Inconsistent => {
            out.earnings_not_sliding.passes = None;
            out.reasons.push(Reason::new("eps_inconsistent"));
            out.earnings_not_sliding
                .reasons
                .push(Reason::new("eps_inconsistent"));
        }
        EpsConsistencyState::Approximate => {
            out.earnings_not_sliding.approximate = true;
            out.earnings_not_sliding.reasons.push(Reason::detail(
                "eps_disagree",
                out.eps_consistency.gap_percent.expect("evaluated gap"),
            ));
        }
        EpsConsistencyState::NotCrossChecked => out
            .earnings_not_sliding
            .reasons
            .push(Reason::new("eps_not_cross_checked")),
        _ => {}
    }
    // Only unavailable inputs explain an unknown business result. Approximation
    // and cross-check markers stay on the sub-test, including a usable EPS peer.
    // Company figures already applies the latest-quarter age rule to these figures.
    for figure in [&figures.profit_margin, &figures.sales_growth, eps, old_eps] {
        if figure.value.is_none() {
            if figure.reasons.is_empty() {
                out.reasons.push(Reason::new("business_figure_unavailable"));
            }
            for reason in &figure.reasons {
                if !out.reasons.contains(reason) {
                    out.reasons.push(reason.clone());
                }
            }
        }
    }
    if !out.reasons.is_empty() {
        return out;
    }
    out.state = if out.sales_not_shrinking.passes == Some(false)
        || out.earnings_not_sliding.passes == Some(false)
    {
        BusinessState::Weakening
    } else if out.profitable.passes == Some(false) {
        BusinessState::LossMaking
    } else {
        BusinessState::Intact
    };
    out
}

#[cfg(test)]
mod tests;
