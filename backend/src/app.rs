use std::sync::Arc;

use crate::{config::AppConfig, state::AppState};

pub async fn serve(config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let address = config.socket_addr();
    let pool = crate::db::connect(config.database_url()).await?;
    crate::engine_info!("database ready at {}", config.database_url());
    let state = AppState::new(
        pool,
        Arc::new(crate::market_data::MarketDataService::live()),
    );
    let _ = spawn_launch_refresh(&config, state.clone());
    let router = if config.static_assets_dir().is_dir() {
        crate::engine_info!(
            "serving frontend assets from {}",
            config.static_assets_dir().display()
        );
        crate::api::router_with_static_assets(config.static_assets_dir(), state)
    } else {
        crate::engine_warn!(
            "frontend assets not found at {}; serving backend routes only",
            config.static_assets_dir().display()
        );
        crate::api::router(state)
    };

    let listener = tokio::net::TcpListener::bind(address).await?;
    let local_address = listener.local_addr()?;

    crate::engine_info!("backend listening on {local_address}");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    crate::engine_info!("backend shutdown complete");

    Ok(())
}

fn spawn_launch_refresh(
    config: &AppConfig,
    state: AppState,
) -> Option<tokio::task::JoinHandle<()>> {
    if !config.market_data_refresh_enabled {
        crate::engine_info!(
            "market data refresh disabled by configuration; skipping launch refresh"
        );
        return None;
    }

    if !config.launch_refresh_enabled {
        crate::engine_info!("launch refresh disabled by configuration; skipping startup refresh");
        return None;
    }

    Some(tokio::spawn(async move {
        let request = crate::market_data::RefreshPricesRequest {
            mode: crate::market_data::RefreshMode::Latest,
            start_date: None,
            end_date: None,
        };

        if let Err(error) = state
            .market_data
            .refresh(
                &state.pool,
                crate::market_data::RefreshTrigger::Launch,
                request,
            )
            .await
        {
            crate::engine_error!("launch refresh failed: {error}");
        }
    }))
}

async fn shutdown_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => crate::engine_info!("shutdown signal received"),
        // Signal registration failures are terminal for this local server path.
        Err(error) => crate::engine_error!("failed to listen for shutdown signal: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use tokio::sync::Notify;

    use crate::{
        db::{self, instruments, provider_symbols, transactions},
        market_data::MarketDataService,
        providers::{
            DailyClose, FakeFxRateProvider, FakePriceProvider, FxProvider, FxRate,
            MarketDataProvider,
        },
        state::AppState,
    };

    async fn seeded_state() -> (AppState, FakePriceProvider, Arc<Notify>) {
        let pool = db::memory_pool().await.expect("memory pool");
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        let gate = Arc::new(Notify::new());

        price_provider.block_next_call_on(Arc::clone(&gate));
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
            close: dec!(101),
            currency: "USD".to_owned(),
        }]));
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
            rate: dec!(10.5),
        }]));

        let state = AppState::with_market_data(
            pool,
            MarketDataService::with_providers(price_provider.clone(), fx_provider),
        );
        let (instrument, _) = instruments::upsert(
            &state.pool,
            &crate::db::instruments::NewInstrument {
                symbol: "MSFT".to_owned(),
                exchange: "NASDAQ".to_owned(),
                name: "Microsoft".to_owned(),
                kind: "STOCK".to_owned(),
                currency: "USD".to_owned(),
                isin: None,
            },
        )
        .await
        .expect("instrument upsert should succeed");
        transactions::insert(
            &state.pool,
            &crate::db::transactions::NewTransaction {
                instrument_id: instrument.id,
                kind: crate::domain::TransactionKind::Buy,
                trade_date: NaiveDate::from_ymd_opt(2026, 6, 10).expect("date"),
                quantity: 10,
                price: Some(dec!(100)),
                currency: Some("USD".to_owned()),
                fx_rate_to_base: Some(dec!(10)),
                brokerage: None,
                note: None,
            },
        )
        .await
        .expect("transaction insert should succeed");

        provider_symbols::upsert(
            &state.pool,
            &crate::db::provider_symbols::NewProviderSymbol {
                instrument_id: instrument.id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "MSFT".to_owned(),
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: crate::import::now_iso8601(),
                updated_at: crate::import::now_iso8601(),
            },
        )
        .await
        .expect("provider symbol upsert should succeed");

        (state, price_provider, gate)
    }

    #[tokio::test]
    async fn launch_refresh_spawns_background_job() {
        let (state, price_provider, gate) = seeded_state().await;
        let config = AppConfig {
            market_data_refresh_enabled: true,
            launch_refresh_enabled: true,
            ..AppConfig::default()
        };

        let handle = spawn_launch_refresh(&config, state.clone())
            .expect("launch refresh should be scheduled");

        while price_provider.calls().is_empty() {
            tokio::task::yield_now().await;
        }

        let status = state
            .market_data
            .status(&state.pool)
            .await
            .expect("status should succeed");
        assert!(status.refreshing);
        assert_eq!(
            status.latest_run.expect("latest run").trigger,
            crate::market_data::RefreshTrigger::Launch
        );

        gate.notify_waiters();
        handle.await.expect("launch task should finish");

        let status = state
            .market_data
            .status(&state.pool)
            .await
            .expect("status should succeed");
        assert!(!status.refreshing);
        assert_eq!(
            status.latest_run.expect("latest run").status,
            crate::market_data::RefreshRunStatus::Succeeded
        );
    }

    #[tokio::test]
    async fn launch_refresh_is_skipped_when_disabled() {
        let (state, _, _) = seeded_state().await;
        let config = AppConfig {
            market_data_refresh_enabled: true,
            launch_refresh_enabled: false,
            ..AppConfig::default()
        };

        assert!(spawn_launch_refresh(&config, state).is_none());
    }
}
