//! Adapt enabled provider mappings to pure listing-market inputs.
use super::effective_prices::PriceSourceMapping;
use crate::providers::MarketDataProvider;

/// The adapter supplies enabled mappings; all recognition policy stays in the domain.
pub(crate) fn from_sources<'a>(
    sources: &'a [PriceSourceMapping],
    currency: &'a str,
) -> crate::domain::market_index::ListingMarket<'a> {
    let yahoo = sources
        .iter()
        .find(|source| source.provider == MarketDataProvider::Yahoo);
    let nordic = sources
        .iter()
        .find(|source| source.provider == MarketDataProvider::NasdaqNordic);
    crate::domain::market_index::listing_market(
        yahoo.map(|source| (source.provider_symbol.as_str(), source.currency.as_deref())),
        nordic.and_then(|source| source.currency.as_deref()),
        nordic.is_some(),
        currency,
    )
}
