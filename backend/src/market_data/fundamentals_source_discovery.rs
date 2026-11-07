use super::{
    effective_prices, provider_registry::ProviderSet, refresh::RefreshLease,
    refresh_contract::MarketDataError, symbol_matching,
};
use crate::{
    db::{
        fundamentals_sources::{
            Classification, FundamentalsSource, LinkDefinition, LinkOrigin, LinkStatus,
        },
        instruments::InstrumentRow,
    },
    domain::fundamentals::{FundamentalValue, PeriodType, SplitEvent},
    providers::{
        yahoo_fundamentals::{FundamentalsRequest, ListingDetails},
        MarketDataProvider, ProviderError, ProviderMissingReason, SymbolSearchMatch,
    },
};
use chrono::NaiveDate;
use sqlx::SqlitePool;

pub(super) struct Discovery {
    pub definition: LinkDefinition,
    pub values: Vec<FundamentalValue>,
    pub splits: Vec<SplitEvent>,
    pub reason: Option<String>,
}
impl Discovery {
    fn empty(
        status: LinkStatus,
        classification: Classification,
        security_type: Option<String>,
        candidates: String,
        reason: &str,
    ) -> Self {
        Self {
            definition: LinkDefinition {
                status,
                classification,
                security_type,
                candidates,
                ..LinkDefinition::pending()
            },
            values: Vec::new(),
            splits: Vec::new(),
            reason: Some(reason.to_owned()),
        }
    }
}
pub(super) fn classify(security_type: Option<&str>) -> Classification {
    match security_type {
        Some("EQUITY") => Classification::Equity,
        Some("ETF" | "MUTUALFUND" | "MONEYMARKET") => Classification::Fund,
        Some("TRACKER_CERTIFICATES") => Classification::Certificate,
        _ => Classification::Unknown,
    }
}
pub(super) fn has_quarters(values: &[FundamentalValue]) -> bool {
    values.iter().any(|v| v.period_type == PeriodType::Quarter)
}

pub(super) fn log_failure(
    instrument_id: i64,
    symbol: &str,
    lease: &RefreshLease,
    generation: i64,
    endpoint: &str,
    error: &ProviderError,
) {
    crate::engine_warn!("fundamentals failure instrument_id={} symbol={} endpoint={} run_id={} link_generation={} reason={} message={}", instrument_id, symbol, endpoint, lease.run_id, generation, error.reason_code(), error.message());
}

pub(super) async fn discover(
    providers: &ProviderSet,
    pool: &SqlitePool,
    instrument: &InstrumentRow,
    today: NaiveDate,
    lease: &RefreshLease,
    generation: i64,
) -> Result<Discovery, MarketDataError> {
    let sources = effective_prices::enabled_price_sources(pool, instrument.id).await?;
    if sources.iter().any(|s| {
        s.provider == MarketDataProvider::NasdaqNordic
            && s.asset_class.as_deref() == Some("TRACKER_CERTIFICATES")
    }) {
        return Ok(Discovery::empty(
            LinkStatus::NotApplicable,
            Classification::Certificate,
            Some("TRACKER_CERTIFICATES".to_owned()),
            "[]".to_owned(),
            "certificate",
        ));
    }
    let client = providers
        .fundamentals_provider()
        .expect("registered fundamentals");
    let mapped = sources
        .iter()
        .find(|s| s.provider == MarketDataProvider::Yahoo);
    let mut mapped_failure = None;
    if let Some(mapped) = mapped {
        match verify(
            providers,
            instrument.id,
            &mapped.provider_symbol,
            None,
            today,
            lease,
            generation,
        )
        .await
        {
            Ok(result)
                if result.definition.status == LinkStatus::Usable
                    || result.definition.status == LinkStatus::NotApplicable =>
            {
                return Ok(result)
            }
            Err(error) if error.reason() == ProviderMissingReason::RateLimited => {
                return Err(provider_error(error))
            }
            result => {
                mapped_failure = Some((mapped.provider_symbol.clone(), result));
            }
        }
    }
    let Some(search) = providers.symbol_search_provider(MarketDataProvider::Yahoo) else {
        if let Some((_, result)) = mapped_failure {
            return result.map_err(provider_error);
        }
        return Ok(Discovery::empty(
            LinkStatus::NoMatch,
            Classification::Unknown,
            None,
            "[]".to_owned(),
            "no_fundamentals_source",
        ));
    };
    let isin = symbol_matching::isin_like_identifier(instrument);
    let mut candidates = Vec::new();
    if let Some(isin) = isin {
        candidates = search.search(isin).await.map_err(|error| {
            log_failure(
                instrument.id,
                isin,
                lease,
                generation,
                "https://query1.finance.yahoo.com/v1/finance/search",
                &error,
            );
            provider_error(error)
        })?;
    }
    // A Nordic home listing is preferable to a secondary ISIN hit. Price
    // orderbook ids are never search symbols; the ledger ticker/name is used.
    let nordic_suffix = match instrument.currency.as_str() {
        "SEK" => Some(".ST"),
        "DKK" => Some(".CO"),
        "EUR"
            if instrument.exchange.eq_ignore_ascii_case("HEL")
                || instrument.exchange.eq_ignore_ascii_case("HELSINKI")
                || isin.is_some_and(|s| s.starts_with("FI")) =>
        {
            Some(".HE")
        }
        _ => None,
    };
    if candidates.is_empty()
        || (sources
            .iter()
            .any(|s| s.provider == MarketDataProvider::NasdaqNordic)
            && nordic_suffix.is_some_and(|suffix| {
                !candidates
                    .iter()
                    .any(|c| c.provider_symbol.ends_with(suffix))
            }))
    {
        let matched_ticker = candidates
            .first()
            .and_then(|c| c.provider_symbol.split('.').next())
            .map(str::to_owned);
        let query = if symbol_matching::is_isin_like(&instrument.symbol) {
            matched_ticker.as_deref().unwrap_or(&instrument.name)
        } else {
            &instrument.symbol
        };
        let found = search.search(query).await.map_err(|error| {
            log_failure(
                instrument.id,
                query,
                lease,
                generation,
                "https://query1.finance.yahoo.com/v1/finance/search",
                &error,
            );
            provider_error(error)
        })?;
        for candidate in found {
            if (candidate
                .provider_symbol
                .eq_ignore_ascii_case(&instrument.symbol)
                || candidate
                    .provider_symbol
                    .split('.')
                    .next()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&instrument.symbol))
                || candidate
                    .provider_symbol
                    .split('.')
                    .next()
                    .is_some_and(|s| matched_ticker.as_deref() == Some(s))
                || candidate.name.as_deref().is_some_and(|name| {
                    symbol_matching::normalized_security_name(name)
                        == symbol_matching::normalized_security_name(&instrument.name)
                }))
                && !candidates
                    .iter()
                    .any(|c| c.provider_symbol == candidate.provider_symbol)
            {
                candidates.push(candidate);
            }
        }
    }
    candidates.retain(|c| c.provider == MarketDataProvider::Yahoo);
    if candidates.is_empty() {
        if let Some((_, result)) = mapped_failure {
            return result.map_err(provider_error);
        }
        return Ok(Discovery::empty(
            LinkStatus::NoMatch,
            Classification::Unknown,
            None,
            "[]".to_owned(),
            "no_fundamentals_source",
        ));
    }
    if candidates
        .iter()
        .all(|c| classify(c.quote_type.as_deref()) == Classification::Fund)
    {
        let mut result = Discovery::empty(
            LinkStatus::NotApplicable,
            Classification::Fund,
            candidates[0].quote_type.clone(),
            candidate_detail(&candidates),
            "fund_or_etf",
        );
        result.definition.source = Some(FundamentalsSource::Yahoo);
        result.definition.symbol = Some(candidates[0].provider_symbol.clone());
        return Ok(result);
    }
    let equities: Vec<_> = candidates
        .iter()
        .filter(|c| classify(c.quote_type.as_deref()) == Classification::Equity)
        .cloned()
        .collect();
    if !equities.is_empty() {
        candidates = equities;
    } else {
        candidates.retain(|c| classify(c.quote_type.as_deref()) == Classification::Unknown);
    }
    if let Some(preferred) = mapped
        .and_then(|m| {
            candidates
                .iter()
                .find(|c| c.provider_symbol == m.provider_symbol)
        })
        .cloned()
    {
        candidates = vec![preferred];
    } else if let Some(suffix) = nordic_suffix {
        let primary: Vec<_> = candidates
            .iter()
            .filter(|c| c.provider_symbol.ends_with(suffix))
            .cloned()
            .collect();
        if !primary.is_empty() {
            candidates = primary;
        }
    }
    if candidates.len() > 1
        && instrument.currency == "USD"
        && (isin.is_some_and(|s| s.starts_with("US"))
            || matches!(instrument.exchange.as_str(), "NASDAQ" | "NYSE"))
    {
        // For US instruments the native US listing is the primary candidate.
        let primary: Vec<_> = candidates
            .iter()
            .filter(|c| {
                !c.provider_symbol.contains('.')
                    && c.exchange
                        .as_deref()
                        .is_some_and(symbol_matching::is_us_exchange)
            })
            .cloned()
            .collect();
        if primary.len() == 1 {
            candidates = primary;
        }
    }
    if candidates.len() != 1 {
        return Ok(Discovery::empty(
            LinkStatus::Ambiguous,
            Classification::Unknown,
            None,
            candidate_detail(&candidates),
            "source_ambiguous",
        ));
    }
    let chosen = &candidates[0];
    if let Some((symbol, result)) = mapped_failure {
        if symbol == chosen.provider_symbol {
            return result.map_err(provider_error);
        }
    }
    // Search already supplies classification. Fetch chart details for split
    // history, allowing an absent type to be established by returned figures.
    let request = FundamentalsRequest {
        symbol: chosen.provider_symbol.clone(),
        end: today,
    };
    let listing = client.listing(&request).await.map_err(|error| {
        log_failure(
            instrument.id,
            &request.symbol,
            lease,
            generation,
            crate::providers::yahoo_fundamentals::CHART_ENDPOINT,
            &error,
        );
        provider_error(error)
    })?;
    verify_with_listing(
        providers,
        instrument.id,
        &request,
        chosen.quote_type.clone().or(listing.security_type.clone()),
        listing,
        lease,
        generation,
    )
    .await
    .map_err(provider_error)
}
fn candidate_detail(candidates: &[SymbolSearchMatch]) -> String {
    serde_json::Value::Array(candidates.iter().map(|c| serde_json::json!({"symbol":c.provider_symbol,"security_type":c.quote_type,"name":c.name,"exchange":c.exchange})).collect()).to_string()
}
async fn verify(
    providers: &ProviderSet,
    instrument_id: i64,
    symbol: &str,
    security_type: Option<String>,
    today: NaiveDate,
    lease: &RefreshLease,
    generation: i64,
) -> Result<Discovery, ProviderError> {
    let request = FundamentalsRequest {
        symbol: symbol.to_owned(),
        end: today,
    };
    let listing = providers
        .fundamentals_provider()
        .expect("registered")
        .listing(&request)
        .await
        .inspect_err(|error| {
            log_failure(
                instrument_id,
                symbol,
                lease,
                generation,
                crate::providers::yahoo_fundamentals::CHART_ENDPOINT,
                error,
            );
        })?;
    verify_with_listing(
        providers,
        instrument_id,
        &request,
        security_type.or(listing.security_type.clone()),
        listing,
        lease,
        generation,
    )
    .await
}
async fn verify_with_listing(
    providers: &ProviderSet,
    instrument_id: i64,
    request: &FundamentalsRequest,
    security_type: Option<String>,
    listing: ListingDetails,
    lease: &RefreshLease,
    generation: i64,
) -> Result<Discovery, ProviderError> {
    let mut classification = classify(security_type.as_deref());
    if classification == Classification::Fund {
        let mut result = Discovery::empty(
            LinkStatus::NotApplicable,
            classification,
            security_type,
            "[]".to_owned(),
            "fund_or_etf",
        );
        result.definition.source = Some(FundamentalsSource::Yahoo);
        result.definition.symbol = Some(request.symbol.clone());
        return Ok(result);
    }
    let values = providers
        .fundamentals_provider()
        .expect("registered")
        .fetch(request)
        .await
        .inspect_err(|error| {
            log_failure(
                instrument_id,
                &request.symbol,
                lease,
                generation,
                crate::providers::yahoo_fundamentals::TIMESERIES_ENDPOINT,
                error,
            );
        })?;
    let has_data = has_quarters(&values);
    if has_data && classification == Classification::Unknown {
        classification = Classification::Figures;
    }
    let reason = if has_data {
        None
    } else if classification == Classification::Unknown {
        Some("security_type_unknown".to_owned())
    } else {
        Some("no_quarterly_data".to_owned())
    };
    Ok(Discovery {
        definition: LinkDefinition {
            source: Some(FundamentalsSource::Yahoo),
            symbol: Some(request.symbol.clone()),
            security_type,
            classification,
            origin: LinkOrigin::Discovered,
            status: if has_data {
                LinkStatus::Usable
            } else {
                LinkStatus::Unavailable
            },
            candidates: "[]".to_owned(),
        },
        values,
        splits: listing.splits,
        reason,
    })
}
pub(super) fn provider_error(error: ProviderError) -> MarketDataError {
    MarketDataError::FundamentalsProvider(error)
}

#[cfg(test)]
mod tests;
