use chrono::{DateTime, Duration, NaiveDate, Utc};
use reqwest::{Client, Url};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Number;
use std::str::FromStr;

use super::{
    DailyClose, DividendCapture, MarketDataProvider, PriceHistory, PriceHistoryRequest,
    ProviderError, ProviderMissingReason, ProviderResult, SymbolSearchMatch,
};

const DEFAULT_BASE_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart";
const DEFAULT_SEARCH_BASE_URL: &str = "https://query1.finance.yahoo.com/v1/finance/search";

#[derive(Clone)]
pub struct YahooChartClient {
    client: Client,
    base_url: String,
}

impl YahooChartClient {
    pub fn new() -> Self {
        Self::with_base_url(DEFAULT_BASE_URL)
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

    fn url(&self, symbol: &str, start: NaiveDate, end: NaiveDate) -> String {
        let start_ts = start
            .and_hms_opt(0, 0, 0)
            .expect("start date should be representable as midnight UTC")
            .and_utc()
            .timestamp();
        let end_ts = end
            .and_hms_opt(0, 0, 0)
            .expect("end date should be representable as midnight UTC")
            .and_utc()
            .timestamp()
            + 86_400;

        let mut url = Url::parse(self.base_url.trim_end_matches('/'))
            .expect("Yahoo base URL should always parse");
        url.path_segments_mut()
            .expect("Yahoo URL should support path segments")
            .push(symbol);
        url.query_pairs_mut()
            .append_pair("period1", &start_ts.to_string())
            .append_pair("period2", &end_ts.to_string())
            .append_pair("interval", "1d")
            .append_pair("events", "div,splits");
        url.to_string()
    }

    fn log_failure(symbol: &str, start: NaiveDate, end: NaiveDate, error: &ProviderError) {
        crate::engine_error!(
            "market data price failure provider={} symbol={} range={}..{} reason={} message={}",
            MarketDataProvider::Yahoo,
            symbol,
            start,
            end,
            error.reason_code(),
            error.message()
        );
    }

    pub(crate) fn parse_response(symbol: &str, body: &str) -> ProviderResult<PriceHistory> {
        let response: YahooChartResponse = serde_json::from_str(body).map_err(|error| {
            ProviderError::provider_error(
                MarketDataProvider::Yahoo.as_str(),
                format!("failed to parse Yahoo chart response for {symbol}: {error}"),
            )
        })?;

        let chart = response.chart;
        if let Some(error) = chart.error {
            return Err(map_chart_error(symbol, error));
        }

        let result = chart.result.into_iter().next().ok_or_else(|| {
            ProviderError::new(
                MarketDataProvider::Yahoo.as_str(),
                ProviderMissingReason::NoDataInRange,
                format!("Yahoo chart returned no result for {symbol}"),
            )
        })?;

        let meta_symbol = result.meta.symbol.unwrap_or_else(|| symbol.to_owned());
        let dividends = parse_dividends(
            &result.events,
            result.meta.currency.as_deref(),
            result.meta.gmtoffset.unwrap_or(0),
        );
        let currency = result.meta.currency.ok_or_else(|| {
            ProviderError::provider_error(
                MarketDataProvider::Yahoo.as_str(),
                format!("Yahoo chart response for {symbol} did not include a currency"),
            )
        })?;
        let quote = result.indicators.quote.into_iter().next().ok_or_else(|| {
            ProviderError::new(
                MarketDataProvider::Yahoo.as_str(),
                ProviderMissingReason::NoDataInRange,
                format!("Yahoo chart returned no quote series for {symbol}"),
            )
        })?;

        let mut rows = Vec::new();
        for (timestamp, close) in result.timestamp.into_iter().zip(quote.close) {
            let Some(close) = close else {
                continue;
            };
            rows.push(DailyClose {
                provider: MarketDataProvider::Yahoo,
                provider_symbol: meta_symbol.clone(),
                date: date_from_timestamp(timestamp, result.meta.gmtoffset.unwrap_or(0))?,
                close: number_to_decimal(&close, symbol)?,
                currency: currency.clone(),
            });
        }

        if rows.is_empty() {
            return Err(ProviderError::new(
                MarketDataProvider::Yahoo.as_str(),
                ProviderMissingReason::NoDataInRange,
                format!("Yahoo chart returned no closes for {symbol}"),
            ));
        }

        rows.sort_by_key(|row| row.date);
        Ok(PriceHistory {
            closes: rows,
            dividends,
        })
    }
}

impl Default for YahooChartClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct YahooSearchClient {
    client: Client,
    base_url: String,
}

impl YahooSearchClient {
    pub fn new() -> Self {
        Self::with_base_url(DEFAULT_SEARCH_BASE_URL)
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

    fn url(&self, query: &str) -> String {
        let mut url = Url::parse(&self.base_url).expect("Yahoo search URL should always parse");
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("quotesCount", "10")
            .append_pair("newsCount", "0");
        url.to_string()
    }

    pub(crate) fn parse_response(
        query: &str,
        body: &str,
    ) -> ProviderResult<Vec<SymbolSearchMatch>> {
        let response: YahooSearchResponse = serde_json::from_str(body).map_err(|error| {
            ProviderError::provider_error(
                MarketDataProvider::Yahoo.as_str(),
                format!("failed to parse Yahoo search response for {query}: {error}"),
            )
        })?;

        Ok(response
            .quotes
            .into_iter()
            .filter_map(|quote| {
                let symbol = quote.symbol?;
                Some(SymbolSearchMatch {
                    provider: MarketDataProvider::Yahoo,
                    provider_symbol: symbol,
                    quote_type: quote.quote_type,
                    exchange: quote.exchange,
                    name: quote.longname.or(quote.shortname),
                    asset_class: None,
                    currency: None,
                })
            })
            .collect())
    }

    fn log_failure(query: &str, error: &ProviderError) {
        crate::engine_warn!(
            "market data symbol search failure provider={} query={} reason={} message={}",
            MarketDataProvider::Yahoo,
            query,
            error.reason_code(),
            error.message()
        );
    }
}

impl Default for YahooSearchClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl super::SymbolSearchProvider for YahooSearchClient {
    async fn search(&self, query: &str) -> ProviderResult<Vec<SymbolSearchMatch>> {
        let url = self.url(query);
        let response = match self.client.get(&url).send().await {
            Ok(response) => response,
            Err(error) => {
                let error = ProviderError::transport(
                    MarketDataProvider::Yahoo.as_str(),
                    format!("failed to request Yahoo symbol search for {query}: {error}"),
                );
                Self::log_failure(query, &error);
                return Err(error);
            }
        };

        let status = response.status();
        let body = match response.text().await {
            Ok(body) => body,
            Err(error) => {
                let error = ProviderError::transport(
                    MarketDataProvider::Yahoo.as_str(),
                    format!("failed to read Yahoo symbol search response for {query}: {error}"),
                );
                Self::log_failure(query, &error);
                return Err(error);
            }
        };

        if !status.is_success() {
            let error = ProviderError::with_http_status(
                MarketDataProvider::Yahoo.as_str(),
                status.as_u16(),
                format!("Yahoo symbol search for {query} failed with HTTP {status}: {body}"),
            );
            Self::log_failure(query, &error);
            return Err(error);
        }

        let parsed = Self::parse_response(query, &body);
        if let Err(error) = &parsed {
            Self::log_failure(query, error);
        }
        parsed
    }
}

#[async_trait::async_trait]
impl super::PriceProvider for YahooChartClient {
    async fn daily_history(&self, request: &PriceHistoryRequest) -> ProviderResult<PriceHistory> {
        let symbol = request.symbol.as_str();
        let start = request.start;
        let end = request.end;
        let url = self.url(symbol, start, end);
        let response = match self.client.get(&url).send().await {
            Ok(response) => response,
            Err(error) => {
                let error = ProviderError::transport(
                    MarketDataProvider::Yahoo.as_str(),
                    format!("failed to request Yahoo chart data for {symbol}: {error}"),
                );
                Self::log_failure(symbol, start, end, &error);
                return Err(error);
            }
        };

        let status = response.status();
        let body = match response.text().await {
            Ok(body) => body,
            Err(error) => {
                let error = ProviderError::transport(
                    MarketDataProvider::Yahoo.as_str(),
                    format!("failed to read Yahoo chart response for {symbol}: {error}"),
                );
                Self::log_failure(symbol, start, end, &error);
                return Err(error);
            }
        };

        if !status.is_success() {
            let error = ProviderError::with_http_status(
                MarketDataProvider::Yahoo.as_str(),
                status.as_u16(),
                format!("Yahoo chart request for {symbol} failed with HTTP {status}: {body}"),
            );
            Self::log_failure(symbol, start, end, &error);
            return Err(error);
        }

        let parsed = Self::parse_response(symbol, &body);
        if let Err(error) = &parsed {
            Self::log_failure(symbol, start, end, error);
        }
        parsed
    }
}

fn date_from_timestamp(timestamp: i64, gmtoffset: i64) -> ProviderResult<NaiveDate> {
    let datetime = DateTime::<Utc>::from_timestamp(timestamp, 0).ok_or_else(|| {
        ProviderError::provider_error(
            MarketDataProvider::Yahoo.as_str(),
            format!("Yahoo timestamp {timestamp} could not be converted to a date"),
        )
    })?;
    datetime
        .checked_add_signed(Duration::try_seconds(gmtoffset).ok_or_else(|| {
            ProviderError::provider_error(
                MarketDataProvider::Yahoo.as_str(),
                "invalid chart GMT offset",
            )
        })?)
        .map(|datetime| datetime.date_naive())
        .ok_or_else(|| {
            ProviderError::provider_error(MarketDataProvider::Yahoo.as_str(), "chart date overflow")
        })
}

fn parse_dividends(
    events: &serde_json::Value,
    currency: Option<&str>,
    offset: i64,
) -> DividendCapture {
    use crate::domain::opportunity_evidence::dividend_check::DividendEvent;
    let parse = || -> Result<Vec<DividendEvent>, &'static str> {
        let currency = currency
            .filter(|value| !value.trim().is_empty())
            .ok_or("dividend_currency_missing")?;
        if matches!(currency, "GBp" | "GBX" | "ZAc" | "ILA") {
            return Err("dividend_currency_subunit_ambiguous");
        }
        if events.is_null() {
            return Ok(vec![]);
        }
        let object = events.as_object().ok_or("dividend_events_invalid")?;
        let Some(dividends) = object.get("dividends") else {
            return Ok(vec![]);
        };
        let dividends = dividends.as_object().ok_or("dividend_events_invalid")?;
        let mut amounts = std::collections::BTreeMap::new();
        for event in dividends.values() {
            let timestamp = event
                .get("date")
                .and_then(serde_json::Value::as_i64)
                .ok_or("dividend_date_invalid")?;
            let ex_date =
                date_from_timestamp(timestamp, offset).map_err(|_| "dividend_date_invalid")?;
            let amount = event
                .get("amount")
                .and_then(serde_json::Value::as_number)
                .and_then(|number| Decimal::from_str(&number.to_string()).ok())
                .filter(|amount| *amount > Decimal::ZERO)
                .ok_or("dividend_amount_invalid")?;
            let total = amounts.entry(ex_date).or_insert(Decimal::ZERO);
            *total = total.checked_add(amount).ok_or("dividend_amount_invalid")?;
        }
        Ok(amounts
            .into_iter()
            .map(|(ex_date, amount)| DividendEvent {
                ex_date,
                amount,
                currency: currency.to_owned(),
            })
            .collect())
    };
    match parse() {
        Ok(events) => DividendCapture::Valid(events),
        Err(reason) => DividendCapture::Invalid {
            reason: reason.to_owned(),
        },
    }
}

fn number_to_decimal(number: &Number, symbol: &str) -> ProviderResult<Decimal> {
    Decimal::from_str(&number.to_string()).map_err(|error| {
        ProviderError::provider_error(
            MarketDataProvider::Yahoo.as_str(),
            format!("Yahoo close value for {symbol} was invalid: {error}"),
        )
    })
}

fn map_chart_error(symbol: &str, error: YahooChartError) -> ProviderError {
    let mut description = error
        .description
        .unwrap_or_else(|| "Yahoo chart error".to_owned());
    if let Some(code) = error.code {
        if !description.contains(&code) {
            description = format!("{code}: {description}");
        }
    }

    let lowered = description.to_ascii_lowercase();
    let reason = if lowered.contains("rate limit") {
        ProviderMissingReason::RateLimited
    } else if lowered.contains("no data") {
        ProviderMissingReason::NoDataInRange
    } else if lowered.contains("not found") || lowered.contains("invalid symbol") {
        ProviderMissingReason::NotListed
    } else if lowered.contains("market closed") {
        ProviderMissingReason::MarketClosed
    } else {
        ProviderMissingReason::ProviderError
    };

    ProviderError::new(
        MarketDataProvider::Yahoo.as_str(),
        reason,
        format!("Yahoo chart reported an error for {symbol}: {description}"),
    )
}

#[derive(Debug, Deserialize)]
struct YahooChartResponse {
    chart: YahooChartEnvelope,
}

#[derive(Debug, Deserialize)]
struct YahooSearchResponse {
    #[serde(default)]
    quotes: Vec<YahooSearchQuote>,
}

#[derive(Debug, Deserialize)]
struct YahooSearchQuote {
    #[serde(default)]
    symbol: Option<String>,
    #[serde(rename = "quoteType", default)]
    quote_type: Option<String>,
    #[serde(default)]
    exchange: Option<String>,
    #[serde(default)]
    shortname: Option<String>,
    #[serde(default)]
    longname: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YahooChartEnvelope {
    #[serde(default)]
    result: Vec<YahooChartResult>,
    #[serde(default)]
    error: Option<YahooChartError>,
}

#[derive(Debug, Deserialize)]
struct YahooChartError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YahooChartResult {
    meta: YahooMeta,
    #[serde(default)]
    events: serde_json::Value,
    #[serde(default)]
    timestamp: Vec<i64>,
    indicators: YahooIndicators,
}

#[derive(Debug, Deserialize)]
struct YahooMeta {
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    gmtoffset: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct YahooIndicators {
    #[serde(default)]
    quote: Vec<YahooQuoteSeries>,
}

#[derive(Debug, Deserialize)]
struct YahooQuoteSeries {
    #[serde(default)]
    close: Vec<Option<Number>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;

    #[test]
    fn dividend_fixtures_preserve_closes_and_chart_currency() {
        for (symbol, body, count, date, amount, currency) in [
            (
                "MSFT",
                include_str!("../../tests/fixtures/market_data/yahoo_msft_dividend.json"),
                15,
                "2026-08-20",
                "0.91",
                "USD",
            ),
            (
                "VOLV-B.ST",
                include_str!("../../tests/fixtures/market_data/yahoo_volv_b_st_dividend.json"),
                32,
                "2026-04-09",
                "13.0",
                "SEK",
            ),
        ] {
            let history = YahooChartClient::parse_response(symbol, body).unwrap();
            assert_eq!(history.closes.len(), count);
            let DividendCapture::Valid(events) = history.dividends else {
                panic!("valid dividends expected");
            };
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].ex_date, date.parse::<NaiveDate>().unwrap());
            assert_eq!(events[0].amount, amount.parse::<Decimal>().unwrap());
            assert_eq!(events[0].currency, currency);
        }
        let history = YahooChartClient::parse_response(
            "INVE-B.ST",
            include_str!("../../tests/fixtures/market_data/yahoo_inve_b_adjusted.json"),
        )
        .unwrap();
        assert!(matches!(history.dividends, DividendCapture::Valid(events) if events.len() == 1));
    }

    #[test]
    fn dividend_dates_share_close_timezone_and_validation_is_all_or_nothing() {
        let body = serde_json::json!({"chart":{"result":[{
            "meta":{"symbol":"TEST", "currency":"USD", "gmtoffset":-18000},
            "timestamp":[0], "indicators":{"quote":[{"close":[100]}]},
            "events":{"dividends":{"0":{"date":0,"amount":1}}}
        }],"error":null}});
        let parsed = YahooChartClient::parse_response("TEST", &body.to_string()).unwrap();
        let DividendCapture::Valid(events) = parsed.dividends else {
            panic!("valid capture");
        };
        assert_eq!(
            events[0].ex_date,
            "1969-12-31".parse::<NaiveDate>().unwrap()
        );
        assert_eq!(events[0].ex_date, parsed.closes[0].date);
        let mut bad = body.clone();
        bad["chart"]["result"][0]["events"]["dividends"]["second"] =
            serde_json::json!({"date":86400});
        let parsed = YahooChartClient::parse_response("TEST", &bad.to_string()).unwrap();
        assert_eq!(parsed.closes.len(), 1);
        assert_eq!(
            parsed.dividends,
            DividendCapture::Invalid {
                reason: "dividend_amount_invalid".into()
            }
        );
    }

    #[test]
    fn invalid_dividends_never_reject_valid_closes() {
        let original: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/market_data/yahoo_msft_dividend.json"
        ))
        .unwrap();
        for field in ["date", "amount"] {
            for value in [
                serde_json::Value::Null,
                serde_json::json!("bad"),
                serde_json::json!(-1),
                serde_json::json!(0),
            ] {
                let mut body = original.clone();
                let dividends = body["chart"]["result"][0]["events"]["dividends"]
                    .as_object_mut()
                    .unwrap();
                dividends.values_mut().next().unwrap()[field] = value;
                if field == "date" && dividends.values().next().unwrap()[field].as_i64().is_some() {
                    continue; // Negative timestamps and the epoch are representable dates.
                }
                let history = YahooChartClient::parse_response("MSFT", &body.to_string()).unwrap();
                assert_eq!(history.closes.len(), 15);
                assert!(matches!(history.dividends, DividendCapture::Invalid { .. }));
            }
        }
        for currency in [
            None,
            Some(""),
            Some("GBp"),
            Some("GBX"),
            Some("ZAc"),
            Some("ILA"),
        ] {
            let mut body = original.clone();
            body["chart"]["result"][0]["meta"]["currency"] = serde_json::json!(currency);
            let parsed = YahooChartClient::parse_response("MSFT", &body.to_string());
            if currency.is_none() {
                assert_eq!(parsed.unwrap_err().reason_code(), "provider_error");
                continue;
            }
            let history = parsed.unwrap();
            assert_eq!(history.closes.len(), 15);
            assert!(history
                .closes
                .iter()
                .all(|row| Some(row.currency.as_str()) == currency));
            assert!(matches!(history.dividends, DividendCapture::Invalid { .. }));
        }
        let mut body = original.clone();
        body["chart"]["result"][0]["events"] = serde_json::json!({"splits": {}});
        assert_eq!(
            YahooChartClient::parse_response("MSFT", &body.to_string())
                .unwrap()
                .dividends,
            DividendCapture::Valid(vec![])
        );
        body["chart"]["result"][0]["events"]["dividends"] = serde_json::json!([]);
        assert!(matches!(
            YahooChartClient::parse_response("MSFT", &body.to_string())
                .unwrap()
                .dividends,
            DividendCapture::Invalid { .. }
        ));
    }

    #[test]
    fn parses_msft_fixture_into_daily_closes() {
        let rows = YahooChartClient::parse_response(
            "MSFT",
            include_str!("../../tests/fixtures/market_data/yahoo_msft.json"),
        )
        .expect("fixture should parse")
        .closes;

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].provider, MarketDataProvider::Yahoo);
        assert_eq!(rows[0].provider_symbol, "MSFT");
        assert_eq!(rows[0].currency, "USD");
        assert_eq!(
            rows[0].date,
            NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid")
        );
        assert_eq!(rows[0].close.round_dp(2).to_string(), "397.36");
    }

    #[test]
    fn parses_asml_fixture_with_amsterdam_timezone() {
        let rows = YahooChartClient::parse_response(
            "ASML.AS",
            include_str!("../../tests/fixtures/market_data/yahoo_asml_as.json"),
        )
        .expect("fixture should parse")
        .closes;

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].provider_symbol, "ASML.AS");
        assert_eq!(rows[0].currency, "EUR");
        assert_eq!(
            rows[2].date,
            NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid")
        );
        assert_eq!(rows[2].close.round_dp(2).to_string(), "1629.60");
    }

    #[test]
    fn maps_yahoo_error_to_stable_reason() {
        let error = map_chart_error(
            "MSFT",
            YahooChartError {
                code: Some("404".to_owned()),
                description: Some("Not Found".to_owned()),
            },
        );

        assert_eq!(error.reason(), ProviderMissingReason::NotListed);
        assert_eq!(error.reason_code(), "not_listed");
    }

    #[test]
    fn parses_search_response_into_symbol_matches() {
        let rows = YahooSearchClient::parse_response(
            "US5949181045",
            r#"{
                "quotes": [
                    {
                        "exchange": "NMS",
                        "shortname": "Microsoft Corporation",
                        "quoteType": "EQUITY",
                        "symbol": "MSFT",
                        "longname": "Microsoft Corporation"
                    }
                ]
            }"#,
        )
        .expect("search fixture should parse");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].provider, MarketDataProvider::Yahoo);
        assert_eq!(rows[0].provider_symbol, "MSFT");
        assert_eq!(rows[0].quote_type.as_deref(), Some("EQUITY"));
        assert_eq!(rows[0].exchange.as_deref(), Some("NMS"));
    }
}
