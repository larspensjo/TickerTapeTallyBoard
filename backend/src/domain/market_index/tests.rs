use super::*;

fn yahoo(symbol: &str, currency: &str) -> Option<&'static MarketIndex> {
    listing_market(Some((symbol, None)), None, false, currency).index()
}
fn assert_suffixes(suffixes: &[&str], currency: &str, code: &str) {
    for suffix in suffixes {
        let market = listing_market(
            Some((&format!("TEST.{suffix}"), Some(currency))),
            Some("SEK"),
            true,
            "GBP",
        );
        let index = market.index().expect(suffix);
        assert_eq!(index.code, code, "{suffix}");
        assert_eq!(index.currency, market.currency, "{suffix}");
    }
}
#[test]
fn no_suffix_is_us() {
    assert_eq!(yahoo("TEST", "USD").unwrap().code, "SP500");
}
#[test]
fn stockholm_suffix() {
    assert_suffixes(&["ST"], "SEK", "OMXSPI");
}
#[test]
fn amsterdam_suffix() {
    assert_suffixes(&["AS"], "EUR", "AEX");
}
#[test]
fn german_suffixes() {
    assert_suffixes(
        &["DE", "F", "BE", "DU", "HM", "HA", "MU", "SG"],
        "EUR",
        "STOXX600",
    );
}
#[test]
fn france_suffix() {
    assert_suffixes(&["PA"], "EUR", "STOXX600");
}
#[test]
fn belgium_suffix() {
    assert_suffixes(&["BR"], "EUR", "STOXX600");
}
#[test]
fn italy_suffix() {
    assert_suffixes(&["MI"], "EUR", "STOXX600");
}
#[test]
fn spain_suffix() {
    assert_suffixes(&["MC"], "EUR", "STOXX600");
}
#[test]
fn portugal_suffix() {
    assert_suffixes(&["LS"], "EUR", "STOXX600");
}
#[test]
fn austria_suffix() {
    assert_suffixes(&["VI"], "EUR", "STOXX600");
}
#[test]
fn ireland_suffix() {
    assert_suffixes(&["IR"], "EUR", "STOXX600");
}
#[test]
fn nordic_currency_precedes_instrument_currency() {
    assert_eq!(
        listing_market(None, Some("SEK"), true, "EUR")
            .index()
            .unwrap()
            .code,
        "OMXSPI"
    );
    for currency in ["EUR", "NOK", "DKK", "ISK", "USD"] {
        assert!(
            listing_market(None, Some(currency), true, "SEK")
                .index()
                .is_none(),
            "{currency}"
        );
    }
}
#[test]
fn instrument_currency_fallback() {
    for (currency, code) in [("SEK", "OMXSPI"), ("EUR", "STOXX600")] {
        assert_eq!(
            listing_market(None, None, false, currency)
                .index()
                .unwrap()
                .code,
            code
        );
    }
    assert!(listing_market(None, None, true, "EUR").index().is_none());
    for currency in [
        "USD", "NOK", "DKK", "ISK", "GBP", "CHF", "PLN", "CAD", "AUD", "HKD", "JPY", "ZZZ",
    ] {
        assert!(
            listing_market(None, None, false, currency)
                .index()
                .is_none(),
            "{currency}"
        );
    }
}
macro_rules! median_case {
    ($name:ident, $suffixes:expr) => {
        #[test]
        fn $name() {
            for suffix in $suffixes {
                // Even a euro instrument does not promote a known or unknown suffix.
                assert!(
                    yahoo(&format!("TEST.{suffix}"), "EUR").is_none(),
                    "{suffix}"
                );
            }
        }
    };
}
median_case!(oslo_keeps_median, ["OL"]);
median_case!(copenhagen_keeps_median, ["CO"]);
median_case!(helsinki_keeps_median, ["HE"]);
median_case!(iceland_keeps_median, ["IC"]);
median_case!(london_keeps_median, ["L"]);
median_case!(switzerland_keeps_median, ["SW", "VX"]);
median_case!(
    non_euro_europe_keeps_median,
    ["AT", "WA", "PR", "BD", "IS", "RG", "TL", "VS"]
);
median_case!(other_markets_keep_median, ["TO", "V", "AX", "HK", "T"]);
median_case!(unknown_suffix_keeps_median, ["ZZ"]);
#[test]
fn every_index_requires_its_own_listing_currency() {
    for index in MARKET_INDEXES {
        let symbol = if index.yahoo_no_suffix {
            "TEST".to_owned()
        } else {
            format!("TEST.{}", index.yahoo_suffixes[0])
        };
        let listing = listing_market(
            Some((&symbol, Some(index.currency))),
            None,
            false,
            "UNKNOWN",
        );
        assert_eq!(listing.index(), Some(index));
        assert_eq!(listing.currency, index.currency);
        assert!(
            listing_market(Some((&symbol, Some("WRONG"))), None, false, index.currency)
                .index()
                .is_none()
        );
    }
    assert!(yahoo("NORD", "SEK").is_none());
}
