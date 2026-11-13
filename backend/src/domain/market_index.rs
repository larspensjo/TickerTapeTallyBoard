//! Listing recognition and index choices live together so a choice changes in one row.
//! Inputs describe enabled sources only; broker exchange labels are deliberately absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketIndex {
    pub market: &'static str,
    pub yahoo_suffixes: &'static [&'static str],
    pub yahoo_no_suffix: bool,
    pub nasdaq_currency: Option<&'static str>,
    pub fallback_currency: Option<&'static str>,
    pub code: &'static str,
    pub provider_symbol: &'static str,
    pub currency: &'static str,
    pub display_name: &'static str,
}

/// Stored series are read only for the provider symbol configured in this table.
pub const MARKET_INDEXES: &[MarketIndex] = &[
    MarketIndex {
        market: "us",
        yahoo_suffixes: &[],
        yahoo_no_suffix: true,
        nasdaq_currency: None,
        fallback_currency: None,
        code: "SP500",
        provider_symbol: "^GSPC",
        currency: "USD",
        display_name: "S&P 500",
    },
    MarketIndex {
        market: "stockholm",
        yahoo_suffixes: &["ST"],
        yahoo_no_suffix: false,
        nasdaq_currency: Some("SEK"),
        fallback_currency: Some("SEK"),
        code: "OMXSPI",
        provider_symbol: "^OMXSPI",
        currency: "SEK",
        display_name: "OMX Stockholm all-share",
    },
    MarketIndex {
        market: "amsterdam",
        yahoo_suffixes: &["AS"],
        yahoo_no_suffix: false,
        nasdaq_currency: None,
        fallback_currency: None,
        code: "AEX",
        provider_symbol: "^AEX",
        currency: "EUR",
        display_name: "AEX",
    },
    MarketIndex {
        market: "euro_europe",
        yahoo_suffixes: &[
            "DE", "F", "BE", "DU", "HM", "HA", "MU", "SG", "PA", "BR", "MI", "MC", "LS", "VI", "IR",
        ],
        yahoo_no_suffix: false,
        nasdaq_currency: None,
        fallback_currency: Some("EUR"),
        code: "STOXX600",
        provider_symbol: "^STOXX",
        currency: "EUR",
        display_name: "STOXX Europe 600",
    },
];

#[derive(Clone, Copy, Debug)]
pub struct ListingMarket<'a> {
    /// None includes known markets without a chosen index and unknown suffixes.
    pub market: Option<&'static MarketIndex>,
    pub currency: &'a str,
}

impl ListingMarket<'_> {
    pub fn index(self) -> Option<&'static MarketIndex> {
        self.market
            .filter(|index| index.currency.eq_ignore_ascii_case(self.currency))
    }
}

/// Yahoo's symbol takes precedence even for an unknown suffix. A Nordic source
/// suppresses the general currency fallback (notably EUR means Helsinki there).
pub fn listing_market<'a>(
    yahoo: Option<(&str, Option<&'a str>)>,
    nasdaq_currency: Option<&'a str>,
    has_nasdaq_mapping: bool,
    instrument_currency: &'a str,
) -> ListingMarket<'a> {
    let (market, currency) = if let Some((symbol, quote_currency)) = yahoo {
        let suffix = symbol.rsplit_once('.').map(|(_, suffix)| suffix);
        (
            MARKET_INDEXES.iter().find(|index| match suffix {
                Some(suffix) => index
                    .yahoo_suffixes
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(suffix)),
                None => index.yahoo_no_suffix,
            }),
            quote_currency.unwrap_or(instrument_currency),
        )
    } else if has_nasdaq_mapping {
        let currency = nasdaq_currency.unwrap_or(instrument_currency);
        (
            MARKET_INDEXES.iter().find(|index| {
                index
                    .nasdaq_currency
                    .is_some_and(|value| value.eq_ignore_ascii_case(currency))
            }),
            currency,
        )
    } else {
        (
            MARKET_INDEXES.iter().find(|index| {
                index
                    .fallback_currency
                    .is_some_and(|value| value.eq_ignore_ascii_case(instrument_currency))
            }),
            instrument_currency,
        )
    };
    ListingMarket { market, currency }
}

#[cfg(test)]
mod tests;
