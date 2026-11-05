use super::*;

use crate::market_data::effective_prices;
use crate::providers::{PriceHistoryRequest, PriceProvider, ProviderResult, YahooChartClient};
use chrono::{DateTime, Duration, Utc};

const VOLVO_CHART: &str =
    include_str!("../../../../tests/fixtures/fundamentals/yahoo_chart_volv_b_st.json");

#[derive(Clone, Default)]
struct FixturePriceProvider {
    calls: Arc<std::sync::Mutex<Vec<PriceHistoryRequest>>>,
}

#[async_trait::async_trait]
impl PriceProvider for FixturePriceProvider {
    async fn daily_history(
        &self,
        request: &PriceHistoryRequest,
    ) -> ProviderResult<Vec<DailyClose>> {
        self.calls.lock().expect("calls").push(request.clone());
        YahooChartClient::parse_response(&request.symbol, VOLVO_CHART, request.as_of)
    }
}

#[tokio::test]
async fn latest_refresh_keeps_the_previous_close_during_the_session_and_overwrites_after_close() {
    let pool = db::memory_pool().await.expect("memory pool");
    let provider = FixturePriceProvider::default();
    let service = MarketDataService::with_providers(provider.clone(), FakeFxRateProvider::new());
    let volvo = instrument(&pool, "VOLV-B", "STOCKHOLM", "SEK").await;
    map_yahoo_symbol(&pool, volvo, "VOLV-B.ST", true).await;
    buy(&pool, volvo, "2026-10-01", 10, "300", "SEK", None).await;
    let mid_session: DateTime<Utc> = "2026-11-04T16:00:00Z".parse().expect("instant");
    let clock = Clock::fixed_instant(mid_session);
    let request = || RefreshPricesRequest {
        mode: RefreshMode::Latest,
        start_date: None,
        end_date: None,
    };
    let response = service
        .refresh(&pool, &clock, RefreshTrigger::Manual, request())
        .await
        .expect("refresh");
    assert_eq!(response.status, RefreshRunStatus::Succeeded);
    assert_eq!(response.prices_written, 22);
    let stored = prices::list(&pool).await.expect("prices");
    assert!(stored
        .iter()
        .all(|row| row.date_value().expect("price date") < clock.today()));
    let latest = effective_prices::effective_latest_on_or_before(&pool, volvo, clock.today())
        .await
        .expect("latest close")
        .expect("previous close");
    assert_eq!(latest.date.to_string(), "2026-11-03");
    assert_eq!(latest.close.round_dp(2), dec!(308.10));

    // Pre-existing provisional rows are deliberately retained until a later
    // refresh can overwrite them with a final close.
    prices::upsert(
        &pool,
        &prices::NewPrice {
            instrument_id: volvo,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "VOLV-B.ST".to_owned(),
            date: clock.today(),
            close: dec!(1),
            currency: "SEK".to_owned(),
            fetched_at: now_iso8601(),
        },
    )
    .await
    .expect("existing provisional row");
    let response = service
        .refresh(&pool, &clock, RefreshTrigger::Manual, request())
        .await
        .expect("mid-session refresh");
    assert_eq!(response.status, RefreshRunStatus::Succeeded);
    let existing = prices::list(&pool)
        .await
        .expect("prices")
        .into_iter()
        .find(|row| row.date_value().expect("price date") == clock.today())
        .expect("existing row retained");
    assert_eq!(existing.close_decimal().expect("close"), dec!(1));

    let final_at =
        DateTime::from_timestamp(1793809800, 0).expect("session end") + Duration::minutes(31);
    clock.set_instant(final_at);
    let response = service
        .refresh(&pool, &clock, RefreshTrigger::Manual, request())
        .await
        .expect("post-close refresh");
    assert_eq!(response.status, RefreshRunStatus::Succeeded);
    assert_eq!(response.prices_written, 23);
    let latest = effective_prices::effective_latest_on_or_before(&pool, volvo, clock.today())
        .await
        .expect("latest close")
        .expect("final close");
    assert_eq!(latest.date.to_string(), "2026-11-04");
    assert_eq!(latest.close.round_dp(2), dec!(313.10));
    let calls = provider.calls.lock().expect("calls");
    assert_eq!(
        calls.iter().map(|call| call.as_of).collect::<Vec<_>>(),
        [mid_session, mid_session, final_at]
    );
}
