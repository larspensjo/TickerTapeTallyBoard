use super::*;
use crate::domain::fundamentals::PeriodType;
use crate::domain::report_dates::{EstimateBasis, ReportExpectation};
use chrono::Duration;

fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
struct Case {
    values: Vec<ReportedValue>,
    price: Price,
    evidence_price: Option<Price>,
    fx: Vec<FxRate>,
    source: SourceState,
    splits: Vec<NaiveDate>,
    due: Option<ReportExpectation>,
    today: NaiveDate,
}
impl Case {
    fn new() -> Self {
        let today = date("2026-11-04");
        let price = Price {
            date: today,
            close: dec!(100),
            currency: "SEK".into(),
            stale: false,
        };
        let mut values = Vec::new();
        for (i, end) in [
            "2025-09-30",
            "2025-12-31",
            "2026-03-31",
            "2026-06-30",
            "2026-09-30",
        ]
        .iter()
        .enumerate()
        {
            for (metric, value) in [
                (Metric::Revenue, dec!(1000) + Decimal::from(i) * dec!(100)),
                (Metric::DilutedNetIncome, dec!(100)),
                (Metric::DilutedShares, dec!(100)),
                (Metric::DilutedEps, dec!(1)),
            ] {
                values.push(ReportedValue {
                    input: FundamentalValue {
                        period_type: PeriodType::Quarter,
                        period_end: date(end),
                        metric,
                        value,
                        currency: Some("SEK".into()),
                    },
                    fetched_on: today,
                });
            }
        }
        values.push(ReportedValue {
            input: FundamentalValue {
                period_type: PeriodType::Evidence,
                period_end: today,
                metric: Metric::MarketCap,
                value: dec!(10000),
                currency: Some("SEK".into()),
            },
            fetched_on: today,
        });
        Self {
            values,
            evidence_price: Some(price.clone()),
            price,
            fx: vec![],
            source: SourceState {
                classification: Classification::Equity,
                status: LinkStatus::Usable,
                awaiting_figures: false,
                unreachable: false,
                last_success: None,
            },
            splits: vec![],
            due: None,
            today,
        }
    }
    fn run(&self) -> CompanyFigures {
        compute(&Inputs {
            today: self.today,
            values: &self.values,
            price: Some(&self.price),
            evidence_price: self.evidence_price.as_ref(),
            fx: &self.fx,
            source: &self.source,
            splits: &self.splits,
            next_report: self.due,
        })
    }
    fn remove(&mut self, end: &str, metric: Option<Metric>) {
        self.values.retain(|v| {
            v.input.period_end != date(end) || metric.is_some_and(|m| m != v.input.metric)
        });
    }
    fn annual(&mut self, metric: Metric, value: Decimal) {
        self.values.push(ReportedValue {
            input: FundamentalValue {
                period_type: PeriodType::Annual,
                period_end: date("2026-06-30"),
                metric,
                value,
                currency: Some("SEK".into()),
            },
            fetched_on: self.today,
        });
    }
}
fn reason(f: &Figure, code: &str) {
    assert!(f.reasons.iter().any(|r| r.code == code), "{code}: {f:?}");
}
fn missing(f: &Figure, code: &str) {
    assert_eq!(f.certainty, Certainty::Unavailable);
    assert_eq!(f.value, None);
    reason(f, code);
}

#[test]
fn complete_figures_use_reported_quarters_and_per_quarter_share_denominators() {
    let mut c = Case::new();
    let f = c.run();
    assert_eq!(f.pe, Figure::available(dec!(25), vec![]));
    assert_eq!(f.ps.value, Some(dec!(2)));
    assert_eq!(f.profit_margin.value, Some(dec!(0.08)));
    assert_eq!(f.sales_growth.value, Some(dec!(0.4)));
    for v in c
        .values
        .iter_mut()
        .filter(|v| v.input.period_type == PeriodType::Quarter)
    {
        let shares = if v.input.period_end < date("2026-06-30") {
            dec!(80)
        } else {
            dec!(100)
        };
        if v.input.metric == Metric::DilutedShares {
            v.input.value = shares;
        }
        if v.input.metric == Metric::DilutedEps {
            v.input.value = dec!(100) / shares;
        }
    }
    assert_eq!(c.run().pe.certainty, Certainty::Complete);
}

#[test]
fn grid_and_three_quarter_scaling_keep_explicit_gaps() {
    let mut c = Case::new();
    c.remove("2026-03-31", None);
    let f = c.run();
    assert_eq!(f.quarters.len(), 5);
    assert!(f.quarters[2].missing);
    assert_eq!(f.quarters[2].sales.value, None);
    assert_eq!(f.pe.value, Some(dec!(25)));
    reason(&f.pe, "ttm_from_three_quarters");
    assert_eq!(f.ps.certainty, Certainty::Approximate);
    c.remove("2026-06-30", None);
    missing(&c.run().pe, "too_few_quarters");
    c.remove("2025-09-30", None);
    missing(&c.run().sales_growth, "year_ago_quarter_missing");
}

#[test]
fn exact_subtraction_is_limited_to_the_same_additive_measure() {
    let mut c = Case::new();
    c.remove("2026-03-31", Some(Metric::Revenue));
    c.remove("2026-03-31", Some(Metric::DilutedNetIncome));
    c.annual(Metric::Revenue, dec!(4600));
    c.annual(Metric::DilutedNetIncome, dec!(400));
    let f = c.run();
    assert_eq!(f.ps.certainty, Certainty::Complete);
    assert_eq!(f.profit_margin.certainty, Certainty::Complete);
    assert_eq!(f.quarters[2].sales.value, Some(dec!(1200)));
    assert_eq!(f.quarters[2].net_income.value, Some(dec!(100)));
    c.remove("2026-03-31", Some(Metric::DilutedEps));
    c.annual(Metric::DilutedEps, dec!(999));
    let f = c.run();
    assert_eq!(f.quarters[2].eps.value, Some(dec!(1)));
    reason(&f.pe, "eps_derived_from_net_income");
    c.remove("2026-03-31", Some(Metric::DilutedShares));
    assert_eq!(c.run().quarters[2].eps.value, None);
    reason(&c.run().pe, "ttm_from_three_quarters");
    c.values.retain(|v| {
        !(v.input.period_type == PeriodType::Annual && v.input.metric == Metric::DilutedNetIncome)
    });
    c.annual(Metric::NetIncome, dec!(400));
    assert_eq!(c.run().quarters[2].net_income.value, None);
}

#[test]
fn short_history_currency_changes_old_quarters_and_invalid_denominators_are_explicit() {
    let mut c = Case::new();
    c.remove("2025-09-30", None);
    c.remove("2025-12-31", None);
    assert_eq!(c.run().pe.certainty, Certainty::Approximate);
    c.remove("2026-03-31", None);
    missing(&c.run().pe, "too_few_quarters");
    let mut c = Case::new();
    for v in c
        .values
        .iter_mut()
        .filter(|v| v.input.period_end == date("2026-03-31"))
    {
        v.input.currency = Some("EUR".into());
    }
    for f in [
        &c.run().pe,
        &c.run().ps,
        &c.run().profit_margin,
        &c.run().sales_growth,
    ] {
        missing(f, "report_currency_changed");
    }
    let mut c = Case::new();
    c.today = date("2027-05-01");
    missing(&c.run().pe, "latest_quarter_too_old");
    for amount in [dec!(-1), dec!(0)] {
        let mut c = Case::new();
        for v in &mut c.values {
            match v.input.metric {
                Metric::Revenue => v.input.value = amount,
                Metric::DilutedEps | Metric::DilutedNetIncome => v.input.value = amount,
                _ => {}
            }
        }
        let f = c.run();
        missing(&f.pe, "loss_making");
        missing(&f.ps, "non_positive_revenue");
        missing(&f.profit_margin, "non_positive_revenue");
        missing(&f.sales_growth, "non_positive_revenue");
        for f in [f.pe, f.ps, f.profit_margin, f.sales_growth] {
            assert!(f.value.is_none());
        }
    }
}

#[test]
fn fx_crosses_via_sek_uses_price_date_carry_forward_and_rejects_unsupported_currencies() {
    let d = date("2026-11-04");
    let rates = vec![
        FxRate {
            currency: "USD".into(),
            date: d - Duration::days(1),
            sek_per_unit: dec!(10),
        },
        FxRate {
            currency: "EUR".into(),
            date: d,
            sek_per_unit: dec!(11),
        },
        FxRate {
            currency: "USD".into(),
            date: d + Duration::days(1),
            sek_per_unit: dec!(50),
        },
    ];
    let f = fx_cross("EUR", "USD", d, &rates).unwrap();
    assert_eq!(f.rate, dec!(1.1));
    assert_eq!(f.to_rate_date, d - Duration::days(1));
    assert_eq!(fx_cross("SEK", "SEK", d, &[]).unwrap().rate, Decimal::ONE);
    assert_eq!(
        fx_cross("TWD", "USD", d, &rates)
            .unwrap_err()
            .detail
            .as_deref(),
        Some("TWD")
    );
    let mut c = Case::new();
    for v in c
        .values
        .iter_mut()
        .filter(|v| v.input.period_type == PeriodType::Quarter)
    {
        v.input.currency = Some("EUR".into());
    }
    missing(&c.run().pe, "fx_unavailable");
    assert_eq!(c.run().profit_margin.certainty, Certainty::Complete);
    c.fx = rates;
    assert_eq!(c.run().pe.value, Some(dec!(25) / dec!(11)));
    assert_eq!(c.run().fx_conversion.unwrap().date, d);
}

#[test]
fn evidence_bands_null_currency_and_missing_or_stale_evidence_fail_safely() {
    for (units, state, ratio) in [
        (dec!(1), ShareBasisState::Established, None),
        (dec!(0.8), ShareBasisState::Established, None),
        (dec!(1.25), ShareBasisState::Established, None),
        (
            dec!(0.2),
            ShareBasisState::ListingUnitMismatch,
            Some(dec!(0.2)),
        ),
        (dec!(5), ShareBasisState::ListingUnitMismatch, Some(dec!(5))),
        (dec!(1.5), ShareBasisState::Unverified, None),
    ] {
        let mut c = Case::new();
        c.values.last_mut().unwrap().input.value *= units;
        let f = c.run();
        assert_eq!(f.share_basis.state, state);
        assert_eq!(f.share_basis.ratio, ratio);
        assert_eq!(f.profit_margin.certainty, Certainty::Complete);
        assert_eq!(f.sales_growth.certainty, Certainty::Complete);
    }
    for kind in 0..4 {
        let mut c = Case::new();
        match kind {
            0 => {
                c.values.pop();
            }
            1 => {
                c.values.last_mut().unwrap().input.period_end -= Duration::days(31);
            }
            2 => c.evidence_price = None,
            _ => c.values.last_mut().unwrap().input.currency = None,
        }
        let f = c.run();
        missing(&f.pe, "share_basis_unverified");
        assert_eq!(f.profit_margin.certainty, Certainty::Complete);
    }
    let mut c = Case::new();
    let mut e = c.values.last().unwrap().clone();
    e.input.period_end -= Duration::days(5);
    c.values.last_mut().unwrap().input.currency = None;
    c.values.push(e);
    assert_eq!(c.run().share_basis.state, ShareBasisState::Established);
    c.values.last_mut().unwrap().input.currency = Some("EUR".into());
    assert_eq!(c.run().share_basis.state, ShareBasisState::Unverified);
}

#[test]
fn splits_block_old_report_units_and_exclude_unrestated_per_share_slots() {
    let mut c = Case::new();
    let split = date("2026-10-15");
    c.splits.push(split);
    c.values.last_mut().unwrap().input.period_end = date("2026-10-10");
    c.evidence_price.as_mut().unwrap().date = date("2026-10-10");
    missing(&c.run().pe, "split_since_report");
    // Post-split evidence plus restated inputs can establish the basis again.
    c.values.last_mut().unwrap().input.period_end = c.today;
    c.evidence_price.as_mut().unwrap().date = c.today;
    assert_eq!(c.run().pe.certainty, Certainty::Complete);
    c.splits = vec![date("2026-08-01")];
    for v in &mut c.values {
        if v.input.period_end < date("2026-08-01") {
            v.fetched_on = date("2026-07-01");
        }
    }
    let f = c.run();
    assert_eq!(f.quarters[3].eps.value, None);
    assert_eq!(f.quarters[3].shares, None);
    assert!(f.quarters[3].sales.value.is_some());
    missing(&f.pe, "too_few_quarters");
    for v in &mut c.values {
        v.fetched_on = c.today;
    }
    assert_eq!(c.run().pe.certainty, Certainty::Complete);
}

#[test]
fn consistency_and_implausible_bounds_control_only_price_ratios() {
    for (eps, expected, code) in [
        (dec!(1.03), Certainty::Complete, ""),
        (dec!(1.10), Certainty::Approximate, "figures_disagree"),
        (dec!(1.30), Certainty::Unavailable, "figures_inconsistent"),
    ] {
        let mut c = Case::new();
        for v in &mut c.values {
            if v.input.metric == Metric::DilutedEps {
                v.input.value = eps;
            }
        }
        let f = c.run();
        assert_eq!(f.pe.certainty, expected);
        assert_eq!(f.ps.certainty, expected);
        if !code.is_empty() {
            reason(&f.pe, code);
        }
        assert_eq!(f.profit_margin.certainty, Certainty::Complete);
    }
    let mut c = Case::new();
    c.price.close = dec!(0.1);
    missing(&c.run().pe, "implausible_ratio");
    missing(&c.run().ps, "implausible_ratio");
}

#[test]
fn missing_prices_and_invalid_latest_share_counts_never_become_zero_or_an_older_denominator() {
    for shares in [Decimal::ZERO, dec!(-100)] {
        let mut c = Case::new();
        c.values
            .iter_mut()
            .find(|v| {
                v.input.period_end == date("2026-09-30") && v.input.metric == Metric::DilutedShares
            })
            .unwrap()
            .input
            .value = shares;
        missing(&c.run().ps, "share_basis_unverified");
        assert_eq!(c.run().profit_margin.certainty, Certainty::Complete);
    }
    let c = Case::new();
    let f = compute(&Inputs {
        today: c.today,
        values: &c.values,
        splits: &[],
        price: None,
        evidence_price: c.evidence_price.as_ref(),
        fx: &[],
        source: &c.source,
        next_report: None,
    });
    missing(&f.pe, "price_unavailable");
    missing(&f.ps, "price_unavailable");
    assert_eq!(f.profit_margin.certainty, Certainty::Complete);
}

#[test]
fn classification_freshness_awaiting_and_outage_are_not_silent_guesses() {
    let mut c = Case::new();
    c.price.stale = true;
    reason(&c.run().pe, "price_stale");
    assert_eq!(c.run().profit_margin.certainty, Certainty::Complete);
    c.due = Some(ReportExpectation {
        date: c.today - Duration::days(22),
        basis: EstimateBasis::TypicalGap,
    });
    reason(&c.run().profit_margin, "newer_report_expected");
    c.source.unreachable = true;
    c.source.last_success = Some(date("2026-10-26"));
    reason(&c.run().pe, "source_unreachable");
    c.source.awaiting_figures = true;
    missing(&c.run().pe, "awaiting_figures");
    c.source.awaiting_figures = false;
    c.source.classification = Classification::Unknown;
    c.values.clear();
    missing(&c.run().pe, "security_type_unknown");
    c.source.classification = Classification::Fund;
    assert_eq!(c.run().pe.certainty, Certainty::NotApplicable);
    reason(&c.run().pe, "fund_or_etf");
    c.source.classification = Classification::Certificate;
    reason(&c.run().pe, "certificate");
    c.source.classification = Classification::NotCompany;
    reason(&c.run().pe, "not_a_company");
    c.source.classification = Classification::Equity;
    for (status, code) in [
        (LinkStatus::NoMatch, "no_fundamentals_source"),
        (LinkStatus::Ambiguous, "source_ambiguous"),
    ] {
        c.source.status = status;
        missing(&c.run().pe, code);
    }
}

#[test]
fn low_eps_consistency_has_one_rounding_allowance_for_the_whole_window() {
    let mut c = Case::new();
    for v in &mut c.values {
        match v.input.metric {
            Metric::DilutedNetIncome => v.input.value = dec!(10),
            Metric::DilutedEps => v.input.value = dec!(0.12),
            _ => {}
        }
    }
    let f = c.run();
    for figure in [f.pe, f.ps] {
        assert_eq!(figure.certainty, Certainty::Approximate);
        assert_eq!(
            figure.reasons,
            vec![Reason::detail("figures_disagree", "15.0%")]
        );
    }
    for v in &mut c.values {
        if v.input.metric == Metric::DilutedEps {
            v.input.value = dec!(0.12049);
        }
    }
    assert_eq!(c.run().pe.reasons[0].detail.as_deref(), Some("15.5%"));
    for v in &mut c.values {
        if v.input.metric == Metric::DilutedEps {
            v.input.value = dec!(0.14);
        }
    }
    let f = c.run();
    missing(&f.pe, "figures_inconsistent");
    assert_eq!(f.pe.reasons[0].detail.as_deref(), Some("35.0%"));
}

#[test]
fn reciprocal_share_ratios_have_bounded_precision() {
    let mut c = Case::new();
    c.values.last_mut().unwrap().input.value /= dec!(3);
    let f = c.run();
    assert_eq!(f.share_basis.state, ShareBasisState::ListingUnitMismatch);
    assert_eq!(f.share_basis.ratio, Some(dec!(0.3333)));
    assert_eq!(f.pe.reasons[0].detail.as_deref(), Some("0.3333"));
}

#[test]
fn a_post_split_quarter_clears_the_split_warning_without_post_split_evidence() {
    let mut c = Case::new();
    c.splits.push(date("2026-10-15"));
    c.values.last_mut().unwrap().input.period_end = date("2026-10-10");
    c.evidence_price.as_mut().unwrap().date = date("2026-10-10");
    missing(&c.run().pe, "split_since_report");
    for v in &mut c.values {
        if v.input.period_end == date("2026-09-30") {
            v.input.period_end = date("2026-10-31");
        }
    }
    let f = c.run();
    assert_eq!(f.share_basis.state, ShareBasisState::Established);
    assert_eq!(f.share_basis.evidence_date, Some(date("2026-10-10")));
    assert_eq!(f.pe.certainty, Certainty::Complete);
    assert!(!f.pe.reasons.iter().any(|r| r.code == "split_since_report"));
}

#[test]
fn matching_non_ecb_currencies_need_no_fx_conversion() {
    let mut c = Case::new();
    for v in &mut c.values {
        v.input.currency = Some("TWD".into());
    }
    c.price.currency = "TWD".into();
    c.evidence_price.as_mut().unwrap().currency = "TWD".into();
    let f = c.run();
    assert_eq!(f.pe.certainty, Certainty::Complete);
    assert_eq!(f.ps.certainty, Certainty::Complete);
    assert!(f.fx_conversion.is_none());
}

#[test]
fn short_history_displays_only_quarters_since_the_first_stored_period() {
    let mut c = Case::new();
    for end in ["2025-09-30", "2025-12-31", "2026-03-31"] {
        c.remove(end, None);
    }
    let f = c.run();
    assert_eq!(f.quarters.len(), 2);
    assert_eq!(f.quarters[0].period_end, date("2026-06-30"));
    assert!(f.quarters.iter().all(|q| !q.missing));
    missing(&f.pe, "too_few_quarters");
}

#[test]
fn nearby_period_ends_count_only_the_later_quarter_in_ttm() {
    let mut c = Case::new();
    let expected = c.run();
    let duplicates: Vec<_> = c
        .values
        .iter()
        .filter(|v| v.input.period_end == date("2026-09-30"))
        .cloned()
        .map(|mut v| {
            v.input.period_end = date("2026-09-25");
            v.input.value *= dec!(10);
            v
        })
        .collect();
    c.values.extend(duplicates);
    let f = c.run();
    assert_eq!(f.quarters.len(), 5);
    assert_eq!(f.pe, expected.pe);
    assert_eq!(f.ps, expected.ps);
    assert_eq!(f.profit_margin, expected.profit_margin);
    assert_eq!(f.sales_growth, expected.sales_growth);
}

#[test]
fn fx_note_is_present_only_when_a_dependent_ratio_is_available() {
    for failure in ["consistency", "bounds", "age", "coverage", "pe_only"] {
        let mut c = Case::new();
        for v in &mut c.values {
            if v.input.period_type == PeriodType::Quarter {
                v.input.currency = Some("EUR".into());
            }
            if failure == "pe_only" && v.input.metric == Metric::DilutedNetIncome {
                v.input.value = dec!(-100);
            }
            if v.input.metric == Metric::DilutedEps {
                v.input.value = match failure {
                    "consistency" => dec!(2),
                    "pe_only" => dec!(-1),
                    _ => v.input.value,
                };
            }
        }
        c.fx.push(FxRate {
            currency: "EUR".into(),
            date: c.today,
            sek_per_unit: dec!(11),
        });
        match failure {
            "bounds" => c.price.close = dec!(0.1),
            "age" => c.today += Duration::days(201),
            "coverage" => {
                c.remove("2026-03-31", None);
                c.remove("2026-06-30", None);
            }
            _ => {}
        }
        let f = c.run();
        assert_eq!(
            f.fx_conversion.is_some(),
            failure == "pe_only",
            "{failure}: {f:?}"
        );
    }
}

#[test]
fn transient_failures_keep_recent_figures_complete_until_the_backstop_expires() {
    let mut c = Case::new();
    c.source.unreachable = true;
    c.source.last_success = Some(c.today - Duration::days(7));
    assert_eq!(c.run().pe.certainty, Certainty::Complete);
    c.source.last_success = Some(c.today - Duration::days(8));
    let f = c.run();
    assert_eq!(f.pe.certainty, Certainty::Approximate);
    assert_eq!(
        f.pe.reasons,
        vec![Reason::detail("source_unreachable", "2026-10-27")]
    );
    c.values.clear();
    c.source.last_success = Some(c.today);
    reason(&c.run().pe, "source_unreachable");
}

#[test]
fn report_overdue_warnings_follow_the_origin_of_the_date() {
    for (basis, grace) in [
        (EstimateBasis::CompanyAnnounced, 7),
        (EstimateBasis::NasdaqExpected, 7),
        (EstimateBasis::LastYearDate, 21),
        (EstimateBasis::TypicalGap, 21),
    ] {
        let mut c = Case::new();
        c.due = Some(ReportExpectation {
            date: c.today - Duration::days(grace),
            basis,
        });
        assert_eq!(c.run().pe.certainty, Certainty::Complete, "{basis:?}");
        c.due.as_mut().unwrap().date -= Duration::days(1);
        for f in [
            c.run().pe,
            c.run().ps,
            c.run().profit_margin,
            c.run().sales_growth,
        ] {
            assert_eq!(f.certainty, Certainty::Approximate);
            reason(&f, "newer_report_expected");
        }
    }
}
