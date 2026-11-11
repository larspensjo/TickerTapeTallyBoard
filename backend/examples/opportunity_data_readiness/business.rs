use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeSet;
use ticker_tape_tally_board_backend::{api::CompanyFigureInputs, domain::company_figures::Quarter};

pub struct Business {
    pub state: &'static str,
    pub reasons: BTreeSet<&'static str>,
    pub eps: &'static str,
}

/// Evaluate just the compared quarters with the existing aggregate rounding/scale rules.
/// This deliberately remains tooling logic until a reusable domain evaluation exists.
pub fn eps_pair(pair: [&Quarter; 2]) -> &'static str {
    if pair.iter().any(|q| q.eps.value.is_none()) {
        return "pair_unavailable";
    }
    let pairs: Option<Vec<_>> = pair
        .iter()
        .map(|q| {
            Some((
                q.eps.value?,
                q.net_income.value? / q.shares.filter(|s| *s > Decimal::ZERO)?,
            ))
        })
        .collect();
    let Some(pairs) = pairs else {
        return "not_cross_checked";
    };
    let reported: Decimal = pairs.iter().map(|(reported, _)| *reported).sum();
    let derived: Decimal = pairs.iter().map(|(_, derived)| *derived).sum();
    let scale: Decimal = pairs.iter().map(|(_, derived)| derived.abs()).sum();
    if scale == Decimal::ZERO {
        return "consistent";
    }
    let gap = ((reported - derived).abs() - dec!(0.02)).max(Decimal::ZERO) / scale;
    if gap > dec!(0.20) {
        "unknown_inconsistent"
    } else if gap >= dec!(0.05) {
        "approximate"
    } else {
        "consistent"
    }
}

pub fn evaluate(today: NaiveDate, loaded: &CompanyFigureInputs) -> Business {
    let figures = &loaded.figures;
    let grid = &figures.quarters;
    let pair = (grid.len() >= 5).then(|| [&grid[grid.len() - 1], &grid[grid.len() - 5]]);
    let eps = pair.map(eps_pair).unwrap_or("pair_unavailable");
    let mut reasons = BTreeSet::new();
    for figure in [&figures.profit_margin, &figures.sales_growth] {
        if figure.value.is_none() {
            reasons.extend(figure.reasons.iter().map(|reason| reason.code));
        }
        if figure
            .reasons
            .iter()
            .any(|r| r.code == "newer_report_expected")
        {
            reasons.insert("newer_report_expected");
        }
    }
    if loaded.next_report.date.is_some_and(|date| date < today) {
        reasons.insert("expected_report_passed");
    }
    match pair {
        None => {
            reasons.insert("year_ago_quarter_missing");
        }
        Some([latest, year_ago]) => {
            if latest.currency != year_ago.currency {
                reasons.insert("report_currency_changed");
            }
            for quarter in [latest, year_ago] {
                if quarter.eps.value.is_none() {
                    reasons.extend(quarter.eps.reasons.iter().map(|r| r.code));
                }
            }
        }
    }
    if eps == "unknown_inconsistent" {
        reasons.insert("eps_inconsistent");
    }
    if !reasons.is_empty() {
        return Business {
            state: "unknown",
            reasons,
            eps,
        };
    }
    let [latest, year_ago] = pair.expect("checked pair");
    let current = latest.eps.value.expect("checked EPS");
    let prior = year_ago.eps.value.expect("checked EPS");
    let sliding = current < prior - dec!(0.10) * prior.abs()
        || (prior > Decimal::ZERO && current < Decimal::ZERO);
    let state = if figures.sales_growth.value.expect("checked growth") < Decimal::ZERO || sliding {
        "weakening"
    } else if figures.profit_margin.value.expect("checked margin") <= Decimal::ZERO {
        "loss_making_only"
    } else {
        "intact"
    };
    Business {
        state,
        reasons,
        eps,
    }
}
