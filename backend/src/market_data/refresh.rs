use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use chrono::{Duration, NaiveDate};
use sqlx::sqlite::SqlitePool;

use crate::{
    db::{market_data_runs, transactions},
    import::now_iso8601,
    providers::{FxRateProvider, MarketDataProvider, PriceProvider, SymbolSearchProvider},
};

use super::provider_registry::ProviderSet;
use super::refresh_execution::{self, RefreshOutcome};

#[cfg(test)]
use crate::providers::ProviderError;

pub use super::provider_registry::ProviderRegistry;
pub(super) use super::refresh_execution::{
    group_transactions, position_for_instrument, RefreshTarget,
};

pub use super::refresh_contract::{
    InstrumentMarketDataStatus, MarketDataError, PriceSnapshotState, PriceSourceStatus,
    PriceStatusResponse, RefreshItem, RefreshItemKind, RefreshItemStatus, RefreshMode,
    RefreshPricesRequest, RefreshPricesResponse, RefreshRunStatus, RefreshRunSummary,
    RefreshTrigger, SnapshotStatus, SymbolSearchLookupMatch, SymbolSearchLookupResponse,
    SymbolSearchLookupStatus,
};

const LATEST_REFRESH_WINDOW_DAYS: i64 = 14;
#[derive(Clone)]
pub struct MarketDataService {
    inner: Arc<MarketDataServiceInner>,
}

struct MarketDataServiceInner {
    providers: ProviderSet,
    running: Arc<AtomicBool>,
    active: Arc<Mutex<Option<RefreshRunSummary>>>,
}

struct RefreshFlightGuard {
    running: Arc<AtomicBool>,
    active: Arc<Mutex<Option<RefreshRunSummary>>>,
}

impl RefreshFlightGuard {
    fn new(running: Arc<AtomicBool>, active: Arc<Mutex<Option<RefreshRunSummary>>>) -> Self {
        Self { running, active }
    }

    fn activate(&self, summary: RefreshRunSummary) {
        let mut active_run = self
            .active
            .lock()
            .expect("market-data active run mutex should not be poisoned");
        *active_run = Some(summary);
    }
}

impl Drop for RefreshFlightGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            *active = None;
        }
        self.running.store(false, Ordering::Release);
    }
}

impl MarketDataService {
    /// Nasdaq Nordic is registered unconditionally: there is no feature flag,
    /// so the default configuration exercises the multi-source path.
    pub fn live() -> Self {
        Self::with_provider_registry(
            ProviderRegistry::new()
                .with_price_provider(
                    MarketDataProvider::Yahoo,
                    crate::providers::YahooChartClient::new(),
                )
                .with_price_provider(
                    MarketDataProvider::NasdaqNordic,
                    crate::providers::NasdaqNordicClient::new(),
                )
                .with_symbol_search_provider(
                    MarketDataProvider::Yahoo,
                    crate::providers::YahooSearchClient::new(),
                )
                .with_symbol_search_provider(
                    MarketDataProvider::NasdaqNordic,
                    crate::providers::NasdaqNordicClient::new(),
                )
                .with_fx_provider(crate::providers::FrankfurterClient::new()),
        )
    }

    /// Single-provider test constructor: the price provider is registered under
    /// `YAHOO`, which is what the pre-multi-provider tests assume.
    pub fn with_providers<P, F>(price_provider: P, fx_provider: F) -> Self
    where
        P: PriceProvider + Send + Sync + 'static,
        F: FxRateProvider + Send + Sync + 'static,
    {
        Self::with_provider_registry(
            ProviderRegistry::new()
                .with_price_provider(MarketDataProvider::Yahoo, price_provider)
                .with_fx_provider(fx_provider),
        )
    }

    /// As [`Self::with_providers`], plus a `YAHOO`-keyed symbol search provider.
    pub fn with_symbol_search_providers<P, F, S>(
        price_provider: P,
        fx_provider: F,
        symbol_search_provider: S,
    ) -> Self
    where
        P: PriceProvider + Send + Sync + 'static,
        F: FxRateProvider + Send + Sync + 'static,
        S: SymbolSearchProvider + Send + Sync + 'static,
    {
        Self::with_provider_registry(
            ProviderRegistry::new()
                .with_price_provider(MarketDataProvider::Yahoo, price_provider)
                .with_symbol_search_provider(MarketDataProvider::Yahoo, symbol_search_provider)
                .with_fx_provider(fx_provider),
        )
    }

    pub fn with_provider_registry(registry: ProviderRegistry) -> Self {
        Self {
            inner: Arc::new(MarketDataServiceInner {
                providers: ProviderSet::from_registry(registry),
                running: Arc::new(AtomicBool::new(false)),
                active: Arc::new(Mutex::new(None)),
            }),
        }
    }

    pub fn is_refreshing(&self) -> bool {
        self.inner.running.load(Ordering::Acquire)
    }

    pub fn active_run(&self) -> Option<RefreshRunSummary> {
        self.inner
            .active
            .lock()
            .expect("market-data active run mutex should not be poisoned")
            .clone()
    }

    pub async fn refresh(
        &self,
        pool: &SqlitePool,
        today: NaiveDate,
        trigger: RefreshTrigger,
        request: RefreshPricesRequest,
    ) -> Result<RefreshPricesResponse, MarketDataError> {
        if self
            .inner
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            crate::engine_info!(
                "market data refresh already running; returning current status trigger={trigger:?} mode={:?}",
                request.mode
            );
            return self.running_response(pool).await;
        }

        let flight = RefreshFlightGuard::new(
            Arc::clone(&self.inner.running),
            Arc::clone(&self.inner.active),
        );
        let started_at = now_iso8601();
        let run = market_data_runs::start_run(pool, trigger.as_db_str(), &started_at)
            .await
            .map_err(MarketDataError::from)?;
        let mut summary =
            RefreshRunSummary::running(run.id, trigger, request.mode, started_at.clone());
        flight.activate(summary.clone());
        crate::engine_info!(
            "market data refresh started run_id={} trigger={trigger:?} mode={:?}",
            run.id,
            request.mode
        );

        let outcome = self.execute_refresh(pool, today, &request).await;
        let (status, message) = match &outcome {
            Ok(outcome) => (outcome.status, outcome.message.clone()),
            Err(error) => (RefreshRunStatus::Failed, Some(error.to_string())),
        };
        let finished_at = now_iso8601();
        let message_ref = message.as_deref();
        if let Ok(outcome) = &outcome {
            summary.prices_written = outcome.prices_written;
            summary.fx_rates_written = outcome.fx_rates_written;
            summary.unmapped_instruments = outcome.unmapped_instruments;
            summary.failed_items = outcome.failed_items;
        }
        {
            let mut active = self
                .inner
                .active
                .lock()
                .expect("market-data active run mutex should not be poisoned");
            *active = Some(summary.clone());
        }
        let finish_result = market_data_runs::finish_run(
            pool,
            run.id,
            &finished_at,
            status.as_db_str(),
            message_ref,
            market_data_runs::RefreshRunCounts {
                prices_written: summary.prices_written as i64,
                fx_rates_written: summary.fx_rates_written as i64,
                unmapped_instruments: summary.unmapped_instruments as i64,
                failed_items: summary.failed_items as i64,
            },
        )
        .await;

        summary.status = status;
        summary.finished_at = Some(finished_at);
        summary.message = message;
        finish_result?;
        crate::engine_info!(
            "market data refresh finished run_id={} trigger={trigger:?} mode={:?} status={status:?} prices_written={} fx_rates_written={} unmapped_instruments={} failed_items={}",
            run.id,
            request.mode,
            summary.prices_written,
            summary.fx_rates_written,
            summary.unmapped_instruments,
            summary.failed_items
        );
        drop(flight);

        match outcome {
            Ok(outcome) => Ok(RefreshPricesResponse {
                run_id: run.id,
                trigger,
                mode: request.mode,
                status: summary.status,
                started_at,
                finished_at: summary.finished_at,
                message: summary.message,
                prices_written: outcome.prices_written,
                fx_rates_written: outcome.fx_rates_written,
                unmapped_instruments: outcome.unmapped_instruments,
                failed_items: outcome.failed_items,
                items: outcome.items,
            }),
            Err(error) => Err(error),
        }
    }

    pub async fn status(
        &self,
        pool: &SqlitePool,
        today: NaiveDate,
    ) -> Result<PriceStatusResponse, MarketDataError> {
        super::price_status::price_status(pool, today, self.is_refreshing(), self.active_run())
            .await
    }

    pub async fn lookup_symbol_search(&self, query: &str) -> SymbolSearchLookupResponse {
        super::symbol_search_lookup::lookup_symbol_search(&self.inner.providers, query).await
    }

    async fn running_response(
        &self,
        _pool: &SqlitePool,
    ) -> Result<RefreshPricesResponse, MarketDataError> {
        if let Some(active) = self.active_run() {
            return Ok(RefreshPricesResponse {
                run_id: active.run_id,
                trigger: active.trigger,
                mode: active.mode,
                status: RefreshRunStatus::Running,
                started_at: active.started_at,
                finished_at: None,
                message: active.message,
                prices_written: active.prices_written,
                fx_rates_written: active.fx_rates_written,
                unmapped_instruments: active.unmapped_instruments,
                failed_items: active.failed_items,
                items: Vec::new(),
            });
        }

        Ok(RefreshPricesResponse {
            run_id: 0,
            trigger: RefreshTrigger::Manual,
            mode: RefreshMode::Latest,
            status: RefreshRunStatus::Running,
            started_at: now_iso8601(),
            finished_at: None,
            message: Some("refresh in progress".to_owned()),
            prices_written: 0,
            fx_rates_written: 0,
            unmapped_instruments: 0,
            failed_items: 0,
            items: Vec::new(),
        })
    }

    async fn execute_refresh(
        &self,
        pool: &SqlitePool,
        today: NaiveDate,
        request: &RefreshPricesRequest,
    ) -> Result<RefreshOutcome, MarketDataError> {
        refresh_execution::execute_refresh(&self.inner.providers, pool, today, request).await
    }
}

pub(crate) struct RefreshWindow {
    pub(crate) start: NaiveDate,
    pub(crate) end: NaiveDate,
}

pub(super) async fn refresh_window(
    request: &RefreshPricesRequest,
    pool: &SqlitePool,
    today: NaiveDate,
) -> Result<RefreshWindow, MarketDataError> {
    match request.mode {
        RefreshMode::Latest => {
            let end = today;
            Ok(RefreshWindow {
                start: end - Duration::days(LATEST_REFRESH_WINDOW_DAYS),
                end,
            })
        }
        RefreshMode::Backfill => {
            let end = match &request.end_date {
                Some(value) => parse_date("end_date", value)?,
                None => today,
            };

            let start = match &request.start_date {
                Some(value) => parse_date("start_date", value)?,
                None => earliest_transaction_date(pool).await?.ok_or_else(|| {
                    MarketDataError::invalid_request(
                        "missing_start_date",
                        "backfill mode requires a start date or at least one transaction",
                    )
                })?,
            };

            if start > end {
                return Err(MarketDataError::invalid_request(
                    "invalid_date_range",
                    format!("start_date {start} must be on or before end_date {end}"),
                ));
            }

            Ok(RefreshWindow { start, end })
        }
    }
}

async fn earliest_transaction_date(
    pool: &SqlitePool,
) -> Result<Option<NaiveDate>, MarketDataError> {
    let rows = transactions::all_for_holdings(pool).await?;
    let mut earliest: Option<NaiveDate> = None;
    for row in rows {
        let date = NaiveDate::parse_from_str(&row.trade_date, "%Y-%m-%d").map_err(|error| {
            MarketDataError::internal(format!(
                "bad stored trade date {:?}: {error}",
                row.trade_date
            ))
        })?;
        earliest = Some(match earliest {
            Some(existing) if existing <= date => existing,
            _ => date,
        });
    }
    Ok(earliest)
}

fn parse_date(field: &'static str, value: &str) -> Result<NaiveDate, MarketDataError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|error| {
        MarketDataError::invalid_request(
            "invalid_date",
            format!("{field} {:?} must be YYYY-MM-DD: {error}", value),
        )
    })
}

#[cfg(test)]
pub(crate) mod tests;
