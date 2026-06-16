use chrono::{DateTime, Duration, NaiveDate, Utc};
use reqwest::{Client, Url};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Number;
use std::str::FromStr;

use super::{DailyClose, MarketDataProvider, ProviderError, ProviderMissingReason, ProviderResult};

const DEFAULT_BASE_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart";

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
            client: build_client(),
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

    fn parse_response(symbol: &str, body: &str) -> ProviderResult<Vec<DailyClose>> {
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
        Ok(rows)
    }
}

impl Default for YahooChartClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl super::PriceProvider for YahooChartClient {
    async fn daily_history(
        &self,
        symbol: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> ProviderResult<Vec<DailyClose>> {
        let url = self.url(symbol, start, end);
        let response = match self.client.get(&url).send().await {
            Ok(response) => response,
            Err(error) => {
                let error = ProviderError::provider_error(
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
                let error = ProviderError::provider_error(
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
    Ok((datetime + Duration::seconds(gmtoffset)).date_naive())
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
    #[serde(default)]
    gmtoffset: Option<i64>,
}

fn build_client() -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .expect("Yahoo HTTP client should build")
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
}
