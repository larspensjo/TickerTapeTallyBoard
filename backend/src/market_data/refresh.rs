use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use chrono::{Duration, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;

use crate::{
    db::{
        fx_rates, instruments, market_data_runs, prices, provider_symbols, transactions, RepoError,
    },
    domain,
    import::now_iso8601,
    providers::{
        FxRateProvider, PriceProvider, ProviderError, ProviderMissingReason, SymbolSearchMatch,
        SymbolSearchProvider,
    },
};

const LATEST_REFRESH_WINDOW_DAYS: i64 = 14;
const YAHOO_PROVIDER: &str = "YAHOO";
const FRANKFURTER_PROVIDER: &str = "FRANKFURTER";
const SEK: &str = "SEK";

#[derive(Clone)]
pub struct MarketDataService {
    inner: Arc<MarketDataServiceInner>,
}

struct MarketDataServiceInner {
    price_provider: Arc<dyn PriceProvider + Send + Sync>,
    fx_provider: Arc<dyn FxRateProvider + Send + Sync>,
    symbol_search_provider: Option<Arc<dyn SymbolSearchProvider + Send + Sync>>,
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
    pub fn live() -> Self {
        Self::from_providers(
            Arc::new(crate::providers::YahooChartClient::new()),
            Arc::new(crate::providers::FrankfurterClient::new()),
            Some(Arc::new(crate::providers::YahooSearchClient::new())),
        )
    }

    pub fn with_providers<P, F>(price_provider: P, fx_provider: F) -> Self
    where
        P: PriceProvider + Send + Sync + 'static,
        F: FxRateProvider + Send + Sync + 'static,
    {
        Self::from_providers(Arc::new(price_provider), Arc::new(fx_provider), None)
    }

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
        Self::from_providers(
            Arc::new(price_provider),
            Arc::new(fx_provider),
            Some(Arc::new(symbol_search_provider)),
        )
    }

    fn from_providers(
        price_provider: Arc<dyn PriceProvider + Send + Sync>,
        fx_provider: Arc<dyn FxRateProvider + Send + Sync>,
        symbol_search_provider: Option<Arc<dyn SymbolSearchProvider + Send + Sync>>,
    ) -> Self {
        Self {
            inner: Arc::new(MarketDataServiceInner {
                price_provider,
                fx_provider,
                symbol_search_provider,
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

        let outcome = self.execute_refresh(pool, &request).await;
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

    pub async fn status(&self, pool: &SqlitePool) -> Result<PriceStatusResponse, MarketDataError> {
        let latest_run = if self.is_refreshing() {
            self.active_run()
        } else {
            latest_run_summary(pool).await?
        };

        let instruments = instruments::list(pool).await?;
        let transactions = transactions::all_for_holdings(pool).await?;
        let grouped = group_transactions(transactions);
        let today = Utc::now().date_naive();

        let mut readiness = Vec::new();
        for instrument in instruments {
            let Some(rows) = grouped.get(&instrument.id) else {
                continue;
            };

            let ledger = rows
                .iter()
                .map(|row| row.to_ledger())
                .collect::<Result<Vec<_>, _>>()?;
            let position = domain::derive_position(&ledger).map_err(|error| {
                MarketDataError::internal(format!(
                    "inconsistent stored ledger for instrument {}: {error:?}",
                    instrument.id
                ))
            })?;

            let mapping =
                provider_symbols::find_by_instrument_provider(pool, instrument.id, YAHOO_PROVIDER)
                    .await?;

            let latest_price = if let Some(mapping) = mapping.as_ref() {
                latest_price_snapshot(
                    pool,
                    instrument.id,
                    mapping,
                    today,
                    instrument.currency.as_str(),
                )
                .await?
            } else {
                PriceSnapshotState::unmapped()
            };

            let latest_fx = latest_fx_snapshot(pool, &instrument.currency, today).await?;

            readiness.push(InstrumentMarketDataStatus {
                instrument_id: instrument.id,
                exchange: instrument.exchange,
                symbol: instrument.symbol,
                currency: instrument.currency,
                mapping_enabled: mapping.as_ref().is_some_and(|row| row.enabled),
                provider_symbol: mapping.as_ref().map(|row| row.provider_symbol.clone()),
                open_quantity: position.quantity,
                latest_price,
                latest_fx,
            });
        }

        Ok(PriceStatusResponse {
            refreshing: self.is_refreshing(),
            latest_run,
            instruments: readiness,
        })
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
        request: &RefreshPricesRequest,
    ) -> Result<RefreshOutcome, MarketDataError> {
        let target_window = refresh_window(request, pool).await?;
        let transactions = transactions::all_for_holdings(pool).await?;
        let grouped = group_transactions(transactions);
        let instruments = instruments::list(pool).await?;
        self.seed_provider_symbols(pool, &instruments).await?;

        let mut targets = Vec::new();
        for instrument in instruments {
            let Some(rows) = grouped.get(&instrument.id) else {
                continue;
            };
            let ledger = rows
                .iter()
                .map(|row| row.to_ledger())
                .collect::<Result<Vec<_>, _>>()?;
            let position = domain::derive_position(&ledger).map_err(|error| {
                MarketDataError::internal(format!(
                    "inconsistent stored ledger for instrument {}: {error:?}",
                    instrument.id
                ))
            })?;
            if request.mode == RefreshMode::Latest && position.quantity == 0 {
                continue;
            }

            let currency = instrument.currency.clone();
            targets.push(RefreshTarget {
                instrument,
                currency,
                provider_symbol: None,
            });
        }

        let mut prices_written = 0usize;
        let mut fx_rates_written = 0usize;
        let mut unmapped_instruments = 0usize;
        let mut failed_items = 0usize;
        let mut items = Vec::new();

        for target in &mut targets {
            let mapping = provider_symbols::find_by_instrument_provider(
                pool,
                target.instrument.id,
                YAHOO_PROVIDER,
            )
            .await?;
            target.provider_symbol = mapping
                .as_ref()
                .filter(|row| row.enabled)
                .map(|row| row.provider_symbol.clone());

            if target.provider_symbol.is_none() {
                unmapped_instruments += 1;
                items.push(RefreshItem {
                    kind: RefreshItemKind::Price,
                    instrument_id: Some(target.instrument.id),
                    symbol_or_pair: target.instrument.symbol.clone(),
                    status: RefreshItemStatus::Unmapped,
                    reason: Some("symbol_unmapped".to_owned()),
                    rows_written: 0,
                });
                continue;
            }

            let provider_symbol = target.provider_symbol.clone().expect("checked above");
            match self
                .inner
                .price_provider
                .daily_history(&provider_symbol, target_window.start, target_window.end)
                .await
            {
                Ok(rows) => {
                    if let Some(row) = rows.iter().find(|row| {
                        !row.currency
                            .trim()
                            .eq_ignore_ascii_case(target.instrument.currency.trim())
                    }) {
                        let now = now_iso8601();
                        provider_symbols::upsert(
                            pool,
                            &provider_symbols::NewProviderSymbol {
                                instrument_id: target.instrument.id,
                                provider: YAHOO_PROVIDER.to_owned(),
                                provider_symbol: provider_symbol.clone(),
                                currency: Some(target.instrument.currency.clone()),
                                enabled: false,
                                created_at: now.clone(),
                                updated_at: now,
                            },
                        )
                        .await?;
                        failed_items += 1;
                        crate::engine_warn!(
                            "market data refresh price currency mismatch instrument_id={} symbol={} expected_currency={} actual_currency={}; disabled mapping",
                            target.instrument.id,
                            provider_symbol,
                            target.instrument.currency,
                            row.currency
                        );
                        items.push(RefreshItem {
                            kind: RefreshItemKind::Price,
                            instrument_id: Some(target.instrument.id),
                            symbol_or_pair: provider_symbol,
                            status: RefreshItemStatus::Failed,
                            reason: Some("currency_mismatch".to_owned()),
                            rows_written: 0,
                        });
                        continue;
                    }

                    for row in &rows {
                        prices::upsert(
                            pool,
                            &prices::NewPrice {
                                instrument_id: target.instrument.id,
                                provider: YAHOO_PROVIDER.to_owned(),
                                provider_symbol: row.provider_symbol.clone(),
                                date: row.date,
                                close: row.close,
                                currency: row.currency.clone(),
                                fetched_at: now_iso8601(),
                            },
                        )
                        .await?;
                    }
                    prices_written += rows.len();
                    items.push(RefreshItem {
                        kind: RefreshItemKind::Price,
                        instrument_id: Some(target.instrument.id),
                        symbol_or_pair: provider_symbol,
                        status: RefreshItemStatus::Fetched,
                        reason: None,
                        rows_written: rows.len(),
                    });
                }
                Err(error) => {
                    failed_items += 1;
                    crate::engine_warn!(
                        "market data refresh price failure instrument_id={} symbol={} reason={} message={}",
                        target.instrument.id,
                        provider_symbol,
                        error.reason_code(),
                        error.message()
                    );
                    items.push(RefreshItem {
                        kind: RefreshItemKind::Price,
                        instrument_id: Some(target.instrument.id),
                        symbol_or_pair: provider_symbol,
                        status: provider_error_status(&error),
                        reason: Some(error.reason_code().to_owned()),
                        rows_written: 0,
                    });
                }
            }
        }

        let currencies = target_currencies(&targets);
        for currency in currencies {
            if currency.eq_ignore_ascii_case(SEK) {
                continue;
            }

            match self
                .inner
                .fx_provider
                .fx_history(&currency, SEK, target_window.start, target_window.end)
                .await
            {
                Ok(rows) => {
                    for row in &rows {
                        fx_rates::upsert(
                            pool,
                            &fx_rates::NewFxRate {
                                base: row.base.clone(),
                                quote: row.quote.clone(),
                                date: row.date,
                                rate: row.rate,
                                provider: FRANKFURTER_PROVIDER.to_owned(),
                                fetched_at: now_iso8601(),
                            },
                        )
                        .await?;
                    }
                    fx_rates_written += rows.len();
                    items.push(RefreshItem {
                        kind: RefreshItemKind::Fx,
                        instrument_id: None,
                        symbol_or_pair: format!("{currency}/{SEK}"),
                        status: RefreshItemStatus::Fetched,
                        reason: None,
                        rows_written: rows.len(),
                    });
                }
                Err(error) => {
                    failed_items += 1;
                    crate::engine_warn!(
                        "market data refresh fx failure pair={}/{} reason={} message={}",
                        currency,
                        SEK,
                        error.reason_code(),
                        error.message()
                    );
                    items.push(RefreshItem {
                        kind: RefreshItemKind::Fx,
                        instrument_id: None,
                        symbol_or_pair: format!("{currency}/{SEK}"),
                        status: provider_error_status(&error),
                        reason: Some(error.reason_code().to_owned()),
                        rows_written: 0,
                    });
                }
            }
        }

        let status = if failed_items == 0 && unmapped_instruments == 0 {
            RefreshRunStatus::Succeeded
        } else if prices_written == 0 && fx_rates_written == 0 {
            RefreshRunStatus::Failed
        } else {
            RefreshRunStatus::Partial
        };

        let message = Some(format!(
            "prices_written={prices_written} fx_rates_written={fx_rates_written} unmapped={unmapped_instruments} failed={failed_items}"
        ));

        Ok(RefreshOutcome {
            status,
            message,
            prices_written,
            fx_rates_written,
            unmapped_instruments,
            failed_items,
            items,
        })
    }

    async fn seed_provider_symbols(
        &self,
        pool: &SqlitePool,
        instruments: &[crate::db::instruments::InstrumentRow],
    ) -> Result<(), MarketDataError> {
        for instrument in instruments {
            let existing_mapping =
                provider_symbols::find_by_instrument_provider(pool, instrument.id, YAHOO_PROVIDER)
                    .await?;
            let known_seed = yahoo_seed_for_known_isin(instrument.isin.as_deref())
                .or_else(|| yahoo_seed_for_known_isin(Some(&instrument.symbol)));

            let seed = match existing_mapping {
                Some(mapping) if mapping.enabled => None,
                Some(_) => known_seed,
                None => match known_seed
                    .or_else(|| yahoo_seed_for_exchange(&instrument.exchange, &instrument.symbol))
                {
                    Some(seed) => Some(seed),
                    None => self.yahoo_seed_from_search(instrument).await,
                },
            };

            let Some(seed) = seed else {
                continue;
            };

            let now = now_iso8601();
            provider_symbols::upsert(
                pool,
                &provider_symbols::NewProviderSymbol {
                    instrument_id: instrument.id,
                    provider: YAHOO_PROVIDER.to_owned(),
                    provider_symbol: seed.provider_symbol,
                    currency: Some(instrument.currency.clone()),
                    enabled: seed.enabled,
                    created_at: now.clone(),
                    updated_at: now,
                },
            )
            .await?;
        }
        Ok(())
    }

    async fn yahoo_seed_from_search(
        &self,
        instrument: &crate::db::instruments::InstrumentRow,
    ) -> Option<YahooSeed> {
        if !instrument.exchange.trim().eq_ignore_ascii_case("AVANZA") {
            return None;
        }

        let query = instrument
            .isin
            .as_deref()
            .filter(|value| is_isin_like(value))
            .or_else(|| is_isin_like(&instrument.symbol).then_some(instrument.symbol.as_str()))?;

        let search = self.inner.symbol_search_provider.as_ref()?;
        let matches = match search.search(query).await {
            Ok(matches) => matches,
            Err(error) => {
                crate::engine_warn!(
                    "market data symbol search failed instrument_id={} isin={} reason={} message={}",
                    instrument.id,
                    query,
                    error.reason_code(),
                    error.message()
                );
                return None;
            }
        };

        let Some(best) = best_yahoo_search_match(instrument, matches) else {
            crate::engine_warn!(
                "market data symbol search returned no supported match instrument_id={} isin={}",
                instrument.id,
                query
            );
            return None;
        };

        crate::engine_info!(
            "market data seeded yahoo symbol from isin instrument_id={} isin={} provider_symbol={}",
            instrument.id,
            query,
            best.provider_symbol
        );

        Some(YahooSeed {
            provider_symbol: best.provider_symbol,
            enabled: true,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshMode {
    Latest,
    Backfill,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshTrigger {
    Manual,
    Launch,
    Backfill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshRunStatus {
    Running,
    Succeeded,
    Partial,
    Failed,
}

impl RefreshRunStatus {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Succeeded => "SUCCEEDED",
            Self::Partial => "PARTIAL",
            Self::Failed => "FAILED",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshItemKind {
    Price,
    Fx,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshItemStatus {
    Fetched,
    Missing,
    Failed,
    Unmapped,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RefreshPricesRequest {
    pub mode: RefreshMode,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RefreshPricesResponse {
    pub run_id: i64,
    pub trigger: RefreshTrigger,
    pub mode: RefreshMode,
    pub status: RefreshRunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub message: Option<String>,
    pub prices_written: usize,
    pub fx_rates_written: usize,
    pub unmapped_instruments: usize,
    pub failed_items: usize,
    pub items: Vec<RefreshItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RefreshRunSummary {
    pub run_id: i64,
    pub trigger: RefreshTrigger,
    pub mode: RefreshMode,
    pub status: RefreshRunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub message: Option<String>,
    pub prices_written: usize,
    pub fx_rates_written: usize,
    pub unmapped_instruments: usize,
    pub failed_items: usize,
}

impl RefreshRunSummary {
    fn running(
        run_id: i64,
        trigger: RefreshTrigger,
        mode: RefreshMode,
        started_at: String,
    ) -> Self {
        Self {
            run_id,
            trigger,
            mode,
            status: RefreshRunStatus::Running,
            started_at,
            finished_at: None,
            message: None,
            prices_written: 0,
            fx_rates_written: 0,
            unmapped_instruments: 0,
            failed_items: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RefreshItem {
    pub kind: RefreshItemKind,
    pub instrument_id: Option<i64>,
    pub symbol_or_pair: String,
    pub status: RefreshItemStatus,
    pub reason: Option<String>,
    pub rows_written: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct PriceStatusResponse {
    pub refreshing: bool,
    pub latest_run: Option<RefreshRunSummary>,
    pub instruments: Vec<InstrumentMarketDataStatus>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstrumentMarketDataStatus {
    pub instrument_id: i64,
    pub exchange: String,
    pub symbol: String,
    pub currency: String,
    pub mapping_enabled: bool,
    pub provider_symbol: Option<String>,
    pub open_quantity: i64,
    pub latest_price: PriceSnapshotState,
    pub latest_fx: PriceSnapshotState,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotStatus {
    Available,
    Missing,
    Unmapped,
}

#[derive(Clone, Debug, Serialize)]
pub struct PriceSnapshotState {
    pub status: SnapshotStatus,
    pub date: Option<String>,
    pub value: Option<String>,
    pub provider: Option<String>,
    pub provider_symbol: Option<String>,
    pub reason: Option<String>,
}

impl PriceSnapshotState {
    fn available(date: String, value: String, provider: String, provider_symbol: String) -> Self {
        Self {
            status: SnapshotStatus::Available,
            date: Some(date),
            value: Some(value),
            provider: Some(provider),
            provider_symbol: Some(provider_symbol),
            reason: None,
        }
    }

    fn missing(reason: impl Into<String>) -> Self {
        Self {
            status: SnapshotStatus::Missing,
            date: None,
            value: None,
            provider: None,
            provider_symbol: None,
            reason: Some(reason.into()),
        }
    }

    fn unmapped() -> Self {
        Self {
            status: SnapshotStatus::Unmapped,
            date: None,
            value: None,
            provider: None,
            provider_symbol: None,
            reason: Some("symbol_unmapped".to_owned()),
        }
    }
}

#[derive(Debug)]
pub enum MarketDataError {
    InvalidRequest { code: &'static str, message: String },
    Internal(String),
    Repo(RepoError),
}

impl MarketDataError {
    pub fn invalid_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            code,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

impl fmt::Display for MarketDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { code, message } => write!(f, "{code}: {message}"),
            Self::Internal(message) => f.write_str(message),
            Self::Repo(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MarketDataError {}

impl From<RepoError> for MarketDataError {
    fn from(error: RepoError) -> Self {
        Self::Repo(error)
    }
}

struct RefreshOutcome {
    status: RefreshRunStatus,
    message: Option<String>,
    prices_written: usize,
    fx_rates_written: usize,
    unmapped_instruments: usize,
    failed_items: usize,
    items: Vec<RefreshItem>,
}

struct RefreshWindow {
    start: NaiveDate,
    end: NaiveDate,
}

struct RefreshTarget {
    instrument: crate::db::instruments::InstrumentRow,
    currency: String,
    provider_symbol: Option<String>,
}

impl RefreshTrigger {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Manual => "MANUAL",
            Self::Launch => "LAUNCH",
            Self::Backfill => "BACKFILL",
        }
    }
}

async fn latest_run_summary(
    pool: &SqlitePool,
) -> Result<Option<RefreshRunSummary>, MarketDataError> {
    let row = market_data_runs::latest(pool).await?;
    Ok(row.map(|row| RefreshRunSummary {
        run_id: row.id,
        trigger: refresh_trigger_from_db(&row.trigger),
        mode: refresh_mode_from_trigger(&row.trigger),
        status: refresh_status_from_db(&row.status),
        started_at: row.started_at,
        finished_at: row.finished_at,
        message: row.message,
        prices_written: row.prices_written as usize,
        fx_rates_written: row.fx_rates_written as usize,
        unmapped_instruments: row.unmapped_instruments as usize,
        failed_items: row.failed_items as usize,
    }))
}

fn refresh_status_from_db(status: &str) -> RefreshRunStatus {
    match status {
        "RUNNING" => RefreshRunStatus::Running,
        "SUCCEEDED" => RefreshRunStatus::Succeeded,
        "PARTIAL" => RefreshRunStatus::Partial,
        "FAILED" => RefreshRunStatus::Failed,
        _ => RefreshRunStatus::Failed,
    }
}

fn refresh_trigger_from_db(trigger: &str) -> RefreshTrigger {
    match trigger {
        "LAUNCH" => RefreshTrigger::Launch,
        "BACKFILL" => RefreshTrigger::Backfill,
        _ => RefreshTrigger::Manual,
    }
}

fn refresh_mode_from_trigger(trigger: &str) -> RefreshMode {
    match trigger {
        "BACKFILL" => RefreshMode::Backfill,
        _ => RefreshMode::Latest,
    }
}

fn provider_error_status(error: &ProviderError) -> RefreshItemStatus {
    match error.reason() {
        ProviderMissingReason::SymbolUnmapped => RefreshItemStatus::Unmapped,
        ProviderMissingReason::NoDataInRange => RefreshItemStatus::Missing,
        ProviderMissingReason::NotListed | ProviderMissingReason::MarketClosed => {
            RefreshItemStatus::Missing
        }
        ProviderMissingReason::RateLimited | ProviderMissingReason::ProviderError => {
            RefreshItemStatus::Failed
        }
    }
}

async fn refresh_window(
    request: &RefreshPricesRequest,
    pool: &SqlitePool,
) -> Result<RefreshWindow, MarketDataError> {
    match request.mode {
        RefreshMode::Latest => {
            let end = Utc::now().date_naive();
            Ok(RefreshWindow {
                start: end - Duration::days(LATEST_REFRESH_WINDOW_DAYS),
                end,
            })
        }
        RefreshMode::Backfill => {
            let end = match &request.end_date {
                Some(value) => parse_date("end_date", value)?,
                None => Utc::now().date_naive(),
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

struct YahooSeed {
    provider_symbol: String,
    enabled: bool,
}

fn best_yahoo_search_match(
    instrument: &crate::db::instruments::InstrumentRow,
    matches: Vec<SymbolSearchMatch>,
) -> Option<SymbolSearchMatch> {
    let mut supported = matches.into_iter().filter(is_supported_yahoo_quote);
    if instrument
        .isin
        .as_deref()
        .is_some_and(|isin| isin.trim().starts_with("US"))
        && instrument.currency.trim().eq_ignore_ascii_case("USD")
    {
        return supported.find(|item| {
            !item.provider_symbol.contains('.')
                && item.exchange.as_deref().is_some_and(is_us_exchange)
        });
    }

    supported.next()
}

fn is_supported_yahoo_quote(item: &SymbolSearchMatch) -> bool {
    let quote_type = item
        .quote_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(quote_type.as_str(), "EQUITY" | "ETF" | "MUTUALFUND")
}

fn is_us_exchange(exchange: &str) -> bool {
    matches!(
        exchange.trim().to_ascii_uppercase().as_str(),
        "NMS" | "NYQ" | "ASE" | "NGM" | "NCM" | "PCX" | "NASDAQ" | "NYSE" | "NYSEARCA"
    )
}

fn is_isin_like(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() == 12 && trimmed.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn yahoo_seed_for_exchange(exchange: &str, symbol: &str) -> Option<YahooSeed> {
    let normalized = exchange.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "NASDAQ" | "NYSE" => Some(YahooSeed {
            provider_symbol: symbol.trim().to_owned(),
            enabled: true,
        }),
        "XETR" | "XTRA" | "XETRA" | "FRANKFURT" | "DE" => Some(YahooSeed {
            provider_symbol: format!("{}.DE", symbol.trim()),
            enabled: false,
        }),
        "XAMS" | "EURONEXT AMSTERDAM" | "AMSTERDAM" => Some(YahooSeed {
            provider_symbol: format!("{}.AS", symbol.trim()),
            enabled: false,
        }),
        "XPAR" | "EURONEXT PARIS" | "PARIS" => Some(YahooSeed {
            provider_symbol: format!("{}.PA", symbol.trim()),
            enabled: false,
        }),
        "XLON" | "LSE" | "LONDON" => Some(YahooSeed {
            provider_symbol: format!("{}.L", symbol.trim()),
            enabled: false,
        }),
        _ => None,
    }
}

fn yahoo_seed_for_known_isin(isin: Option<&str>) -> Option<YahooSeed> {
    let normalized = isin?.trim().to_ascii_uppercase();
    let provider_symbol = match normalized.as_str() {
        "IE00B0M63391" => "IQQK.DE",
        "US02079K3059" => "GOOGL",
        "US8740391003" => "TSM",
        _ => return None,
    };

    Some(YahooSeed {
        provider_symbol: provider_symbol.to_owned(),
        enabled: true,
    })
}

fn group_transactions(
    rows: Vec<crate::db::transactions::TransactionRow>,
) -> BTreeMap<i64, Vec<crate::db::transactions::TransactionRow>> {
    let mut grouped: BTreeMap<i64, Vec<crate::db::transactions::TransactionRow>> = BTreeMap::new();
    for row in rows {
        grouped.entry(row.instrument_id).or_default().push(row);
    }
    grouped
}

fn target_currencies(targets: &[RefreshTarget]) -> BTreeSet<String> {
    targets
        .iter()
        .map(|target| target.currency.trim().to_ascii_uppercase())
        .filter(|currency| !currency.is_empty())
        .collect()
}

fn parse_date(field: &'static str, value: &str) -> Result<NaiveDate, MarketDataError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|error| {
        MarketDataError::invalid_request(
            "invalid_date",
            format!("{field} {:?} must be YYYY-MM-DD: {error}", value),
        )
    })
}

async fn latest_price_snapshot(
    pool: &SqlitePool,
    instrument_id: i64,
    mapping: &provider_symbols::ProviderSymbolRow,
    as_of_date: NaiveDate,
    _currency: &str,
) -> Result<PriceSnapshotState, MarketDataError> {
    if !mapping.enabled {
        return Ok(PriceSnapshotState::unmapped());
    }

    let row =
        prices::find_latest_on_or_before(pool, instrument_id, YAHOO_PROVIDER, as_of_date).await?;
    Ok(match row {
        Some(row) => {
            PriceSnapshotState::available(row.date, row.close, row.provider, row.provider_symbol)
        }
        None => PriceSnapshotState::missing("missing_price"),
    })
}

async fn latest_fx_snapshot(
    pool: &SqlitePool,
    currency: &str,
    as_of_date: NaiveDate,
) -> Result<PriceSnapshotState, MarketDataError> {
    if currency.eq_ignore_ascii_case(SEK) {
        return Ok(PriceSnapshotState::available(
            as_of_date.format("%Y-%m-%d").to_string(),
            Decimal::ONE.to_string(),
            "identity".to_owned(),
            format!("{SEK}/{SEK}"),
        ));
    }

    let row =
        fx_rates::find_latest_on_or_before(pool, currency, SEK, FRANKFURTER_PROVIDER, as_of_date)
            .await?;
    Ok(match row {
        Some(row) => PriceSnapshotState::available(
            row.date,
            row.rate,
            row.provider,
            format!("{}/{}", row.base, row.quote),
        ),
        None => PriceSnapshotState::missing("missing_fx"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        db::{self, fx_rates, instruments, prices, provider_symbols, transactions},
        providers::{
            DailyClose, FakeFxRateProvider, FakePriceProvider, FakeSymbolSearchProvider,
            FxProvider, FxRate, MarketDataProvider, SymbolSearchMatch,
        },
    };
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    async fn test_state(
        price_provider: FakePriceProvider,
        fx_provider: FakeFxRateProvider,
    ) -> (SqlitePool, MarketDataService) {
        let pool = db::memory_pool().await.expect("memory pool");
        let service = MarketDataService::with_providers(price_provider, fx_provider);
        (pool, service)
    }

    async fn instrument(pool: &SqlitePool, symbol: &str, exchange: &str, currency: &str) -> i64 {
        let (row, _) = instruments::upsert(
            pool,
            &crate::db::instruments::NewInstrument {
                symbol: symbol.to_owned(),
                exchange: exchange.to_owned(),
                name: symbol.to_owned(),
                kind: "STOCK".to_owned(),
                currency: currency.to_owned(),
                isin: None,
            },
        )
        .await
        .expect("instrument upsert should succeed");
        row.id
    }

    async fn instrument_with_isin(
        pool: &SqlitePool,
        symbol: &str,
        exchange: &str,
        currency: &str,
        isin: &str,
    ) -> i64 {
        let (row, _) = instruments::upsert(
            pool,
            &crate::db::instruments::NewInstrument {
                symbol: symbol.to_owned(),
                exchange: exchange.to_owned(),
                name: symbol.to_owned(),
                kind: "STOCK".to_owned(),
                currency: currency.to_owned(),
                isin: Some(isin.to_owned()),
            },
        )
        .await
        .expect("instrument upsert should succeed");
        row.id
    }

    async fn buy(
        pool: &SqlitePool,
        instrument_id: i64,
        trade_date: &str,
        quantity: i64,
        price: &str,
        currency: &str,
        fx_rate_to_base: Option<&str>,
    ) {
        transactions::insert(
            pool,
            &crate::db::transactions::NewTransaction {
                instrument_id,
                kind: domain::TransactionKind::Buy,
                trade_date: NaiveDate::parse_from_str(trade_date, "%Y-%m-%d").expect("date"),
                quantity,
                price: Some(price.parse().expect("price")),
                dividend_per_share: None,
                currency: Some(currency.to_owned()),
                fx_rate_to_base: fx_rate_to_base.map(|value| value.parse().expect("fx")),
                brokerage: None,
                note: None,
            },
        )
        .await
        .expect("transaction insert should succeed");
    }

    #[tokio::test]
    async fn latest_refresh_writes_prices_fx_and_seeds_mappings() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        price_provider.push_response(Ok(vec![
            DailyClose {
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 10).expect("date"),
                close: dec!(100),
                currency: "USD".to_owned(),
            },
            DailyClose {
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
                close: dec!(101),
                currency: "USD".to_owned(),
            },
        ]));
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            rate: dec!(10.5),
        }]));

        let (pool, service) = test_state(price_provider, fx_provider).await;
        let msft = instrument(&pool, "MSFT", "NASDAQ", "USD").await;
        buy(&pool, msft, "2026-06-01", 10, "100", "USD", Some("10")).await;

        let response = service
            .refresh(
                &pool,
                RefreshTrigger::Manual,
                RefreshPricesRequest {
                    mode: RefreshMode::Latest,
                    start_date: None,
                    end_date: None,
                },
            )
            .await
            .expect("refresh should succeed");

        assert_eq!(response.status, RefreshRunStatus::Succeeded);
        assert_eq!(response.prices_written, 2);
        assert_eq!(response.fx_rates_written, 1);
        assert_eq!(response.unmapped_instruments, 0);
        assert_eq!(response.failed_items, 0);
        assert!(!response.items.is_empty());

        let mapping = provider_symbols::find_by_instrument_provider(&pool, msft, YAHOO_PROVIDER)
            .await
            .expect("mapping lookup should succeed")
            .expect("mapping should exist");
        assert!(mapping.enabled);
        assert_eq!(mapping.provider_symbol, "MSFT");

        let prices = prices::list(&pool).await.expect("price list");
        assert_eq!(prices.len(), 2);
        let fx = fx_rates::list(&pool).await.expect("fx list");
        assert_eq!(fx.len(), 1);
    }

    #[tokio::test]
    async fn avanza_isin_refresh_seeds_yahoo_mapping_from_search() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(101),
            currency: "USD".to_owned(),
        }]));
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            rate: dec!(10.5),
        }]));
        let symbol_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        symbol_search.push_response(Ok(vec![SymbolSearchMatch {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            quote_type: Some("EQUITY".to_owned()),
            exchange: Some("NMS".to_owned()),
            name: Some("Microsoft Corporation".to_owned()),
        }]));

        let pool = db::memory_pool().await.expect("memory pool");
        let service = MarketDataService::with_symbol_search_providers(
            price_provider,
            fx_provider,
            symbol_search.clone(),
        );
        let msft =
            instrument_with_isin(&pool, "US5949181045", "AVANZA", "USD", "US5949181045").await;
        buy(&pool, msft, "2026-06-01", 10, "100", "USD", Some("10")).await;

        let response = service
            .refresh(
                &pool,
                RefreshTrigger::Manual,
                RefreshPricesRequest {
                    mode: RefreshMode::Latest,
                    start_date: None,
                    end_date: None,
                },
            )
            .await
            .expect("refresh should succeed");

        assert_eq!(response.status, RefreshRunStatus::Succeeded);
        assert_eq!(response.prices_written, 1);
        assert_eq!(response.unmapped_instruments, 0);
        assert_eq!(symbol_search.calls()[0].query, "US5949181045");

        let mapping = provider_symbols::find_by_instrument_provider(&pool, msft, YAHOO_PROVIDER)
            .await
            .expect("mapping lookup should succeed")
            .expect("mapping should exist");
        assert!(mapping.enabled);
        assert_eq!(mapping.provider_symbol, "MSFT");
    }

    #[tokio::test]
    async fn avanza_known_isins_seed_yahoo_mappings_without_search() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "IQQK.DE".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(115),
            currency: "EUR".to_owned(),
        }]));
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "GOOGL".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(245),
            currency: "USD".to_owned(),
        }]));
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "TSM".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(243),
            currency: "USD".to_owned(),
        }]));

        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "EUR".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            rate: dec!(11),
        }]));
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            rate: dec!(10),
        }]));

        let symbol_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        let pool = db::memory_pool().await.expect("memory pool");
        let service = MarketDataService::with_symbol_search_providers(
            price_provider,
            fx_provider,
            symbol_search.clone(),
        );

        let korea =
            instrument_with_isin(&pool, "IE00B0M63391", "AVANZA", "EUR", "IE00B0M63391").await;
        let alphabet =
            instrument_with_isin(&pool, "US02079K3059", "AVANZA", "USD", "US02079K3059").await;
        let tsm =
            instrument_with_isin(&pool, "US8740391003", "AVANZA", "USD", "US8740391003").await;
        provider_symbols::upsert(
            &pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: korea,
                provider: YAHOO_PROVIDER.to_owned(),
                provider_symbol: "IDKO.L".to_owned(),
                currency: Some("EUR".to_owned()),
                enabled: false,
                created_at: now_iso8601(),
                updated_at: now_iso8601(),
            },
        )
        .await
        .expect("stale mapping should insert");
        provider_symbols::upsert(
            &pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: tsm,
                provider: YAHOO_PROVIDER.to_owned(),
                provider_symbol: "TSMN.MX".to_owned(),
                currency: Some("USD".to_owned()),
                enabled: false,
                created_at: now_iso8601(),
                updated_at: now_iso8601(),
            },
        )
        .await
        .expect("stale mapping should insert");
        buy(&pool, korea, "2026-06-01", 10, "100", "EUR", Some("11")).await;
        buy(&pool, alphabet, "2026-06-01", 10, "100", "USD", Some("10")).await;
        buy(&pool, tsm, "2026-06-01", 10, "100", "USD", Some("10")).await;

        let response = service
            .refresh(
                &pool,
                RefreshTrigger::Manual,
                RefreshPricesRequest {
                    mode: RefreshMode::Latest,
                    start_date: None,
                    end_date: None,
                },
            )
            .await
            .expect("refresh should succeed");

        assert_eq!(response.status, RefreshRunStatus::Succeeded);
        assert_eq!(response.prices_written, 3);
        assert_eq!(response.fx_rates_written, 2);
        assert_eq!(response.unmapped_instruments, 0);
        assert!(symbol_search.calls().is_empty());

        let cases = [(korea, "IQQK.DE"), (alphabet, "GOOGL"), (tsm, "TSM")];
        for (instrument_id, expected_symbol) in cases {
            let mapping =
                provider_symbols::find_by_instrument_provider(&pool, instrument_id, YAHOO_PROVIDER)
                    .await
                    .expect("mapping lookup should succeed")
                    .expect("mapping should exist");
            assert!(mapping.enabled);
            assert_eq!(mapping.provider_symbol, expected_symbol);
        }
    }

    #[tokio::test]
    async fn refresh_rejects_price_rows_with_wrong_currency() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(101),
            currency: "EUR".to_owned(),
        }]));
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            rate: dec!(10.5),
        }]));
        let (pool, service) = test_state(price_provider, fx_provider).await;
        let msft = instrument(&pool, "MSFT", "NASDAQ", "USD").await;
        buy(&pool, msft, "2026-06-01", 10, "100", "USD", Some("10")).await;

        let response = service
            .refresh(
                &pool,
                RefreshTrigger::Manual,
                RefreshPricesRequest {
                    mode: RefreshMode::Latest,
                    start_date: None,
                    end_date: None,
                },
            )
            .await
            .expect("refresh should complete with item failure");

        assert_eq!(response.status, RefreshRunStatus::Partial);
        assert_eq!(response.prices_written, 0);
        assert_eq!(response.failed_items, 1);
        assert_eq!(
            response.items[0].reason.as_deref(),
            Some("currency_mismatch")
        );

        let prices = prices::list(&pool).await.expect("price list");
        assert!(prices.is_empty());
    }

    #[tokio::test]
    async fn backfill_refresh_uses_earliest_transaction_date() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "ASML.DE".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
            close: dec!(600),
            currency: "EUR".to_owned(),
        }]));
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "EUR".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
            rate: dec!(11),
        }]));

        let (pool, service) = test_state(price_provider, fx_provider).await;
        let asml = instrument(&pool, "ASML", "XETR", "EUR").await;
        buy(&pool, asml, "2026-06-10", 3, "600", "EUR", None).await;
        provider_symbols::upsert(
            &pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: asml,
                provider: YAHOO_PROVIDER.to_owned(),
                provider_symbol: "ASML.DE".to_owned(),
                currency: Some("EUR".to_owned()),
                enabled: true,
                created_at: now_iso8601(),
                updated_at: now_iso8601(),
            },
        )
        .await
        .expect("mapping upsert should succeed");

        let response = service
            .refresh(
                &pool,
                RefreshTrigger::Backfill,
                RefreshPricesRequest {
                    mode: RefreshMode::Backfill,
                    start_date: None,
                    end_date: None,
                },
            )
            .await
            .expect("refresh should succeed");

        assert_eq!(response.status, RefreshRunStatus::Succeeded);
        assert_eq!(response.prices_written, 1);
        assert_eq!(response.fx_rates_written, 1);
    }

    #[tokio::test]
    async fn unmapped_instruments_are_reported() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        let (pool, service) = test_state(price_provider, fx_provider).await;
        let instrument_id = instrument(&pool, "ABC", "OTC", "USD").await;
        buy(
            &pool,
            instrument_id,
            "2026-06-10",
            1,
            "5",
            "USD",
            Some("10"),
        )
        .await;

        let response = service
            .refresh(
                &pool,
                RefreshTrigger::Manual,
                RefreshPricesRequest {
                    mode: RefreshMode::Latest,
                    start_date: None,
                    end_date: None,
                },
            )
            .await
            .expect("refresh should succeed");

        assert_eq!(response.unmapped_instruments, 1);
        assert_eq!(response.items[0].status, RefreshItemStatus::Unmapped);
        assert_eq!(response.items[0].reason.as_deref(), Some("symbol_unmapped"));
    }

    #[tokio::test]
    async fn second_refresh_returns_current_running_status_without_starting_new_work() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        let (pool, service) = test_state(price_provider.clone(), fx_provider.clone()).await;
        let msft = instrument(&pool, "MSFT", "NASDAQ", "USD").await;
        buy(&pool, msft, "2026-06-01", 10, "100", "USD", Some("10")).await;

        let gate = Arc::new(tokio::sync::Notify::new());
        price_provider.block_next_call_on(Arc::clone(&gate));
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(101),
            currency: "USD".to_owned(),
        }]));
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            rate: dec!(10.5),
        }]));

        let service_clone = service.clone();
        let pool_clone = pool.clone();
        let first = tokio::spawn(async move {
            service_clone
                .refresh(
                    &pool_clone,
                    RefreshTrigger::Manual,
                    RefreshPricesRequest {
                        mode: RefreshMode::Latest,
                        start_date: None,
                        end_date: None,
                    },
                )
                .await
        });

        while price_provider.calls().is_empty() {
            tokio::task::yield_now().await;
        }
        let running = service
            .refresh(
                &pool,
                RefreshTrigger::Manual,
                RefreshPricesRequest {
                    mode: RefreshMode::Latest,
                    start_date: None,
                    end_date: None,
                },
            )
            .await
            .expect("running status should succeed");

        assert_eq!(running.status, RefreshRunStatus::Running);
        gate.notify_waiters();
        let completed = first
            .await
            .expect("task should complete")
            .expect("refresh should succeed");
        assert_eq!(completed.status, RefreshRunStatus::Succeeded);
    }
}
