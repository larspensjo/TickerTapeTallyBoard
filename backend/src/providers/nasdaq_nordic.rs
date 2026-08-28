use chrono::NaiveDate;
use reqwest::{Client, Url};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

use super::{
    DailyClose, MarketDataProvider, PriceHistoryRequest, ProviderError, ProviderMissingReason,
    ProviderResult, SymbolSearchMatch,
};

const DEFAULT_BASE_URL: &str = "https://api.nasdaq.com/api/nordic";

#[derive(Clone)]
pub struct NasdaqNordicClient {
    client: Client,
    base_url: String,
}

impl NasdaqNordicClient {
    pub fn new() -> Self {
        let base_url = std::env::var("TTTB_NASDAQ_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        Self::with_base_url(base_url)
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            client: super::http::build_client(),
            base_url: base_url.into(),
        }
    }

    pub fn with_client(client: Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into(),
        }
    }

    fn price_history_url(&self, request: &PriceHistoryRequest) -> ProviderResult<String> {
        let asset_class = request
            .asset_class
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ProviderError::provider_error(
                    MarketDataProvider::NasdaqNordic.as_str(),
                    format!(
                        "missing asset_class for Nasdaq orderbook {}",
                        request.symbol
                    ),
                )
            })?;
        let mut url = Url::parse(self.base_url.trim_end_matches('/'))
            .expect("Nasdaq Nordic base URL should always parse");
        url.path_segments_mut()
            .expect("Nasdaq Nordic URL should support path segments")
            .push("instruments")
            .push(&request.symbol)
            .push("price-history");
        url.query_pairs_mut()
            .append_pair("assetClass", asset_class)
            .append_pair("fromDate", &request.start.format("%Y-%m-%d").to_string())
            .append_pair("toDate", &request.end.format("%Y-%m-%d").to_string());
        Ok(url.to_string())
    }

    fn search_url(&self, query: &str) -> String {
        let mut url = Url::parse(self.base_url.trim_end_matches('/'))
            .expect("Nasdaq Nordic base URL should always parse");
        url.path_segments_mut()
            .expect("Nasdaq Nordic URL should support path segments")
            .push("search");
        url.query_pairs_mut().append_pair("searchText", query);
        url.to_string()
    }

    fn log_price_failure(request: &PriceHistoryRequest, error: &ProviderError) {
        crate::engine_error!(
            "market data price failure provider={} orderbook_id={} asset_class={} range={}..{} reason={} message={}",
            MarketDataProvider::NasdaqNordic,
            request.symbol,
            request.asset_class.as_deref().unwrap_or("missing"),
            request.start,
            request.end,
            error.reason_code(),
            error.message()
        );
    }

    fn log_search_failure(query: &str, error: &ProviderError) {
        crate::engine_warn!(
            "market data symbol search failure provider={} query={} reason={} message={}",
            MarketDataProvider::NasdaqNordic,
            query,
            error.reason_code(),
            error.message()
        );
    }

    fn parse_price_response(
        request: &PriceHistoryRequest,
        body: &str,
    ) -> ProviderResult<Vec<DailyClose>> {
        let response: NasdaqEnvelope<NasdaqPriceData> = parse_envelope(body, "price history")?;
        if let Some(error) = response.status.error_for("price history", &request.symbol) {
            return Err(error);
        }
        let data = response.data.ok_or_else(|| {
            ProviderError::new(
                MarketDataProvider::NasdaqNordic.as_str(),
                ProviderMissingReason::NoDataInRange,
                format!(
                    "Nasdaq price history returned no data for {}",
                    request.symbol
                ),
            )
        })?;

        let currency = request
            .quote_currency
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ProviderError::provider_error(
                    MarketDataProvider::NasdaqNordic.as_str(),
                    format!(
                        "missing_source_currency for Nasdaq orderbook {}",
                        request.symbol
                    ),
                )
            })?;

        let mut rows = Vec::new();
        for row in data.price_history.rows {
            if row.close.trim().is_empty() {
                continue;
            }
            let date = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d").map_err(|error| {
                ProviderError::provider_error(
                    MarketDataProvider::NasdaqNordic.as_str(),
                    format!(
                        "Nasdaq date field for orderbook {} was invalid at row {:?}: {error}",
                        request.symbol, row.date
                    ),
                )
            })?;
            rows.push(DailyClose {
                provider: MarketDataProvider::NasdaqNordic,
                provider_symbol: request.symbol.clone(),
                date,
                close: parse_number("close", &row.close, date)?,
                currency: currency.to_owned(),
            });
        }

        if rows.is_empty() {
            return Err(ProviderError::new(
                MarketDataProvider::NasdaqNordic.as_str(),
                ProviderMissingReason::NoDataInRange,
                format!("Nasdaq returned no closes for orderbook {}", request.symbol),
            ));
        }

        rows.sort_by_key(|row| row.date);
        Ok(rows)
    }

    fn parse_search_response(query: &str, body: &str) -> ProviderResult<Vec<SymbolSearchMatch>> {
        let response: NasdaqEnvelope<Vec<NasdaqSearchGroup>> = parse_envelope(body, "search")?;
        if let Some(error) = response.status.error_for("search", query) {
            return Err(error);
        }
        let groups = response.data.unwrap_or_default();

        Ok(groups
            .into_iter()
            .flat_map(|group| {
                let exchange = Some(group.group);
                group
                    .instruments
                    .into_iter()
                    .map(move |instrument| SymbolSearchMatch {
                        provider: MarketDataProvider::NasdaqNordic,
                        provider_symbol: instrument.orderbook_id,
                        quote_type: None,
                        exchange: exchange.clone(),
                        name: instrument.full_name,
                        asset_class: instrument.asset_class,
                        currency: instrument.currency,
                    })
            })
            .collect())
    }
}

impl Default for NasdaqNordicClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl super::PriceProvider for NasdaqNordicClient {
    async fn daily_history(
        &self,
        request: &PriceHistoryRequest,
    ) -> ProviderResult<Vec<DailyClose>> {
        if request
            .quote_currency
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            let error = ProviderError::provider_error(
                MarketDataProvider::NasdaqNordic.as_str(),
                format!(
                    "missing_source_currency for Nasdaq orderbook {}",
                    request.symbol
                ),
            );
            Self::log_price_failure(request, &error);
            return Err(error);
        }

        let url = match self.price_history_url(request) {
            Ok(url) => url,
            Err(error) => {
                Self::log_price_failure(request, &error);
                return Err(error);
            }
        };
        let response = match self.client.get(&url).send().await {
            Ok(response) => response,
            Err(error) => {
                let error = ProviderError::provider_error(
                    MarketDataProvider::NasdaqNordic.as_str(),
                    format!(
                        "failed to request Nasdaq price history for {}: {error}",
                        request.symbol
                    ),
                );
                Self::log_price_failure(request, &error);
                return Err(error);
            }
        };

        let status = response.status();
        let body = match response.text().await {
            Ok(body) => body,
            Err(error) => {
                let error = ProviderError::provider_error(
                    MarketDataProvider::NasdaqNordic.as_str(),
                    format!(
                        "failed to read Nasdaq price history for {}: {error}",
                        request.symbol
                    ),
                );
                Self::log_price_failure(request, &error);
                return Err(error);
            }
        };

        let parsed = if !status.is_success() {
            map_http_or_envelope_error(status.as_u16(), &body, "price history", &request.symbol)
        } else {
            Self::parse_price_response(request, &body)
        };
        if let Err(error) = &parsed {
            Self::log_price_failure(request, error);
        }
        parsed
    }
}

#[async_trait::async_trait]
impl super::SymbolSearchProvider for NasdaqNordicClient {
    async fn search(&self, query: &str) -> ProviderResult<Vec<SymbolSearchMatch>> {
        let url = self.search_url(query);
        let response = match self.client.get(&url).send().await {
            Ok(response) => response,
            Err(error) => {
                let error = ProviderError::provider_error(
                    MarketDataProvider::NasdaqNordic.as_str(),
                    format!("failed to request Nasdaq symbol search for {query}: {error}"),
                );
                Self::log_search_failure(query, &error);
                return Err(error);
            }
        };

        let status = response.status();
        let body = match response.text().await {
            Ok(body) => body,
            Err(error) => {
                let error = ProviderError::provider_error(
                    MarketDataProvider::NasdaqNordic.as_str(),
                    format!("failed to read Nasdaq symbol search for {query}: {error}"),
                );
                Self::log_search_failure(query, &error);
                return Err(error);
            }
        };

        let parsed = if !status.is_success() {
            map_http_or_envelope_error(status.as_u16(), &body, "search", query)
        } else {
            Self::parse_search_response(query, &body)
        };
        if let Err(error) = &parsed {
            Self::log_search_failure(query, error);
        }
        parsed
    }
}

fn parse_number(field: &str, value: &str, date: NaiveDate) -> ProviderResult<Decimal> {
    let normalized = value.replace([',', ' ', '\u{a0}'], "");
    Decimal::from_str(&normalized).map_err(|error| {
        ProviderError::provider_error(
            MarketDataProvider::NasdaqNordic.as_str(),
            format!("Nasdaq {field} field was invalid for row date {date}: {error}"),
        )
    })
}

fn parse_envelope<T>(body: &str, operation: &str) -> ProviderResult<NasdaqEnvelope<T>>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(body).map_err(|error| {
        ProviderError::provider_error(
            MarketDataProvider::NasdaqNordic.as_str(),
            format!("failed to parse Nasdaq {operation} response: {error}"),
        )
    })
}

fn map_http_or_envelope_error<T>(
    http_status: u16,
    body: &str,
    operation: &str,
    subject: &str,
) -> ProviderResult<T> {
    let envelope: NasdaqEnvelope<serde_json::Value> = parse_envelope(body, operation)?;
    Err(envelope
        .status
        .error_for(operation, subject)
        .unwrap_or_else(|| {
            ProviderError::with_http_status(
                MarketDataProvider::NasdaqNordic.as_str(),
                http_status,
                format!("Nasdaq {operation} request for {subject} failed with HTTP {http_status}"),
            )
        }))
}

#[derive(Debug, Deserialize)]
struct NasdaqEnvelope<T> {
    data: Option<T>,
    #[serde(default)]
    status: NasdaqStatus,
}

#[derive(Debug, Default, Deserialize)]
struct NasdaqStatus {
    #[serde(rename = "rCode", default)]
    r_code: u16,
    #[serde(rename = "bCodeMessage", default)]
    b_code_message: Option<Vec<NasdaqErrorMessage>>,
}

impl NasdaqStatus {
    fn error_for(&self, operation: &str, subject: &str) -> Option<ProviderError> {
        let message = self
            .b_code_message
            .as_ref()
            .and_then(|messages| messages.first());
        if self.r_code < 400 && message.is_none() {
            return None;
        }
        let error_message = message
            .map(|message| message.error_message.clone())
            .unwrap_or_else(|| format!("Nasdaq returned status code {}", self.r_code));
        let reason = if error_message
            .to_ascii_lowercase()
            .contains("instrument not found")
        {
            ProviderMissingReason::NotListed
        } else {
            ProviderMissingReason::ProviderError
        };
        let code = message
            .and_then(|message| message.code)
            .map(|code| code.to_string())
            .unwrap_or_else(|| self.r_code.to_string());
        Some(ProviderError::new(
            MarketDataProvider::NasdaqNordic.as_str(),
            reason,
            format!(
                "Nasdaq {operation} for {subject} failed with error code {code}: {error_message}"
            ),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct NasdaqErrorMessage {
    #[serde(default)]
    code: Option<i64>,
    #[serde(rename = "errorMessage")]
    error_message: String,
}

#[derive(Debug, Deserialize)]
struct NasdaqPriceData {
    #[serde(rename = "priceHistory")]
    price_history: NasdaqPriceHistory,
}

#[derive(Debug, Deserialize)]
struct NasdaqPriceHistory {
    #[serde(default)]
    rows: Vec<NasdaqPriceRow>,
}

#[derive(Debug, Deserialize)]
struct NasdaqPriceRow {
    date: String,
    #[serde(default)]
    close: String,
}

#[derive(Debug, Deserialize)]
struct NasdaqSearchGroup {
    group: String,
    #[serde(default)]
    instruments: Vec<NasdaqSearchInstrument>,
}

#[derive(Debug, Deserialize)]
struct NasdaqSearchInstrument {
    #[serde(rename = "orderbookId")]
    orderbook_id: String,
    #[serde(rename = "fullName", default)]
    full_name: Option<String>,
    #[serde(rename = "assetClass", default)]
    asset_class: Option<String>,
    #[serde(default)]
    currency: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::PriceProvider;

    fn request(
        symbol: &str,
        asset_class: Option<&str>,
        currency: Option<&str>,
    ) -> PriceHistoryRequest {
        PriceHistoryRequest {
            symbol: symbol.to_owned(),
            asset_class: asset_class.map(str::to_owned),
            quote_currency: currency.map(str::to_owned),
            start: NaiveDate::from_ymd_opt(2025, 1, 1).expect("date"),
            end: NaiveDate::from_ymd_opt(2026, 8, 27).expect("date"),
        }
    }

    #[test]
    fn parses_price_history_fixture_and_uses_request_currency() {
        let rows = NasdaqNordicClient::parse_price_response(
            &request("TX2997672", Some("TRACKER_CERTIFICATES"), Some("SEK")),
            include_str!("../../tests/fixtures/market_data/nasdaq_price_history_ava_samsung.json"),
        )
        .expect("fixture should parse");

        assert_eq!(rows.len(), 413);
        assert_eq!(
            rows[0].date,
            NaiveDate::from_ymd_opt(2025, 1, 2).expect("date")
        );
        assert_eq!(
            rows[412].date,
            NaiveDate::from_ymd_opt(2026, 8, 27).expect("date")
        );
        assert_eq!(rows[0].provider_symbol, "TX2997672");
        assert_eq!(rows[0].currency, "SEK");
    }

    #[test]
    fn parses_comma_thousands_in_astrazeneca_fixture() {
        let rows = NasdaqNordicClient::parse_price_response(
            &request("TX271", Some("SHARES"), Some("SEK")),
            include_str!("../../tests/fixtures/market_data/nasdaq_price_history_astrazeneca.json"),
        )
        .expect("fixture should parse");

        assert_eq!(rows.len(), 311);
        assert_eq!(
            rows[0].close,
            Decimal::from_str("1369.00").expect("decimal")
        );
    }

    #[test]
    fn skips_blank_close_rows_in_synthetic_fixture() {
        let body = include_str!(
            "../../tests/fixtures/market_data/nasdaq_price_history_synthetic_gaps.json"
        )
        .replace("\"close\": \"\"", "\"close\": \" \u{a0} \"");
        let rows = NasdaqNordicClient::parse_price_response(
            &request("TX-SYNTHETIC-GAPS", Some("SHARES"), Some("SEK")),
            &body,
        )
        .expect("synthetic fixture should parse");

        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].close,
            Decimal::from_str("1609.00").expect("decimal")
        );
    }

    #[test]
    fn all_empty_closes_return_no_data() {
        let body = include_str!(
            "../../tests/fixtures/market_data/nasdaq_price_history_synthetic_gaps.json"
        )
        .replace(" 1,609.00 ", "");
        let error = NasdaqNordicClient::parse_price_response(
            &request("TX-SYNTHETIC-NODATA", Some("SHARES"), Some("SEK")),
            &body,
        )
        .expect_err("all empty closes should be missing");

        assert_eq!(error.reason(), ProviderMissingReason::NoDataInRange);
    }

    #[test]
    fn missing_currency_is_reported() {
        let body =
            include_str!("../../tests/fixtures/market_data/nasdaq_price_history_ava_samsung.json");
        let error = NasdaqNordicClient::parse_price_response(
            &request("TX2997672", Some("TRACKER_CERTIFICATES"), None),
            body,
        )
        .expect_err("missing currency should fail");

        assert_eq!(error.reason(), ProviderMissingReason::ProviderError);
        assert!(error.message().contains("missing_source_currency"));
    }

    #[test]
    fn parses_ambiguous_search_fixture() {
        let matches = NasdaqNordicClient::parse_search_response(
            "SE0000108656",
            include_str!("../../tests/fixtures/market_data/nasdaq_search_ericsson_ambiguous.json"),
        )
        .expect("fixture should parse");

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].provider_symbol, "TX50143");
        assert_eq!(matches[0].currency.as_deref(), Some("EUR"));
        assert_eq!(matches[1].provider_symbol, "TX69");
        assert_eq!(matches[1].currency.as_deref(), Some("SEK"));
        assert!(matches.iter().all(|item| item.quote_type.is_none()));
    }

    #[test]
    fn parses_ava_search_fixture_with_group_and_asset_fields() {
        let matches = NasdaqNordicClient::parse_search_response(
            "JE00BJ7HNC92",
            include_str!("../../tests/fixtures/market_data/nasdaq_search_ava_samsung.json"),
        )
        .expect("fixture should parse");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].exchange.as_deref(), Some("Warrants"));
        assert_eq!(matches[0].name.as_deref(), Some("AVA SAMSUNG TRACKER"));
        assert_eq!(
            matches[0].asset_class.as_deref(),
            Some("TRACKER_CERTIFICATES")
        );
        assert_eq!(matches[0].currency.as_deref(), Some("SEK"));
        assert_eq!(matches[0].provider_symbol, "TX2997672");
    }

    #[test]
    fn parses_no_match_search_fixture_as_empty_match_list() {
        let matches = NasdaqNordicClient::parse_search_response(
            "ZZ0000000000",
            include_str!("../../tests/fixtures/market_data/nasdaq_search_no_match.json"),
        )
        .expect("a successful no-match response should parse");

        assert!(matches.is_empty());
    }

    #[test]
    fn search_error_envelope_is_still_an_error() {
        let error = NasdaqNordicClient::parse_search_response(
            "ZZ0000000000",
            include_str!("../../tests/fixtures/market_data/nasdaq_error_not_found.json"),
        )
        .expect_err("an error envelope should fail through the search response parser");

        assert_eq!(error.reason(), ProviderMissingReason::NotListed);
        assert!(error.message().contains("10003"));
        assert!(error.message().contains("Instrument not found"));
    }

    #[test]
    fn maps_not_found_envelope_to_not_listed() {
        let error = NasdaqNordicClient::parse_price_response(
            &request("TX-MISSING", Some("SHARES"), Some("SEK")),
            include_str!("../../tests/fixtures/market_data/nasdaq_error_not_found.json"),
        )
        .expect_err("not-found fixture should map through the price response parser");

        assert_eq!(error.reason(), ProviderMissingReason::NotListed);
        assert!(error.message().contains("10003"));
        assert!(error.message().contains("Instrument not found"));
    }

    #[test]
    fn non_empty_error_message_is_an_error_even_with_success_status_code() {
        let body = include_str!("../../tests/fixtures/market_data/nasdaq_error_not_found.json")
            .replace("\"rCode\": 400", "\"rCode\": 200");
        let error = NasdaqNordicClient::parse_price_response(
            &request("TX-MISSING", Some("SHARES"), Some("SEK")),
            &body,
        )
        .expect_err("a status message should fail even when rCode is 200");

        assert_eq!(error.reason(), ProviderMissingReason::NotListed);
        assert!(error.message().contains("10003"));
        assert!(error.message().contains("Instrument not found"));
    }

    #[test]
    fn status_code_at_or_above_400_is_an_error_without_a_message() {
        let body = include_str!(
            "../../tests/fixtures/market_data/nasdaq_price_history_synthetic_gaps.json"
        )
        .replace("\"rCode\": 200", "\"rCode\": 400");
        let error = NasdaqNordicClient::parse_price_response(
            &request("TX-SYNTHETIC-STATUS", Some("SHARES"), Some("SEK")),
            &body,
        )
        .expect_err("status rCode 400 should fail");

        assert_eq!(error.reason(), ProviderMissingReason::ProviderError);
        assert!(error.message().contains("400"));
    }

    #[test]
    fn builds_expected_price_history_and_search_urls() {
        let client = NasdaqNordicClient::with_base_url("http://localhost:8080/api/nordic/");
        let request = PriceHistoryRequest {
            symbol: "TX271/quoted".to_owned(),
            asset_class: Some("SHARES & CERTIFICATES".to_owned()),
            quote_currency: Some("SEK".to_owned()),
            start: NaiveDate::from_ymd_opt(2025, 1, 2).expect("date"),
            end: NaiveDate::from_ymd_opt(2026, 8, 27).expect("date"),
        };
        let price_url = Url::parse(
            &client
                .price_history_url(&request)
                .expect("valid request should build a URL"),
        )
        .expect("URL");
        let query = price_url.query_pairs().collect::<Vec<_>>();

        assert_eq!(
            price_url.path(),
            "/api/nordic/instruments/TX271%2Fquoted/price-history"
        );
        assert!(query.contains(&("assetClass".into(), "SHARES & CERTIFICATES".into())));
        assert!(query.contains(&("fromDate".into(), "2025-01-02".into())));
        assert!(query.contains(&("toDate".into(), "2026-08-27".into())));

        let search_url = Url::parse(&client.search_url("SE0000108656 & more")).expect("URL");
        assert_eq!(search_url.path(), "/api/nordic/search");
        assert_eq!(
            search_url
                .query_pairs()
                .find(|(key, _)| key == "searchText")
                .map(|(_, value)| value),
            Some("SE0000108656 & more".into())
        );
    }

    #[test]
    fn invalid_number_names_field_and_row_date() {
        let body = include_str!(
            "../../tests/fixtures/market_data/nasdaq_price_history_synthetic_gaps.json"
        )
        .replace(" 1,609.00 ", "not-a-number");
        let error = NasdaqNordicClient::parse_price_response(
            &request("TX-SYNTHETIC-BAD-NUMBER", Some("SHARES"), Some("SEK")),
            &body,
        )
        .expect_err("invalid close should fail");

        assert!(error.message().contains("close"));
        assert!(error.message().contains("2026-01-03"));
    }

    #[test]
    fn parses_number_with_non_breaking_space_separator() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 3).expect("date");

        assert_eq!(
            parse_number("close", "1\u{a0}369.00", date).expect("number should parse"),
            Decimal::from_str("1369.00").expect("decimal")
        );
    }

    #[tokio::test]
    async fn missing_asset_class_names_the_orderbook() {
        let client = NasdaqNordicClient::with_base_url("http://127.0.0.1:1");
        let error = client
            .daily_history(&request("TX271", None, Some("SEK")))
            .await
            .expect_err("missing asset class should fail before transport");

        assert_eq!(error.reason(), ProviderMissingReason::ProviderError);
        assert!(error.message().contains("TX271"));
        assert!(error.message().contains("asset_class"));
    }

    #[test]
    fn split_adjusted_nasdaq_history_agrees_with_yahoo_within_tolerance() {
        let nasdaq = NasdaqNordicClient::parse_price_response(
            &PriceHistoryRequest {
                symbol: "TX76".to_owned(),
                asset_class: Some("SHARES".to_owned()),
                quote_currency: Some("SEK".to_owned()),
                start: NaiveDate::from_ymd_opt(2021, 3, 1).expect("date"),
                end: NaiveDate::from_ymd_opt(2021, 8, 31).expect("date"),
            },
            include_str!(
                "../../tests/fixtures/market_data/nasdaq_price_history_investor_b_split.json"
            ),
        )
        .expect("Nasdaq fixture should parse");
        let yahoo = crate::providers::yahoo::YahooChartClient::parse_response(
            "INVE-B.ST",
            include_str!("../../tests/fixtures/market_data/yahoo_inve_b_adjusted.json"),
        )
        .expect("Yahoo fixture should parse");

        let yahoo_by_date = yahoo
            .into_iter()
            .map(|row| (row.date, row.close))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut overlap = 0;
        // This guards the split-adjustment contract: unadjusted Nasdaq history
        // would diverge by roughly 4x across the split, silently mis-valuing
        // every split instrument in money of record by the same factor.
        for row in nasdaq {
            let Some(yahoo_close) = yahoo_by_date.get(&row.date) else {
                continue;
            };
            overlap += 1;
            let difference = (row.close - *yahoo_close).abs();
            let tolerance = yahoo_close.abs() * Decimal::new(1, 3);
            assert!(
                difference <= tolerance,
                "{} differs beyond 0.1%: Nasdaq={} Yahoo={} difference={} tolerance={}",
                row.date,
                row.close,
                yahoo_close,
                difference,
                tolerance
            );
        }

        assert!(
            overlap >= 100,
            "expected substantial date overlap, got {overlap}"
        );
    }
}
