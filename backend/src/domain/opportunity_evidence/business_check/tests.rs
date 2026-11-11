use super::*;
use crate::domain::{
    company_figures::{self, Certainty, Inputs, Price, ReportedValue},
    fundamentals::{FundamentalValue, Metric, PeriodType},
    opportunity_evidence::test_support::*,
};
use rust_decimal_macros::dec;

fn run(figures: &CompanyFigures) -> BusinessOutcome {
    business_check(today(), figures, &source(), &report(None, None))
}
fn set_eps(f: &mut CompanyFigures, index: usize, eps: Decimal) {
    f.quarters[index].eps.value = Some(eps);
    f.quarters[index].net_income.value = Some(eps * dec!(10));
}

#[test]
fn profitability_sales_and_earnings_have_independent_subtests() {
    let mut f = figures();
    let out = run(&f);
    assert_eq!(out.state, BusinessState::Intact);
    assert_eq!(out.profitable.passes, Some(true));
    assert_eq!(out.sales_not_shrinking.passes, Some(true));
    assert_eq!(out.earnings_not_sliding.passes, Some(true));
    f.profit_margin.value = Some(Decimal::ZERO);
    assert_eq!(run(&f).state, BusinessState::LossMaking);
    f.quarters[4].sales.value = Some(dec!(99));
    f.sales_growth.value = Some(dec!(-0.01));
    assert_eq!(run(&f).state, BusinessState::Weakening);
    assert_eq!(run(&f).sales_not_shrinking.passes, Some(false));
    f.profit_margin.value = Some(dec!(0.1));
    assert_eq!(run(&f).state, BusinessState::Weakening);
    f.quarters[4].sales.value = Some(dec!(100));
    f.sales_growth.value = Some(Decimal::ZERO);
    for (eps, expected) in [
        (dec!(0.90), BusinessState::Intact),
        (dec!(0.8999), BusinessState::Weakening),
        (dec!(-0.01), BusinessState::Weakening),
    ] {
        set_eps(&mut f, 4, eps);
        assert_eq!(run(&f).state, expected);
    }
}

#[test]
fn losses_that_do_not_deepen_pass_but_shrinking_sales_or_deepening_losses_weaken() {
    let mut f = figures();
    f.profit_margin.value = Some(dec!(-0.1));
    set_eps(&mut f, 0, dec!(-1));
    for (eps, expected) in [
        (dec!(-0.8), BusinessState::LossMaking),
        (dec!(-1.1), BusinessState::LossMaking),
        (dec!(-1.1001), BusinessState::Weakening),
    ] {
        set_eps(&mut f, 4, eps);
        assert_eq!(run(&f).state, expected);
    }
    set_eps(&mut f, 4, dec!(-0.8));
    f.quarters[4].sales.value = Some(dec!(99));
    f.sales_growth.value = Some(dec!(-0.01));
    assert_eq!(run(&f).state, BusinessState::Weakening);
    assert!(earnings_not_sliding(Decimal::ZERO, Decimal::ZERO));
    assert!(!earnings_not_sliding(dec!(-0.01), Decimal::ZERO));
}

#[test]
fn pair_consistency_carries_its_gap_without_valuation_dependencies() {
    let mut f = figures();
    for (reported, state, business, gap) in [
        (
            dec!(1.119),
            EpsConsistencyState::Consistent,
            BusinessState::Intact,
            dec!(4.95),
        ),
        (
            dec!(1.12),
            EpsConsistencyState::Approximate,
            BusinessState::Intact,
            dec!(5),
        ),
        (
            dec!(1.42),
            EpsConsistencyState::Approximate,
            BusinessState::Intact,
            dec!(20),
        ),
        (
            dec!(1.421),
            EpsConsistencyState::Inconsistent,
            BusinessState::Unknown,
            dec!(20.05),
        ),
    ] {
        f.quarters[4].eps.value = Some(reported);
        let out = run(&f);
        assert_eq!(out.eps_consistency.state, state);
        assert_eq!(out.eps_consistency.gap_percent, Some(gap));
        assert_eq!(out.state, business);
        if state == EpsConsistencyState::Approximate {
            assert!(out.earnings_not_sliding.approximate);
            assert!(out
                .earnings_not_sliding
                .reasons
                .iter()
                .any(|r| r.code == "eps_disagree"
                    && r.detail.as_deref().and_then(|s| s.parse::<Decimal>().ok()) == Some(gap)));
        }
        if state == EpsConsistencyState::Inconsistent {
            assert_eq!(out.earnings_not_sliding.passes, None);
            assert!(out.reasons.iter().any(|r| r.code == "eps_inconsistent"));
        }
    }
    // Profit/loss cancellation uses the summed quarter sizes, not the net sum.
    set_eps(&mut f, 0, dec!(-1));
    f.quarters[4].eps.value = Some(dec!(1.08));
    assert_eq!(
        run(&f).eps_consistency.state,
        EpsConsistencyState::Consistent
    );
}

#[test]
fn missing_cross_check_inputs_do_not_block_and_derived_eps_keeps_approximation() {
    for missing_shares in [true, false] {
        let mut f = figures();
        if missing_shares {
            f.quarters[0].shares = None;
        } else {
            f.quarters[4].net_income = Figure::unavailable("quarter_missing");
        }
        let out = run(&f);
        assert_eq!(out.state, BusinessState::Intact);
        assert_eq!(
            out.eps_consistency.state,
            EpsConsistencyState::NotCrossChecked
        );
        assert!(out
            .earnings_not_sliding
            .reasons
            .iter()
            .any(|r| r.code == "eps_not_cross_checked"));
    }
    let mut data = values();
    data.retain(|v| {
        !(v.input.metric == Metric::DilutedEps && v.input.period_end == date("2026-09-30"))
    });
    let f = company_figures::compute(&Inputs {
        today: today(),
        values: &data,
        splits: &[],
        price: None,
        evidence_price: None,
        fx: &[],
        source: &source(),
        next_report: None,
    });
    let out = run(&f);
    assert_eq!(out.state, BusinessState::Intact);
    assert!(out.earnings_not_sliding.approximate);
    assert!(out
        .earnings_not_sliding
        .reasons
        .iter()
        .any(|r| r.code == "eps_derived_from_net_income"));
}

#[test]
fn unusable_figures_override_known_subtest_failures() {
    for reason in [
        "latest_quarter_too_old",
        "report_currency_changed",
        "source_unreachable",
    ] {
        let mut f = figures();
        f.profit_margin = Figure::unavailable(reason);
        f.quarters[4].sales.value = Some(dec!(90));
        f.sales_growth.value = Some(dec!(-0.1));
        let out = run(&f);
        assert_eq!(out.sales_not_shrinking.passes, Some(false));
        assert_eq!(out.state, BusinessState::Unknown);
        assert!(out.reasons.iter().any(|r| r.code == reason));
    }
    let mut f = figures();
    f.quarters[0].currency = Some("EUR".into());
    assert!(run(&f)
        .reasons
        .iter()
        .any(|r| r.code == "report_currency_changed"));
    f = figures();
    f.quarters[4].eps = Figure::unavailable("quarter_missing");
    assert_eq!(run(&f).state, BusinessState::Unknown);
    f = figures();
    f.quarters.truncate(4);
    assert!(run(&f)
        .reasons
        .iter()
        .any(|r| r.code == "year_ago_quarter_missing"));
    let mut s = source();
    s.awaiting_figures = true;
    assert_eq!(
        business_check(today(), &figures(), &s, &report(None, None)).state,
        BusinessState::Unknown
    );
    s.awaiting_figures = false;
    for classification in [
        Classification::Fund,
        Classification::Certificate,
        Classification::NotCompany,
    ] {
        s.classification = classification;
        assert_eq!(
            business_check(today(), &figures(), &s, &report(None, None)).state,
            BusinessState::NotApplicable
        );
    }
    s.classification = Classification::Unknown;
    assert_eq!(
        business_check(today(), &figures(), &s, &report(None, None)).state,
        BusinessState::Unknown
    );
}

#[test]
fn current_known_negative_beats_missing_subtests_and_eps_disagreement() {
    let mut f = figures();
    f.sales_growth.value = Some(dec!(-0.1));
    f.profit_margin = Figure::unavailable("too_few_quarters");
    let out = run(&f);
    assert_eq!(out.state, BusinessState::Weakening);
    assert_eq!(out.profitable.passes, None);
    assert!(out.reasons.is_empty());
    f.quarters[4].eps.value = Some(dec!(1.421));
    let out = run(&f);
    assert_eq!(out.state, BusinessState::Weakening);
    assert_eq!(out.earnings_not_sliding.passes, None);
    assert!(out.reasons.is_empty());
    assert!(out
        .earnings_not_sliding
        .reasons
        .iter()
        .any(|r| r.code == "eps_inconsistent"));
    // Earnings alone establishes weakening, without either sales or profitability.
    for (prior, latest) in [
        (dec!(1), dec!(0.89)),
        (dec!(1), dec!(-0.01)),
        (dec!(-1), dec!(-1.11)),
    ] {
        let mut f = figures();
        f.sales_growth = Figure::unavailable("year_ago_quarter_missing");
        f.profit_margin = Figure::unavailable("too_few_quarters");
        set_eps(&mut f, 0, prior);
        set_eps(&mut f, 4, latest);
        assert_eq!(run(&f).state, BusinessState::Weakening);
    }
}

#[test]
fn known_negative_never_overrides_out_of_date_or_passed_report_or_source_blockers() {
    let mut f = figures();
    f.sales_growth.value = Some(dec!(-0.1));
    f.profit_margin
        .reasons
        .push(Reason::new("newer_report_expected"));
    assert_eq!(run(&f).state, BusinessState::Unknown);
    f.profit_margin.reasons.clear();
    let next = report(
        Some(today() - chrono::Duration::days(1)),
        Some(crate::domain::report_dates::EstimateBasis::TypicalGap),
    );
    assert_eq!(
        business_check(today(), &f, &source(), &next).state,
        BusinessState::Unknown
    );
    let mut s = source();
    s.awaiting_figures = true;
    assert_eq!(
        business_check(today(), &f, &s, &report(None, None)).state,
        BusinessState::Unknown
    );
    s.awaiting_figures = false;
    s.classification = Classification::Unknown;
    assert_eq!(
        business_check(today(), &f, &s, &report(None, None)).state,
        BusinessState::Unknown
    );
}

#[test]
fn missing_subtest_without_known_negative_stays_unknown() {
    for subtest in ["margin", "sales", "earnings"] {
        let mut f = figures();
        match subtest {
            "margin" => f.profit_margin = Figure::unavailable("too_few_quarters"),
            "sales" => f.sales_growth = Figure::unavailable("year_ago_quarter_missing"),
            _ => f.quarters[4].eps = Figure::unavailable("quarter_missing"),
        }
        assert_eq!(run(&f).state, BusinessState::Unknown);
    }
}

#[test]
fn cached_source_warning_does_not_make_available_current_figures_unusable() {
    let mut f = figures();
    f.profit_margin
        .reasons
        .push(Reason::new("source_unreachable"));
    assert_eq!(run(&f).state, BusinessState::Intact);
    f.sales_growth.value = Some(dec!(-0.1));
    assert_eq!(run(&f).state, BusinessState::Weakening);
}

#[test]
fn missing_latest_eps_does_not_blame_derived_year_ago_eps() {
    let mut f = figures();
    f.quarters[4].eps = Figure::unavailable("quarter_missing");
    f.quarters[0].eps =
        Figure::available(dec!(1), vec![Reason::new("eps_derived_from_net_income")]);
    let out = run(&f);
    assert_eq!(out.state, BusinessState::Unknown);
    assert_eq!(out.reasons, vec![Reason::new("quarter_missing")]);
    assert_eq!(out.earnings_not_sliding.passes, None);
    assert!(out.earnings_not_sliding.approximate);
    assert_eq!(
        out.earnings_not_sliding.reasons,
        vec![
            Reason::new("quarter_missing"),
            Reason::new("eps_derived_from_net_income")
        ]
    );
}

#[test]
fn severe_disagreement_does_not_blame_a_derived_eps_quarter() {
    let mut f = figures();
    f.quarters[4].eps.value = Some(dec!(1.421));
    f.quarters[0].eps =
        Figure::available(dec!(1), vec![Reason::new("eps_derived_from_net_income")]);
    let out = run(&f);
    assert_eq!(out.state, BusinessState::Unknown);
    assert_eq!(out.eps_consistency.state, EpsConsistencyState::Inconsistent);
    assert_eq!(out.reasons, vec![Reason::new("eps_inconsistent")]);
    assert_eq!(out.earnings_not_sliding.passes, None);
    assert!(out.earnings_not_sliding.approximate);
    assert_eq!(
        out.earnings_not_sliding.reasons,
        vec![
            Reason::new("eps_derived_from_net_income"),
            Reason::new("eps_inconsistent")
        ]
    );
}

#[test]
fn stale_quarter_availability_comes_from_company_figures() {
    for (age, expected) in [(200, BusinessState::Intact), (201, BusinessState::Unknown)] {
        let as_of = date("2026-09-30") + chrono::Duration::days(age);
        let f = company_figures::compute(&Inputs {
            today: as_of,
            values: &values(),
            splits: &[],
            price: None,
            evidence_price: None,
            fx: &[],
            source: &source(),
            next_report: None,
        });
        let out = business_check(as_of, &f, &source(), &report(None, None));
        assert_eq!(out.state, expected);
        assert_eq!(
            out.reasons,
            if age > 200 {
                vec![Reason::new("latest_quarter_too_old")]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn out_of_date_and_passed_expectations_block_even_inside_the_existing_grace_period() {
    let mut f = figures();
    f.profit_margin
        .reasons
        .push(Reason::new("newer_report_expected"));
    assert!(run(&f)
        .reasons
        .iter()
        .any(|r| r.code == "newer_report_expected"));
    f = figures();
    for basis in [
        crate::domain::report_dates::EstimateBasis::CompanyAnnounced,
        crate::domain::report_dates::EstimateBasis::NasdaqExpected,
        crate::domain::report_dates::EstimateBasis::LastYearDate,
        crate::domain::report_dates::EstimateBasis::TypicalGap,
    ] {
        let next = report(Some(date("2026-11-10")), Some(basis));
        assert!(!next.expectation().unwrap().is_overdue(today()));
        let out = business_check(today(), &f, &source(), &next);
        assert_eq!(out.state, BusinessState::Unknown);
        assert!(out
            .reasons
            .iter()
            .any(|r| r.code == "expected_report_passed"));
        let today_report = report(Some(today()), Some(basis));
        assert_eq!(
            business_check(today(), &f, &source(), &today_report).state,
            BusinessState::Intact
        );
    }
    f.profit_margin = Figure::available(dec!(0.1), vec![Reason::new("ttm_from_three_quarters")]);
    assert_eq!(run(&f).state, BusinessState::Intact);
    assert!(run(&f).profitable.approximate);
    assert_eq!(f.profit_margin.certainty, Certainty::Approximate);
}

#[test]
fn actual_loss_making_and_unverified_share_basis_leave_business_usable() {
    let price = Price {
        date: today(),
        close: dec!(100),
        currency: "SEK".into(),
        stale: false,
    };
    let s = source();
    let mut data = values();
    let compute = |data: &[ReportedValue]| {
        company_figures::compute(&Inputs {
            today: today(),
            values: data,
            splits: &[],
            price: Some(&price),
            evidence_price: Some(&price),
            fx: &[],
            source: &s,
            next_report: None,
        })
    };
    let f = compute(&data);
    assert_eq!(f.pe.reasons[0].code, "share_basis_unverified");
    assert_eq!(run(&f).state, BusinessState::Intact);
    for v in &mut data {
        if matches!(
            v.input.metric,
            Metric::DilutedEps | Metric::DilutedNetIncome
        ) {
            v.input.value = -v.input.value;
        }
    }
    data.push(ReportedValue {
        input: FundamentalValue {
            period_type: PeriodType::Evidence,
            period_end: today(),
            metric: Metric::MarketCap,
            value: dec!(1000),
            currency: Some("SEK".into()),
        },
        fetched_on: today(),
    });
    let f = compute(&data);
    assert_eq!(f.pe.reasons[0].code, "loss_making");
    assert_eq!(f.eps_consistency.state, EpsConsistencyState::Consistent);
    assert_eq!(run(&f).state, BusinessState::LossMaking);
    for reason in [
        "implausible_ratio",
        "split_since_report",
        "price_stale",
        "figures_inconsistent",
    ] {
        let mut f = figures();
        f.pe = Figure::unavailable(reason);
        f.ps = Figure::unavailable(reason);
        assert_eq!(run(&f).state, BusinessState::Intact);
    }
}

#[test]
fn trailing_consistency_is_reported_even_when_pe_cannot_be_computed() {
    let mut data = values();
    for v in &mut data {
        if v.input.metric == Metric::DilutedEps {
            v.input.value = dec!(2);
        }
    }
    let f = company_figures::compute(&Inputs {
        today: today(),
        values: &data,
        splits: &[],
        price: None,
        evidence_price: None,
        fx: &[],
        source: &source(),
        next_report: None,
    });
    assert_eq!(f.pe.reasons[0].code, "price_unavailable");
    assert_eq!(f.eps_consistency.state, EpsConsistencyState::Inconsistent);
    assert_eq!(f.eps_consistency.compared_quarters, 4);
    assert_eq!(f.eps_consistency.gap_percent, Some(dec!(99.5)));
    assert_eq!(run(&f).state, BusinessState::Unknown);
}

#[test]
fn unavailable_figures_cannot_pass_even_without_a_source_reason() {
    let mut f = figures();
    f.profit_margin.value = None;
    f.profit_margin.reasons.clear();
    assert_eq!(run(&f).state, BusinessState::Unknown);
}

#[test]
fn unrecognised_figure_reason_blocks_a_current_known_negative() {
    for available in [false, true] {
        let mut f = figures();
        f.sales_growth.value = Some(dec!(-0.1));
        f.profit_margin
            .reasons
            .push(Reason::new("future_staleness_rule"));
        if !available {
            f.profit_margin.value = None;
        }
        assert_eq!(run(&f).state, BusinessState::Unknown);
    }
}
