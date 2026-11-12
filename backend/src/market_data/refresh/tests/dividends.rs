use super::*;
use crate::{
    db::dividends::{self, StoredDividends},
    domain::opportunity_evidence::{dividend_check::went_ex_dividend, YesNoUnknown},
    providers::{DividendCapture, PriceHistory, YahooChartClient},
};
use chrono::Duration;

const BODY: &str =
    include_str!("../../../../tests/fixtures/market_data/yahoo_volv_b_st_dividend.json");

fn date(value: &str) -> NaiveDate {
    value.parse().unwrap()
}
fn history() -> PriceHistory {
    YahooChartClient::parse_response("VOLV-B.ST", BODY).unwrap()
}
fn malformed(field: &str) -> PriceHistory {
    let mut body: serde_json::Value = serde_json::from_str(BODY).unwrap();
    let event = body["chart"]["result"][0]["events"]["dividends"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    if field == "amount" {
        event.as_object_mut().unwrap().remove("amount");
    } else {
        event["date"] = serde_json::json!("unparsable");
    }
    YahooChartClient::parse_response("VOLV-B.ST", &body.to_string()).unwrap()
}
async fn fixture() -> (SqlitePool, MarketDataService, FakePriceProvider, i64) {
    let fake = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let (pool, service) = test_state(fake.clone(), FakeFxRateProvider::new()).await;
    let id = instrument(&pool, "VOLV-B", "XSTO", "SEK").await;
    buy(&pool, id, "2025-01-01", 1, "100", "SEK", None).await;
    map_yahoo_symbol(&pool, id, "VOLV-B.ST", true).await;
    (pool, service, fake, id)
}
async fn refresh(
    pool: &SqlitePool,
    service: &MarketDataService,
    mode: RefreshMode,
) -> RefreshPricesResponse {
    service
        .refresh(
            pool,
            &Clock::fixed(date("2026-04-30")),
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode,
                start_date: (mode == RefreshMode::Backfill).then(|| "2026-03-01".into()),
                end_date: (mode == RefreshMode::Backfill).then(|| "2026-04-30".into()),
            },
        )
        .await
        .unwrap()
}
async fn read(pool: &SqlitePool, id: i64) -> StoredDividends {
    dividends::read(
        pool,
        id,
        MarketDataProvider::Yahoo,
        date("2025-03-01"),
        date("2026-04-30"),
    )
    .await
    .unwrap()
}
fn valid_ranges(data: &StoredDividends) -> Vec<(NaiveDate, NaiveDate)> {
    data.coverage
        .iter()
        .filter(|range| range.valid)
        .map(|range| (range.start, range.end))
        .collect()
}
fn check(data: &StoredDividends, start: &str, end: &str) -> YesNoUnknown {
    went_ex_dividend(
        date(start),
        date(end),
        dec!(100),
        "SEK",
        data.source_reports_dividends,
        &data.events,
        &data.coverage,
    )
    .state
}
fn counts(response: &RefreshPricesResponse) -> (RefreshRunStatus, usize, usize, usize, usize) {
    (
        response.status,
        response.prices_written,
        response.fx_rates_written,
        response.unmapped_instruments,
        response.failed_items,
    )
}

#[tokio::test]
async fn missing_and_empty_chart_currency_preserve_price_failures() {
    for path in ["latest", "backfill", "topup"] {
        for currency in [None, Some("")] {
            let (pool, service, fake, id) = fixture().await;
            if path == "topup" {
                fake.push_response(Ok(vec![history().closes.last().unwrap().clone()]));
            }
            let mut body: serde_json::Value = serde_json::from_str(BODY).unwrap();
            if let Some(currency) = currency {
                body["chart"]["result"][0]["meta"]["currency"] = serde_json::json!(currency);
            } else {
                body["chart"]["result"][0]["meta"]
                    .as_object_mut()
                    .unwrap()
                    .remove("currency");
            }
            fake.push_history_response(YahooChartClient::parse_response(
                "VOLV-B.ST",
                &body.to_string(),
            ));
            let response = refresh(
                &pool,
                &service,
                if path == "backfill" {
                    RefreshMode::Backfill
                } else {
                    RefreshMode::Latest
                },
            )
            .await;
            let expected = if path == "topup" {
                (RefreshRunStatus::Succeeded, 1, 0, 0, 0)
            } else {
                (RefreshRunStatus::Failed, 0, 0, 0, 1)
            };
            assert_eq!(counts(&response), expected, "{path} {currency:?}");
            let item = response
                .items
                .iter()
                .find(|item| {
                    item.kind
                        == if path == "topup" {
                            RefreshItemKind::History
                        } else {
                            RefreshItemKind::Price
                        }
                })
                .unwrap();
            assert_eq!(item.status, RefreshItemStatus::Failed);
            assert_eq!(
                item.reason.as_deref(),
                Some(if currency.is_none() || path == "topup" {
                    "provider_error"
                } else {
                    "currency_mismatch"
                })
            );
            assert_eq!(item.rows_written, 0);
            assert!(response
                .items
                .iter()
                .all(|item| item.kind != RefreshItemKind::Dividend));
            assert_eq!(
                prices::list(&pool).await.unwrap().len(),
                usize::from(path == "topup")
            );
            let data = read(&pool, id).await;
            assert!(data.events.is_empty());
            assert!(data.coverage.is_empty());
            let run = db::market_data_runs::find(&pool, response.run_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(run.prices_written, expected.1 as i64);
            assert_eq!(run.failed_items, expected.4 as i64);
        }
    }
}

async fn capture_range(
    pool: &SqlitePool,
    service: &MarketDataService,
    fake: &FakePriceProvider,
    start: &str,
    end: &str,
    valid: bool,
) {
    let mut capture = history();
    capture.closes.truncate(1);
    capture.closes[0].date = date(end);
    capture.dividends = if valid {
        DividendCapture::Valid(vec![])
    } else {
        DividendCapture::Invalid {
            reason: "dividend_amount_invalid".into(),
        }
    };
    fake.push_history_response(Ok(capture));
    service
        .refresh(
            pool,
            &Clock::fixed(date(end)),
            RefreshTrigger::Backfill,
            RefreshPricesRequest {
                mode: RefreshMode::Backfill,
                start_date: Some(start.into()),
                end_date: Some(end.into()),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn overlapping_daily_captures_merge_for_each_outcome() {
    for valid in [true, false] {
        let (pool, service, fake, id) = fixture().await;
        for (start, end) in [
            ("2026-04-01", "2026-04-10"),
            ("2026-04-02", "2026-04-11"),
            ("2026-04-02", "2026-04-11"),
        ] {
            capture_range(&pool, &service, &fake, start, end, valid).await;
        }
        let data = read(&pool, id).await;
        assert_eq!(data.coverage.len(), 1);
        assert_eq!(
            (
                data.coverage[0].start,
                data.coverage[0].end,
                data.coverage[0].valid
            ),
            (date("2026-04-01"), date("2026-04-11"), valid)
        );
    }
}

#[tokio::test]
async fn adjacent_valid_captures_merge_without_crossing_uncaptured_or_failed_days() {
    for failed_gap in [false, true] {
        let (pool, service, fake, id) = fixture().await;
        capture_range(&pool, &service, &fake, "2026-04-01", "2026-04-03", true).await;
        capture_range(&pool, &service, &fake, "2026-04-07", "2026-04-10", true).await;
        if failed_gap {
            capture_range(&pool, &service, &fake, "2026-04-04", "2026-04-06", false).await;
        }
        capture_range(&pool, &service, &fake, "2026-04-04", "2026-04-05", true).await;
        let data = read(&pool, id).await;
        assert_eq!(
            valid_ranges(&data),
            vec![
                (date("2026-04-01"), date("2026-04-05")),
                (date("2026-04-07"), date("2026-04-10")),
            ]
        );
        assert_eq!(
            check(&data, "2026-04-01", "2026-04-10"),
            YesNoUnknown::Unknown
        );
        let failures: Vec<_> = data.coverage.iter().filter(|range| !range.valid).collect();
        assert_eq!(failures.len(), usize::from(failed_gap));
        if failed_gap {
            assert_eq!(
                (failures[0].start, failures[0].end),
                (date("2026-04-06"), date("2026-04-06"))
            );
        }
        // Filling the final uncovered day joins both sides, without inventing coverage.
        capture_range(&pool, &service, &fake, "2026-04-06", "2026-04-06", true).await;
        let data = read(&pool, id).await;
        assert_eq!(data.coverage.len(), 1);
        assert_eq!(
            valid_ranges(&data),
            vec![(date("2026-04-01"), date("2026-04-10"))]
        );
        assert_eq!(check(&data, "2026-04-01", "2026-04-10"), YesNoUnknown::No);
    }
}

#[tokio::test]
async fn valid_coverage_stops_at_latest_returned_close_in_every_close_path() {
    for path in ["latest", "backfill", "topup"] {
        let (pool, service, fake, id) = fixture().await;
        if path == "topup" {
            fake.push_response(Ok(vec![history().closes.last().unwrap().clone()]));
        }
        let mut capture = history();
        capture.closes.retain(|row| row.date <= date("2026-04-29"));
        fake.push_history_response(Ok(capture));
        refresh(
            &pool,
            &service,
            if path == "backfill" {
                RefreshMode::Backfill
            } else {
                RefreshMode::Latest
            },
        )
        .await;
        let calls = fake.calls();
        let request = &calls[usize::from(path == "topup")];
        let data = read(&pool, id).await;
        assert_eq!(
            valid_ranges(&data),
            vec![(request.start, date("2026-04-29"))]
        );
        let result = went_ex_dividend(
            date("2026-04-28"),
            date("2026-04-30"),
            dec!(100),
            "SEK",
            data.source_reports_dividends,
            &data.events,
            &data.coverage,
        );
        assert_eq!(result.state, YesNoUnknown::Unknown);
        assert_eq!(result.reasons[0].code, "dividend_coverage_missing");
    }
}

#[tokio::test]
async fn bounded_valid_capture_replaces_all_requested_events_but_only_clears_seen_failures() {
    let (pool, service, fake, id) = fixture().await;
    let mut initial = history();
    let DividendCapture::Valid(events) = &mut initial.dividends else {
        panic!("valid fixture");
    };
    events[0].ex_date = date("2026-04-30");
    fake.push_history_response(Ok(initial));
    refresh(&pool, &service, RefreshMode::Backfill).await;
    assert_eq!(read(&pool, id).await.events[0].ex_date, date("2026-04-30"));
    fake.push_history_response(Ok(malformed("amount")));
    refresh(&pool, &service, RefreshMode::Backfill).await;
    let mut bounded = history();
    bounded.closes.retain(|row| row.date < date("2026-04-30"));
    bounded.dividends = DividendCapture::Valid(vec![]);
    fake.push_history_response(Ok(bounded));
    refresh(&pool, &service, RefreshMode::Backfill).await;
    let data = read(&pool, id).await;
    assert!(data.events.is_empty());
    let invalid: Vec<_> = data
        .coverage
        .iter()
        .filter(|range| !range.valid)
        .map(|range| (range.start, range.end))
        .collect();
    assert_eq!(invalid, vec![(date("2026-04-30"), date("2026-04-30"))]);
    assert_eq!(check(&data, "2026-04-28", "2026-04-29"), YesNoUnknown::No);
    assert_eq!(
        check(&data, "2026-04-28", "2026-04-30"),
        YesNoUnknown::Unknown
    );
}

#[tokio::test]
async fn latest_and_backfill_replace_dividends_and_keep_invalid_capture_separate() {
    for mode in [RefreshMode::Latest, RefreshMode::Backfill] {
        for field in ["amount", "date"] {
            let (pool, service, fake, id) = fixture().await;
            fake.push_history_response(Ok(history()));
            let initial = refresh(&pool, &service, mode).await;
            assert_eq!(counts(&initial), (RefreshRunStatus::Succeeded, 32, 0, 0, 0));
            let before = read(&pool, id).await;
            assert_eq!(before.events.len(), 1);
            assert_eq!(
                check(&before, "2026-04-08", "2026-04-10"),
                YesNoUnknown::Yes
            );
            let call = fake.calls()[0].clone();
            assert_eq!(valid_ranges(&before), vec![(call.start, call.end)]);

            let mut invalid = malformed(field);
            invalid.closes[0].close = dec!(999);
            fake.push_history_response(Ok(invalid));
            let response = refresh(&pool, &service, mode).await;
            assert_eq!(counts(&response), counts(&initial));
            let price = response
                .items
                .iter()
                .find(|item| item.kind == RefreshItemKind::Price)
                .unwrap();
            assert_eq!(price.status, RefreshItemStatus::Fetched);
            assert_eq!(price.rows_written, 32);
            assert_eq!(prices::list(&pool).await.unwrap()[0].close, "999");
            let item = response
                .items
                .iter()
                .find(|item| item.kind == RefreshItemKind::Dividend)
                .unwrap();
            assert_eq!(item.status, RefreshItemStatus::Failed);
            assert_eq!(item.instrument_id, Some(id));
            assert_eq!(item.provider.as_deref(), Some("YAHOO"));
            assert_eq!(item.symbol_or_pair, "VOLV-B.ST");
            assert_eq!(item.rows_written, 0);
            assert_eq!(
                item.reason.as_deref(),
                Some(if field == "amount" {
                    "dividend_amount_invalid"
                } else {
                    "dividend_date_invalid"
                })
            );
            assert_eq!(serde_json::to_value(item).unwrap()["kind"], "dividend");
            let after = read(&pool, id).await;
            assert_eq!(after.events, before.events);
            assert_eq!(valid_ranges(&after), valid_ranges(&before));
            assert_eq!(
                check(&after, "2026-04-08", "2026-04-10"),
                YesNoUnknown::Unknown
            );
            assert!(after
                .coverage
                .iter()
                .any(|range| !range.valid && range.start == call.start && range.end == call.end));
            let run = db::market_data_runs::find(&pool, response.run_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(run.status, "SUCCEEDED");
            assert_eq!(run.prices_written, 32);
            assert_eq!(run.failed_items, 0);

            // A transport failure cannot clear either the earlier success or the validation failure.
            fake.push_response(Err(ProviderError::transport("YAHOO", "offline fixture")));
            refresh(&pool, &service, mode).await;
            let failed = read(&pool, id).await;
            assert_eq!(failed.events, after.events);
            assert_eq!(valid_ranges(&failed), valid_ranges(&after));
            assert_eq!(failed.coverage.len(), after.coverage.len());
            assert_eq!(prices::list(&pool).await.unwrap()[0].close, "999");

            // A successful empty capture is evidence of absence and removes the restated event.
            let mut empty = history();
            empty.dividends = DividendCapture::Valid(vec![]);
            fake.push_history_response(Ok(empty));
            let repaired = refresh(&pool, &service, mode).await;
            assert_eq!(counts(&repaired), counts(&initial));
            let repaired = read(&pool, id).await;
            assert!(repaired.events.is_empty());
            assert!(repaired.coverage.iter().all(|range| range.valid));
            assert_eq!(
                check(&repaired, "2026-04-08", "2026-04-10"),
                YesNoUnknown::No
            );
        }
    }
}

#[tokio::test]
async fn topup_stores_dividend_capture_with_history_without_changing_price_counts() {
    for invalid in [false, true] {
        let (pool, service, fake, id) = fixture().await;
        let latest = history().closes.last().unwrap().clone();
        fake.push_response(Ok(vec![latest]));
        fake.push_history_response(Ok(if invalid {
            malformed("date")
        } else {
            history()
        }));
        let response = refresh(&pool, &service, RefreshMode::Latest).await;
        assert_eq!(counts(&response), (RefreshRunStatus::Succeeded, 1, 0, 0, 0));
        assert_eq!(fake.calls().len(), 2);
        let topup = &fake.calls()[1];
        let data = read(&pool, id).await;
        assert_eq!(data.coverage.len(), 1);
        assert_eq!(data.coverage[0].start, topup.start);
        assert_eq!(data.coverage[0].end, topup.end);
        assert_eq!(data.coverage[0].valid, !invalid);
        assert_eq!(
            check(&data, "2026-04-08", "2026-04-10"),
            if invalid {
                YesNoUnknown::Unknown
            } else {
                YesNoUnknown::Yes
            }
        );
        let item = response
            .items
            .iter()
            .find(|item| item.kind == RefreshItemKind::History)
            .unwrap();
        assert_eq!(item.status, RefreshItemStatus::Fetched);
        assert_eq!(item.rows_written, 32);
        assert_eq!(prices::list(&pool).await.unwrap().len(), 32);
        assert_eq!(
            response
                .items
                .iter()
                .filter(|item| item.kind == RefreshItemKind::Dividend)
                .count(),
            usize::from(invalid)
        );
    }
}

#[tokio::test]
async fn nasdaq_only_has_no_dividend_coverage() {
    let fake = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
    let service = multi_provider_service(
        silent_fake_price(),
        empty_search(MarketDataProvider::Yahoo),
        fake.clone(),
        empty_search(MarketDataProvider::NasdaqNordic),
        FakeFxRateProvider::new(),
    );
    let pool = db::memory_pool().await.unwrap();
    let id = instrument(&pool, "TRACKER", "XSTO", "SEK").await;
    buy(&pool, id, "2025-01-01", 1, "100", "SEK", None).await;
    map_nasdaq_symbol(&pool, id, "TX1", "TRACKER_CERTIFICATES", "SEK", true).await;
    let mut rows = history().closes;
    for row in &mut rows {
        row.provider = MarketDataProvider::NasdaqNordic;
        row.provider_symbol = "TX1".into();
    }
    fake.push_response(Ok(rows.clone()));
    fake.push_response(Ok(rows));
    let response = refresh(&pool, &service, RefreshMode::Latest).await;
    assert_eq!(response.status, RefreshRunStatus::Succeeded);
    assert_eq!(fake.calls().len(), 2);
    let data = dividends::read(
        &pool,
        id,
        MarketDataProvider::NasdaqNordic,
        date("2025-01-01"),
        date("2026-04-30"),
    )
    .await
    .unwrap();
    assert!(!data.source_reports_dividends);
    assert!(data.coverage.is_empty());
    assert!(data.events.is_empty());
    assert_eq!(
        check(&data, "2026-04-08", "2026-04-10"),
        YesNoUnknown::Unknown
    );
}

#[tokio::test]
async fn a_valid_capture_clears_only_its_part_of_an_invalid_range() {
    let (pool, service, fake, id) = fixture().await;
    fake.push_history_response(Ok(history()));
    refresh(&pool, &service, RefreshMode::Backfill).await;
    fake.push_history_response(Ok(malformed("amount")));
    refresh(&pool, &service, RefreshMode::Backfill).await;
    fake.push_history_response(Ok(history()));
    service
        .refresh(
            &pool,
            &Clock::fixed(date("2026-04-30")),
            RefreshTrigger::Backfill,
            RefreshPricesRequest {
                mode: RefreshMode::Backfill,
                start_date: Some("2026-04-08".into()),
                end_date: Some("2026-04-10".into()),
            },
        )
        .await
        .unwrap();
    let data = read(&pool, id).await;
    assert_eq!(data.events.len(), 1);
    assert_eq!(check(&data, "2026-04-08", "2026-04-10"), YesNoUnknown::Yes);
    assert_eq!(
        check(&data, "2026-03-20", "2026-04-01"),
        YesNoUnknown::Unknown
    );
    assert_eq!(
        check(&data, "2026-04-15", "2026-04-20"),
        YesNoUnknown::Unknown
    );
    let invalid: Vec<_> = data
        .coverage
        .iter()
        .filter(|range| !range.valid)
        .map(|range| (range.start, range.end))
        .collect();
    assert_eq!(
        invalid,
        vec![
            (date("2026-03-01"), date("2026-04-07")),
            (date("2026-04-11"), date("2026-04-30"))
        ]
    );

    // Re-fetching a disjoint empty interval must retain the event on 9 April.
    let mut empty = history();
    empty.dividends = DividendCapture::Valid(vec![]);
    fake.push_history_response(Ok(empty));
    service
        .refresh(
            &pool,
            &Clock::fixed(date("2026-04-30")),
            RefreshTrigger::Backfill,
            RefreshPricesRequest {
                mode: RefreshMode::Backfill,
                start_date: Some("2026-04-11".into()),
                end_date: Some("2026-04-30".into()),
            },
        )
        .await
        .unwrap();
    let data = read(&pool, id).await;
    assert_eq!(data.events.len(), 1);
    assert_eq!(check(&data, "2026-04-15", "2026-04-20"), YesNoUnknown::No);
    assert_eq!(
        check(&data, "2026-03-20", "2026-04-01"),
        YesNoUnknown::Unknown
    );
    // The repository filters the selected source and requested interval.
    let data = dividends::read(
        &pool,
        id,
        MarketDataProvider::Yahoo,
        date("2026-04-11"),
        date("2026-04-30"),
    )
    .await
    .unwrap();
    assert!(data.events.is_empty());
    assert!(data.coverage.iter().all(|range| range.valid));
    let other_source = dividends::read(
        &pool,
        id,
        MarketDataProvider::NasdaqNordic,
        date("2026-03-01"),
        date("2026-04-30"),
    )
    .await
    .unwrap();
    assert!(other_source.events.is_empty());
    assert!(other_source.coverage.is_empty());
    assert!(!other_source.source_reports_dividends);
}

#[derive(Clone)]
struct GatedCapture {
    fake: FakePriceProvider,
    call_number: usize,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
#[async_trait::async_trait]
impl crate::providers::PriceProvider for GatedCapture {
    async fn daily_history(
        &self,
        request: &crate::providers::PriceHistoryRequest,
    ) -> crate::providers::ProviderResult<PriceHistory> {
        let result = self.fake.daily_history(request).await;
        if self.fake.calls().len() == self.call_number {
            self.entered.notify_one();
            self.release.notified().await;
        }
        result
    }
}

#[tokio::test]
async fn lease_loss_discards_valid_and_invalid_dividends_in_every_close_path() {
    use super::lease_coordination::{close_file_pools, two_file_pools};
    use std::time::Duration as StdDuration;
    for path_kind in ["latest", "backfill", "topup"] {
        for invalid in [false, true] {
            let (pool, other_pool, path) =
                two_file_pools(&format!("dividend-{path_kind}-{invalid}")).await;
            let id = instrument(&pool, "VOLV-B", "XSTO", "SEK").await;
            buy(&pool, id, "2025-01-01", 1, "100", "SEK", None).await;
            map_yahoo_symbol(&pool, id, "VOLV-B.ST", true).await;
            let fake = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
            if path_kind == "topup" {
                fake.push_response(Ok(vec![history().closes.last().unwrap().clone()]));
            }
            let mut pending = if invalid {
                malformed("amount")
            } else {
                history()
            };
            pending.closes[0].close = dec!(999);
            fake.push_history_response(Ok(pending));
            let gated = GatedCapture {
                fake,
                call_number: if path_kind == "topup" { 2 } else { 1 },
                entered: Arc::new(tokio::sync::Notify::new()),
                release: Arc::new(tokio::sync::Notify::new()),
            };
            let service = MarketDataService::with_provider_registry_and_lease(
                ProviderRegistry::new()
                    .with_price_provider(MarketDataProvider::Yahoo, gated.clone())
                    .with_fx_provider(FakeFxRateProvider::new()),
                "dividend-owner".into(),
                StdDuration::from_millis(1),
                StdDuration::from_secs(3600),
            );
            let clock = Clock::fixed_instant("2026-04-30T12:00:00Z".parse().unwrap());
            let task = {
                let pool = pool.clone();
                let clock = clock.clone();
                tokio::spawn(async move {
                    service
                        .refresh(
                            &pool,
                            &clock,
                            RefreshTrigger::Manual,
                            RefreshPricesRequest {
                                mode: if path_kind == "backfill" {
                                    RefreshMode::Backfill
                                } else {
                                    RefreshMode::Latest
                                },
                                start_date: (path_kind == "backfill").then(|| "2026-03-01".into()),
                                end_date: (path_kind == "backfill").then(|| "2026-04-30".into()),
                            },
                        )
                        .await
                        .unwrap()
                })
            };
            tokio::time::timeout(StdDuration::from_secs(5), gated.entered.notified())
                .await
                .unwrap();
            clock.set_instant(clock.now_utc() + Duration::seconds(10));
            let replacement = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
            let mut rows = history().closes;
            for row in &mut rows {
                row.close = dec!(111);
            }
            replacement.push_response(Ok(rows));
            let successor = MarketDataService::with_provider_registry_and_lease(
                ProviderRegistry::new()
                    .with_price_provider(MarketDataProvider::Yahoo, replacement)
                    .with_fx_provider(FakeFxRateProvider::new()),
                "dividend-successor".into(),
                StdDuration::from_millis(1),
                StdDuration::from_secs(3600),
            );
            let response = successor
                .refresh(
                    &other_pool,
                    &clock,
                    RefreshTrigger::Backfill,
                    RefreshPricesRequest {
                        mode: RefreshMode::Backfill,
                        start_date: Some("2026-03-01".into()),
                        end_date: Some("2026-04-30".into()),
                    },
                )
                .await
                .unwrap();
            assert_eq!(response.status, RefreshRunStatus::Succeeded);
            gated.release.notify_one();
            let stale = task.await.unwrap();
            assert_eq!(stale.status, RefreshRunStatus::Failed);
            assert_eq!(stale.message.as_deref(), Some("lease_lost"));
            let data = read(&other_pool, id).await;
            assert!(data.events.is_empty());
            assert!(data.coverage.is_empty());
            assert!(prices::list(&other_pool)
                .await
                .unwrap()
                .iter()
                .all(|row| row.close == "111"));
            close_file_pools(pool, other_pool, path).await;
        }
    }
}
