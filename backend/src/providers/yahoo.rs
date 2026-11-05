use chrono::{DateTime, Duration, NaiveDate, Utc};
use reqwest::{Client, Url};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Number;
use std::str::FromStr;

use super::{
    DailyClose, MarketDataProvider, PriceHistoryRequest, ProviderError, ProviderMissingReason,
    ProviderResult, SymbolSearchMatch,
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

    pub(crate) fn parse_response(
        symbol: &str,
        body: &str,
        as_of: DateTime<Utc>,
    ) -> ProviderResult<Vec<DailyClose>> {
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

        let finality = YahooSessionFinality::from_meta(&result.meta, as_of)?;
        let gmtoffset = finality.gmtoffset;
        let meta_symbol = result.meta.symbol.unwrap_or_else(|| symbol.to_owned());
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
                date: date_from_timestamp(timestamp, gmtoffset)?,
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

        // An otherwise valid response containing only provisional bars is a
        // successful fetch with no final closes, not a provider failure.
        rows.retain(|row| finality.is_final(row.date));
        rows.sort_by_key(|row| row.date);
        Ok(rows)
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

    fn parse_response(query: &str, body: &str) -> ProviderResult<Vec<SymbolSearchMatch>> {
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
    async fn daily_history(
        &self,
        request: &PriceHistoryRequest,
    ) -> ProviderResult<Vec<DailyClose>> {
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

        let parsed = Self::parse_response(symbol, &body, request.as_of);
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
        .checked_add_signed(Duration::seconds(gmtoffset))
        .map(|local| local.date_naive())
        .ok_or_else(|| {
            ProviderError::provider_error(
                MarketDataProvider::Yahoo.as_str(),
                format!(
                    "Yahoo timestamp {timestamp} with offset {gmtoffset} is outside the date range"
                ),
            )
        })
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
    timestamp: Vec<i64>,
    indicators: YahooIndicators,
}

#[derive(Debug, Deserialize)]
struct YahooMeta {
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default, deserialize_with = "deserialize_gmt_offset")]
    gmtoffset: Option<i64>,
    #[serde(rename = "currentTradingPeriod", default)]
    current_trading_period: serde_json::Value,
}

fn deserialize_gmt_offset<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Only an absent field defaults to None; a present value must be valid.
    let offset = i64::deserialize(deserializer)?;
    if (-86_399..=86_399).contains(&offset) {
        Ok(Some(offset))
    } else {
        Err(serde::de::Error::custom(
            "Yahoo gmtoffset is outside the supported range",
        ))
    }
}

#[derive(Debug, Deserialize)]
struct YahooRegularSession {
    start: i64,
    end: i64,
    gmtoffset: i64,
}

/// Session dates use the listing's offset, never the application's timezone.
/// Keep malformed session metadata out of the typed chart envelope so it can
/// fall back conservatively instead of rejecting otherwise usable history.
struct YahooSessionFinality {
    gmtoffset: i64,
    provisional_from: NaiveDate,
    session_final: bool,
}

impl YahooSessionFinality {
    fn from_meta(meta: &YahooMeta, as_of: DateTime<Utc>) -> ProviderResult<Self> {
        let regular = serde_json::from_value::<YahooRegularSession>(
            meta.current_trading_period["regular"].clone(),
        )
        .ok()
        .filter(|session| {
            session.start < session.end && (-86_399..=86_399).contains(&session.gmtoffset)
        });
        if let Some(session) = regular {
            if let (Some(start), Some(end)) = (
                DateTime::<Utc>::from_timestamp(session.start, 0),
                DateTime::<Utc>::from_timestamp(session.end, 0),
            ) {
                if let (Some(local_start), Some(final_at)) = (
                    start.checked_add_signed(Duration::seconds(session.gmtoffset)),
                    end.checked_add_signed(Duration::minutes(30)),
                ) {
                    return Ok(Self {
                        gmtoffset: session.gmtoffset,
                        provisional_from: local_start.date_naive(),
                        session_final: as_of >= final_at,
                    });
                }
            }
        }
        Ok(Self {
            gmtoffset: meta.gmtoffset.unwrap_or(0),
            provisional_from: date_from_timestamp(as_of.timestamp(), meta.gmtoffset.unwrap_or(0))?,
            session_final: false,
        })
    }

    fn is_final(&self, date: NaiveDate) -> bool {
        date < self.provisional_from || (date == self.provisional_from && self.session_final)
    }
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
    fn parses_msft_fixture_into_daily_closes() {
        let rows = YahooChartClient::parse_response(
            "MSFT",
            include_str!("../../tests/fixtures/market_data/yahoo_msft.json"),
            "2026-06-13T12:00:00Z".parse().expect("instant"),
        )
        .expect("fixture should parse");

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
            "2026-06-13T12:00:00Z".parse().expect("instant"),
        )
        .expect("fixture should parse");

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
    fn current_session_closes_wait_for_the_thirty_minute_margin() {
        for body in [
            include_str!("../../tests/fixtures/fundamentals/yahoo_chart_volv_b_st.json"),
            include_str!("../../tests/fixtures/fundamentals/yahoo_chart_msft.json"),
        ] {
            let chart: serde_json::Value = serde_json::from_str(body).expect("fixture");
            let meta = &chart["chart"]["result"][0]["meta"];
            let symbol = meta["symbol"].as_str().expect("symbol");
            let recorded = meta["regularMarketTime"].as_i64().expect("recorded time");
            let end = meta["currentTradingPeriod"]["regular"]["end"]
                .as_i64()
                .expect("session end");
            let today = NaiveDate::from_ymd_opt(2026, 11, 4).expect("date");
            let previous = NaiveDate::from_ymd_opt(2026, 11, 3).expect("date");
            for instant in [recorded, end, end + 29 * 60, end + 30 * 60 - 1] {
                let rows = YahooChartClient::parse_response(
                    symbol,
                    body,
                    DateTime::from_timestamp(instant, 0).expect("instant"),
                )
                .expect("closes");
                assert!(
                    rows.iter().all(|row| row.date < today),
                    "{symbol} at {instant}"
                );
                assert_eq!(rows.last().expect("earlier closes").date, previous);
            }
            for instant in [end + 30 * 60, end + 31 * 60] {
                let rows = YahooChartClient::parse_response(
                    symbol,
                    body,
                    DateTime::from_timestamp(instant, 0).expect("instant"),
                )
                .expect("closes");
                assert_eq!(
                    rows.last().expect("final close").date,
                    today,
                    "{symbol} at {instant}"
                );
            }
        }
    }

    #[test]
    fn finalized_regular_session_does_not_finalize_later_bars() {
        let mut chart: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/fundamentals/yahoo_chart_volv_b_st.json"
        ))
        .expect("fixture");
        let result = &mut chart["chart"]["result"][0];
        let end = result["meta"]["currentTradingPeriod"]["regular"]["end"]
            .as_i64()
            .expect("session end");
        result["timestamp"]
            .as_array_mut()
            .expect("timestamps")
            .push(serde_json::json!(end + 24 * 60 * 60));
        result["indicators"]["quote"][0]["close"]
            .as_array_mut()
            .expect("closes")
            .push(serde_json::json!(999));

        let rows = YahooChartClient::parse_response(
            "VOLV-B.ST",
            &chart.to_string(),
            DateTime::from_timestamp(end + 30 * 60, 0).expect("instant"),
        )
        .expect("closes");
        assert_eq!(rows.len(), 23);
        assert_eq!(
            rows.last().expect("final close").date.to_string(),
            "2026-11-04"
        );
    }

    #[test]
    fn malformed_gmt_offset_is_a_provider_error() {
        let original: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/fundamentals/yahoo_chart_volv_b_st.json"
        ))
        .expect("fixture");
        for offset in [
            serde_json::json!("3600"),
            serde_json::json!(3600.5),
            serde_json::json!(true),
            serde_json::Value::Null,
            serde_json::json!(86400),
            serde_json::json!(-86400),
            serde_json::json!(u64::MAX),
        ] {
            // Reject bad offsets even when regular-session metadata is valid.
            for has_session in [true, false] {
                let mut chart = original.clone();
                let meta = chart["chart"]["result"][0]["meta"]
                    .as_object_mut()
                    .expect("meta");
                meta.insert("gmtoffset".to_owned(), offset.clone());
                if !has_session {
                    meta.remove("currentTradingPeriod");
                }
                let error = YahooChartClient::parse_response(
                    "VOLV-B.ST",
                    &chart.to_string(),
                    "2026-11-04T16:00:00Z".parse().expect("instant"),
                )
                .expect_err("malformed offset must fail");
                assert_eq!(error.reason(), ProviderMissingReason::ProviderError);
                assert!(error.message().contains("VOLV-B.ST"));
            }
        }
    }

    #[test]
    fn missing_or_malformed_session_uses_listing_local_today() {
        let original: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/market_data/yahoo_msft.json"
        ))
        .expect("fixture");
        // UTC June 13 is still June 12 in New York: June 12 must be withheld.
        let as_of = "2026-06-13T01:00:00Z".parse().expect("instant");
        for period in [
            serde_json::Value::Null,
            serde_json::json!({"regular": {"start": "bad", "end": 1781280000, "gmtoffset": -14400}}),
            serde_json::json!({"regular": {"start": 1781280000, "end": 1781280000, "gmtoffset": -14400}}),
            serde_json::json!({"regular": {"start": 1781271000, "end": 1781280000}}),
            serde_json::json!({"regular": {"start": 1781271000, "end": 1781280000, "gmtoffset": 999999}}),
            serde_json::json!({"regular": {"start": 1781271000, "end": i64::MAX, "gmtoffset": -14400}}),
        ] {
            let mut chart = original.clone();
            chart["chart"]["result"][0]["meta"]["currentTradingPeriod"] = period;
            let rows = YahooChartClient::parse_response("MSFT", &chart.to_string(), as_of)
                .expect("fallback closes");
            assert_eq!(rows.len(), 2);
            assert_eq!(
                rows.last().expect("previous close").date.to_string(),
                "2026-06-11"
            );
        }
    }

    #[test]
    fn missing_offset_uses_utc_today_and_withholds_future_dates() {
        let mut chart: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/market_data/yahoo_msft.json"
        ))
        .expect("fixture");
        chart["chart"]["result"][0]["meta"]
            .as_object_mut()
            .expect("meta")
            .remove("gmtoffset");
        let body = chart.to_string();
        let rows = YahooChartClient::parse_response(
            "MSFT",
            &body,
            "2026-06-11T23:00:00Z".parse().expect("instant"),
        )
        .expect("UTC fallback");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date.to_string(), "2026-06-10");
        let rows = YahooChartClient::parse_response(
            "MSFT",
            &body,
            "2026-06-10T12:00:00Z".parse().expect("instant"),
        )
        .expect("provisional bars are not an error");
        assert!(rows.is_empty());
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
