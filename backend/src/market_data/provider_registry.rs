use std::sync::Arc;

use crate::providers::{
    FxRateProvider, MarketDataProvider, PriceProvider, SymbolSearchProvider,
    PRICE_PROVIDER_PRECEDENCE,
};

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

pub(crate) struct ProviderSet {
    /// Both registries are ordered by `PRICE_PROVIDER_PRECEDENCE`, which stays
    /// the single source of truth for precedence; lookup is a linear scan over
    /// a handful of entries.
    price_providers: Vec<(MarketDataProvider, Arc<dyn PriceProvider + Send + Sync>)>,
    symbol_search_providers: Vec<(
        MarketDataProvider,
        Arc<dyn SymbolSearchProvider + Send + Sync>,
    )>,
    fx_provider: Arc<dyn FxRateProvider + Send + Sync>,
}

impl ProviderSet {
    pub(crate) fn from_registry(registry: ProviderRegistry) -> Self {
        let ProviderRegistry {
            mut price_providers,
            mut symbol_search_providers,
            fx_provider,
        } = registry;
        sort_by_precedence(&mut price_providers);
        sort_by_precedence(&mut symbol_search_providers);

        Self {
            price_providers,
            symbol_search_providers,
            fx_provider: fx_provider
                .unwrap_or_else(|| Arc::new(crate::providers::FrankfurterClient::new())),
        }
    }

    pub(crate) fn price_provider(
        &self,
        provider: MarketDataProvider,
    ) -> Option<&Arc<dyn PriceProvider + Send + Sync>> {
        self.price_providers
            .iter()
            .find(|(registered, _)| *registered == provider)
            .map(|(_, client)| client)
    }

    pub(crate) fn symbol_search_provider(
        &self,
        provider: MarketDataProvider,
    ) -> Option<&Arc<dyn SymbolSearchProvider + Send + Sync>> {
        self.symbol_search_providers
            .iter()
            .find(|(registered, _)| *registered == provider)
            .map(|(_, client)| client)
    }

    pub(crate) fn symbol_search_providers(
        &self,
    ) -> &[(
        MarketDataProvider,
        Arc<dyn SymbolSearchProvider + Send + Sync>,
    )] {
        &self.symbol_search_providers
    }

    pub(crate) fn fx_provider(&self) -> &Arc<dyn FxRateProvider + Send + Sync> {
        &self.fx_provider
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

/// Render the per-provider row split for the run message and the finished log.
pub(super) fn describe_provider_split(rows_by_provider: &[(MarketDataProvider, usize)]) -> String {
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

pub(super) fn add_provider_rows(
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
