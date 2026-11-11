// Demo cannot enter this pass: api::prices::tests::refresh_endpoint_in_demo_mode_rejects_before_provider_call
// and app::composition::tests::launch_refresh_is_skipped_in_demo_mode cover both entry points.
use super::*;
use chrono::{Datelike, Duration, Weekday};

fn day() -> NaiveDate {
    "2026-11-11".parse().unwrap()
}
fn request() -> RefreshPricesRequest {
    RefreshPricesRequest {
        mode: RefreshMode::Latest,
        start_date: None,
        end_date: None,
    }
}
fn close(symbol: &str, date: NaiveDate) -> DailyClose {
    DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: symbol.into(),
        date,
        close: dec!(100),
        currency: "SEK".into(),
    }
}
fn weekdays(start: NaiveDate, end: NaiveDate) -> Vec<NaiveDate> {
    start
        .iter_days()
        .take_while(|date| *date <= end)
        .filter(|date| !matches!(date.weekday(), Weekday::Sat | Weekday::Sun))
        .collect()
}
async fn fixture() -> (SqlitePool, MarketDataService, FakePriceProvider, i64) {
    let price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let (pool, service) = test_state(price.clone(), FakeFxRateProvider::new()).await;
    let id = instrument(&pool, "TEST", "XSTO", "SEK").await;
    buy(&pool, id, "2025-01-01", 1, "100", "SEK", None).await;
    map_yahoo_symbol(&pool, id, "TEST.ST", true).await;
    (pool, service, price, id)
}
async fn seed(pool: &SqlitePool, id: i64, dates: &[NaiveDate], fetched_at: &str) {
    for date in dates {
        prices::upsert(
            pool,
            &prices::NewPrice {
                instrument_id: id,
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "TEST.ST".into(),
                date: *date,
                close: dec!(100),
                currency: "SEK".into(),
                fetched_at: fetched_at.into(),
            },
        )
        .await
        .unwrap();
    }
}
async fn refresh(
    pool: &SqlitePool,
    service: &MarketDataService,
    date: NaiveDate,
) -> RefreshPricesResponse {
    service
        .refresh(pool, &Clock::fixed(date), RefreshTrigger::Manual, request())
        .await
        .unwrap()
}
fn history_items(response: &RefreshPricesResponse) -> Vec<&RefreshItem> {
    response
        .items
        .iter()
        .filter(|item| item.kind == RefreshItemKind::History)
        .collect()
}
fn assert_range(call: &crate::providers::PriceHistoryRequest, today: NaiveDate) {
    assert_eq!(call.symbol, "TEST.ST");
    assert_eq!(call.start, today - Duration::days(365 + 30));
    assert_eq!(call.end, today);
}
async fn split(pool: &SqlitePool, id: i64) {
    sqlx::query("INSERT INTO fundamental_split_events (instrument_id, provider_symbol, split_date, numerator, denominator, updated_at) VALUES (?, 'TEST.ST', '2026-06-01', '2', '1', '2026-11-11T00:00:00Z')").bind(id).execute(pool).await.unwrap();
}

#[tokio::test]
async fn each_repairable_need_makes_one_request_for_year_plus_longest_window() {
    for need in ["short", "gap", "provider_split", "ledger_split"] {
        let (pool, service, price, id) = fixture().await;
        let mut dates = weekdays(day() - Duration::days(365), day());
        match need {
            "short" => {
                dates = (day() - Duration::days(365))
                    .iter_days()
                    .take(366)
                    .step_by(3)
                    .collect();
            }
            "gap" => dates.retain(|date| {
                *date < "2026-04-01".parse().unwrap() || *date > "2026-04-20".parse().unwrap()
            }),
            "provider_split" => split(&pool, id).await,
            "ledger_split" => {
                transactions::insert(
                    &pool,
                    &transactions::NewTransaction {
                        instrument_id: id,
                        kind: domain::TransactionKind::Split,
                        trade_date: "2026-06-01".parse().unwrap(),
                        quantity: 1,
                        price: None,
                        dividend_per_share: None,
                        currency: None,
                        fx_rate_to_base: None,
                        brokerage: None,
                        note: None,
                    },
                )
                .await
                .unwrap();
            }
            _ => unreachable!(),
        }
        seed(
            &pool,
            id,
            &dates,
            if need.ends_with("split") {
                "2026-01-01T00:00:00Z"
            } else {
                "2026-11-10T00:00:00Z"
            },
        )
        .await;
        price.push_response(Ok(vec![close("TEST.ST", day())]));
        price.push_response(Ok(vec![close("TEST.ST", day())]));
        let response = refresh(&pool, &service, day()).await;
        assert_eq!(price.calls().len(), 2, "{need}");
        assert_eq!(price.calls()[0].start, day() - Duration::days(37));
        assert_range(&price.calls()[1], day());
        assert_eq!(history_items(&response).len(), 1);
        assert_eq!(response.status, RefreshRunStatus::Succeeded);
        assert_eq!(response.prices_written, 1);
        assert_eq!(response.failed_items, 0);
    }
}

#[tokio::test]
async fn recently_listed_history_and_leading_gap_do_not_retry_as_days_advance() {
    let (pool, service, price, _) = fixture().await;
    for offset in [0, 0, 1, 2, 3, 10] {
        let today = day() + Duration::days(offset);
        price.push_response(Ok(vec![close("TEST.ST", today)]));
        if price.calls().is_empty() {
            price.push_response(Ok(vec![close("TEST.ST", day())]));
        }
        let result = refresh(&pool, &service, today).await;
        assert_eq!(
            history_items(&result).len(),
            usize::from(offset == 0 && price.calls().len() == 2)
        );
    }
    assert_eq!(price.calls().len(), 7); // Six latest requests and just one top-up.
    assert_range(&price.calls()[1], day());
}

#[tokio::test]
async fn new_gap_retries_but_shrinking_or_aged_out_needs_do_not() {
    let (pool, service, price, id) = fixture().await;
    let dates = weekdays(day() - Duration::days(350), day());
    seed(&pool, id, &dates, "2026-11-10T00:00:00Z").await;
    for offset in [0, 1, 2, 20] {
        let today = day() + Duration::days(offset);
        if offset == 2 {
            sqlx::query("DELETE FROM prices WHERE instrument_id=? AND date BETWEEN '2026-04-01' AND '2026-04-20'").bind(id).execute(&pool).await.unwrap();
        }
        if offset == 20 {
            // Repair a part of that gap. Its remaining sub-gaps are covered by the old need.
            seed(
                &pool,
                id,
                &["2026-04-10".parse().unwrap()],
                "2026-11-14T00:00:00Z",
            )
            .await;
            // Fill the forward dates so this test doesn't introduce an unrelated new gap.
            seed(&pool, id, &weekdays(day(), today), "2026-11-30T00:00:00Z").await;
        }
        price.push_response(Ok(vec![close("TEST.ST", today)]));
        if offset == 0 || offset == 2 {
            price.push_response(Ok(Vec::new()));
        }
        let result = refresh(&pool, &service, today).await;
        assert_eq!(
            history_items(&result).len(),
            usize::from(offset == 0 || offset == 2)
        );
    }
    assert_eq!(price.calls().len(), 6);
    assert_range(&price.calls()[1], day());
    assert_range(&price.calls()[4], day() + Duration::days(2));
}

#[tokio::test]
async fn failures_retry_only_on_a_later_local_day_and_never_change_price_outcome() {
    for reason in [
        ProviderMissingReason::ProviderError,
        ProviderMissingReason::RateLimited,
        ProviderMissingReason::NotListed,
        ProviderMissingReason::ProviderUnavailable,
    ] {
        let (pool, service, price, id) = fixture().await;
        // Establish the exact outcome without a top-up, using complete stored history.
        seed(
            &pool,
            id,
            &weekdays(day() - Duration::days(365), day()),
            "2026-11-10T00:00:00Z",
        )
        .await;
        price.push_response(Ok(vec![close("TEST.ST", day())]));
        let baseline = refresh(&pool, &service, day()).await;
        sqlx::query("DELETE FROM prices WHERE instrument_id=?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        for (index, offset) in [0, 0, 1].into_iter().enumerate() {
            let today = day() + Duration::days(offset);
            price.push_response(Ok(vec![close("TEST.ST", today)]));
            if index != 1 {
                price.push_response(Err(ProviderError::new("YAHOO", reason, "fake failure")));
            }
            let result = refresh(&pool, &service, today).await;
            assert_eq!(
                (
                    result.status,
                    result.prices_written,
                    result.fx_rates_written,
                    result.failed_items,
                    result.unmapped_instruments
                ),
                (
                    baseline.status,
                    baseline.prices_written,
                    baseline.fx_rates_written,
                    baseline.failed_items,
                    baseline.unmapped_instruments
                )
            );
            assert_eq!(history_items(&result).len(), usize::from(index != 1));
            if index != 1 {
                assert_eq!(
                    history_items(&result)[0].reason.as_deref(),
                    Some(reason.as_str())
                );
            }
        }
        assert_eq!(price.calls().len(), 6);
        assert_range(&price.calls()[2], day());
        assert_range(&price.calls()[5], day().succ_opt().unwrap());
    }
}

#[tokio::test]
async fn rate_limit_and_outage_stop_later_topups_even_when_bookkeeping_fails() {
    for reason in [
        ProviderMissingReason::RateLimited,
        ProviderMissingReason::ProviderUnavailable,
    ] {
        for fail_bookkeeping in [false, true] {
            let (pool, service, price, _) = fixture().await;
            if fail_bookkeeping {
                sqlx::query("CREATE TRIGGER fail_history_outcome BEFORE UPDATE ON history_topup_attempts WHEN NEW.outcome='FAILED' BEGIN SELECT RAISE(ABORT, 'fake bookkeeping failure'); END").execute(&pool).await.unwrap();
            }
            let second = instrument(&pool, "SECOND", "XSTO", "SEK").await;
            buy(&pool, second, "2025-01-01", 1, "100", "SEK", None).await;
            map_yahoo_symbol(&pool, second, "SECOND.ST", true).await;
            // Instruments are listed by exchange/symbol (SECOND then TEST).
            price.push_response(Ok(vec![close("SECOND.ST", day())]));
            price.push_response(Ok(vec![close("TEST.ST", day())]));
            price.push_response(Err(ProviderError::new("YAHOO", reason, "fake limit")));
            let result = refresh(&pool, &service, day()).await;
            assert_eq!(result.status, RefreshRunStatus::Succeeded);
            assert_eq!(result.prices_written, 2);
            assert_eq!(result.failed_items, 0);
            assert_eq!(history_items(&result).len(), 1);
            assert_eq!(
                history_items(&result)[0].reason.as_deref(),
                Some(if fail_bookkeeping {
                    "repository_error"
                } else {
                    reason.as_str()
                })
            );
            assert_eq!(price.calls().len(), 3);
            let attempts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_topup_attempts")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(attempts, 1);
            // The untouched mapping retains its attempt for another refresh today.
            price.push_response(Ok(vec![close("SECOND.ST", day())]));
            price.push_response(Ok(vec![close("TEST.ST", day())]));
            price.push_response(Ok(Vec::new()));
            let next = refresh(&pool, &service, day()).await;
            assert_eq!(history_items(&next).len(), 1);
            assert_eq!(history_items(&next)[0].symbol_or_pair, "TEST.ST");
            assert_eq!(price.calls().len(), 6);
        }
    }
}

#[tokio::test]
async fn backfill_and_zero_quantity_never_top_up() {
    for held in [true, false] {
        let (pool, service, price, id) = fixture().await;
        if !held {
            sell(&pool, id, "2026-01-02", 1, "100", "SEK", None).await;
            set_conviction(&pool, id, "HIGH").await;
        }
        price.push_response(Ok(vec![close("TEST.ST", day())]));
        let mut request = request();
        if held {
            request.mode = RefreshMode::Backfill;
            request.start_date = Some("2025-01-01".into());
        }
        let result = service
            .refresh(&pool, &Clock::fixed(day()), RefreshTrigger::Manual, request)
            .await
            .unwrap();
        assert!(history_items(&result).is_empty());
        assert_eq!(price.calls().len(), 1);
    }
}

#[tokio::test]
async fn split_on_unverified_source_is_never_a_topup_trigger() {
    let pool = db::testing::memory_pool().await;
    let id = instrument(&pool, "TEST", "XSTO", "SEK").await;
    buy(&pool, id, "2025-01-01", 1, "100", "SEK", None).await;
    map_nasdaq_symbol(&pool, id, "123", "SHARES", "SEK", true).await;
    seed(
        &pool,
        id,
        &weekdays(day() - Duration::days(365), day()),
        "2026-01-01T00:00:00Z",
    )
    .await;
    sqlx::query(
        "UPDATE prices SET provider='NASDAQ_NORDIC', provider_symbol='123' WHERE instrument_id=?",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    split(&pool, id).await;
    let nasdaq = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::NasdaqNordic,
        provider_symbol: "123".into(),
        ..close("123", day())
    }]));
    let service = multi_provider_service(
        silent_fake_price(),
        empty_search(MarketDataProvider::Yahoo),
        nasdaq.clone(),
        empty_search(MarketDataProvider::NasdaqNordic),
        FakeFxRateProvider::new(),
    );
    let result = refresh(&pool, &service, day()).await;
    assert!(history_items(&result).is_empty());
    assert_eq!(nasdaq.calls().len(), 1);
}

#[tokio::test]
async fn corrupt_snapshot_retries_explicitly_without_panicking() {
    let (pool, service, price, _) = fixture().await;
    for offset in [0, 1] {
        let today = day() + Duration::days(offset);
        price.push_response(Ok(vec![close("TEST.ST", today)]));
        price.push_response(Ok(Vec::new()));
        let result = refresh(&pool, &service, today).await;
        assert_eq!(history_items(&result).len(), 1);
        if offset == 0 {
            sqlx::query("UPDATE history_topup_attempts SET remaining_needs='broken JSON'")
                .execute(&pool)
                .await
                .unwrap();
        }
    }
    assert_eq!(price.calls().len(), 4);
}

#[tokio::test]
async fn changed_mapping_and_new_split_are_new_needs_after_success() {
    for change in ["mapping", "split"] {
        let (pool, service, price, id) = fixture().await;
        price.push_response(Ok(vec![close("TEST.ST", day())]));
        price.push_response(Ok(Vec::new()));
        refresh(&pool, &service, day()).await;
        if change == "mapping" {
            map_yahoo_symbol(&pool, id, "NEW.ST", true).await;
        } else {
            split(&pool, id).await;
        }
        let symbol = if change == "mapping" {
            "NEW.ST"
        } else {
            "TEST.ST"
        };
        // Make the split newer than the closes' fetches (a newly observed split).
        if change == "split" {
            sqlx::query(
                "UPDATE fundamental_split_events SET split_date='2026-11-12' WHERE instrument_id=?",
            )
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        }
        let tomorrow = day().succ_opt().unwrap();
        price.push_response(Ok(vec![close(symbol, tomorrow)]));
        price.push_response(Ok(Vec::new()));
        let result = refresh(&pool, &service, tomorrow).await;
        assert_eq!(history_items(&result).len(), 1, "{change}");
        assert_eq!(price.calls().len(), 4);
        assert_eq!(price.calls()[3].symbol, symbol);
        assert_eq!(price.calls()[3].start, tomorrow - Duration::days(395));
    }
}

#[derive(Clone)]
struct GatedHistory {
    fake: FakePriceProvider,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl PriceProvider for GatedHistory {
    async fn daily_history(
        &self,
        request: &crate::providers::PriceHistoryRequest,
    ) -> crate::providers::ProviderResult<Vec<DailyClose>> {
        if (request.end - request.start).num_days() > 37 {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.fake.daily_history(request).await
    }
}

async fn wait_for_history(gate: &GatedHistory) {
    tokio::time::timeout(std::time::Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
}

#[tokio::test]
async fn losing_lease_during_topup_cannot_write_or_continue_the_pass() {
    let (pool, observer, path) =
        super::lease_coordination::two_file_pools("history-topup-fence").await;
    for symbol in ["A", "B"] {
        let id = instrument(&pool, symbol, "XSTO", "SEK").await;
        buy(&pool, id, "2025-01-01", 1, "100", "SEK", None).await;
        map_yahoo_symbol(&pool, id, &format!("{symbol}.ST"), true).await;
    }
    let fake = FakePriceProvider::new();
    fake.push_response(Ok(vec![close("A.ST", day())]));
    fake.push_response(Ok(vec![close("B.ST", day())]));
    fake.push_response(Ok(vec![close("A.ST", "2026-01-02".parse().unwrap())]));
    let gated = GatedHistory {
        fake: fake.clone(),
        entered: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Notify::new()),
    };
    let service = MarketDataService::with_provider_registry_and_lease(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, gated.clone())
            .with_fx_provider(FakeFxRateProvider::new()),
        "history-owner".into(),
        std::time::Duration::from_secs(1),
        std::time::Duration::from_secs(3600),
    );
    let task_pool = pool.clone();
    let task = tokio::spawn(async move { refresh(&task_pool, &service, day()).await });
    wait_for_history(&gated).await;
    let claim = crate::db::market_data_runs::try_claim_run(
        &observer,
        "MANUAL",
        "new-owner",
        Clock::fixed(day()).now_utc() + Duration::minutes(1),
        std::time::Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert!(matches!(
        claim,
        crate::db::market_data_runs::ClaimResult::Claimed { .. }
    ));
    let revision = crate::data_revision::current(&observer).await.unwrap();
    gated.release.notify_one();
    let result = task.await.unwrap();
    assert_eq!(result.message.as_deref(), Some("lease_lost"));
    assert_eq!(fake.calls().len(), 3); // Two ordinary latest calls; only A's top-up.
    assert_eq!(fake.calls()[2].symbol, "A.ST");
    assert_eq!(fake.calls()[2].start, day() - Duration::days(395));
    let rows = prices::list(&observer).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.date == day().to_string()));
    let attempts: Vec<String> = sqlx::query_scalar("SELECT outcome FROM history_topup_attempts")
        .fetch_all(&observer)
        .await
        .unwrap();
    assert_eq!(attempts, ["PENDING"]);
    assert_eq!(
        crate::data_revision::current(&observer).await.unwrap(),
        revision
    );
    super::lease_coordination::close_file_pools(pool, observer, path).await;
}

#[tokio::test]
async fn successful_topup_is_visible_through_existing_finish_revision() {
    let (pool, observer, path) =
        super::lease_coordination::two_file_pools("history-topup-publication").await;
    let id = instrument(&pool, "TEST", "XSTO", "SEK").await;
    buy(&pool, id, "2025-01-01", 1, "100", "SEK", None).await;
    map_yahoo_symbol(&pool, id, "TEST.ST", true).await;
    let fake = FakePriceProvider::new();
    fake.push_response(Ok(vec![close("TEST.ST", day())]));
    fake.push_response(Ok(vec![close("TEST.ST", "2026-01-02".parse().unwrap())]));
    let gated = GatedHistory {
        fake,
        entered: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Notify::new()),
    };
    let service = MarketDataService::with_provider_registry(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, gated.clone())
            .with_fx_provider(FakeFxRateProvider::new()),
    );
    let before = crate::data_revision::current(&observer).await.unwrap();
    let task_pool = pool.clone();
    let task = tokio::spawn(async move { refresh(&task_pool, &service, day()).await });
    wait_for_history(&gated).await;
    let published_prices = crate::data_revision::current(&observer).await.unwrap();
    assert_ne!(before, published_prices);
    assert_eq!(prices::list(&observer).await.unwrap().len(), 1);
    gated.release.notify_one();
    let result = task.await.unwrap();
    assert_ne!(
        crate::data_revision::current(&observer).await.unwrap(),
        published_prices
    );
    assert_eq!(prices::list(&observer).await.unwrap().len(), 2);
    assert_eq!(result.prices_written, 1);
    assert_eq!(history_items(&result)[0].rows_written, 1);
    let attempt: (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT outcome, earliest_returned_date, remaining_needs FROM history_topup_attempts",
    )
    .fetch_one(&observer)
    .await
    .unwrap();
    assert_eq!(attempt.0, "SUCCEEDED");
    assert_eq!(attempt.1.as_deref(), Some("2026-01-02"));
    assert!(attempt.2.is_some());
    super::lease_coordination::close_file_pools(pool, observer, path).await;
}

#[tokio::test]
async fn price_write_failure_rolls_back_prices_and_success_snapshot_together() {
    let (pool, service, price, _) = fixture().await;
    sqlx::query("CREATE TRIGGER fail_history_row BEFORE INSERT ON prices WHEN NEW.date='2026-01-05' BEGIN SELECT RAISE(ABORT, 'fake write failure'); END").execute(&pool).await.unwrap();
    price.push_response(Ok(vec![close("TEST.ST", day())]));
    price.push_response(Ok(vec![
        close("TEST.ST", "2026-01-02".parse().unwrap()),
        close("TEST.ST", "2026-01-05".parse().unwrap()),
    ]));
    let result = refresh(&pool, &service, day()).await;
    assert_eq!(result.status, RefreshRunStatus::Succeeded);
    assert_eq!(result.prices_written, 1);
    assert_eq!(result.failed_items, 0);
    assert_eq!(history_items(&result)[0].status, RefreshItemStatus::Failed);
    assert_eq!(
        history_items(&result)[0].reason.as_deref(),
        Some("repository_error")
    );
    assert_eq!(prices::list(&pool).await.unwrap().len(), 1);
    let attempt: (String, Option<String>) =
        sqlx::query_as("SELECT outcome, remaining_needs FROM history_topup_attempts")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(attempt, ("FAILED".to_owned(), None));
    // The reserved attempt survives the failed result transaction: no second call today.
    price.push_response(Ok(vec![close("TEST.ST", day())]));
    assert!(history_items(&refresh(&pool, &service, day()).await).is_empty());
    assert_eq!(price.calls().len(), 3);
}

#[tokio::test]
async fn history_needs_are_evaluated_after_company_figures_complete() {
    use crate::domain::fundamentals::SplitEvent;
    use crate::providers::{yahoo_fundamentals::ListingDetails, FakeFundamentalsProvider};
    let (pool, _, price, id) = fixture().await;
    seed(
        &pool,
        id,
        &weekdays(day() - Duration::days(365), day()),
        "2026-01-01T00:00:00Z",
    )
    .await;
    price.push_response(Ok(vec![close("TEST.ST", day())]));
    price.push_response(Ok(Vec::new()));
    let figures = FakeFundamentalsProvider::new();
    figures.push_response(Ok(Vec::new()));
    figures.push_listing(Ok(ListingDetails {
        security_type: Some("EQUITY".into()),
        splits: vec![SplitEvent {
            date: "2026-06-01".parse().unwrap(),
            numerator: dec!(2),
            denominator: dec!(1),
        }],
    }));
    let release = Arc::new(tokio::sync::Notify::new());
    figures.block_next_call_on(release.clone());
    let service = MarketDataService::with_provider_registry(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, price.clone())
            .with_fx_provider(FakeFxRateProvider::new())
            .with_fundamentals_provider(figures.clone()),
    );
    let task_pool = pool.clone();
    let task = tokio::spawn(async move { refresh(&task_pool, &service, day()).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while figures.calls().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        price.calls().len(),
        1,
        "history must wait for company figures"
    );
    release.notify_one();
    let result = task.await.unwrap();
    assert_eq!(
        crate::db::fundamental_values::active_splits(&pool, id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(history_items(&result).len(), 1);
    assert_range(&price.calls()[1], day());
}

#[tokio::test]
async fn split_day_defers_only_unfixable_need_without_reserving_and_retries_tomorrow() {
    for other_need in [false, true] {
        let (pool, service, price, id) = fixture().await;
        let dates = weekdays(
            day() - Duration::days(if other_need { 160 } else { 365 }),
            day(),
        );
        seed(&pool, id, &dates, "2026-11-10T00:00:00Z").await;
        split(&pool, id).await;
        sqlx::query("UPDATE fundamental_split_events SET split_date=? WHERE instrument_id=?")
            .bind(day().to_string())
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        price.push_response(Ok(vec![close("TEST.ST", day())]));
        if other_need {
            price.push_response(Ok(dates
                .iter()
                .map(|date| close("TEST.ST", *date))
                .collect()));
        }
        let first = refresh(&pool, &service, day()).await;
        assert_eq!(history_items(&first).len(), usize::from(other_need));
        let snapshots: Vec<Option<String>> =
            sqlx::query_scalar("SELECT remaining_needs FROM history_topup_attempts")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(snapshots.len(), usize::from(other_need));
        assert!(snapshots
            .iter()
            .flatten()
            .all(|value| !value.contains("split_stale")));
        let tomorrow = day() + Duration::days(1);
        price.push_response(Ok(vec![close("TEST.ST", tomorrow)]));
        price.push_response(Ok(Vec::new()));
        let second = refresh(&pool, &service, tomorrow).await;
        assert_eq!(history_items(&second).len(), 1);
        assert_range(price.calls().last().unwrap(), tomorrow);
        assert_eq!(price.calls().len(), 3 + usize::from(other_need));
    }
}

#[tokio::test]
async fn canonical_symbol_and_out_of_range_rows_preserve_usable_history() {
    let (pool, service, price, id) = fixture().await;
    map_yahoo_symbol(&pool, id, "test.st", true).await;
    let in_range = day() - Duration::days(200);
    price.push_response(Ok(vec![close("TEST.ST", day())]));
    price.push_response(Ok(vec![
        close("TEST.ST", day() - Duration::days(396)),
        close("TEST.ST", in_range),
        close("TEST.ST", day() + Duration::days(1)),
    ]));
    let result = refresh(&pool, &service, day()).await;
    assert_eq!(history_items(&result)[0].status, RefreshItemStatus::Fetched);
    assert_eq!(history_items(&result)[0].rows_written, 1);
    assert_eq!(price.calls()[1].symbol, "test.st");
    let rows = prices::list(&pool).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.provider_symbol == "TEST.ST"));
    let attempt: (String, String) =
        sqlx::query_as("SELECT outcome, earliest_returned_date FROM history_topup_attempts")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(attempt, ("SUCCEEDED".into(), in_range.to_string()));
}

#[tokio::test]
async fn changed_mapping_finalizes_attempt_without_writing_stale_response() {
    let (pool, _, fake, id) = fixture().await;
    fake.push_response(Ok(vec![close("TEST.ST", day())]));
    fake.push_response(Ok(vec![close("TEST.ST", day() - Duration::days(200))]));
    let gated = GatedHistory {
        fake: fake.clone(),
        entered: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Notify::new()),
    };
    let service = MarketDataService::with_provider_registry(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, gated.clone())
            .with_fx_provider(FakeFxRateProvider::new()),
    );
    let task_pool = pool.clone();
    let task = tokio::spawn(async move { refresh(&task_pool, &service, day()).await });
    wait_for_history(&gated).await;
    map_yahoo_symbol(&pool, id, "NEW.ST", true).await;
    gated.release.notify_one();
    let result = task.await.unwrap();
    assert_eq!(
        history_items(&result)[0].reason.as_deref(),
        Some("mapping_changed")
    );
    assert_eq!(prices::list(&pool).await.unwrap().len(), 1);
    let attempt: (String, Option<String>) =
        sqlx::query_as("SELECT outcome, remaining_needs FROM history_topup_attempts")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(attempt, ("FAILED".into(), None));
}
