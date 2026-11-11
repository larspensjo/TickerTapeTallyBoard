use super::*;
use chrono::{Duration, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ticker_tape_tally_board_backend::providers::MarketDataProvider;
use ticker_tape_tally_board_backend::{
    api::CompanyFigureInputs,
    domain::{
        company_figures::{self, Inputs, ReportedValue, SourceState},
        fundamentals::{Classification, FundamentalValue, LinkStatus, Metric, PeriodType},
        report_dates::{self, EstimateBasis, Observation, ReportCertainty},
    },
    market_data::effective_prices::PriceSourceMapping,
};

fn d(date: &str) -> NaiveDate {
    date.parse().unwrap()
}
fn loaded() -> CompanyFigureInputs {
    let today = d("2026-11-11");
    let source = SourceState {
        classification: Classification::Equity,
        status: LinkStatus::Usable,
        awaiting_figures: false,
        unreachable: false,
        last_success: None,
    };
    let mut values = Vec::new();
    for end in [
        "2025-09-30",
        "2025-12-31",
        "2026-03-31",
        "2026-06-30",
        "2026-09-30",
    ] {
        for (metric, value) in [
            (Metric::Revenue, dec!(100)),
            (Metric::DilutedNetIncome, dec!(10)),
            (Metric::DilutedShares, dec!(10)),
            (Metric::DilutedEps, dec!(1)),
        ] {
            values.push(ReportedValue {
                input: FundamentalValue {
                    period_type: PeriodType::Quarter,
                    period_end: d(end),
                    metric,
                    value,
                    currency: Some("SEK".into()),
                },
                fetched_on: today,
            });
        }
    }
    let figures = company_figures::compute(&Inputs {
        today,
        values: &values,
        splits: &[],
        price: None,
        evidence_price: None,
        fx: &[],
        source: &source,
        next_report: None,
    });
    let latest = Some(d("2026-09-30"));
    CompanyFigureInputs {
        figures,
        next_report: report_dates::next_report(today, latest, &[], &[], &source),
        source,
        link: None,
        latest,
        annual: vec![],
    }
}
fn observation(first: &str, absent: Option<&str>) -> Observation {
    Observation {
        period_end: d("2026-09-30"),
        first_seen: d(first),
        absent_on: absent.map(d),
    }
}
fn outcome(
    loaded: &CompanyFigureInputs,
    observation: &Observation,
    relaxed: bool,
) -> reports::Outcome {
    reports::in_period(
        &reports::ReportInputs::new(
            d("2026-11-11"),
            loaded,
            std::slice::from_ref(observation),
            &[],
        ),
        RecentWindow::ThirtyDays,
        relaxed,
    )
}

#[test]
fn reports_use_intervals_and_upper_bounds_without_midpoint_substitution() {
    let mut loaded = loaded();
    assert_eq!(
        outcome(
            &loaded,
            &observation("2026-11-05", Some("2026-10-25")),
            false
        )
        .state,
        "yes"
    );
    assert_eq!(
        outcome(
            &loaded,
            &observation("2026-10-15", Some("2026-09-20")),
            false
        )
        .reason,
        "observed_interval_overlaps_window"
    );
    assert_eq!(
        outcome(&loaded, &observation("2026-10-15", None), false).reason,
        "first_fetch_without_baseline"
    );
    // The widened window begins Oct 5; an earlier upper bound and unended quarter suffice.
    assert_eq!(
        outcome(&loaded, &observation("2026-10-04", None), false).state,
        "no"
    );
    loaded.latest = Some(d("2026-06-30"));
    let old = Observation {
        period_end: d("2026-06-30"),
        first_seen: d("2026-08-01"),
        absent_on: None,
    };
    assert_eq!(
        outcome(&loaded, &old, false).reason,
        "next_quarter_ended_no_recent_check"
    );
    loaded.source.last_success = Some(d("2026-11-10"));
    assert_eq!(outcome(&loaded, &old, false).state, "no");
    loaded.source.last_success = Some(d("2026-11-09"));
    assert_eq!(outcome(&loaded, &old, false).state, "unknown");
}

#[test]
fn report_history_horizon_changes_only_the_relaxed_rule_at_fourteen_days() {
    let mut loaded = loaded();
    loaded.latest = Some(d("2026-06-30"));
    let observations = [Observation {
        period_end: d("2026-06-30"),
        first_seen: d("2026-08-01"),
        absent_on: None,
    }];
    let mut inputs = reports::ReportInputs::new(d("2026-11-11"), &loaded, &observations, &[]);
    inputs.app_next.basis = Some(EstimateBasis::LastYearDate);
    inputs.app_next.date = Some(d("2026-11-25"));
    assert_eq!(
        reports::in_period(&inputs, RecentWindow::ThirtyDays, false).state,
        "unknown"
    );
    assert_eq!(
        reports::in_period(&inputs, RecentWindow::ThirtyDays, true).state,
        "no"
    );
    inputs.app_next.date = Some(d("2026-11-24"));
    assert_eq!(
        reports::in_period(&inputs, RecentWindow::ThirtyDays, true).state,
        "unknown"
    );
    inputs.app_next.date = Some(d("2026-11-25"));
    inputs.app_next.basis = Some(EstimateBasis::TypicalGap);
    assert_eq!(
        reports::in_period(&inputs, RecentWindow::ThirtyDays, true).state,
        "unknown"
    );
}

#[test]
fn passed_schedules_preserve_certainty_and_today_does_not_count_as_a_past_report() {
    let mut loaded = loaded();
    let old = observation("2026-10-01", None);
    for basis in [
        EstimateBasis::CompanyAnnounced,
        EstimateBasis::NasdaqExpected,
        EstimateBasis::LastYearDate,
        EstimateBasis::TypicalGap,
    ] {
        loaded.next_report.date = Some(d("2026-11-10"));
        loaded.next_report.basis = Some(basis);
        let result = outcome(&loaded, &old, false);
        assert_eq!(
            result.state,
            if matches!(
                basis,
                EstimateBasis::CompanyAnnounced | EstimateBasis::NasdaqExpected
            ) {
                "yes"
            } else {
                "unknown"
            }
        );
    }
    loaded.next_report.date = Some(d("2026-11-11"));
    assert_eq!(outcome(&loaded, &old, false).state, "no");
    let observations = [old];
    let mut input = reports::ReportInputs::new(d("2026-11-11"), &loaded, &observations, &[]);
    input.last.certainty = ReportCertainty::Confirmed;
    input.last.date = Some(d("2026-11-10"));
    assert_eq!(
        reports::in_period(&input, RecentWindow::ThirtyDays, false).state,
        "yes"
    );
    input.last.date = Some(d("2026-11-11"));
    assert_eq!(
        reports::in_period(&input, RecentWindow::ThirtyDays, false).state,
        "unknown"
    );
}

#[test]
fn knowledge_keeps_wide_observations_and_missing_baselines_distinct() {
    let loaded = loaded();
    for (first, absent, expected) in [
        ("2026-11-10", Some("2026-11-03"), "observed_at_most_7_days"),
        ("2026-11-10", Some("2026-11-02"), "observed_wider"),
        ("2026-11-10", None, "first_fetch_without_baseline"),
    ] {
        let observations = [observation(first, absent)];
        assert_eq!(
            reports::knowledge(&reports::ReportInputs::new(
                d("2026-11-11"),
                &loaded,
                &observations,
                &[]
            )),
            expected
        );
    }
}

#[test]
fn business_is_independent_of_pe_and_loss_making_is_not_weakening() {
    let today = d("2026-11-11");
    let mut loaded = loaded();
    assert!(loaded.figures.pe.value.is_none());
    assert_eq!(business::evaluate(today, &loaded).state, "intact");
    loaded.figures.profit_margin.value = Some(dec!(-0.1));
    assert_eq!(business::evaluate(today, &loaded).state, "loss_making_only");
    loaded.figures.sales_growth.value = Some(dec!(-0.01));
    assert_eq!(business::evaluate(today, &loaded).state, "weakening");
    loaded.figures.sales_growth.value = Some(Decimal::ZERO);
    loaded.figures.quarters[4].eps.value = Some(dec!(-1.1));
    loaded.figures.quarters[4].net_income.value = Some(dec!(-11));
    loaded.figures.quarters[0].eps.value = Some(dec!(-1));
    loaded.figures.quarters[0].net_income.value = Some(dec!(-10));
    assert_eq!(business::evaluate(today, &loaded).state, "loss_making_only");
    loaded.figures.quarters[4].eps.value = Some(dec!(-1.11));
    loaded.figures.quarters[4].net_income.value = Some(dec!(-11.1));
    assert_eq!(business::evaluate(today, &loaded).state, "weakening");
    loaded.next_report.date = Some(today - Duration::days(1));
    assert!(business::evaluate(today, &loaded)
        .reasons
        .contains("expected_report_passed"));
}

#[test]
fn eps_pair_uses_rounding_allowance_absolute_quarter_scale_and_both_boundaries() {
    let mut loaded = loaded();
    let pair = |loaded: &CompanyFigureInputs| {
        business::eps_pair([&loaded.figures.quarters[4], &loaded.figures.quarters[0]])
    };
    for (reported, expected) in [
        (dec!(1.119), "consistent"),
        (dec!(1.12), "approximate"),
        (dec!(1.42), "approximate"),
        (dec!(1.421), "unknown_inconsistent"),
    ] {
        loaded.figures.quarters[4].eps.value = Some(reported);
        assert_eq!(pair(&loaded), expected);
    }
    assert!(business::evaluate(d("2026-11-11"), &loaded)
        .reasons
        .contains("eps_inconsistent"));
    loaded.figures.quarters[0].shares = None;
    assert_eq!(pair(&loaded), "not_cross_checked");
    assert_ne!(
        business::evaluate(d("2026-11-11"), &loaded).state,
        "unknown"
    );
    loaded.figures.quarters[0].shares = Some(dec!(10));
    loaded.figures.quarters[0].eps.value = Some(dec!(-1));
    loaded.figures.quarters[0].net_income.value = Some(dec!(-10));
    loaded.figures.quarters[4].eps.value = Some(dec!(1.08));
    assert_eq!(pair(&loaded), "consistent");
}

#[test]
fn history_gaps_include_the_start_and_preserve_holiday_shaped_short_gaps() {
    assert!(!history::has_gap(d("2026-01-01"), &[d("2026-01-09")]));
    assert!(history::has_gap(d("2026-01-01"), &[d("2026-01-12")]));
    assert!(!history::has_gap(d("2026-04-01"), &[d("2026-04-08")]));
    assert!(!history::has_gap(d("2026-12-23"), &[d("2026-12-30")]));
}

fn price(fetched: &str, provider: MarketDataProvider) -> prices::PriceRow {
    prices::PriceRow {
        id: 1,
        instrument_id: 1,
        provider,
        provider_symbol: "SYNTHETIC".into(),
        date: "2026-11-10".into(),
        close: "1".into(),
        currency: "SEK".into(),
        fetched_at: fetched.into(),
    }
}

#[test]
fn final_closes_use_utc_calendar_and_splits_require_refetched_yahoo_closes() {
    assert!(
        !history::final_close(&price("2026-11-10T23:59:59Z", MarketDataProvider::Yahoo)).unwrap()
    );
    assert!(
        history::final_close(&price("2026-11-11T00:00:00Z", MarketDataProvider::Yahoo)).unwrap()
    );
    assert!(!history::final_close(&price(
        "2026-11-11T00:30:00+02:00",
        MarketDataProvider::Yahoo
    ))
    .unwrap());
    let splits = [d("2026-11-10")];
    assert!(history::split_blocks(
        &[price("2026-11-09T23:59:59Z", MarketDataProvider::Yahoo)],
        &splits
    )
    .unwrap());
    assert!(history::split_blocks(
        &[price("2026-11-10T12:00:00Z", MarketDataProvider::Yahoo)],
        &splits
    )
    .unwrap());
    assert!(!history::split_blocks(
        &[price("2026-11-11T00:00:00Z", MarketDataProvider::Yahoo)],
        &splits
    )
    .unwrap());
    // The UTC date is still the split date despite the offset's next local date.
    assert!(history::split_blocks(
        &[price(
            "2026-11-11T00:30:00+02:00",
            MarketDataProvider::Yahoo
        )],
        &splits
    )
    .unwrap());
    assert!(history::split_blocks(
        &[price(
            "2026-11-11T12:00:00Z",
            MarketDataProvider::NasdaqNordic
        )],
        &splits
    )
    .unwrap());
}

#[test]
fn split_inventory_ignores_unused_prices_and_stops_decoding_at_the_first_blocker() {
    let splits = [d("2026-11-10")];
    let mut adjusted = price("2026-11-11T00:00:00Z", MarketDataProvider::Yahoo);
    adjusted.close = "malformed synthetic close".into();
    adjusted.date = "malformed synthetic date".into();
    assert!(!history::split_blocks(&[adjusted.clone()], &splits).unwrap());

    let mut malformed = adjusted;
    malformed.fetched_at = "malformed synthetic timestamp".into();
    assert!(!history::split_blocks(std::slice::from_ref(&malformed), &[]).unwrap());
    assert!(!history::split_blocks(&[], &splits).unwrap());
    // A needed timestamp remains an evaluation error if no earlier row blocks.
    assert!(history::split_blocks(std::slice::from_ref(&malformed), &splits).is_err());
    let stale = price("2026-11-10T12:00:00Z", MarketDataProvider::Yahoo);
    assert!(history::split_blocks(&[stale, malformed.clone()], &splits).unwrap());
    for provider in [
        MarketDataProvider::NasdaqNordic,
        MarketDataProvider::Manual,
        MarketDataProvider::TwelveData,
    ] {
        malformed.provider = provider;
        assert!(history::split_blocks(std::slice::from_ref(&malformed), &splits).unwrap());
    }
}

#[test]
fn markets_follow_enabled_source_before_currency_and_never_broker_tags() {
    let mapping = |symbol: &str, provider, currency: Option<&str>| PriceSourceMapping {
        provider,
        provider_symbol: symbol.into(),
        asset_class: None,
        currency: currency.map(str::to_owned),
    };
    for (symbol, expected) in [
        ("SYN.ST", "stockholm"),
        ("SYN.OL", "oslo"),
        ("SYN.CO", "copenhagen"),
        ("SYN.HE", "helsinki"),
        ("SYN.PA", "france"),
        ("SYN", "us"),
        ("SYN.ZZ", "other_or_unknown"),
    ] {
        assert_eq!(
            history::listing_market(&[mapping(symbol, MarketDataProvider::Yahoo, None)], "NOK"),
            expected
        );
    }
    assert_eq!(
        history::listing_market(
            &[mapping(
                "123",
                MarketDataProvider::NasdaqNordic,
                Some("EUR")
            )],
            "USD"
        ),
        "helsinki"
    );
    assert_eq!(history::listing_market(&[], "EUR"), "other_europe");
}

#[tokio::test]
async fn single_source_history_thresholds_do_not_borrow_other_currency_or_sources() {
    let pool = db::memory_pool().await.unwrap();
    let instrument = instruments::upsert(
        &pool,
        &instruments::NewInstrument {
            symbol: "SYNTHETIC".into(),
            exchange: "BROKER".into(),
            name: "Synthetic fixture".into(),
            kind: "STOCK".into(),
            currency: "SEK".into(),
            isin: None,
        },
    )
    .await
    .unwrap()
    .0;
    db::provider_symbols::upsert(
        &pool,
        &db::provider_symbols::NewProviderSymbol {
            instrument_id: instrument.id,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "SYNTHETIC.ST".into(),
            asset_class: None,
            currency: Some("SEK".into()),
            enabled: true,
            created_at: "2026-11-12T12:00:00Z".into(),
            updated_at: "2026-11-12T12:00:00Z".into(),
        },
    )
    .await
    .unwrap();
    let today = d("2026-11-11");
    for (count, leading_gap, internal_gap) in [
        (119, true, false),
        (120, true, false),
        (199, false, false),
        (200, false, false),
        (200, true, false),
        (220, true, false),
        (260, false, true),
    ] {
        sqlx::query("DELETE FROM prices")
            .execute(&pool)
            .await
            .unwrap();
        let mut dates = if leading_gap {
            use chrono::{Datelike, Weekday};
            let mut dates = (0..365)
                .map(|offset| today - Duration::days(offset))
                .filter(|date| !matches!(date.weekday(), Weekday::Sat | Weekday::Sun))
                .take(count as usize)
                .collect::<Vec<_>>();
            dates.reverse();
            dates
        } else {
            (0..count)
                .map(|index| {
                    today - Duration::days(365) + Duration::days(index * 365 / (count - 1))
                })
                .collect()
        };
        if internal_gap {
            dates.retain(|date| *date < d("2026-03-01") || *date > d("2026-03-16"));
        }
        let actual_count = dates.len();
        for date in dates {
            prices::upsert(
                &pool,
                &prices::NewPrice {
                    instrument_id: instrument.id,
                    provider: MarketDataProvider::Yahoo,
                    provider_symbol: "SYNTHETIC.ST".into(),
                    date,
                    close: dec!(1),
                    currency: "SEK".into(),
                    fetched_at: "2026-11-12T12:00:00Z".into(),
                },
            )
            .await
            .unwrap();
        }
        let latest = effective_prices::effective_latest_on_or_before(&pool, instrument.id, today)
            .await
            .unwrap()
            .unwrap();
        let snapshot = ticker_tape_tally_board_backend::domain::PriceSnapshot {
            date: latest.date,
            close: latest.close,
            currency: latest.currency,
            source: latest.source,
            freshness: ticker_tape_tally_board_backend::domain::DataFreshness::Fresh,
        };
        let mappings = effective_prices::enabled_price_sources(&pool, instrument.id)
            .await
            .unwrap();
        let mut table = Table::new(1, history::HISTORY_LABELS);
        history::measure_history(
            &pool,
            &instrument,
            today,
            Some(&snapshot),
            &mappings,
            &[],
            &mut table,
        )
        .await
        .unwrap();
        assert_eq!(
            table.counts["unusual_drop_history_ready"],
            usize::from(actual_count >= 120 && !internal_gap)
        );
        assert_eq!(
            table.counts["range_history_ready"],
            usize::from(actual_count >= 200 && !leading_gap && !internal_gap)
        );
        assert_eq!(
            table.counts["gap_over_limit"],
            usize::from(leading_gap || internal_gap)
        );
        assert_eq!(
            table.counts["at_least_200_with_gap"],
            usize::from(actual_count >= 200 && (leading_gap || internal_gap))
        );
        assert_eq!(
            table.counts["top_up_needed"],
            usize::from(actual_count < 200 || leading_gap || internal_gap)
        );
        if count == 200 && !leading_gap && !internal_gap {
            // A differently denominated close is missing, even though it has the same provider.
            sqlx::query(
                "UPDATE prices SET currency='USD' WHERE date=(SELECT MIN(date) FROM prices)",
            )
            .execute(&pool)
            .await
            .unwrap();
            let mut table = Table::new(1, history::HISTORY_LABELS);
            history::measure_history(
                &pool,
                &instrument,
                today,
                Some(&snapshot),
                &mappings,
                &[],
                &mut table,
            )
            .await
            .unwrap();
            assert_eq!(table.counts["120_to_199"], 1);
            assert_eq!(table.counts["currency_excluded"], 1);
            assert_eq!(table.counts["range_history_ready"], 0);
        }
    }
    pool.close().await;
}

#[tokio::test]
async fn historical_buy_more_counts_respect_splits_closing_and_same_day_order() {
    let pool = db::memory_pool().await.unwrap();
    let instrument = instruments::upsert(
        &pool,
        &instruments::NewInstrument {
            symbol: "SYNTHETIC".into(),
            exchange: "BROKER".into(),
            name: "Synthetic fixture".into(),
            kind: "STOCK".into(),
            currency: "SEK".into(),
            isin: None,
        },
    )
    .await
    .unwrap()
    .0;
    for (day, kind, quantity) in [
        ("2025-12-30", TransactionKind::Buy, 1),
        ("2025-12-30", TransactionKind::Buy, 1),
        ("2026-01-01", TransactionKind::Split, 2),
        ("2026-01-02", TransactionKind::Buy, 1),
        ("2026-01-02", TransactionKind::Sell, -5),
        ("2026-01-02", TransactionKind::Buy, 1),
        ("2026-01-02", TransactionKind::Sell, -1),
    ] {
        let is_split = kind == TransactionKind::Split;
        transactions::insert(
            &pool,
            &transactions::NewTransaction {
                instrument_id: instrument.id,
                kind,
                trade_date: d(day),
                quantity,
                price: (!is_split).then_some(dec!(1)),
                dividend_per_share: None,
                currency: (!is_split).then(|| "SEK".into()),
                fx_rate_to_base: (!is_split).then_some(dec!(1)),
                brokerage: None,
                note: None,
            },
        )
        .await
        .unwrap();
    }
    let summary = measure(&pool, &Clock::fixed(d("2026-11-11")))
        .await
        .unwrap();
    assert_eq!(summary.tables["R1 classification"].population, 0);
    assert_eq!(summary.tables["R9 buys year=2025"].population, 2);
    assert_eq!(summary.tables["R9 buys year=2025"].counts["buy_more"], 1);
    assert_eq!(summary.tables["R9 buys year=2026"].population, 2);
    assert_eq!(summary.tables["R9 buys year=2026"].counts["buy_more"], 1);
    pool.close().await;
}

#[test]
fn first_fetch_is_distinct_from_later_missing_baselines_under_both_rules() {
    let loaded = loaded();
    let mut observations = vec![observation("2026-11-05", None)];
    for earliest in ["2026-11-05", "2026-10-01"] {
        observations.push(Observation {
            period_end: d("2026-06-30"),
            first_seen: d(earliest),
            absent_on: None,
        });
        let input = reports::ReportInputs::new(d("2026-11-11"), &loaded, &observations, &[]);
        let reason = if earliest == "2026-11-05" {
            "first_fetch_without_baseline"
        } else {
            "figures_without_absence_baseline"
        };
        assert_eq!(reports::knowledge(&input), reason);
        for relaxed in [false, true] {
            assert_eq!(
                reports::in_period(&input, RecentWindow::ThirtyDays, relaxed).reason,
                reason
            );
        }
        observations.pop();
    }
    observations[0].absent_on = Some(d("2026-11-04"));
    let input = reports::ReportInputs::new(d("2026-11-11"), &loaded, &observations, &[]);
    assert!(!input.first_fetch_without_baseline());
}

#[test]
fn dividend_source_and_top_up_follow_the_actual_series_provider() {
    let yahoo = PriceSourceMapping {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "SYNTHETIC".into(),
        asset_class: None,
        currency: None,
    };
    let nasdaq = PriceSourceMapping {
        provider: MarketDataProvider::NasdaqNordic,
        ..yahoo.clone()
    };
    for source in ["MANUAL", "TWELVE_DATA"] {
        assert_eq!(history::dividend_source(Some(source), &[]), "other_source");
        assert_eq!(
            history::dividend_source(Some(source), std::slice::from_ref(&yahoo)),
            "other_source"
        );
    }
    assert_eq!(
        history::dividend_source(Some("NASDAQ_NORDIC"), &[]),
        "nasdaq_only"
    );
    assert_eq!(
        history::dividend_source(Some("NASDAQ_NORDIC"), std::slice::from_ref(&yahoo)),
        "nasdaq_priced_with_yahoo_mapping"
    );
    let mappings = [yahoo, nasdaq];
    assert_eq!(
        history::top_up_effect(Some("NASDAQ_NORDIC"), &mappings, true),
        "top_up_cannot_fix_nasdaq_split"
    );
    assert_eq!(
        history::top_up_effect(Some("YAHOO"), &mappings, true),
        "top_up_would_fix_if_provider_supplies_year"
    );
    assert_eq!(
        history::top_up_effect(Some("NASDAQ_NORDIC"), &mappings, false),
        "top_up_would_fix_if_provider_supplies_year"
    );
    assert_eq!(
        history::top_up_effect(Some("MANUAL"), &mappings, true),
        "top_up_cannot_fix_other_source_split"
    );
}

#[tokio::test]
async fn snapshot_date_uses_latest_completed_latest_mode_refresh_and_override() {
    let pool = db::memory_pool().await.unwrap();
    let clock = Clock::fixed(d("2026-11-11"));
    assert!(ledger_measurement_date(&pool, &clock, None)
        .await
        .unwrap_err()
        .contains("--as-of"));
    for (trigger, status, finished) in [
        ("LAUNCH", "SUCCEEDED", Some("2026-11-05T22:00:00Z")),
        ("MANUAL", "PARTIAL", Some("2026-11-08T22:00:00Z")),
        ("BACKFILL", "SUCCEEDED", Some("2026-11-10T22:00:00Z")),
        ("LAUNCH", "RUNNING", None),
    ] {
        sqlx::query("INSERT INTO market_data_refresh_runs (\"trigger\", started_at, status, finished_at) VALUES (?, 'synthetic', ?, ?)")
            .bind(trigger).bind(status).bind(finished).execute(&pool).await.unwrap();
    }
    assert_eq!(
        ledger_measurement_date(&pool, &clock, None).await.unwrap(),
        d("2026-11-08")
    );
    assert_eq!(
        ledger_measurement_date(&pool, &clock, Some(d("2026-11-03")))
            .await
            .unwrap(),
        d("2026-11-03")
    );
    let system = Clock::System;
    let expected = system.local_date("2026-11-08T22:00:00Z".parse().unwrap());
    assert_eq!(
        ledger_measurement_date(&pool, &system, None).await.unwrap(),
        expected
    );
    assert_eq!(
        date_offset(&clock, d("2026-11-08")),
        "Clock today minus measurement date: 3 days.\n"
    );
    assert_eq!(
        date_offset(&clock, d("2026-11-12")),
        "Clock today minus measurement date: -1 days.\n"
    );
    sqlx::query("UPDATE market_data_refresh_runs SET finished_at='private malformed value' WHERE \"trigger\"='MANUAL'").execute(&pool).await.unwrap();
    let error = ledger_measurement_date(&pool, &clock, None)
        .await
        .unwrap_err();
    assert_eq!(error, "Measurement failed: snapshot refresh date decoding.");
    assert_eq!(
        ledger_measurement_date(&pool, &clock, Some(d("2026-11-03")))
            .await
            .unwrap(),
        d("2026-11-03")
    );
    pool.close().await;
}
