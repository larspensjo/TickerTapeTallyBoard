use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, OnceLock,
};

use chrono::{Duration, NaiveDate};
use sqlx::sqlite::SqlitePool;

use crate::{
    db::{market_data_runs, transactions},
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
const REFRESH_HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
#[derive(Clone)]
pub struct MarketDataService {
    inner: Arc<MarketDataServiceInner>,
}

struct MarketDataServiceInner {
    providers: ProviderSet,
    owner: String,
    stale_after: std::time::Duration,
    heartbeat_interval: std::time::Duration,
}

#[derive(Clone)]
pub(crate) struct RefreshLease {
    pub(crate) run_id: i64,
    pub(crate) owner: String,
    pub(crate) cancelled: Arc<AtomicBool>,
}

struct RefreshFlightGuard {
    heartbeat: Option<tokio::task::JoinHandle<()>>,
    pool: SqlitePool,
    run_id: i64,
    owner: String,
    clock: crate::clock::Clock,
    armed: bool,
}

impl RefreshFlightGuard {
    async fn stop_heartbeat(&mut self) {
        if let Some(heartbeat) = self.heartbeat.take() {
            heartbeat.abort();
            let _ = heartbeat.await;
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for RefreshFlightGuard {
    fn drop(&mut self) {
        if let Some(heartbeat) = self.heartbeat.take() {
            heartbeat.abort();
        }
        if !self.armed {
            return;
        }
        let pool = self.pool.clone();
        let run_id = self.run_id;
        let owner = self.owner.clone();
        let finished_at = self.clock.now_utc();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                match market_data_runs::cancel_run(&pool, run_id, &owner, finished_at).await {
                    Ok(market_data_runs::LeaseState::Held) => crate::engine_warn!(
                        "market data refresh cancelled and released run_id={} owner={}",
                        run_id,
                        owner
                    ),
                    Ok(market_data_runs::LeaseState::Lost) => crate::engine_warn!(
                        "market data refresh cancellation found lease lost run_id={} owner={}",
                        run_id,
                        owner
                    ),
                    Err(error) => crate::engine_warn!(
                        "market data refresh cancellation cleanup failed run_id={} owner={} error={}",
                        run_id,
                        owner,
                        error
                    ),
                }
            });
        }
    }
}

fn process_owner() -> &'static str {
    static OWNER: OnceLock<String> = OnceLock::new();
    OWNER.get_or_init(|| {
        let host = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "unknown-host".to_owned());
        format!(
            "{}:{}:{}",
            host,
            std::process::id(),
            crate::clock::now_utc().timestamp_millis()
        )
    })
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
        Self::with_provider_registry_and_lease(
            registry,
            process_owner().to_owned(),
            market_data_runs::REFRESH_CLAIM_STALE_AFTER,
            REFRESH_HEARTBEAT_INTERVAL,
        )
    }

    fn with_provider_registry_and_lease(
        registry: ProviderRegistry,
        owner: String,
        stale_after: std::time::Duration,
        heartbeat_interval: std::time::Duration,
    ) -> Self {
        Self {
            inner: Arc::new(MarketDataServiceInner {
                providers: ProviderSet::from_registry(registry),
                owner,
                stale_after,
                heartbeat_interval,
            }),
        }
    }

    pub async fn is_refreshing(
        &self,
        pool: &SqlitePool,
        clock: &crate::clock::Clock,
    ) -> Result<bool, MarketDataError> {
        Ok(
            market_data_runs::live_claim(pool, clock.now_utc(), self.inner.stale_after)
                .await?
                .is_some(),
        )
    }

    pub async fn active_run(
        &self,
        pool: &SqlitePool,
        clock: &crate::clock::Clock,
    ) -> Result<Option<RefreshRunSummary>, MarketDataError> {
        Ok(
            market_data_runs::live_claim(pool, clock.now_utc(), self.inner.stale_after)
                .await?
                .map(run_summary),
        )
    }

    pub async fn refresh(
        &self,
        pool: &SqlitePool,
        clock: &crate::clock::Clock,
        trigger: RefreshTrigger,
        request: RefreshPricesRequest,
    ) -> Result<RefreshPricesResponse, MarketDataError> {
        self.refresh_at(pool, clock, trigger, request).await
    }

    pub async fn refresh_at(
        &self,
        pool: &SqlitePool,
        clock: &crate::clock::Clock,
        trigger: RefreshTrigger,
        request: RefreshPricesRequest,
    ) -> Result<RefreshPricesResponse, MarketDataError> {
        let now = clock.now_utc();
        let claim = market_data_runs::try_claim_run(
            pool,
            trigger.as_db_str(),
            &self.inner.owner,
            now,
            self.inner.stale_after,
        )
        .await?;
        let run = match claim {
            market_data_runs::ClaimResult::Held(held) => {
                crate::engine_info!(
                "market data refresh already running; returning current status trigger={trigger:?} mode={:?}",
                request.mode
            );
                return Ok(self.running_response(held));
            }
            market_data_runs::ClaimResult::Claimed { run, reclaimed } => {
                for abandoned in reclaimed {
                    crate::engine_warn!("market data refresh reclaimed abandoned run abandoned_run_id={} previous_owner={} new_run_id={} new_owner={}", abandoned.id, abandoned.owner, run.id, self.inner.owner);
                }
                run
            }
        };
        let started_at = run.started_at.clone();
        let mut summary =
            RefreshRunSummary::running(run.id, trigger, request.mode, started_at.clone());
        crate::engine_info!(
            "market data refresh started run_id={} owner={} trigger={trigger:?} mode={:?}",
            run.id,
            self.inner.owner,
            request.mode
        );

        let cancelled = Arc::new(AtomicBool::new(false));
        let heartbeat_pool = pool.clone();
        let heartbeat_owner = self.inner.owner.clone();
        let heartbeat_cancelled = Arc::clone(&cancelled);
        let heartbeat_run_id = run.id;
        let heartbeat_clock = clock.clone();
        let stale_after = self.inner.stale_after;
        let heartbeat_interval = self.inner.heartbeat_interval;
        let heartbeat = tokio::spawn(async move {
            let mut interval = tokio::time::interval(heartbeat_interval);
            loop {
                interval.tick().await;
                match market_data_runs::heartbeat(
                    &heartbeat_pool,
                    heartbeat_run_id,
                    &heartbeat_owner,
                    heartbeat_clock.now_utc(),
                )
                .await
                {
                    Ok(market_data_runs::LeaseState::Held) => {}
                    Ok(market_data_runs::LeaseState::Lost) => {
                        heartbeat_cancelled.store(true, Ordering::Release);
                        let holder = market_data_runs::live_claim(
                            &heartbeat_pool,
                            heartbeat_clock.now_utc(),
                            stale_after,
                        )
                        .await
                        .ok()
                        .flatten()
                        .and_then(|row| row.claim_owner)
                        .unwrap_or_else(|| "none".to_owned());
                        crate::engine_warn!(
                            "market data refresh lease lost run_id={} owner={} current_holder={}",
                            heartbeat_run_id,
                            heartbeat_owner,
                            holder
                        );
                        break;
                    }
                    Err(error) => crate::engine_warn!(
                        "market data refresh heartbeat failed run_id={} owner={} error={}",
                        heartbeat_run_id,
                        heartbeat_owner,
                        error
                    ),
                }
            }
        });
        let mut flight = RefreshFlightGuard {
            heartbeat: Some(heartbeat),
            pool: pool.clone(),
            run_id: run.id,
            owner: self.inner.owner.clone(),
            clock: clock.clone(),
            armed: true,
        };
        let lease = RefreshLease {
            run_id: run.id,
            owner: self.inner.owner.clone(),
            cancelled,
        };
        let outcome = self
            .execute_refresh(pool, clock.today(), &request, &lease)
            .await;
        let (status, message) = match &outcome {
            Ok(outcome) => (outcome.status, outcome.message.clone()),
            Err(error) => (RefreshRunStatus::Failed, Some(error.to_string())),
        };
        let finished_at = clock.now_utc();
        let message_ref = message.as_deref();
        if let Ok(outcome) = &outcome {
            summary.prices_written = outcome.prices_written;
            summary.fx_rates_written = outcome.fx_rates_written;
            summary.unmapped_instruments = outcome.unmapped_instruments;
            summary.failed_items = outcome.failed_items;
        }
        flight.stop_heartbeat().await;
        let finish_result = market_data_runs::finish_run(
            pool,
            run.id,
            &self.inner.owner,
            finished_at,
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
        summary.finished_at = Some(finished_at.to_rfc3339());
        summary.message = message;
        let finish_state = finish_result?;
        flight.disarm();
        if finish_state == market_data_runs::LeaseState::Lost {
            summary.status = RefreshRunStatus::Failed;
            summary.message = Some("lease_lost".to_owned());
            crate::engine_warn!(
                "market data refresh lease lost at finish run_id={} owner={}",
                run.id,
                self.inner.owner
            );
        }
        crate::engine_info!(
            "market data refresh finished run_id={} owner={} trigger={trigger:?} mode={:?} status={:?} prices_written={} fx_rates_written={} unmapped_instruments={} failed_items={}",
            run.id,
            self.inner.owner,
            request.mode,
            summary.status,
            summary.prices_written,
            summary.fx_rates_written,
            summary.unmapped_instruments,
            summary.failed_items
        );
        crate::engine_info!(
            "market data refresh released run_id={} owner={}",
            run.id,
            self.inner.owner
        );

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
        clock: &crate::clock::Clock,
    ) -> Result<PriceStatusResponse, MarketDataError> {
        let active = self.active_run(pool, clock).await?;
        super::price_status::price_status(pool, clock.today(), active.is_some(), active).await
    }

    pub async fn lookup_symbol_search(&self, query: &str) -> SymbolSearchLookupResponse {
        super::symbol_search_lookup::lookup_symbol_search(&self.inner.providers, query).await
    }

    fn running_response(&self, row: market_data_runs::RefreshRunRow) -> RefreshPricesResponse {
        let active = run_summary(row);
        RefreshPricesResponse {
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
        }
    }

    async fn execute_refresh(
        &self,
        pool: &SqlitePool,
        today: NaiveDate,
        request: &RefreshPricesRequest,
        lease: &RefreshLease,
    ) -> Result<RefreshOutcome, MarketDataError> {
        refresh_execution::execute_refresh(&self.inner.providers, pool, today, request, lease).await
    }
}

pub(super) fn run_summary(row: market_data_runs::RefreshRunRow) -> RefreshRunSummary {
    RefreshRunSummary {
        run_id: row.id,
        trigger: super::refresh_contract::refresh_trigger_from_db(&row.trigger),
        mode: super::refresh_contract::refresh_mode_from_trigger(&row.trigger),
        status: super::refresh_contract::refresh_status_from_db(&row.status),
        started_at: row.started_at,
        finished_at: row.finished_at,
        message: row.message,
        prices_written: row.prices_written as usize,
        fx_rates_written: row.fx_rates_written as usize,
        unmapped_instruments: row.unmapped_instruments as usize,
        failed_items: row.failed_items as usize,
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
