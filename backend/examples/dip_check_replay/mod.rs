mod demo_fixture;
mod report;
#[cfg(test)]
mod tests;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
pub use demo_fixture::demo_fixture;
use report::{Observation, Report};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeMap;
use ticker_tape_tally_board_backend::{
    domain::{
        derive_position,
        opportunity_evidence::{
            parameters::*,
            price_checks::{self, OffHighState},
            replay_rules::{self, CheckHistory, Exclusion, ReplayChecks, Sufficiency},
            verdict::{price_evidence, PriceEvidence},
            YesNoUnknown,
        },
        LedgerTransaction, RecentWindow, TransactionKind,
    },
    ledger_analysis::replay_inputs::ReplayInstrument,
};

const WINDOWS: [RecentWindow; 3] = [
    RecentWindow::OneWeek,
    RecentWindow::TwoWeeks,
    RecentWindow::ThirtyDays,
];
const HORIZONS: [usize; 3] = [5, 10, 20];
const GROUPS: [&str; 8] = [
    "unconditional",
    "all_dips",
    "flagged_dips",
    "unusual_only",
    "deep_only",
    "flagged_stopped_yes",
    "flagged_stopped_no",
    "flagged_stopped_unknown",
];
const DEVIATIONS: [Decimal; 4] = [dec!(1.0), dec!(1.5), dec!(2.0), dec!(2.5)];
const DEPTHS: [Decimal; 5] = [dec!(10), dec!(15), dec!(20), dec!(25), dec!(30)];

fn checks(
    instrument: &ReplayInstrument,
    date: NaiveDate,
    window: RecentWindow,
) -> Option<ReplayChecks> {
    replay_rules::checks_as_of(
        &CheckHistory {
            currency: &instrument.currency,
            split_adjusted_at_fetch: &|source| instrument.split_adjusted_sources.contains(source),
            stored: &instrument.stored,
            effective_finals: &instrument.finals,
            ledger_splits: &instrument.ledger_splits,
            provider_splits: &instrument.provider_splits,
        },
        date,
        window,
    )
}

fn position(ledger: &[LedgerTransaction], date: NaiveDate) -> Result<bool, &'static str> {
    let rows: Vec<_> = ledger
        .iter()
        .filter(|t| t.trade_date <= date)
        .cloned()
        .collect();
    derive_position(&rows)
        .map(|p| p.quantity > 0)
        .map_err(|_| "Replay position evaluation failed.")
}

fn dates(inputs: &[ReplayInstrument], end: NaiveDate) -> Vec<NaiveDate> {
    let Some(mut date) = inputs
        .iter()
        .flat_map(|i| &i.ledger)
        .filter(|t| t.kind == TransactionKind::Buy)
        .map(|t| t.trade_date)
        .min()
    else {
        return vec![];
    };
    let mut weekdays = 0usize;
    let mut dates = Vec::new();
    while date <= end {
        if !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
            if weekdays.is_multiple_of(5) {
                dates.push(date);
            }
            weekdays += 1;
        }
        date += Duration::days(1);
    }
    dates
}

/// Forward returns are independent of long-history eligibility. Benchmark peers
/// are all held rows with usable own horizons, including rows outside the universe.
fn excess_returns(
    inputs: &[ReplayInstrument],
    date: NaiveDate,
    held: &[bool],
) -> Vec<[Result<Decimal, Vec<Exclusion>>; 3]> {
    let forward: Vec<_> = inputs
        .iter()
        .map(|i| {
            let latest = i.finals.iter().rev().find(|c| c.date <= date);
            let splits: Vec<_> = i
                .ledger_splits
                .iter()
                .chain(&i.provider_splits)
                .copied()
                .collect();
            HORIZONS.map(|h| match latest {
                Some(c) => replay_rules::forward_return(
                    &i.sessions,
                    c,
                    h,
                    &i.currency,
                    &splits,
                    &|source| i.split_adjusted_sources.contains(source),
                ),
                None => Err(vec![Exclusion::Incomplete]),
            })
        })
        .collect();
    forward
        .iter()
        .enumerate()
        .map(|(index, row)| {
            std::array::from_fn(|h| {
                let own = row[h].clone()?;
                let reference = replay_rules::benchmark(
                    forward
                        .iter()
                        .enumerate()
                        .filter(|(other, _)| *other != index && held[*other])
                        .map(|(_, values)| values[h].as_ref().ok().copied()),
                )
                .map_err(|r| vec![r])?;
                own.checked_sub(reference)
                    .ok_or_else(|| vec![Exclusion::InvalidClose])
            })
        })
        .collect()
}

fn groups(check: &ReplayChecks) -> Vec<&'static str> {
    let mut groups = vec!["unconditional"];
    if check.change >= Decimal::ZERO {
        return groups;
    }
    groups.push("all_dips");
    if check.unusual.state != YesNoUnknown::Unknown {
        groups.push("unusual_computable_dips");
    }
    if check.off_high.state != OffHighState::Unknown {
        groups.push("deep_computable_dips");
    }
    if check.unusual.state == YesNoUnknown::Yes {
        groups.push("unusual_only");
    }
    if check.off_high.state == OffHighState::Deep {
        groups.push("deep_only");
    }
    if price_evidence(check.unusual.state, check.off_high.state) == PriceEvidence::Present {
        groups.push("flagged_dips");
        groups.push(match check.stopped.state {
            YesNoUnknown::Yes => "flagged_stopped_yes",
            YesNoUnknown::No => "flagged_stopped_no",
            _ => "flagged_stopped_unknown",
        });
    }
    groups
}

fn grid_groups(check: &ReplayChecks, window: RecentWindow) -> Vec<String> {
    if check.change >= Decimal::ZERO {
        return vec![];
    }
    let mut groups = Vec::new();
    for threshold in DEVIATIONS {
        let unusual = price_checks::unusual_drop_with_threshold(
            &check.series,
            window,
            check.start,
            check.change,
            None,
            threshold,
        );
        if unusual.state == YesNoUnknown::Yes {
            groups.push(format!("unusual={threshold} single"));
        }
        if price_evidence(unusual.state, check.off_high.state) == PriceEvidence::Present {
            groups.push(format!("unusual={threshold} combined"));
        }
    }
    for threshold in DEPTHS {
        let deep = price_checks::off_high_with_threshold(&check.series, threshold);
        if deep.state == OffHighState::Deep {
            groups.push(format!("deep={threshold} single"));
        }
        if price_evidence(check.unusual.state, deep.state) == PriceEvidence::Present {
            groups.push(format!("deep={threshold} combined"));
        }
    }
    if price_evidence(check.unusual.state, check.off_high.state) == PriceEvidence::Present {
        for age in [1, 2, 3] {
            if price_checks::stopped_falling_with_age(&check.series, check.start, check.end, age)
                .state
                == YesNoUnknown::Yes
            {
                groups.push(format!("stopped_age={age}"));
            }
        }
    }
    groups
}

fn observed_company_pair(instrument: &ReplayInstrument, date: NaiveDate) -> bool {
    let mut ends: Vec<_> = instrument
        .observed_eps
        .iter()
        .filter(|(end, seen)| *end <= date && *seen <= date)
        .map(|(end, _)| *end)
        .collect();
    ends.sort_unstable();
    ends.dedup();
    ends.last().is_some_and(|latest| {
        (date - *latest).num_days() <= 200
            && ends
                .iter()
                .any(|prior| latest.year() - prior.year() == 1 && latest.month() == prior.month())
    })
}

fn universe_midpoint(
    inputs: &[ReplayInstrument],
    dates: &[NaiveDate],
    end: NaiveDate,
) -> Result<NaiveDate, &'static str> {
    let mut eligible = Vec::new();
    for date in dates {
        for instrument in inputs {
            if position(&instrument.ledger, *date)?
                && WINDOWS.into_iter().any(|window| {
                    checks(instrument, *date, window).is_some_and(|c| {
                        c.unusual.state != YesNoUnknown::Unknown
                            || c.off_high.state != OffHighState::Unknown
                    })
                })
            {
                eligible.push(*date);
                break;
            }
        }
    }
    Ok(eligible.get(eligible.len() / 2).copied().unwrap_or(end))
}

type GridDefinition = (&'static str, Vec<Decimal>, Decimal, &'static str);

fn registered_grids() -> Result<[GridDefinition; 3], &'static str> {
    use replay_rules::criteria::default_index;
    let ages = [1, 2, 3].map(Decimal::from);
    Ok([
        (
            "unusual",
            DEVIATIONS.to_vec(),
            DEVIATIONS[default_index(&DEVIATIONS, UNUSUAL_DROP_DEVIATIONS)?],
            " combined",
        ),
        (
            "deep",
            DEPTHS.to_vec(),
            DEPTHS[default_index(&DEPTHS, DEEP_BELOW_HIGH_PERCENT)?],
            " combined",
        ),
        (
            "stopped_age",
            ages.to_vec(),
            ages[default_index(&ages, Decimal::from(STOPPED_FALLING_MIN_DAYS))?],
            "",
        ),
    ])
}

pub fn validate_parameters() -> Result<(), &'static str> {
    registered_grids().map(|_| ())
}

pub fn run(
    inputs: &[ReplayInstrument],
    end: NaiveDate,
    real: bool,
) -> Result<String, &'static str> {
    validate_parameters()?;
    let dates = dates(inputs, end);
    let midpoint = universe_midpoint(inputs, &dates, end)?;
    let mut report = Report::default();
    for window in WINDOWS {
        for horizon in HORIZONS {
            for group in GROUPS
                .into_iter()
                .chain(["unusual_computable_dips", "deep_computable_dips"])
            {
                report.ensure(window, horizon, group);
            }
            for group in [
                "buy_more_flagged",
                "buy_more_unflagged",
                "buy_more_rose",
                "buy_more_undetermined",
                "buy_more_unavailable",
            ] {
                report.ensure(window, horizon, group);
            }
            for reason in Exclusion::ALL {
                report
                    .exclusions
                    .entry((window.as_str(), horizon, reason.as_str()))
                    .or_default();
                report
                    .buy_exclusions
                    .entry((window.as_str(), horizon, reason.as_str()))
                    .or_default();
            }
        }
        for threshold in DEVIATIONS {
            for kind in ["single", "combined"] {
                report.ensure(window, 20, &format!("unusual={threshold} {kind}"));
            }
        }
        for threshold in DEPTHS {
            for kind in ["single", "combined"] {
                report.ensure(window, 20, &format!("deep={threshold} {kind}"));
            }
        }
        for age in [1, 2, 3] {
            report.ensure(window, 20, &format!("stopped_age={age}"));
        }
    }
    for date in dates {
        let held = inputs
            .iter()
            .map(|i| position(&i.ledger, date))
            .collect::<Result<Vec<_>, _>>()?;
        let excess = excess_returns(inputs, date, &held);
        for (index, instrument) in inputs.iter().enumerate().filter(|(index, _)| held[*index]) {
            for window in WINDOWS {
                let Some(check) = checks(instrument, date, window) else {
                    report.reject_universe(window, "change_unavailable");
                    continue;
                };
                if check.unusual.state == YesNoUnknown::Unknown
                    && check.off_high.state == OffHighState::Unknown
                {
                    report.reject_universe(window, "no_computable_long_history_check");
                    continue;
                }
                if observed_company_pair(instrument, date) {
                    *report.company_pairs.entry(window.as_str()).or_default() += 1;
                }
                let observation = Observation {
                    instrument: instrument.key,
                    date,
                    first_half: date < midpoint,
                };
                let groups = groups(&check);
                for (h, value) in HORIZONS.into_iter().zip(&excess[index]) {
                    for group in &groups {
                        report.add(window, h, group, observation, value);
                    }
                    if let Err(reasons) = value {
                        for reason in reasons {
                            *report
                                .exclusions
                                .entry((window.as_str(), h, reason.as_str()))
                                .or_default() += 1;
                        }
                    }
                }
                for group in grid_groups(&check, window) {
                    report.add(window, 20, &group, observation, &excess[index][2]);
                }
            }
        }
    }
    replay_buys(inputs, midpoint, end, &mut report)?;
    Ok(report.render(real))
}

fn buy_more_dates(
    inputs: &[ReplayInstrument],
) -> Result<BTreeMap<NaiveDate, Vec<usize>>, &'static str> {
    // Group by date solely to reuse benchmark work. Events retain buy multiplicity.
    let mut buys: BTreeMap<NaiveDate, Vec<usize>> = BTreeMap::new();
    for (index, i) in inputs.iter().enumerate() {
        for (offset, trade) in i
            .ledger
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == TransactionKind::Buy)
        {
            let before = derive_position(&i.ledger[..offset])
                .map_err(|_| "Replay pre-buy position evaluation failed.")?;
            if before.quantity > 0 {
                buys.entry(trade.trade_date).or_default().push(index);
            }
        }
    }
    Ok(buys)
}

fn buy_more_group(check: Option<&ReplayChecks>) -> &'static str {
    match check {
        Some(check) if check.change < Decimal::ZERO => {
            match price_evidence(check.unusual.state, check.off_high.state) {
                PriceEvidence::Present => "buy_more_flagged",
                PriceEvidence::Absent => "buy_more_unflagged",
                PriceEvidence::Undetermined => "buy_more_undetermined",
            }
        }
        Some(_) => "buy_more_rose",
        None => "buy_more_unavailable",
    }
}

fn replay_buys(
    inputs: &[ReplayInstrument],
    midpoint: NaiveDate,
    end: NaiveDate,
    report: &mut Report,
) -> Result<(), &'static str> {
    for (date, events) in buy_more_dates(inputs)?
        .into_iter()
        .filter(|(date, _)| *date <= end)
    {
        let held = inputs
            .iter()
            .map(|i| position(&i.ledger, date))
            .collect::<Result<Vec<_>, _>>()?;
        let excess = excess_returns(inputs, date, &held);
        for index in events {
            for window in WINDOWS {
                let group = buy_more_group(checks(&inputs[index], date, window).as_ref());
                let observation = Observation {
                    instrument: inputs[index].key,
                    date,
                    first_half: date < midpoint,
                };
                for (h, value) in HORIZONS.into_iter().zip(&excess[index]) {
                    report.add(window, h, group, observation, value);
                    if let Err(reasons) = value {
                        for reason in reasons {
                            *report
                                .buy_exclusions
                                .entry((window.as_str(), h, reason.as_str()))
                                .or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn overall(outcomes: impl IntoIterator<Item = Sufficiency>) -> Sufficiency {
    let outcomes: Vec<_> = outcomes.into_iter().collect();
    if outcomes.len() == WINDOWS.len() && outcomes.iter().all(|o| *o == Sufficiency::Calibrated) {
        Sufficiency::Calibrated
    } else {
        Sufficiency::InsufficientHistory
    }
}
