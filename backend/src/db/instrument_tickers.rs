//! Display identity, independent of the price source selected for valuation.
use super::RepoError;
use crate::providers::MarketDataProvider;
use sqlx::SqlitePool;
use std::collections::BTreeMap;

pub type TickerMap = BTreeMap<i64, String>;

/// Load all display tickers in one query per response, never once per row.
pub async fn list(pool: &SqlitePool) -> Result<TickerMap, RepoError> {
    let rows = sqlx::query_as::<_, (i64, String, String, Option<String>, Option<String>)>(
        "SELECT i.id, i.symbol, i.exchange, p.provider_symbol, f.provider_symbol \
         FROM instruments i \
         LEFT JOIN instrument_provider_symbols p ON p.instrument_id = i.id AND p.provider = ? AND p.enabled = 1 \
         LEFT JOIN instrument_fundamentals_sources f ON f.instrument_id = i.id"
    ).bind(MarketDataProvider::Yahoo.as_str()).fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, symbol, exchange, price, figures)| {
            display_ticker(&symbol, &exchange, price.as_deref(), figures.as_deref())
                .map(|ticker| (id, ticker))
        })
        .collect())
}

fn display_ticker(
    symbol: &str,
    exchange: &str,
    yahoo: Option<&str>,
    figures: Option<&str>,
) -> Option<String> {
    let mapped = yahoo.or(figures);
    let candidate = match mapped {
        Some(value) => value
            .trim()
            .rsplit_once('.')
            .map_or(value.trim(), |(ticker, _)| ticker),
        None => {
            let value = symbol.trim();
            let isin_like =
                value.len() == 12 && value.bytes().all(|byte| byte.is_ascii_alphanumeric());
            if exchange.eq_ignore_ascii_case("AVANZA")
                || isin_like
                || value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            value
        }
    };
    // Trusted Yahoo symbols may be numeric (for example 0700.HK); Nasdaq
    // mappings are never read, and numeric raw orderbook identities are excluded above.
    (!candidate.is_empty()).then(|| candidate.to_owned())
}

#[cfg(test)]
mod tests {
    use super::display_ticker;
    #[test]
    fn display_identity_precedence_and_suffixes() {
        for (symbol, exchange, yahoo, figures, expected) in [
            (
                "SE0012673267",
                "AVANZA",
                Some("EVO.ST"),
                Some("WRONG.AS"),
                Some("EVO"),
            ),
            (
                "SE0012673267",
                "AVANZA",
                None,
                Some("ASML.AS"),
                Some("ASML"),
            ),
            ("MSFT", "NASDAQ", None, None, Some("MSFT")),
            ("SE0012673267", "XSTO", None, None, None),
            ("12345", "AVANZA", None, None, None),
            ("12345", "NASDAQ_NORDIC", None, None, None),
            ("CORN", "AVANZA", None, None, None),
            ("SE0012673267", "AVANZA", Some("MSFT"), None, Some("MSFT")),
            (
                "HKINSTRUMENT",
                "AVANZA",
                Some("0700.HK"),
                None,
                Some("0700"),
            ),
        ] {
            assert_eq!(
                display_ticker(symbol, exchange, yahoo, figures).as_deref(),
                expected
            );
        }
    }
}
