use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use chrono::NaiveDate;
use sqlx::sqlite::SqlitePool;

use crate::{
    db::provider_symbols,
    import::now_iso8601,
    market_data::symbol_matching::{
        self, best_yahoo_search_match, is_isin_like, is_plausible_symbol_recovery,
        is_supported_quote, isin_like_identifier, CurrencyMatch, MAX_SYMBOL_RECOVERY_CANDIDATES,
    },
    providers::{
        MarketDataProvider, PriceHistoryRequest, PriceProvider, ProviderError,
        ProviderMissingReason, SymbolSearchProvider,
    },
};

use super::refresh_contract::MarketDataError;

pub(crate) struct ResolvedPriceHistory {
    pub(crate) provider_symbol: String,
    pub(crate) rows: Vec<crate::providers::DailyClose>,
}

impl ResolvedPriceHistory {
    pub(crate) fn new(provider_symbol: &str, rows: Vec<crate::providers::DailyClose>) -> Self {
        Self {
            provider_symbol: provider_symbol.to_owned(),
            rows,
        }
    }
}

pub(crate) async fn price_history_with_symbol_recovery(
    instrument: &crate::db::instruments::InstrumentRow,
    mapped_symbol: &str,
    window: &super::refresh::RefreshWindow,
    price_provider: Option<&Arc<dyn PriceProvider + Send + Sync>>,
    symbol_search_provider: Option<&Arc<dyn SymbolSearchProvider + Send + Sync>>,
) -> Result<ResolvedPriceHistory, ProviderError> {
    let Some(client) = price_provider else {
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

            let Some(recovered) = recover_changed_yahoo_symbol(
                instrument,
                mapped_symbol,
                window,
                latest_date,
                Some(client),
                symbol_search_provider,
            )
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
                if let Some(recovered) = recover_changed_yahoo_symbol(
                    instrument,
                    mapped_symbol,
                    window,
                    None,
                    Some(client),
                    symbol_search_provider,
                )
                .await
                {
                    return Ok(recovered);
                }
            }
            Err(error)
        }
    }
}

pub(crate) async fn recover_changed_yahoo_symbol(
    instrument: &crate::db::instruments::InstrumentRow,
    mapped_symbol: &str,
    window: &super::refresh::RefreshWindow,
    mapped_latest_date: Option<NaiveDate>,
    price_provider: Option<&Arc<dyn PriceProvider + Send + Sync>>,
    symbol_search_provider: Option<&Arc<dyn SymbolSearchProvider + Send + Sync>>,
) -> Option<ResolvedPriceHistory> {
    if !instrument.exchange.trim().eq_ignore_ascii_case("AVANZA") {
        return None;
    }
    let name = instrument.name.trim();
    if name.is_empty() {
        return None;
    }
    let search = symbol_search_provider?;
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

    let client = price_provider?;
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
pub(crate) async fn seed_provider_symbols(
    pool: &SqlitePool,
    instruments: &[crate::db::instruments::InstrumentRow],
    symbol_search_providers: &[(
        MarketDataProvider,
        Arc<dyn SymbolSearchProvider + Send + Sync>,
    )],
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
                None => yahoo_seed_from_search(instrument, symbol_search_providers).await,
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

        if let Some(candidates) =
            seed_nasdaq_symbol(pool, instrument, symbol_search_providers).await?
        {
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
pub(crate) async fn seed_nasdaq_symbol(
    pool: &SqlitePool,
    instrument: &crate::db::instruments::InstrumentRow,
    symbol_search_providers: &[(
        MarketDataProvider,
        Arc<dyn SymbolSearchProvider + Send + Sync>,
    )],
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
    let Some(search) =
        symbol_search_provider(symbol_search_providers, MarketDataProvider::NasdaqNordic)
    else {
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

pub(crate) async fn yahoo_seed_from_search(
    instrument: &crate::db::instruments::InstrumentRow,
    symbol_search_providers: &[(
        MarketDataProvider,
        Arc<dyn SymbolSearchProvider + Send + Sync>,
    )],
) -> Option<YahooSeed> {
    if !instrument.exchange.trim().eq_ignore_ascii_case("AVANZA") {
        return None;
    }

    let query = instrument
        .isin
        .as_deref()
        .filter(|value| is_isin_like(value))
        .or_else(|| is_isin_like(&instrument.symbol).then_some(instrument.symbol.as_str()))?;

    let search = symbol_search_provider(symbol_search_providers, MarketDataProvider::Yahoo)?;
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

fn symbol_search_provider(
    entries: &[(
        MarketDataProvider,
        Arc<dyn SymbolSearchProvider + Send + Sync>,
    )],
    provider: MarketDataProvider,
) -> Option<&Arc<dyn SymbolSearchProvider + Send + Sync>> {
    entries
        .iter()
        .find(|(registered, _)| *registered == provider)
        .map(|(_, client)| client)
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

pub(crate) struct YahooSeed {
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
