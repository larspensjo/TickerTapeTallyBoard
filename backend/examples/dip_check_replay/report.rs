use super::*;
use replay_rules::{
    criteria::{self, Calibration, Comparison, GridValue},
    SampleSize,
};
use std::{collections::BTreeSet, fmt::Write};
use ticker_tape_tally_board_backend::ledger_analysis::{
    aggregate_median, aggregate_share, aggregate_tenth_percentile, replay_statistics_allowed,
};

#[derive(Clone, Copy)]
pub struct Observation {
    pub instrument: i64,
    pub date: NaiveDate,
    pub first_half: bool,
}

#[derive(Default)]
struct Sample {
    selected: usize,
    values: Vec<(Observation, Decimal)>,
}
impl Sample {
    fn numbers(&self) -> Vec<Decimal> {
        self.values.iter().map(|(_, v)| *v).collect()
    }
    fn instruments(&self) -> usize {
        self.values
            .iter()
            .map(|(o, _)| o.instrument)
            .collect::<BTreeSet<_>>()
            .len()
    }
    fn weeks(&self) -> usize {
        self.values
            .iter()
            .map(|(o, _)| replay_rules::week(o.date))
            .collect::<BTreeSet<_>>()
            .len()
    }
    fn visible(&self) -> bool {
        replay_statistics_allowed(self.values.len(), self.instruments())
    }
    fn median(&self) -> Option<Decimal> {
        self.visible()
            .then(|| aggregate_median(&self.numbers()))
            .flatten()
    }
    fn percentile(&self) -> Option<Decimal> {
        self.visible()
            .then(|| aggregate_tenth_percentile(&self.numbers()))
            .flatten()
    }
    fn share(&self) -> String {
        if !self.visible() {
            return "suppressed".into();
        }
        aggregate_share(
            self.values
                .iter()
                .filter(|(_, v)| *v > Decimal::ZERO)
                .count(),
            self.values.len(),
        )
    }
}

#[derive(Default)]
pub struct Report {
    groups: BTreeMap<(&'static str, usize, String), [Sample; 3]>,
    pub exclusions: BTreeMap<(&'static str, usize, &'static str), usize>,
    pub buy_exclusions: BTreeMap<(&'static str, usize, &'static str), usize>,
    universe_exclusions: BTreeMap<(&'static str, &'static str), usize>,
    pub company_pairs: BTreeMap<&'static str, usize>,
}

fn value(value: Option<Decimal>) -> String {
    value
        .map(|v| {
            format!(
                "{:.2}",
                v.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointNearestEven)
            )
        })
        .unwrap_or_else(|| "suppressed_or_unavailable".into())
}

impl Report {
    pub fn ensure(&mut self, window: RecentWindow, horizon: usize, group: &str) {
        self.groups
            .entry((window.as_str(), horizon, group.to_owned()))
            .or_default();
    }
    pub fn reject_universe(&mut self, window: RecentWindow, reason: &'static str) {
        *self
            .universe_exclusions
            .entry((window.as_str(), reason))
            .or_default() += 1;
    }
    pub fn add(
        &mut self,
        window: RecentWindow,
        horizon: usize,
        group: &str,
        observation: Observation,
        result: &Result<Decimal, Vec<Exclusion>>,
    ) {
        let samples = self
            .groups
            .entry((window.as_str(), horizon, group.to_owned()))
            .or_default();
        for bucket in [0, if observation.first_half { 1 } else { 2 }] {
            samples[bucket].selected += 1;
            if let Ok(value) = result {
                samples[bucket].values.push((observation, *value));
            }
        }
    }
    fn sample(&self, window: RecentWindow, group: &str, half: usize) -> &Sample {
        static EMPTY: Sample = Sample {
            selected: 0,
            values: Vec::new(),
        };
        self.groups
            .get(&(window.as_str(), 20, group.to_owned()))
            .map(|samples| &samples[half])
            .unwrap_or(&EMPTY)
    }
    fn size(&self, window: RecentWindow) -> SampleSize {
        let flagged = self.sample(window, "flagged_dips", 0);
        SampleSize {
            dips: self.sample(window, "all_dips", 0).values.len(),
            flagged: flagged.values.len(),
            flagged_instruments: flagged.instruments(),
            flagged_weeks: flagged.weeks(),
        }
    }
    fn improvement(
        &self,
        window: RecentWindow,
        candidate: &str,
        baseline: &str,
        half: usize,
    ) -> Option<Decimal> {
        self.sample(window, candidate, half)
            .median()?
            .checked_sub(self.sample(window, baseline, half).median()?)
    }
    fn comparison(&self, window: RecentWindow, candidate: &str, baseline: &str) -> Comparison {
        let sample = self.sample(window, candidate, 0);
        Comparison {
            events: sample.values.len(),
            instruments: sample.instruments(),
            improvements: [0, 1, 2].map(|half| self.improvement(window, candidate, baseline, half)),
        }
    }
    pub fn render(&self, real: bool) -> String {
        let mut out = String::from("Caveats: overlapping horizons; correlated holdings; small samples; no significance claimed; survivor effects; restated prices and current mappings. Different markets can end horizons on different calendar dates.\nPrimary outcome: 20 stored sessions; 5/10 sessions are descriptive only. Exact decimal median; nearest-rank p10; positive means >0; statistics suppressed below five events or three instruments.\nNot replayable: historical report timing, point-in-time company checks, uncaptured ex-dividends, intraday prices.\n");
        for ((window, horizon, group), samples) in &self.groups {
            for (label, sample) in ["whole", "first_half", "second_half"]
                .into_iter()
                .zip(samples)
            {
                let values = sample.numbers();
                writeln!(out,"window={window} horizon={horizon} group={group} period={label} selected={} count={} instruments={} weeks={} median_excess_pp={} positive={} p10_excess_pp={}",
                    sample.selected, values.len(),sample.instruments(),sample.weeks(),value(sample.median()),
                    sample.share(),value(sample.percentile())).unwrap();
            }
        }
        for (kind, counts) in [
            ("weekly", &self.exclusions),
            ("buy_more", &self.buy_exclusions),
        ] {
            for ((window, horizon, reason), count) in counts {
                writeln!(out,"exclusion scope={kind} window={window} horizon={horizon} reason={reason} count={count}").unwrap();
            }
        }
        for ((window, reason), count) in &self.universe_exclusions {
            writeln!(
                out,
                "universe_exclusion window={window} reason={reason} count={count}"
            )
            .unwrap();
        }
        let outcomes = WINDOWS.map(|w| replay_rules::sufficiency(self.size(w)));
        let overall = super::overall(outcomes);
        for (window, outcome) in WINDOWS.into_iter().zip(outcomes) {
            let size = self.size(window);
            writeln!(out,"sufficiency window={} usable_dips={} flagged={} flagged_instruments={} flagged_weeks={} shortfall_dips={} shortfall_flagged={} shortfall_instruments={} shortfall_weeks={} outcome={}",
                window.as_str(),size.dips,size.flagged,size.flagged_instruments,size.flagged_weeks,
                CALIBRATION_MIN_DIPS.saturating_sub(size.dips),CALIBRATION_MIN_FLAGGED_DIPS.saturating_sub(size.flagged),
                CALIBRATION_MIN_INSTRUMENTS.saturating_sub(size.flagged_instruments),CALIBRATION_MIN_WEEKS.saturating_sub(size.flagged_weeks),outcome.as_str()).unwrap();
            writeln!(
                out,
                "window={} observed_company_pairs_not_point_in_time={} replayable_company_checks=0",
                window.as_str(),
                self.company_pairs
                    .get(window.as_str())
                    .copied()
                    .unwrap_or(0)
            )
            .unwrap();
        }
        let calibration = Calibration {
            outcome: overall,
            real,
        };
        let mut decisions = Vec::new();
        let grids = registered_grids().expect("registered grids validated before replay");
        for (prefix, candidates, initial, suffix) in &grids {
            let baseline = format!("{prefix}={initial}{suffix}");
            let mut grid = Vec::new();
            for candidate in candidates {
                let label = format!("{prefix}={candidate}{suffix}");
                let windows = WINDOWS.map(|window| {
                    let sample = self.comparison(window, &label, &baseline);
                    let improvements = sample.improvements;
                    writeln!(out,"change window={} grid={prefix} candidate={label} baseline={baseline} improvement_pp={} first_half_pp={} second_half_pp={} criterion={}",
                        window.as_str(),value(improvements[0]),value(improvements[1]),value(improvements[2]),criteria::change(calibration, sample).as_str()).unwrap();
                    sample
                });
                let criterion = criteria::all_windows(
                    windows.map(|sample| criteria::change(calibration, sample)),
                );
                writeln!(
                    out,
                    "change_value grid={prefix} value={candidate} criterion={}",
                    criterion.as_str()
                )
                .unwrap();
                grid.push(GridValue {
                    value: *candidate,
                    windows,
                });
            }
            decisions.push(criteria::choose_value(calibration, *initial, &grid));
        }
        let chosen = criteria::choose_one(calibration, &decisions);
        for (index, ((prefix, _, _, _), decision)) in grids.iter().zip(&decisions).enumerate() {
            let disposition = if chosen == Some(index) {
                "selected_for_threshold_change"
            } else if decision.chosen.is_some() {
                "candidate_for_later_replay_not_applied"
            } else {
                "keep_initial"
            };
            writeln!(out, "change_grid grid={prefix} criterion={} chosen_value={} improvement_30d_pp={} decision={disposition}",
                decision.criterion.as_str(), value(decision.chosen.map(|(v, _)| v)), value(decision.chosen.map(|(_, gain)| gain))).unwrap();
        }
        writeln!(
            out,
            "Replay decision: {}",
            chosen
                .map(|index| grids[index].0)
                .unwrap_or("keep_initial_thresholds")
        )
        .unwrap();
        for (name, thresholds) in [("unusual", DEVIATIONS.to_vec()), ("deep", DEPTHS.to_vec())] {
            let baseline = format!("{name}_computable_dips");
            let windows = WINDOWS.map(|window| {
                let comparisons: Vec<_> = thresholds
                    .iter()
                    .map(|threshold| {
                        let group = format!("{name}={threshold} single");
                        [1, 2].map(|half| self.improvement(window, &group, &baseline, half))
                    })
                    .collect();
                let criterion = criteria::drop_window(calibration, &comparisons);
                writeln!(
                    out,
                    "drop window={} check={name} baseline={baseline} criterion={}",
                    window.as_str(),
                    criterion.drop_label()
                )
                .unwrap();
                criterion
            });
            writeln!(
                out,
                "drop_overall check={name} criterion={}",
                criteria::all_windows(windows).drop_label()
            )
            .unwrap();
        }
        writeln!(out, "Overall outcome: {}", overall.as_str()).unwrap();
        if overall == Sufficiency::InsufficientHistory {
            out.push_str("Keep initial thresholds. Calibration prerequisite unmet; verdict UI remains gated. Session lead requests a backfill and repeats on a fresh scratch copy.\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn fill(report: &mut Report, group: &str, instruments: usize, gain: Decimal) {
        for half in [true, false] {
            for event in 0..30 {
                let observation = Observation {
                    instrument: (event % instruments) as i64,
                    date: NaiveDate::from_ymd_opt(2025, 1, 6).unwrap(),
                    first_half: half,
                };
                for window in WINDOWS {
                    report.add(window, 20, group, observation, &Ok(gain));
                }
            }
        }
    }

    #[test]
    fn every_group_and_half_suppresses_all_statistics_below_three_instruments() {
        let mut report = Report::default();
        for group in [
            "unconditional",
            "all_dips",
            "unusual=1.0 combined",
            "buy_more_flagged",
        ] {
            fill(&mut report, group, 2, dec!(2));
            for half in 0..3 {
                let sample = report.sample(WINDOWS[0], group, half);
                assert!(sample.values.len() >= 5);
                assert_eq!(sample.median(), None);
                assert_eq!(sample.percentile(), None);
                assert_eq!(sample.share(), "suppressed");
            }
            assert_eq!(report.improvement(WINDOWS[0], group, "all_dips", 0), None);
        }
        fill(&mut report, "three", 3, dec!(1));
        assert_eq!(
            report.sample(WINDOWS[0], "three", 1).median(),
            Some(dec!(1))
        );
        assert_eq!(
            report.sample(WINDOWS[0], "three", 1).percentile(),
            Some(dec!(1))
        );
        assert_eq!(report.sample(WINDOWS[0], "three", 1).share(), "100.00%");
        // Both populations must be large enough; a visible candidate cannot expose
        // a difference against a suppressed baseline.
        assert_eq!(report.improvement(WINDOWS[0], "three", "all_dips", 0), None);
    }

    #[test]
    fn drop_compares_with_the_checks_computable_dips_and_is_gated_by_overall_outcome() {
        let mut report = Report::default();
        for group in ["all_dips", "flagged_dips"] {
            fill(&mut report, group, 8, dec!(-5));
        }
        for name in ["unusual", "deep"] {
            fill(&mut report, &format!("{name}_computable_dips"), 8, dec!(2));
        }
        for (name, values) in [("unusual", DEVIATIONS.to_vec()), ("deep", DEPTHS.to_vec())] {
            for threshold in values {
                fill(
                    &mut report,
                    &format!("{name}={threshold} single"),
                    8,
                    dec!(1),
                );
            }
        }
        // Directly exercise the report's baseline wiring with calibrated sizes/weeks.
        for samples in report.groups.values_mut() {
            for sample in samples {
                for (index, (o, _)) in sample.values.iter_mut().enumerate() {
                    o.date += Duration::weeks(index as i64);
                }
            }
        }
        // Dips floor is 100; add enough while keeping the baseline deliberately lower.
        fill(&mut report, "all_dips", 8, dec!(-5));
        let out = report.render(true);
        assert!(out.contains("Overall outcome: Calibrated"));
        assert!(out.contains(
            "drop_overall check=unusual criterion=proposal_supported_owner_decision_required"
        ));
        assert!(out.contains(
            "drop_overall check=deep criterion=proposal_supported_owner_decision_required"
        ));
        report
            .groups
            .remove(&(WINDOWS[0].as_str(), 20, "flagged_dips".into()));
        let out = report.render(true);
        assert!(!out.contains("proposal_supported_owner_decision_required"));
        assert!(out.contains("drop_overall check=unusual criterion=undetermined"));
    }
}
