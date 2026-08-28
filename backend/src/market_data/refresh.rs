use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use chrono::{Duration, NaiveDate, Utc};
use futures::future::join_all;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;

use crate::{
    db::{
        fx_rates, instruments, market_data_runs, prices, provider_symbols, transactions, RepoError,
    },
    domain,
    import::now_iso8601,
    market_data::{
        effective_prices::{self, PriceSourceMapping},
        symbol_matching::{
            self, best_yahoo_search_match, is_isin_like, is_plausible_symbol_recovery,
            is_supported_quote, isin_like_identifier, CurrencyMatch,
            MAX_SYMBOL_RECOVERY_CANDIDATES,
        },
    },
    providers::{
        FxRateProvider, MarketDataProvider, PriceHistoryRequest, PriceProvider, ProviderError,
        ProviderMissingReason, SymbolSearchMatch, SymbolSearchProvider, BASE_FX_PROVIDER,
        PRICE_PROVIDER_PRECEDENCE,
    },
};

const LATEST_REFRESH_WINDOW_DAYS: i64 = 14;
const SEK: &str = "SEK";
/// How far after a requested backfill start the earliest returned row may fall
/// before the run reports `history_clamped`. Nasdaq silently truncates history
/// to roughly ten years and says nothing, so the gap is the only signal.
const HISTORY_CLAMP_TOLERANCE_DAYS: i64 = 5;

#[derive(Clone)]
pub struct MarketDataService {
    inner: Arc<MarketDataServiceInner>,
}

struct MarketDataServiceInner {
    /// Both registries are ordered by `PRICE_PROVIDER_PRECEDENCE`, which stays
    /// the single source of truth for precedence; lookup is a linear scan over
    /// a handful of entries.
    price_providers: Vec<(MarketDataProvider, Arc<dyn PriceProvider + Send + Sync>)>,
    symbol_search_providers: Vec<(
        MarketDataProvider,
        Arc<dyn SymbolSearchProvider + Send + Sync>,
    )>,
    fx_provider: Arc<dyn FxRateProvider + Send + Sync>,
    running: Arc<AtomicBool>,
    active: Arc<Mutex<Option<RefreshRunSummary>>>,
}

/// Registration surface for the multi-provider tests and for `live()`.
#[derive(Default)]
pub struct ProviderRegistry {
    price_providers: Vec<(MarketDataProvider, Arc<dyn PriceProvider + Send + Sync>)>,
    symbol_search_providers: Vec<(
        MarketDataProvider,
        Arc<dyn SymbolSearchProvider + Send + Sync>,
    )>,
    fx_provider: Option<Arc<dyn FxRateProvider + Send + Sync>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_price_provider<P>(mut self, provider: MarketDataProvider, client: P) -> Self
    where
        P: PriceProvider + Send + Sync + 'static,
    {
        self.price_providers.push((provider, Arc::new(client)));
        self
    }

    pub fn with_symbol_search_provider<S>(mut self, provider: MarketDataProvider, client: S) -> Self
    where
        S: SymbolSearchProvider + Send + Sync + 'static,
    {
        self.symbol_search_providers
            .push((provider, Arc::new(client)));
        self
    }

    pub fn with_fx_provider<F>(mut self, client: F) -> Self
    where
        F: FxRateProvider + Send + Sync + 'static,
    {
        self.fx_provider = Some(Arc::new(client));
        self
    }
}

/// Sort a registry into `PRICE_PROVIDER_PRECEDENCE` order so the registration
/// order at the call site can never disagree with the read-path precedence.
fn sort_by_precedence<T>(entries: &mut [(MarketDataProvider, T)]) {
    debug_assert!(
        entries
            .iter()
            .all(|(provider, _)| precedence_index(*provider).is_some()),
        "every registered market-data provider must appear in PRICE_PROVIDER_PRECEDENCE"
    );
    entries.sort_by_key(|(provider, _)| precedence_index(*provider).unwrap_or(usize::MAX));
}

fn precedence_index(provider: MarketDataProvider) -> Option<usize> {
    PRICE_PROVIDER_PRECEDENCE
        .iter()
        .position(|candidate| *candidate == provider)
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
        let ProviderRegistry {
            mut price_providers,
            mut symbol_search_providers,
            fx_provider,
        } = registry;
        sort_by_precedence(&mut price_providers);
        sort_by_precedence(&mut symbol_search_providers);

        Self {
            inner: Arc::new(MarketDataServiceInner {
                price_providers,
                symbol_search_providers,
                fx_provider: fx_provider
                    .unwrap_or_else(|| Arc::new(crate::providers::FrankfurterClient::new())),
                running: Arc::new(AtomicBool::new(false)),
                active: Arc::new(Mutex::new(None)),
            }),
        }
    }

    fn price_provider(
        &self,
        provider: MarketDataProvider,
    ) -> Option<&Arc<dyn PriceProvider + Send + Sync>> {
        self.inner
            .price_providers
            .iter()
            .find(|(registered, _)| *registered == provider)
            .map(|(_, client)| client)
    }

    fn symbol_search_provider(
        &self,
        provider: MarketDataProvider,
    ) -> Option<&Arc<dyn SymbolSearchProvider + Send + Sync>> {
        self.inner
            .symbol_search_providers
            .iter()
            .find(|(registered, _)| *registered == provider)
            .map(|(_, client)| client)
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
            let position = position_for_instrument(&grouped, instrument.id)?;

            let price_sources = all_price_sources(pool, instrument.id).await?;
            let enabled_sources =
                effective_prices::enabled_price_sources(pool, instrument.id).await?;

            let latest_price =
                latest_price_snapshot(pool, instrument.id, &enabled_sources, today).await?;
            let effective_price_source = latest_price.provider.clone();

            let latest_fx = latest_fx_snapshot(pool, &instrument.currency, today).await?;

            readiness.push(InstrumentMarketDataStatus {
                instrument_id: instrument.id,
                exchange: instrument.exchange,
                symbol: instrument.symbol,
                currency: instrument.currency,
                price_sources,
                effective_price_source,
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

    pub async fn lookup_symbol_search(&self, query: &str) -> SymbolSearchLookupResponse {
        let query = query.trim().to_owned();
        let search_results = join_all(self.inner.symbol_search_providers.iter().map(
            |(provider, client)| {
                let query = &query;
                async move { (*provider, client.search(query).await) }
            },
        ))
        .await;
        let providers_tried = search_results.len();
        let mut providers_failed = 0;

        let mut reachable_provider = false;
        let mut supported_matches = Vec::new();
        for (provider, result) in search_results {
            match result {
                Ok(matches) => {
                    reachable_provider = true;
                    for item in matches.into_iter().filter(is_supported_quote) {
                        if !supported_matches
                            .iter()
                            .any(|existing: &SymbolSearchMatch| {
                                existing.provider == item.provider
                                    && existing.provider_symbol == item.provider_symbol
                            })
                        {
                            supported_matches.push(item);
                        }
                    }
                }
                Err(error) => {
                    providers_failed += 1;
                    crate::engine_warn!(
                        "instrument lookup provider search failed provider={} query={} reason={} message={}",
                        provider,
                        query,
                        error.reason_code(),
                        error.message()
                    );
                }
            }
        }

        let supported_matches = supported_matches
            .into_iter()
            .map(SymbolSearchLookupMatch::from)
            .collect::<Vec<_>>();

        let (status, response) = if !supported_matches.is_empty() {
            (
                "matches",
                SymbolSearchLookupResponse::matches(query.clone(), supported_matches),
            )
        } else if reachable_provider {
            (
                "no_match",
                SymbolSearchLookupResponse::no_match(query.clone()),
            )
        } else {
            (
                "provider_unavailable",
                SymbolSearchLookupResponse::provider_unavailable(query.clone()),
            )
        };

        crate::engine_info!(
            "instrument lookup merged outcome query={} status={} matches={} providers_tried={} providers_failed={}",
            query,
            status,
            response.matches.len(),
            providers_tried,
            providers_failed
        );
        response
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
        let ambiguous = self.seed_provider_symbols(pool, &instruments).await?;

        let mut targets = Vec::new();
        for instrument in instruments {
            let has_history = grouped.contains_key(&instrument.id);
            let position = position_for_instrument(&grouped, instrument.id)?;
            let should_skip = match request.mode {
                RefreshMode::Latest => {
                    let conviction =
                        match domain::ConvictionLevel::from_db_str(&instrument.conviction) {
                            Some(conviction) => conviction,
                            None => {
                                return Err(MarketDataError::internal(format!(
                                    "stored unknown conviction {:?} for instrument {}",
                                    instrument.conviction, instrument.id
                                )));
                            }
                        };
                    position.quantity == 0
                        && !matches!(
                            conviction,
                            domain::ConvictionLevel::Low
                                | domain::ConvictionLevel::Medium
                                | domain::ConvictionLevel::High
                        )
                }
                RefreshMode::Backfill => !has_history,
            };
            if should_skip {
                continue;
            }

            let currency = instrument.currency.clone();
            targets.push(RefreshTarget {
                instrument,
                currency,
            });
        }

        let mut prices_written = 0usize;
        let mut fx_rates_written = 0usize;
        let mut unmapped_instruments = 0usize;
        let mut failed_items = 0usize;
        let mut items = Vec::new();
        let mut rows_by_provider: Vec<(MarketDataProvider, usize)> = Vec::new();

        for target in &targets {
            let sources =
                effective_prices::enabled_price_sources(pool, target.instrument.id).await?;

            if sources.is_empty() {
                unmapped_instruments += 1;
                items.push(self.unsourced_item(target, &ambiguous));
                continue;
            }

            for source in &sources {
                let outcome = self
                    .refresh_one_source(pool, target, source, &target_window, request.mode)
                    .await?;
                if outcome.failed {
                    failed_items += 1;
                }
                prices_written += outcome.item.rows_written;
                if outcome.item.rows_written > 0 {
                    add_provider_rows(
                        &mut rows_by_provider,
                        source.provider,
                        outcome.item.rows_written,
                    );
                }
                items.push(outcome.item);
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
                                provider: BASE_FX_PROVIDER,
                                fetched_at: now_iso8601(),
                            },
                        )
                        .await?;
                    }
                    fx_rates_written += rows.len();
                    items.push(RefreshItem {
                        kind: RefreshItemKind::Fx,
                        instrument_id: None,
                        provider: Some(BASE_FX_PROVIDER.to_string()),
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
                        provider: Some(BASE_FX_PROVIDER.to_string()),
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

        let by_provider = describe_provider_split(&rows_by_provider);
        crate::engine_info!(
            "market data refresh provider split {by_provider} unmapped={unmapped_instruments} failed={failed_items}"
        );
        let message = Some(format!(
            "prices_written={prices_written} fx_rates_written={fx_rates_written} unmapped={unmapped_instruments} failed={failed_items} by_provider=[{by_provider}]"
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

    /// Report an instrument that ended the run with no usable price source.
    ///
    /// An ambiguity is a distinct state from "no provider carries this": it
    /// means a hand mapping would fix it, so it is named rather than folded
    /// into `Unmapped`. Both count as unmapped, because action is needed.
    fn unsourced_item(
        &self,
        target: &RefreshTarget,
        ambiguous: &BTreeMap<i64, String>,
    ) -> RefreshItem {
        match ambiguous.get(&target.instrument.id) {
            Some(candidates) => RefreshItem {
                kind: RefreshItemKind::Price,
                instrument_id: Some(target.instrument.id),
                provider: Some(MarketDataProvider::NasdaqNordic.to_string()),
                symbol_or_pair: target.instrument.symbol.clone(),
                status: RefreshItemStatus::Ambiguous,
                reason: Some(format!("ambiguous_match: {candidates}")),
                rows_written: 0,
            },
            None => RefreshItem {
                kind: RefreshItemKind::Price,
                instrument_id: Some(target.instrument.id),
                provider: None,
                symbol_or_pair: target.instrument.symbol.clone(),
                status: RefreshItemStatus::Unmapped,
                reason: Some("symbol_unmapped".to_owned()),
                rows_written: 0,
            },
        }
    }

    /// Fetch and store one instrument's prices from one enabled mapping.
    ///
    /// A failure disables or reports only *this* mapping; the instrument's other
    /// sources are fetched independently by the caller.
    async fn refresh_one_source(
        &self,
        pool: &SqlitePool,
        target: &RefreshTarget,
        source: &PriceSourceMapping,
        window: &RefreshWindow,
        mode: RefreshMode,
    ) -> Result<SourceRefreshOutcome, MarketDataError> {
        let instrument = &target.instrument;
        let mapped_symbol = source.provider_symbol.clone();

        if self.price_provider(source.provider).is_none() {
            crate::engine_warn!(
                "market data refresh has no registered client for a stored mapping instrument_id={} provider={} provider_symbol={}",
                instrument.id,
                source.provider,
                mapped_symbol
            );
            return Ok(SourceRefreshOutcome::failed(RefreshItem {
                kind: RefreshItemKind::Price,
                instrument_id: Some(instrument.id),
                provider: Some(source.provider.to_string()),
                symbol_or_pair: mapped_symbol,
                status: RefreshItemStatus::Unavailable,
                reason: Some("provider_unregistered".to_owned()),
                rows_written: 0,
            }));
        }

        let resolved = match self
            .price_history_for_source(instrument, source, window)
            .await
        {
            Ok(resolved) => resolved,
            Err(error) => {
                crate::engine_warn!(
                    "market data refresh price failure instrument_id={} provider={} symbol={} asset_class={:?} reason={} message={}",
                    instrument.id,
                    source.provider,
                    mapped_symbol,
                    source.asset_class,
                    error.reason_code(),
                    error.message()
                );
                return Ok(SourceRefreshOutcome::failed(RefreshItem {
                    kind: RefreshItemKind::Price,
                    instrument_id: Some(instrument.id),
                    provider: Some(source.provider.to_string()),
                    symbol_or_pair: mapped_symbol,
                    status: provider_error_status(&error),
                    reason: Some(error.reason_code().to_owned()),
                    rows_written: 0,
                }));
            }
        };

        let provider_symbol = resolved.provider_symbol;
        let rows = resolved.rows;

        if !provider_symbol.eq_ignore_ascii_case(&mapped_symbol) {
            // The recovery path only accepts candidates whose rows quote in the
            // instrument's own currency, so that is what the mapping records.
            self.persist_mapping(
                pool,
                instrument,
                source,
                &provider_symbol,
                Some(instrument.currency.clone()),
                true,
            )
            .await?;
            crate::engine_info!(
                "market data recovered changed yahoo symbol instrument_id={} isin={:?} old_symbol={} new_symbol={}",
                instrument.id,
                instrument.isin,
                mapped_symbol,
                provider_symbol
            );
        }

        if let Some(row) = rows.iter().find(|row| {
            !row.currency
                .trim()
                .eq_ignore_ascii_case(instrument.currency.trim())
        }) {
            // Keep the currency the mapping claimed: for Nasdaq it is what
            // stamped the rows, and it is the value that needs correcting.
            self.persist_mapping(
                pool,
                instrument,
                source,
                &provider_symbol,
                source
                    .currency
                    .clone()
                    .or_else(|| Some(instrument.currency.clone())),
                false,
            )
            .await?;
            crate::engine_warn!(
                "market data refresh price currency mismatch instrument_id={} provider={} symbol={} expected_currency={} actual_currency={}; disabled mapping",
                instrument.id,
                source.provider,
                provider_symbol,
                instrument.currency,
                row.currency
            );
            return Ok(SourceRefreshOutcome::failed(RefreshItem {
                kind: RefreshItemKind::Price,
                instrument_id: Some(instrument.id),
                provider: Some(source.provider.to_string()),
                symbol_or_pair: provider_symbol,
                status: RefreshItemStatus::Failed,
                reason: Some("currency_mismatch".to_owned()),
                rows_written: 0,
            }));
        }

        for row in &rows {
            prices::upsert(
                pool,
                &prices::NewPrice {
                    instrument_id: instrument.id,
                    provider: source.provider,
                    provider_symbol: row.provider_symbol.clone(),
                    date: row.date,
                    close: row.close,
                    currency: row.currency.clone(),
                    fetched_at: now_iso8601(),
                },
            )
            .await?;
        }

        let reason = clamp_reason(instrument, source, &provider_symbol, &rows, window, mode);

        Ok(SourceRefreshOutcome::succeeded(RefreshItem {
            kind: RefreshItemKind::Price,
            instrument_id: Some(instrument.id),
            provider: Some(source.provider.to_string()),
            symbol_or_pair: provider_symbol,
            status: RefreshItemStatus::Fetched,
            reason,
            rows_written: rows.len(),
        }))
    }

    /// Rewrite one mapping's enabled flag (and, after a symbol recovery, its
    /// identifier) while preserving the asset class and currency that make a
    /// Nasdaq mapping fetchable at all.
    async fn persist_mapping(
        &self,
        pool: &SqlitePool,
        instrument: &crate::db::instruments::InstrumentRow,
        source: &PriceSourceMapping,
        provider_symbol: &str,
        currency: Option<String>,
        enabled: bool,
    ) -> Result<(), MarketDataError> {
        let now = now_iso8601();
        provider_symbols::upsert(
            pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: instrument.id,
                provider: source.provider,
                provider_symbol: provider_symbol.to_owned(),
                asset_class: source.asset_class.clone(),
                currency,
                enabled,
                created_at: now.clone(),
                updated_at: now,
            },
        )
        .await?;
        Ok(())
    }

    /// Yahoo mappings go through the changed-symbol recovery path; every other
    /// provider is a straight fetch against the mapping as stored.
    async fn price_history_for_source(
        &self,
        instrument: &crate::db::instruments::InstrumentRow,
        source: &PriceSourceMapping,
        window: &RefreshWindow,
    ) -> Result<ResolvedPriceHistory, ProviderError> {
        if source.provider == MarketDataProvider::Yahoo {
            return self
                .price_history_with_symbol_recovery(instrument, &source.provider_symbol, window)
                .await;
        }

        let client = self.price_provider(source.provider).ok_or_else(|| {
            ProviderError::provider_error(
                source.provider.as_str(),
                format!("no registered client for provider {}", source.provider),
            )
        })?;
        let rows = client
            .daily_history(&PriceHistoryRequest {
                symbol: source.provider_symbol.clone(),
                asset_class: source.asset_class.clone(),
                quote_currency: source.currency.clone(),
                start: window.start,
                end: window.end,
            })
            .await?;
        Ok(ResolvedPriceHistory::new(&source.provider_symbol, rows))
    }

    async fn price_history_with_symbol_recovery(
        &self,
        instrument: &crate::db::instruments::InstrumentRow,
        mapped_symbol: &str,
        window: &RefreshWindow,
    ) -> Result<ResolvedPriceHistory, ProviderError> {
        let Some(client) = self.price_provider(MarketDataProvider::Yahoo) else {
            return Err(ProviderError::provider_error(
                MarketDataProvider::Yahoo.as_str(),
                format!("no registered Yahoo price provider for {mapped_symbol}"),
            ));
        };
        let initial = client
            .daily_history(&PriceHistoryRequest {
                symbol: mapped_symbol.to_owned(),
                asset_class: None,
                quote_currency: None,
                start: window.start,
                end: window.end,
            })
            .await;

        match initial {
            Ok(rows) => {
                let latest_date = latest_close_date(&rows);
                if latest_date.is_some_and(|date| date >= window.end) {
                    return Ok(ResolvedPriceHistory::new(mapped_symbol, rows));
                }

                let Some(recovered) = self
                    .recover_changed_yahoo_symbol(instrument, mapped_symbol, window, latest_date)
                    .await
                else {
                    return Ok(ResolvedPriceHistory::new(mapped_symbol, rows));
                };

                Ok(ResolvedPriceHistory {
                    provider_symbol: recovered.provider_symbol,
                    rows: merge_price_histories(rows, recovered.rows),
                })
            }
            Err(error) => {
                if should_recover_symbol_after_error(&error) {
                    if let Some(recovered) = self
                        .recover_changed_yahoo_symbol(instrument, mapped_symbol, window, None)
                        .await
                    {
                        return Ok(recovered);
                    }
                }
                Err(error)
            }
        }
    }

    async fn recover_changed_yahoo_symbol(
        &self,
        instrument: &crate::db::instruments::InstrumentRow,
        mapped_symbol: &str,
        window: &RefreshWindow,
        mapped_latest_date: Option<NaiveDate>,
    ) -> Option<ResolvedPriceHistory> {
        if !instrument.exchange.trim().eq_ignore_ascii_case("AVANZA") {
            return None;
        }
        let name = instrument.name.trim();
        if name.is_empty() {
            return None;
        }
        let search = self.symbol_search_provider(MarketDataProvider::Yahoo)?;
        let mut queries = Vec::new();
        if let Some(isin) = instrument
            .isin
            .as_deref()
            .filter(|value| is_isin_like(value))
        {
            queries.push(isin.trim());
        }
        if queries
            .first()
            .is_none_or(|query| !query.eq_ignore_ascii_case(name))
        {
            queries.push(name);
        }

        let mut matches = Vec::new();
        for query in queries {
            match search.search(query).await {
                Ok(found) => matches.extend(found),
                Err(error) => {
                    crate::engine_warn!(
                        "market data symbol recovery search failed instrument_id={} query={:?} old_symbol={} reason={} message={}",
                        instrument.id,
                        query,
                        mapped_symbol,
                        error.reason_code(),
                        error.message()
                    );
                }
            }
        }

        let mut seen = BTreeSet::new();
        let candidates = matches
            .into_iter()
            .filter(|candidate| is_plausible_symbol_recovery(instrument, candidate))
            .filter(|candidate| {
                !candidate
                    .provider_symbol
                    .eq_ignore_ascii_case(mapped_symbol)
            })
            .filter(|candidate| seen.insert(candidate.provider_symbol.to_ascii_uppercase()))
            .take(MAX_SYMBOL_RECOVERY_CANDIDATES)
            .collect::<Vec<_>>();

        let client = self.price_provider(MarketDataProvider::Yahoo)?;
        let mut viable = Vec::new();
        for candidate in candidates {
            let rows = match client
                .daily_history(&PriceHistoryRequest {
                    symbol: candidate.provider_symbol.clone(),
                    asset_class: None,
                    quote_currency: None,
                    start: window.start,
                    end: window.end,
                })
                .await
            {
                Ok(rows) => rows,
                Err(_) => continue,
            };
            if rows.is_empty()
                || rows.iter().any(|row| {
                    !row.currency
                        .trim()
                        .eq_ignore_ascii_case(instrument.currency.trim())
                })
            {
                continue;
            }
            let Some(latest_date) = latest_close_date(&rows) else {
                continue;
            };
            if mapped_latest_date.is_some_and(|mapped_date| latest_date <= mapped_date) {
                continue;
            }
            viable.push((latest_date, candidate.provider_symbol, rows));
        }

        let newest_date = viable.iter().map(|(date, _, _)| *date).max()?;
        let mut newest = viable
            .into_iter()
            .filter(|(date, _, _)| *date == newest_date);
        let selected = newest.next()?;
        if newest.next().is_some() {
            crate::engine_warn!(
                "market data symbol recovery ambiguous instrument_id={} name={:?} old_symbol={} candidate_date={}",
                instrument.id,
                name,
                mapped_symbol,
                newest_date
            );
            return None;
        }

        Some(ResolvedPriceHistory {
            provider_symbol: selected.1,
            rows: selected.2,
        })
    }

    /// Create the mappings a refresh will then fetch through.
    ///
    /// Yahoo seeding is unchanged. Nasdaq is consulted only for instruments
    /// Yahoo cannot price — no enabled Yahoo mapping — so a working holding
    /// never gains a second source, and never costs a second call per refresh.
    ///
    /// Returns the instruments whose Nasdaq search was ambiguous, keyed by id,
    /// with a rendering of the candidates for the refresh item and the log.
    async fn seed_provider_symbols(
        &self,
        pool: &SqlitePool,
        instruments: &[crate::db::instruments::InstrumentRow],
    ) -> Result<BTreeMap<i64, String>, MarketDataError> {
        let mut ambiguous = BTreeMap::new();

        for instrument in instruments {
            let existing_mapping = provider_symbols::find_by_instrument_provider(
                pool,
                instrument.id,
                MarketDataProvider::Yahoo,
            )
            .await?;
            let known_seed = yahoo_seed_for_known_isin(instrument.isin.as_deref())
                .or_else(|| yahoo_seed_for_known_isin(Some(&instrument.symbol)));

            let seed = match &existing_mapping {
                Some(mapping) if mapping.enabled => None,
                Some(_) => known_seed,
                None => match known_seed
                    .or_else(|| yahoo_seed_for_exchange(&instrument.exchange, &instrument.symbol))
                {
                    Some(seed) => Some(seed),
                    None => self.yahoo_seed_from_search(instrument).await,
                },
            };

            let yahoo_enabled = match (&seed, &existing_mapping) {
                (Some(seed), _) => seed.enabled,
                (None, Some(mapping)) => mapping.enabled,
                (None, None) => false,
            };

            if let Some(seed) = seed {
                let now = now_iso8601();
                provider_symbols::upsert(
                    pool,
                    &provider_symbols::NewProviderSymbol {
                        instrument_id: instrument.id,
                        provider: MarketDataProvider::Yahoo,
                        provider_symbol: seed.provider_symbol,
                        asset_class: None,
                        currency: Some(instrument.currency.clone()),
                        enabled: seed.enabled,
                        created_at: now.clone(),
                        updated_at: now,
                    },
                )
                .await?;
            }

            if yahoo_enabled {
                continue;
            }

            if let Some(candidates) = self.seed_nasdaq_symbol(pool, instrument).await? {
                ambiguous.insert(instrument.id, candidates);
            }
        }

        Ok(ambiguous)
    }

    /// Auto-connect Nasdaq Nordic, but only when the answer is unambiguous.
    ///
    /// Candidates are narrowed to the instrument's own currency and connected
    /// only if exactly one survives. More than one survivor stays unmapped and
    /// is returned as an ambiguity; a bad guess would silently value the holding
    /// off the wrong exchange with nothing marking it a guess.
    async fn seed_nasdaq_symbol(
        &self,
        pool: &SqlitePool,
        instrument: &crate::db::instruments::InstrumentRow,
    ) -> Result<Option<String>, MarketDataError> {
        let existing = provider_symbols::find_by_instrument_provider(
            pool,
            instrument.id,
            MarketDataProvider::NasdaqNordic,
        )
        .await?;
        if existing.is_some() {
            return Ok(None);
        }

        let Some(query) = isin_like_identifier(instrument) else {
            return Ok(None);
        };
        let Some(search) = self.symbol_search_provider(MarketDataProvider::NasdaqNordic) else {
            return Ok(None);
        };

        let matches = match search.search(query).await {
            Ok(matches) => matches,
            Err(error) => {
                crate::engine_warn!(
                    "market data nasdaq search failed instrument_id={} isin={} reason={} message={}",
                    instrument.id,
                    query,
                    error.reason_code(),
                    error.message()
                );
                return Ok(None);
            }
        };

        let supported = matches
            .into_iter()
            .filter(is_supported_quote)
            .collect::<Vec<_>>();

        match symbol_matching::unique_currency_match(&instrument.currency, supported) {
            CurrencyMatch::Unique(candidate) => {
                let now = now_iso8601();
                provider_symbols::upsert(
                    pool,
                    &provider_symbols::NewProviderSymbol {
                        instrument_id: instrument.id,
                        provider: MarketDataProvider::NasdaqNordic,
                        provider_symbol: candidate.provider_symbol.clone(),
                        asset_class: candidate.asset_class.clone(),
                        currency: candidate.currency.clone(),
                        enabled: true,
                        created_at: now.clone(),
                        updated_at: now,
                    },
                )
                .await?;
                crate::engine_info!(
                    "market data connected nasdaq source instrument_id={} isin={} orderbook_id={} asset_class={:?} currency={:?}",
                    instrument.id,
                    query,
                    candidate.provider_symbol,
                    candidate.asset_class,
                    candidate.currency
                );
                Ok(None)
            }
            CurrencyMatch::Ambiguous(candidates) => {
                let described = symbol_matching::describe_candidates(&candidates);
                crate::engine_warn!(
                    "market data nasdaq match ambiguous instrument_id={} isin={} instrument_currency={} candidates=[{}]; needs a hand mapping",
                    instrument.id,
                    query,
                    instrument.currency,
                    described
                );
                Ok(Some(described))
            }
            CurrencyMatch::None => {
                crate::engine_info!(
                    "market data nasdaq search returned no same-currency match instrument_id={} isin={} instrument_currency={}",
                    instrument.id,
                    query,
                    instrument.currency
                );
                Ok(None)
            }
        }
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

        let search = self.symbol_search_provider(MarketDataProvider::Yahoo)?;
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
    /// The provider answered, but badly: a bad payload, a typed error envelope,
    /// or a currency that contradicts the instrument.
    Failed,
    Unmapped,
    /// A provider search returned more than one same-currency candidate. The
    /// instrument stays unmapped and needs a hand mapping.
    Ambiguous,
    /// The provider could not be reached at all. Distinct from `Failed` so a
    /// provider outage is explicit rather than a silent staleness slide.
    Unavailable,
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
    /// The feed this item is about, so a failure names the provider that failed.
    pub provider: Option<String>,
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

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SymbolSearchLookupMatch {
    pub provider: String,
    pub provider_symbol: String,
    pub quote_type: Option<String>,
    pub exchange: Option<String>,
    pub name: Option<String>,
    pub asset_class: Option<String>,
    pub currency: Option<String>,
}

impl From<SymbolSearchMatch> for SymbolSearchLookupMatch {
    fn from(value: SymbolSearchMatch) -> Self {
        Self {
            provider: value.provider.to_string(),
            provider_symbol: value.provider_symbol,
            quote_type: value.quote_type,
            exchange: value.exchange,
            name: value.name,
            asset_class: value.asset_class,
            currency: value.currency,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolSearchLookupStatus {
    Matches,
    NoMatch,
    ProviderUnavailable,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SymbolSearchLookupResponse {
    pub query: String,
    pub status: SymbolSearchLookupStatus,
    pub matches: Vec<SymbolSearchLookupMatch>,
}

impl SymbolSearchLookupResponse {
    pub(crate) fn matches(query: String, matches: Vec<SymbolSearchLookupMatch>) -> Self {
        Self {
            query,
            status: SymbolSearchLookupStatus::Matches,
            matches,
        }
    }

    pub(crate) fn no_match(query: String) -> Self {
        Self {
            query,
            status: SymbolSearchLookupStatus::NoMatch,
            matches: Vec::new(),
        }
    }

    pub(crate) fn provider_unavailable(query: String) -> Self {
        Self {
            query,
            status: SymbolSearchLookupStatus::ProviderUnavailable,
            matches: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct InstrumentMarketDataStatus {
    pub instrument_id: i64,
    pub exchange: String,
    pub symbol: String,
    pub currency: String,
    /// Every mapping this instrument has, enabled or not, in precedence order.
    /// This replaces the single `mapping_enabled` / `provider_symbol` pair,
    /// which has no honest single-valued meaning once two sources exist.
    pub price_sources: Vec<PriceSourceStatus>,
    /// The provider code behind `latest_price`, or `None` when nothing resolved.
    pub effective_price_source: Option<String>,
    pub open_quantity: i64,
    pub latest_price: PriceSnapshotState,
    pub latest_fx: PriceSnapshotState,
}

#[derive(Clone, Debug, Serialize)]
pub struct PriceSourceStatus {
    pub provider: String,
    pub provider_symbol: String,
    pub asset_class: Option<String>,
    pub currency: Option<String>,
    pub enabled: bool,
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

struct ResolvedPriceHistory {
    provider_symbol: String,
    rows: Vec<crate::providers::DailyClose>,
}

impl ResolvedPriceHistory {
    fn new(provider_symbol: &str, rows: Vec<crate::providers::DailyClose>) -> Self {
        Self {
            provider_symbol: provider_symbol.to_owned(),
            rows,
        }
    }
}

struct RefreshTarget {
    instrument: crate::db::instruments::InstrumentRow,
    currency: String,
}

/// The result of fetching one instrument from one of its sources.
struct SourceRefreshOutcome {
    item: RefreshItem,
    failed: bool,
}

impl SourceRefreshOutcome {
    fn succeeded(item: RefreshItem) -> Self {
        Self {
            item,
            failed: false,
        }
    }

    fn failed(item: RefreshItem) -> Self {
        Self { item, failed: true }
    }
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
        ProviderMissingReason::ProviderUnavailable => RefreshItemStatus::Unavailable,
    }
}

fn should_recover_symbol_after_error(error: &ProviderError) -> bool {
    matches!(
        error.reason(),
        ProviderMissingReason::SymbolUnmapped
            | ProviderMissingReason::NotListed
            | ProviderMissingReason::NoDataInRange
    )
}

fn latest_close_date(rows: &[crate::providers::DailyClose]) -> Option<NaiveDate> {
    rows.iter().map(|row| row.date).max()
}

fn merge_price_histories(
    existing: Vec<crate::providers::DailyClose>,
    replacement: Vec<crate::providers::DailyClose>,
) -> Vec<crate::providers::DailyClose> {
    let mut by_date = BTreeMap::new();
    for row in existing.into_iter().chain(replacement) {
        by_date.insert(row.date, row);
    }
    by_date.into_values().collect()
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

fn position_for_instrument(
    grouped: &BTreeMap<i64, Vec<crate::db::transactions::TransactionRow>>,
    instrument_id: i64,
) -> Result<domain::Position, MarketDataError> {
    let ledger = match grouped.get(&instrument_id) {
        Some(rows) => rows
            .iter()
            .map(|row| row.to_ledger())
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    domain::derive_position(&ledger).map_err(|error| {
        MarketDataError::internal(format!(
            "inconsistent stored ledger for instrument {}: {error:?}",
            instrument_id
        ))
    })
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

/// Every mapping an instrument has, enabled or not, in precedence order.
///
/// `effective_prices::enabled_price_sources` deliberately drops disabled rows
/// because valuation must never read behind them; the status endpoint needs the
/// full list so the UI can say a source exists but is switched off.
async fn all_price_sources(
    pool: &SqlitePool,
    instrument_id: i64,
) -> Result<Vec<PriceSourceStatus>, MarketDataError> {
    let mut sources = Vec::new();
    for provider in PRICE_PROVIDER_PRECEDENCE {
        let mapping =
            provider_symbols::find_by_instrument_provider(pool, instrument_id, *provider).await?;
        if let Some(mapping) = mapping {
            sources.push(PriceSourceStatus {
                provider: mapping.provider.to_string(),
                provider_symbol: mapping.provider_symbol,
                asset_class: mapping.asset_class,
                currency: mapping.currency,
                enabled: mapping.enabled,
            });
        }
    }
    Ok(sources)
}

/// Resolve the latest close across every enabled source, so `/api/prices/status`
/// agrees with what valuation actually used.
///
/// `Unmapped` now means "no enabled source at all", not "the Yahoo mapping is
/// off": a disabled mapping alongside an enabled one reports the resolved price.
async fn latest_price_snapshot(
    pool: &SqlitePool,
    instrument_id: i64,
    sources: &[PriceSourceMapping],
    as_of_date: NaiveDate,
) -> Result<PriceSnapshotState, MarketDataError> {
    if sources.is_empty() {
        return Ok(PriceSnapshotState::unmapped());
    }

    let candidate =
        effective_prices::effective_latest_on_or_before(pool, instrument_id, as_of_date).await?;
    let Some(candidate) = candidate else {
        return Ok(PriceSnapshotState::missing("missing_price"));
    };

    let provider = candidate.source.as_str().to_owned();
    let provider_symbol = sources
        .iter()
        .find(|source| source.provider.as_str() == provider)
        .map(|source| source.provider_symbol.clone())
        .unwrap_or_default();

    Ok(PriceSnapshotState::available(
        candidate.date.format("%Y-%m-%d").to_string(),
        candidate.close.to_string(),
        provider,
        provider_symbol,
    ))
}

/// Render the per-provider row split for the run message and the finished log.
fn describe_provider_split(rows_by_provider: &[(MarketDataProvider, usize)]) -> String {
    if rows_by_provider.is_empty() {
        return "none".to_owned();
    }
    let mut entries = rows_by_provider.to_vec();
    entries.sort_by_key(|(provider, _)| precedence_index(*provider).unwrap_or(usize::MAX));
    entries
        .into_iter()
        .map(|(provider, rows)| format!("{provider}={rows}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn add_provider_rows(
    rows_by_provider: &mut Vec<(MarketDataProvider, usize)>,
    provider: MarketDataProvider,
    rows: usize,
) {
    match rows_by_provider
        .iter_mut()
        .find(|(registered, _)| *registered == provider)
    {
        Some((_, total)) => *total += rows,
        None => rows_by_provider.push((provider, rows)),
    }
}

/// Detect a provider silently truncating requested history.
///
/// Nasdaq clamps to roughly ten years with no error and no warning, and a
/// backfill cannot otherwise tell "no data" from "older than the provider keeps".
/// This is a heuristic with benign false positives — a recently-listed
/// instrument or a long holiday stretch trips it with no clamp involved — so it
/// is warning-only and the reason must not be read as proof of a clamp.
fn clamp_reason(
    instrument: &crate::db::instruments::InstrumentRow,
    source: &PriceSourceMapping,
    provider_symbol: &str,
    rows: &[crate::providers::DailyClose],
    window: &RefreshWindow,
    mode: RefreshMode,
) -> Option<String> {
    if mode != RefreshMode::Backfill || source.provider != MarketDataProvider::NasdaqNordic {
        return None;
    }
    let earliest = rows.iter().map(|row| row.date).min()?;
    if earliest - window.start <= Duration::days(HISTORY_CLAMP_TOLERANCE_DAYS) {
        return None;
    }

    crate::engine_warn!(
        "market data history clamped instrument_id={} provider={} orderbook_id={} requested_start={} actual_start={}",
        instrument.id,
        source.provider,
        provider_symbol,
        window.start,
        earliest
    );
    Some("history_clamped".to_owned())
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

    let row = fx_rates::find_latest_on_or_before(pool, currency, SEK, BASE_FX_PROVIDER, as_of_date)
        .await?;
    Ok(match row {
        Some(row) => PriceSnapshotState::available(
            row.date,
            row.rate,
            row.provider.to_string(),
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

    async fn sell(
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
                kind: domain::TransactionKind::Sell,
                trade_date: NaiveDate::parse_from_str(trade_date, "%Y-%m-%d").expect("date"),
                quantity: -quantity,
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

    async fn map_yahoo_symbol(
        pool: &SqlitePool,
        instrument_id: i64,
        provider_symbol: &str,
        enabled: bool,
    ) {
        let now = now_iso8601();
        provider_symbols::upsert(
            pool,
            &crate::db::provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: MarketDataProvider::Yahoo,
                provider_symbol: provider_symbol.to_owned(),
                asset_class: None,
                currency: Some("SEK".to_owned()),
                enabled,
                created_at: now.clone(),
                updated_at: now,
            },
        )
        .await
        .expect("provider symbol upsert should succeed");
    }

    async fn set_conviction(pool: &SqlitePool, instrument_id: i64, conviction: &str) {
        instruments::update_conviction(pool, instrument_id, conviction)
            .await
            .expect("update conviction should succeed");
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

        let mapping =
            provider_symbols::find_by_instrument_provider(&pool, msft, MarketDataProvider::Yahoo)
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
    async fn latest_refresh_fetches_never_traded_convicted_instrument() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "WATCH".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(101),
            currency: "SEK".to_owned(),
        }]));
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

        let (pool, service) = test_state(price_provider.clone(), fx_provider).await;
        let watch = instrument(&pool, "WATCH", "NASDAQ", "SEK").await;
        map_yahoo_symbol(&pool, watch, "WATCH", true).await;
        set_conviction(&pool, watch, "LOW").await;

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
        assert_eq!(response.fx_rates_written, 0);
        assert_eq!(price_provider.calls().len(), 1);
        assert_eq!(price_provider.calls()[0].symbol, "WATCH");
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].status, RefreshItemStatus::Fetched);
        assert_eq!(response.items[0].symbol_or_pair, "WATCH");
    }

    #[tokio::test]
    async fn latest_refresh_skips_closed_other_instrument() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

        let (pool, service) = test_state(price_provider.clone(), fx_provider).await;
        let closed = instrument(&pool, "CLOSED", "NASDAQ", "SEK").await;
        buy(&pool, closed, "2026-06-01", 10, "100", "SEK", None).await;
        sell(&pool, closed, "2026-06-02", 10, "100", "SEK", None).await;
        map_yahoo_symbol(&pool, closed, "CLOSED", true).await;

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

        assert!(price_provider.calls().is_empty());
        assert!(response.items.is_empty());
    }

    #[tokio::test]
    async fn latest_refresh_fetches_closed_convicted_instrument() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "CLOSED".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(101),
            currency: "SEK".to_owned(),
        }]));
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

        let (pool, service) = test_state(price_provider.clone(), fx_provider).await;
        let closed = instrument(&pool, "CLOSED", "NASDAQ", "SEK").await;
        buy(&pool, closed, "2026-06-01", 10, "100", "SEK", None).await;
        sell(&pool, closed, "2026-06-02", 10, "100", "SEK", None).await;
        map_yahoo_symbol(&pool, closed, "CLOSED", true).await;
        set_conviction(&pool, closed, "HIGH").await;

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
        assert_eq!(response.fx_rates_written, 0);
        assert_eq!(price_provider.calls().len(), 1);
        assert_eq!(price_provider.calls()[0].symbol, "CLOSED");
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].status, RefreshItemStatus::Fetched);
    }

    #[tokio::test]
    async fn status_readiness_includes_convicted_never_traded_instrument() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

        let (pool, service) = test_state(price_provider, fx_provider).await;
        let watch = instrument(&pool, "WATCH", "NASDAQ", "SEK").await;
        map_yahoo_symbol(&pool, watch, "WATCH", true).await;
        set_conviction(&pool, watch, "MEDIUM").await;

        let status = service.status(&pool).await.expect("status should succeed");
        assert_eq!(status.instruments.len(), 1);
        assert_eq!(status.instruments[0].symbol, "WATCH");
        assert_eq!(status.instruments[0].open_quantity, 0);
        let sources = &status.instruments[0].price_sources;
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].provider, "YAHOO");
        assert_eq!(sources[0].provider_symbol, "WATCH");
        assert!(sources[0].enabled);
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
            asset_class: None,
            currency: None,
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

        let mapping =
            provider_symbols::find_by_instrument_provider(&pool, msft, MarketDataProvider::Yahoo)
                .await
                .expect("mapping lookup should succeed")
                .expect("mapping should exist");
        assert!(mapping.enabled);
        assert_eq!(mapping.provider_symbol, "MSFT");
    }

    #[tokio::test]
    async fn avanza_known_isins_seed_yahoo_mappings_without_search() {
        let today = Utc::now().date_naive();
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "IQQK.DE".to_owned(),
            date: today,
            close: dec!(115),
            currency: "EUR".to_owned(),
        }]));
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "GOOGL".to_owned(),
            date: today,
            close: dec!(245),
            currency: "USD".to_owned(),
        }]));
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "TSM".to_owned(),
            date: today,
            close: dec!(243),
            currency: "USD".to_owned(),
        }]));

        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "EUR".to_owned(),
            quote: "SEK".to_owned(),
            date: today,
            rate: dec!(11),
        }]));
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: today,
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
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "IDKO.L".to_owned(),
                asset_class: None,
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
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "TSMN.MX".to_owned(),
                asset_class: None,
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
            let mapping = provider_symbols::find_by_instrument_provider(
                &pool,
                instrument_id,
                MarketDataProvider::Yahoo,
            )
            .await
            .expect("mapping lookup should succeed")
            .expect("mapping should exist");
            assert!(mapping.enabled);
            assert_eq!(mapping.provider_symbol, expected_symbol);
        }
    }

    #[tokio::test]
    async fn stale_avanza_symbol_is_replaced_only_after_newer_candidate_is_verified() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "SKHYV".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 7, 10).expect("date"),
            close: dec!(168),
            currency: "USD".to_owned(),
        }]));
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "SKHY".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 7, 13).expect("date"),
            close: dec!(170),
            currency: "USD".to_owned(),
        }]));
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 7, 13).expect("date"),
            rate: dec!(9.65),
        }]));
        let symbol_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        symbol_search.push_response(Ok(vec![SymbolSearchMatch {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "SKHYV".to_owned(),
            quote_type: Some("EQUITY".to_owned()),
            exchange: Some("NGM".to_owned()),
            name: Some("SK hynix Inc.".to_owned()),
            asset_class: None,
            currency: None,
        }]));
        symbol_search.push_response(Ok(vec![
            SymbolSearchMatch {
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "SKHYV".to_owned(),
                quote_type: Some("EQUITY".to_owned()),
                exchange: Some("NGM".to_owned()),
                name: Some("SK hynix Inc.".to_owned()),
                asset_class: None,
                currency: None,
            },
            SymbolSearchMatch {
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "SKHY".to_owned(),
                quote_type: Some("EQUITY".to_owned()),
                exchange: Some("NGM".to_owned()),
                name: Some("SK hynix Inc.".to_owned()),
                asset_class: None,
                currency: None,
            },
        ]));

        let pool = db::memory_pool().await.expect("memory pool");
        let service = MarketDataService::with_symbol_search_providers(
            price_provider.clone(),
            fx_provider,
            symbol_search.clone(),
        );
        let (instrument, _) = instruments::upsert(
            &pool,
            &crate::db::instruments::NewInstrument {
                symbol: "US78392B2060".to_owned(),
                exchange: "AVANZA".to_owned(),
                name: "SK Hynix Inc".to_owned(),
                kind: "STOCK".to_owned(),
                currency: "USD".to_owned(),
                isin: Some("US78392B2060".to_owned()),
            },
        )
        .await
        .expect("instrument should insert");
        let sk_hynix = instrument.id;
        map_yahoo_symbol(&pool, sk_hynix, "SKHYV", true).await;
        buy(
            &pool,
            sk_hynix,
            "2026-07-10",
            10,
            "168",
            "USD",
            Some("9.65"),
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

        assert_eq!(response.status, RefreshRunStatus::Succeeded);
        assert_eq!(response.prices_written, 2);
        assert_eq!(price_provider.calls()[0].symbol, "SKHYV");
        assert_eq!(price_provider.calls()[1].symbol, "SKHY");
        assert_eq!(symbol_search.calls()[0].query, "US78392B2060");
        assert_eq!(symbol_search.calls()[1].query, "SK Hynix Inc");
        let mapping = provider_symbols::find_by_instrument_provider(
            &pool,
            sk_hynix,
            MarketDataProvider::Yahoo,
        )
        .await
        .expect("mapping lookup should succeed")
        .expect("mapping should exist");
        assert!(mapping.enabled);
        assert_eq!(mapping.provider_symbol, "SKHY");
        assert_eq!(mapping.currency.as_deref(), Some("USD"));
    }

    #[tokio::test]
    async fn stale_avanza_symbol_is_preserved_when_candidate_is_not_newer() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        for symbol in ["OLD", "POSSIBLE"] {
            price_provider.push_response(Ok(vec![DailyClose {
                provider: MarketDataProvider::Yahoo,
                provider_symbol: symbol.to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 7, 10).expect("date"),
                close: dec!(168),
                currency: "USD".to_owned(),
            }]));
        }
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 7, 13).expect("date"),
            rate: dec!(9.65),
        }]));
        let symbol_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        symbol_search.push_response(Ok(vec![SymbolSearchMatch {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "OLD".to_owned(),
            quote_type: Some("EQUITY".to_owned()),
            exchange: Some("NGM".to_owned()),
            name: Some("Example Inc.".to_owned()),
            asset_class: None,
            currency: None,
        }]));
        symbol_search.push_response(Ok(vec![SymbolSearchMatch {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "POSSIBLE".to_owned(),
            quote_type: Some("EQUITY".to_owned()),
            exchange: Some("NGM".to_owned()),
            name: Some("Example Inc.".to_owned()),
            asset_class: None,
            currency: None,
        }]));

        let pool = db::memory_pool().await.expect("memory pool");
        let service = MarketDataService::with_symbol_search_providers(
            price_provider.clone(),
            fx_provider,
            symbol_search,
        );
        let (instrument, _) = instruments::upsert(
            &pool,
            &crate::db::instruments::NewInstrument {
                symbol: "US0000000001".to_owned(),
                exchange: "AVANZA".to_owned(),
                name: "Example Inc".to_owned(),
                kind: "STOCK".to_owned(),
                currency: "USD".to_owned(),
                isin: Some("US0000000001".to_owned()),
            },
        )
        .await
        .expect("instrument should insert");
        map_yahoo_symbol(&pool, instrument.id, "OLD", true).await;
        buy(
            &pool,
            instrument.id,
            "2026-07-10",
            10,
            "168",
            "USD",
            Some("9.65"),
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

        assert_eq!(response.status, RefreshRunStatus::Succeeded);
        assert_eq!(response.prices_written, 1);
        assert_eq!(price_provider.calls().len(), 2);
        let mapping = provider_symbols::find_by_instrument_provider(
            &pool,
            instrument.id,
            MarketDataProvider::Yahoo,
        )
        .await
        .expect("mapping lookup should succeed")
        .expect("mapping should exist");
        assert_eq!(mapping.provider_symbol, "OLD");
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
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "ASML.DE".to_owned(),
                asset_class: None,
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
    async fn backfill_refresh_skips_never_traded_instruments() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "HELD.ST".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
            close: dec!(100),
            currency: "SEK".to_owned(),
        }]));
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

        let (pool, service) = test_state(price_provider.clone(), fx_provider).await;
        let held = instrument(&pool, "HELD", "XSTO", "SEK").await;
        let convicted_watch = instrument(&pool, "WATCH", "NASDAQ", "SEK").await;
        let other_watch = instrument(&pool, "OTHER", "NASDAQ", "SEK").await;
        buy(&pool, held, "2026-06-10", 3, "100", "SEK", None).await;
        map_yahoo_symbol(&pool, held, "HELD.ST", true).await;
        map_yahoo_symbol(&pool, convicted_watch, "WATCH", true).await;
        map_yahoo_symbol(&pool, other_watch, "OTHER", true).await;
        set_conviction(&pool, convicted_watch, "HIGH").await;

        let response = service
            .refresh(
                &pool,
                RefreshTrigger::Backfill,
                RefreshPricesRequest {
                    mode: RefreshMode::Backfill,
                    start_date: None,
                    end_date: Some("2026-06-12".to_owned()),
                },
            )
            .await
            .expect("refresh should succeed");

        let calls = price_provider.calls();
        assert_eq!(response.status, RefreshRunStatus::Succeeded);
        assert_eq!(response.prices_written, 1);
        assert_eq!(response.unmapped_instruments, 0);
        assert_eq!(response.failed_items, 0);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].symbol, "HELD.ST");
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

    // ----- Multi-provider refresh -------------------------------------------

    fn nasdaq_search_match(
        orderbook_id: &str,
        currency: &str,
        asset_class: &str,
    ) -> SymbolSearchMatch {
        SymbolSearchMatch {
            provider: MarketDataProvider::NasdaqNordic,
            provider_symbol: orderbook_id.to_owned(),
            quote_type: None,
            exchange: Some("Warrants".to_owned()),
            name: Some("AVA SAMSUNG TRACKER".to_owned()),
            asset_class: Some(asset_class.to_owned()),
            currency: Some(currency.to_owned()),
        }
    }

    fn nasdaq_close(orderbook_id: &str, day: u32, close: &str) -> DailyClose {
        DailyClose {
            provider: MarketDataProvider::NasdaqNordic,
            provider_symbol: orderbook_id.to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, day).expect("date"),
            close: close.parse().expect("close"),
            currency: "SEK".to_owned(),
        }
    }

    /// A service with Yahoo and Nasdaq registered as both price and search
    /// providers, plus FX, so a run exercises the full dispatch.
    fn multi_provider_service(
        yahoo_price: FakePriceProvider,
        yahoo_search: FakeSymbolSearchProvider,
        nasdaq_price: FakePriceProvider,
        nasdaq_search: FakeSymbolSearchProvider,
        fx_provider: FakeFxRateProvider,
    ) -> MarketDataService {
        MarketDataService::with_provider_registry(
            ProviderRegistry::new()
                .with_price_provider(MarketDataProvider::Yahoo, yahoo_price)
                .with_price_provider(MarketDataProvider::NasdaqNordic, nasdaq_price)
                .with_symbol_search_provider(MarketDataProvider::Yahoo, yahoo_search)
                .with_symbol_search_provider(MarketDataProvider::NasdaqNordic, nasdaq_search)
                .with_fx_provider(fx_provider),
        )
    }

    fn silent_fake_price() -> FakePriceProvider {
        FakePriceProvider::with_provider(MarketDataProvider::Yahoo)
    }

    fn empty_search(provider: MarketDataProvider) -> FakeSymbolSearchProvider {
        let search = FakeSymbolSearchProvider::with_provider(provider);
        for _ in 0..8 {
            search.push_response(Ok(Vec::new()));
        }
        search
    }

    async fn map_nasdaq_symbol(
        pool: &SqlitePool,
        instrument_id: i64,
        orderbook_id: &str,
        asset_class: &str,
        currency: &str,
        enabled: bool,
    ) {
        let now = now_iso8601();
        provider_symbols::upsert(
            pool,
            &crate::db::provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: MarketDataProvider::NasdaqNordic,
                provider_symbol: orderbook_id.to_owned(),
                asset_class: Some(asset_class.to_owned()),
                currency: Some(currency.to_owned()),
                enabled,
                created_at: now.clone(),
                updated_at: now,
            },
        )
        .await
        .expect("nasdaq mapping upsert should succeed");
    }

    /// The reported defect: instrument 32's shape. Yahoo carries nothing, so the
    /// instrument had no mapping, no prices and no valuation at all.
    #[tokio::test]
    async fn instrument_yahoo_cannot_price_is_valued_from_nasdaq() {
        let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_price.push_response(Ok(vec![
            nasdaq_close("TX2997672", 10, "123.30"),
            nasdaq_close("TX2997672", 11, "124.10"),
        ]));
        let nasdaq_search =
            FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_search.push_response(Ok(vec![nasdaq_search_match(
            "TX2997672",
            "SEK",
            "TRACKER_CERTIFICATES",
        )]));

        let service = multi_provider_service(
            silent_fake_price(),
            empty_search(MarketDataProvider::Yahoo),
            nasdaq_price,
            nasdaq_search,
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let pool = db::memory_pool().await.expect("memory pool");
        let tracker =
            instrument_with_isin(&pool, "JE00BJ7HNC92", "AVANZA", "SEK", "JE00BJ7HNC92").await;
        buy(&pool, tracker, "2026-06-01", 10, "120", "SEK", None).await;

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
        assert_eq!(response.unmapped_instruments, 0);
        assert_eq!(response.prices_written, 2);
        assert_eq!(response.items[0].provider.as_deref(), Some("NASDAQ_NORDIC"));

        let mapping = provider_symbols::find_by_instrument_provider(
            &pool,
            tracker,
            MarketDataProvider::NasdaqNordic,
        )
        .await
        .expect("mapping lookup should succeed")
        .expect("nasdaq mapping should exist");
        assert!(mapping.enabled);
        assert_eq!(mapping.provider_symbol, "TX2997672");
        assert_eq!(mapping.asset_class.as_deref(), Some("TRACKER_CERTIFICATES"));
        assert_eq!(mapping.currency.as_deref(), Some("SEK"));

        let candidate = effective_prices::effective_latest_on_or_before(
            &pool,
            tracker,
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
        )
        .await
        .expect("resolution should succeed")
        .expect("a price should resolve");
        assert_eq!(candidate.source.as_str(), "NASDAQ_NORDIC");
        assert_eq!(candidate.close, dec!(124.10));
    }

    /// Instrument 38's shape: a disabled Yahoo mapping with stored rows in the
    /// wrong currency. Those rows must never be promoted into valuation, and the
    /// disabled mapping must not be switched back on.
    #[tokio::test]
    async fn disabled_yahoo_mapping_is_repaired_by_nasdaq_without_promoting_its_rows() {
        let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_price.push_response(Ok(vec![nasdaq_close("TX271", 11, "1369.00")]));
        let nasdaq_search =
            FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_search.push_response(Ok(vec![nasdaq_search_match("TX271", "SEK", "SHARES")]));

        let service = multi_provider_service(
            silent_fake_price(),
            empty_search(MarketDataProvider::Yahoo),
            nasdaq_price,
            nasdaq_search,
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let pool = db::memory_pool().await.expect("memory pool");
        let azn =
            instrument_with_isin(&pool, "GB0009895292", "AVANZA", "SEK", "GB0009895292").await;
        buy(&pool, azn, "2026-06-01", 10, "1300", "SEK", None).await;
        map_yahoo_symbol(&pool, azn, "AZN.L", false).await;
        prices::upsert(
            &pool,
            &prices::NewPrice {
                instrument_id: azn,
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "AZN.L".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
                close: dec!(110.00),
                currency: "GBP".to_owned(),
                fetched_at: now_iso8601(),
            },
        )
        .await
        .expect("stored yahoo row should insert");

        service
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

        let yahoo =
            provider_symbols::find_by_instrument_provider(&pool, azn, MarketDataProvider::Yahoo)
                .await
                .expect("mapping lookup should succeed")
                .expect("yahoo mapping should still exist");
        assert!(!yahoo.enabled, "a disabled mapping must not be re-enabled");

        let candidate = effective_prices::effective_latest_on_or_before(
            &pool,
            azn,
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
        )
        .await
        .expect("resolution should succeed")
        .expect("a price should resolve");
        assert_eq!(candidate.source.as_str(), "NASDAQ_NORDIC");
        assert_eq!(candidate.close, dec!(1369.00));
    }

    #[tokio::test]
    async fn two_same_currency_candidates_stay_unmapped_and_report_ambiguous() {
        let nasdaq_search =
            FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_search.push_response(Ok(vec![
            nasdaq_search_match("TX69", "SEK", "SHARES"),
            nasdaq_search_match("TX70", "SEK", "SHARES"),
        ]));

        let yahoo_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        yahoo_price.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "WORKS".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(50.00),
            currency: "SEK".to_owned(),
        }]));
        let service = multi_provider_service(
            yahoo_price,
            empty_search(MarketDataProvider::Yahoo),
            FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic),
            nasdaq_search,
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let pool = db::memory_pool().await.expect("memory pool");
        // A holding that prices normally, so the run is a realistic PARTIAL
        // rather than the degenerate "nothing was written at all" case.
        let works = instrument(&pool, "WORKS", "STO", "SEK").await;
        buy(&pool, works, "2026-06-01", 10, "50", "SEK", None).await;
        map_yahoo_symbol(&pool, works, "WORKS", true).await;
        let ericsson =
            instrument_with_isin(&pool, "SE0000108656", "AVANZA", "SEK", "SE0000108656").await;
        buy(&pool, ericsson, "2026-06-01", 10, "80", "SEK", None).await;

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
            .expect("refresh should complete");

        assert_eq!(response.status, RefreshRunStatus::Partial);
        assert_eq!(response.unmapped_instruments, 1);
        let ambiguous = response
            .items
            .iter()
            .find(|item| item.instrument_id == Some(ericsson))
            .expect("the ambiguous instrument should be reported");
        assert_eq!(ambiguous.status, RefreshItemStatus::Ambiguous);
        assert!(ambiguous
            .reason
            .as_deref()
            .expect("reason")
            .contains("TX69"));

        assert!(provider_symbols::find_by_instrument_provider(
            &pool,
            ericsson,
            MarketDataProvider::NasdaqNordic
        )
        .await
        .expect("mapping lookup should succeed")
        .is_none());
    }

    #[tokio::test]
    async fn currency_narrowing_connects_the_matching_listing() {
        let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_price.push_response(Ok(vec![nasdaq_close("TX69", 11, "78.20")]));
        let nasdaq_search =
            FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_search.push_response(Ok(vec![
            nasdaq_search_match("TX50143", "EUR", "SHARES"),
            nasdaq_search_match("TX69", "SEK", "SHARES"),
        ]));

        let service = multi_provider_service(
            silent_fake_price(),
            empty_search(MarketDataProvider::Yahoo),
            nasdaq_price,
            nasdaq_search,
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let pool = db::memory_pool().await.expect("memory pool");
        let ericsson =
            instrument_with_isin(&pool, "SE0000108656", "AVANZA", "SEK", "SE0000108656").await;
        buy(&pool, ericsson, "2026-06-01", 10, "80", "SEK", None).await;

        service
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

        let mapping = provider_symbols::find_by_instrument_provider(
            &pool,
            ericsson,
            MarketDataProvider::NasdaqNordic,
        )
        .await
        .expect("mapping lookup should succeed")
        .expect("nasdaq mapping should exist");
        assert_eq!(mapping.provider_symbol, "TX69");
        assert_eq!(mapping.currency.as_deref(), Some("SEK"));
    }

    /// Yahoo wins a shared date; Nasdaq fills the dates Yahoo does not cover.
    #[tokio::test]
    async fn per_date_precedence_prefers_yahoo_and_lets_nasdaq_fill_gaps() {
        let yahoo_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        yahoo_price.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "ERIC-B.ST".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(78.00),
            currency: "SEK".to_owned(),
        }]));
        let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_price.push_response(Ok(vec![
            nasdaq_close("TX69", 11, "77.90"),
            nasdaq_close("TX69", 12, "79.40"),
        ]));

        let service = multi_provider_service(
            yahoo_price,
            empty_search(MarketDataProvider::Yahoo),
            nasdaq_price,
            empty_search(MarketDataProvider::NasdaqNordic),
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let pool = db::memory_pool().await.expect("memory pool");
        let ericsson = instrument(&pool, "ERIC", "STO", "SEK").await;
        buy(&pool, ericsson, "2026-06-01", 10, "80", "SEK", None).await;
        map_yahoo_symbol(&pool, ericsson, "ERIC-B.ST", true).await;
        map_nasdaq_symbol(&pool, ericsson, "TX69", "SHARES", "SEK", true).await;

        service
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

        let series = effective_prices::effective_series(&pool, ericsson, "SEK", None, None)
            .await
            .expect("series should resolve");
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].source.as_str(), "YAHOO");
        assert_eq!(series[0].close, dec!(78.00));
        assert_eq!(series[1].source.as_str(), "NASDAQ_NORDIC");
        assert_eq!(series[1].close, dec!(79.40));

        let latest = effective_prices::effective_latest_on_or_before(
            &pool,
            ericsson,
            NaiveDate::from_ymd_opt(2026, 6, 13).expect("date"),
        )
        .await
        .expect("resolution should succeed")
        .expect("a price should resolve");
        assert_eq!(latest.source.as_str(), "NASDAQ_NORDIC");
    }

    /// PARTIAL must keep meaning "action needed": a Nasdaq-only instrument is
    /// fully priced and must not inflate the unmapped counter.
    #[tokio::test]
    async fn partial_still_means_action_needed() {
        let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_price.push_response(Ok(vec![nasdaq_close("TX69", 11, "78.20")]));

        let service = multi_provider_service(
            silent_fake_price(),
            empty_search(MarketDataProvider::Yahoo),
            nasdaq_price,
            empty_search(MarketDataProvider::NasdaqNordic),
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let pool = db::memory_pool().await.expect("memory pool");
        let nordic = instrument(&pool, "ERIC", "STO", "SEK").await;
        buy(&pool, nordic, "2026-06-01", 10, "80", "SEK", None).await;
        map_nasdaq_symbol(&pool, nordic, "TX69", "SHARES", "SEK", true).await;

        let succeeded = service
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
        assert_eq!(succeeded.status, RefreshRunStatus::Succeeded);
        assert_eq!(succeeded.unmapped_instruments, 0);

        let orphan = instrument(&pool, "NOFEED", "STO", "SEK").await;
        buy(&pool, orphan, "2026-06-01", 10, "80", "SEK", None).await;

        let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_price.push_response(Ok(vec![nasdaq_close("TX69", 11, "78.20")]));
        let service = multi_provider_service(
            silent_fake_price(),
            empty_search(MarketDataProvider::Yahoo),
            nasdaq_price,
            empty_search(MarketDataProvider::NasdaqNordic),
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let partial = service
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
            .expect("refresh should complete");
        assert_eq!(partial.status, RefreshRunStatus::Partial);
        assert_eq!(partial.unmapped_instruments, 1);
    }

    /// A provider outage is named rather than silently sliding into staleness,
    /// and it does not stop the other sources from being written.
    #[tokio::test]
    async fn provider_outage_reports_unavailable_and_leaves_other_sources_intact() {
        let yahoo_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        yahoo_price.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "ERIC-B.ST".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(78.00),
            currency: "SEK".to_owned(),
        }]));
        let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_price.push_response(Err(ProviderError::transport(
            MarketDataProvider::NasdaqNordic.as_str(),
            "connection refused",
        )));

        let service = multi_provider_service(
            yahoo_price,
            empty_search(MarketDataProvider::Yahoo),
            nasdaq_price,
            empty_search(MarketDataProvider::NasdaqNordic),
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let pool = db::memory_pool().await.expect("memory pool");
        let ericsson = instrument(&pool, "ERIC", "STO", "SEK").await;
        buy(&pool, ericsson, "2026-06-01", 10, "80", "SEK", None).await;
        map_yahoo_symbol(&pool, ericsson, "ERIC-B.ST", true).await;
        map_nasdaq_symbol(&pool, ericsson, "TX69", "SHARES", "SEK", true).await;
        // A stored Nasdaq row from an earlier run: valuation must fall back to
        // it with normal staleness rather than losing the holding.
        prices::upsert(
            &pool,
            &prices::NewPrice {
                instrument_id: ericsson,
                provider: MarketDataProvider::NasdaqNordic,
                provider_symbol: "TX69".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 5).expect("date"),
                close: dec!(74.10),
                currency: "SEK".to_owned(),
                fetched_at: now_iso8601(),
            },
        )
        .await
        .expect("stored nasdaq row should insert");

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
            .expect("refresh should complete");

        let outage = response
            .items
            .iter()
            .find(|item| item.status == RefreshItemStatus::Unavailable)
            .expect("an unavailable item should be reported");
        assert_eq!(outage.provider.as_deref(), Some("NASDAQ_NORDIC"));
        assert_eq!(outage.reason.as_deref(), Some("provider_unavailable"));

        assert_eq!(response.prices_written, 1);
        let series = effective_prices::effective_series(&pool, ericsson, "SEK", None, None)
            .await
            .expect("series should resolve");
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].source.as_str(), "NASDAQ_NORDIC");
        assert_eq!(series[1].source.as_str(), "YAHOO");
    }

    #[tokio::test]
    async fn backfill_reports_history_clamped_when_rows_start_well_after_the_window() {
        let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq_price.push_response(Ok(vec![nasdaq_close("TX69", 11, "78.20")]));

        let service = multi_provider_service(
            silent_fake_price(),
            empty_search(MarketDataProvider::Yahoo),
            nasdaq_price,
            empty_search(MarketDataProvider::NasdaqNordic),
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let pool = db::memory_pool().await.expect("memory pool");
        let nordic = instrument(&pool, "ERIC", "STO", "SEK").await;
        buy(&pool, nordic, "2016-01-04", 10, "80", "SEK", None).await;
        map_nasdaq_symbol(&pool, nordic, "TX69", "SHARES", "SEK", true).await;

        let response = service
            .refresh(
                &pool,
                RefreshTrigger::Backfill,
                RefreshPricesRequest {
                    mode: RefreshMode::Backfill,
                    start_date: None,
                    end_date: Some("2026-06-12".to_owned()),
                },
            )
            .await
            .expect("backfill should complete");

        let item = response
            .items
            .iter()
            .find(|item| item.instrument_id == Some(nordic))
            .expect("a price item should be reported");
        assert_eq!(item.status, RefreshItemStatus::Fetched);
        assert_eq!(item.reason.as_deref(), Some("history_clamped"));
    }

    #[tokio::test]
    async fn price_status_lists_every_source_and_names_the_effective_one() {
        let service = multi_provider_service(
            silent_fake_price(),
            empty_search(MarketDataProvider::Yahoo),
            FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic),
            empty_search(MarketDataProvider::NasdaqNordic),
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let pool = db::memory_pool().await.expect("memory pool");
        let azn = instrument(&pool, "AZN", "STO", "SEK").await;
        buy(&pool, azn, "2026-06-01", 10, "1300", "SEK", None).await;
        map_yahoo_symbol(&pool, azn, "AZN.L", false).await;
        map_nasdaq_symbol(&pool, azn, "TX271", "SHARES", "SEK", true).await;
        prices::upsert(
            &pool,
            &prices::NewPrice {
                instrument_id: azn,
                provider: MarketDataProvider::NasdaqNordic,
                provider_symbol: "TX271".to_owned(),
                date: Utc::now().date_naive(),
                close: dec!(1369.00),
                currency: "SEK".to_owned(),
                fetched_at: now_iso8601(),
            },
        )
        .await
        .expect("nasdaq row should insert");

        let status = service.status(&pool).await.expect("status should succeed");
        let entry = &status.instruments[0];

        assert_eq!(entry.price_sources.len(), 2);
        assert_eq!(entry.price_sources[0].provider, "YAHOO");
        assert!(!entry.price_sources[0].enabled);
        assert_eq!(entry.price_sources[1].provider, "NASDAQ_NORDIC");
        assert_eq!(
            entry.price_sources[1].asset_class.as_deref(),
            Some("SHARES")
        );
        assert!(entry.price_sources[1].enabled);

        assert_eq!(
            entry.effective_price_source.as_deref(),
            Some("NASDAQ_NORDIC")
        );
        assert!(matches!(
            entry.latest_price.status,
            SnapshotStatus::Available
        ));
    }
}
