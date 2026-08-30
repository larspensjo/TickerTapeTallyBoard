use futures::future::join_all;

use crate::{market_data::symbol_matching::is_supported_quote, providers::SymbolSearchMatch};

use super::{
    provider_registry::ProviderSet,
    refresh_contract::{SymbolSearchLookupMatch, SymbolSearchLookupResponse},
};

pub(super) async fn lookup_symbol_search(
    providers: &ProviderSet,
    query: &str,
) -> SymbolSearchLookupResponse {
    let query = query.trim().to_owned();
    let search_results = join_all(providers.symbol_search_providers().iter().map(
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
