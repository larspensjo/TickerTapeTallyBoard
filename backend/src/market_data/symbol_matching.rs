//! Pure rules for turning a provider's symbol-search results into a mapping
//! decision. Nothing here touches the database, the network or the clock, so
//! every rule is unit-testable in isolation.

use crate::{
    db::instruments::InstrumentRow,
    providers::{MarketDataProvider, SymbolSearchMatch},
};

/// Cap on how many recovery candidates are probed for a changed Yahoo symbol.
pub const MAX_SYMBOL_RECOVERY_CANDIDATES: usize = 5;

/// The outcome of narrowing search candidates to the instrument's own currency.
///
/// `Ambiguous` is the state that must never be auto-connected: more than one
/// listing survives the currency filter, so picking one would silently value
/// the holding off a guessed exchange.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurrencyMatch {
    Unique(Box<SymbolSearchMatch>),
    None,
    Ambiguous(Vec<SymbolSearchMatch>),
}

/// Whether a search hit is a security this application can price at all.
///
/// Dispatches on the provider that produced the hit: Yahoo's quote types are an
/// allow-list, while a Nasdaq Nordic search by ISIN only ever returns listings
/// of that one security, so every returned instrument is acceptable and the
/// narrowing that matters is [`unique_currency_match`].
pub fn is_supported_quote(item: &SymbolSearchMatch) -> bool {
    match item.provider {
        MarketDataProvider::NasdaqNordic => true,
        _ => is_supported_yahoo_quote(item),
    }
}

/// Narrow candidates to those quoted in the instrument's own currency.
///
/// Candidates without a currency are dropped: an unknown quote currency cannot
/// be shown to match, and connecting one would defeat the guard entirely.
pub fn unique_currency_match(
    instrument_currency: &str,
    matches: Vec<SymbolSearchMatch>,
) -> CurrencyMatch {
    let wanted = instrument_currency.trim();
    let mut narrowed = matches
        .into_iter()
        .filter(|item| {
            item.currency
                .as_deref()
                .is_some_and(|currency| currency.trim().eq_ignore_ascii_case(wanted))
        })
        .collect::<Vec<_>>();

    match narrowed.len() {
        0 => CurrencyMatch::None,
        1 => CurrencyMatch::Unique(Box::new(narrowed.remove(0))),
        _ => CurrencyMatch::Ambiguous(narrowed),
    }
}

/// A one-line rendering of every candidate, for the ambiguity warning log.
pub fn describe_candidates(matches: &[SymbolSearchMatch]) -> String {
    matches
        .iter()
        .map(|item| {
            format!(
                "{}({}/{})",
                item.provider_symbol,
                item.currency.as_deref().unwrap_or("?"),
                item.asset_class.as_deref().unwrap_or("?")
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn best_yahoo_search_match(
    instrument: &InstrumentRow,
    matches: Vec<SymbolSearchMatch>,
) -> Option<SymbolSearchMatch> {
    let mut supported = matches.into_iter().filter(is_supported_yahoo_quote);
    if prefers_us_listing(instrument) {
        return supported.find(|item| {
            !item.provider_symbol.contains('.')
                && item.exchange.as_deref().is_some_and(is_us_exchange)
        });
    }

    supported.next()
}

pub fn is_supported_yahoo_quote(item: &SymbolSearchMatch) -> bool {
    let quote_type = item
        .quote_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(quote_type.as_str(), "EQUITY" | "ETF" | "MUTUALFUND")
}

pub fn is_us_exchange(exchange: &str) -> bool {
    matches!(
        exchange.trim().to_ascii_uppercase().as_str(),
        "NMS" | "NYQ" | "ASE" | "NGM" | "NCM" | "PCX" | "NASDAQ" | "NYSE" | "NYSEARCA"
    )
}

pub fn is_plausible_symbol_recovery(
    instrument: &InstrumentRow,
    candidate: &SymbolSearchMatch,
) -> bool {
    if !is_supported_yahoo_quote(candidate) {
        return false;
    }
    let Some(candidate_name) = candidate.name.as_deref() else {
        return false;
    };
    if normalized_security_name(candidate_name) != normalized_security_name(&instrument.name) {
        return false;
    }

    if prefers_us_listing(instrument) {
        return !candidate.provider_symbol.contains('.')
            && candidate.exchange.as_deref().is_some_and(is_us_exchange);
    }

    true
}

pub fn normalized_security_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn is_isin_like(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() == 12 && trimmed.chars().all(|ch| ch.is_ascii_alphanumeric())
}

/// The ISIN-like identifier to search a provider with, if the instrument has
/// one. Avanza rows carry the ISIN in `symbol` when `isin` is unset.
pub fn isin_like_identifier(instrument: &InstrumentRow) -> Option<&str> {
    instrument
        .isin
        .as_deref()
        .map(str::trim)
        .filter(|value| is_isin_like(value))
        .or_else(|| is_isin_like(&instrument.symbol).then(|| instrument.symbol.trim()))
}

fn prefers_us_listing(instrument: &InstrumentRow) -> bool {
    instrument
        .isin
        .as_deref()
        .is_some_and(|isin| isin.trim().starts_with("US"))
        && instrument.currency.trim().eq_ignore_ascii_case("USD")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nasdaq_match(symbol: &str, currency: &str, asset_class: &str) -> SymbolSearchMatch {
        SymbolSearchMatch {
            provider: MarketDataProvider::NasdaqNordic,
            provider_symbol: symbol.to_owned(),
            quote_type: None,
            exchange: Some("Shares Main Market".to_owned()),
            name: Some("Ericsson B".to_owned()),
            asset_class: Some(asset_class.to_owned()),
            currency: Some(currency.to_owned()),
        }
    }

    #[test]
    fn nasdaq_quotes_are_supported_regardless_of_asset_class() {
        assert!(is_supported_quote(&nasdaq_match(
            "TX2997672",
            "SEK",
            "TRACKER_CERTIFICATES"
        )));
    }

    #[test]
    fn yahoo_quote_allow_list_still_applies() {
        let mut item = nasdaq_match("MSFT", "USD", "SHARES");
        item.provider = MarketDataProvider::Yahoo;
        item.quote_type = Some("EQUITY".to_owned());
        assert!(is_supported_quote(&item));

        item.quote_type = Some("CURRENCY".to_owned());
        assert!(!is_supported_quote(&item));
    }

    #[test]
    fn currency_narrowing_picks_the_single_same_currency_listing() {
        let matches = vec![
            nasdaq_match("TX50143", "EUR", "SHARES"),
            nasdaq_match("TX69", "SEK", "SHARES"),
        ];

        match unique_currency_match("SEK", matches) {
            CurrencyMatch::Unique(item) => assert_eq!(item.provider_symbol, "TX69"),
            other => panic!("expected a unique match, got {other:?}"),
        }
    }

    #[test]
    fn two_same_currency_listings_are_ambiguous() {
        let matches = vec![
            nasdaq_match("TX69", "SEK", "SHARES"),
            nasdaq_match("TX70", "SEK", "SHARES"),
        ];

        match unique_currency_match("SEK", matches) {
            CurrencyMatch::Ambiguous(candidates) => assert_eq!(candidates.len(), 2),
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn no_same_currency_listing_is_none() {
        let matches = vec![nasdaq_match("TX50143", "EUR", "SHARES")];
        assert_eq!(unique_currency_match("SEK", matches), CurrencyMatch::None);
    }

    #[test]
    fn candidates_without_a_currency_are_never_connected() {
        let mut item = nasdaq_match("TX69", "SEK", "SHARES");
        item.currency = None;
        assert_eq!(
            unique_currency_match("SEK", vec![item]),
            CurrencyMatch::None
        );
    }

    #[test]
    fn currency_narrowing_ignores_case_and_padding() {
        let mut item = nasdaq_match("TX69", " sek ", "SHARES");
        item.currency = Some(" sek ".to_owned());
        match unique_currency_match("SEK", vec![item]) {
            CurrencyMatch::Unique(item) => assert_eq!(item.provider_symbol, "TX69"),
            other => panic!("expected a unique match, got {other:?}"),
        }
    }

    #[test]
    fn candidate_description_names_every_orderbook_id() {
        let described = describe_candidates(&[
            nasdaq_match("TX69", "SEK", "SHARES"),
            nasdaq_match("TX70", "SEK", "WARRANTS"),
        ]);
        assert_eq!(described, "TX69(SEK/SHARES), TX70(SEK/WARRANTS)");
    }
}
