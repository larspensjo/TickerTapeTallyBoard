use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use reqwest::{Client, Url};
use rust_decimal::Decimal;
use serde_json::Value;
use std::{
    collections::VecDeque,
    str::FromStr,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

use super::{ProviderError, ProviderMissingReason, ProviderResult};
use crate::domain::fundamentals::{FundamentalValue, Metric, PeriodType, SplitEvent};

pub const TIMESERIES_ENDPOINT: &str =
    "https://query1.finance.yahoo.com/ws/fundamentals-timeseries/v1/finance/timeseries";
pub const CHART_ENDPOINT: &str = "https://query1.finance.yahoo.com/v8/finance/chart";

// Keep the native measures distinct: only matching annual/quarter measures can
// be subtracted, and trailing values are evidence or cross-checks, never figures.
pub const SERIES: &[(&str, PeriodType, Metric)] = &[
    (
        "quarterlyTotalRevenue",
        PeriodType::Quarter,
        Metric::Revenue,
    ),
    (
        "quarterlyDilutedEPS",
        PeriodType::Quarter,
        Metric::DilutedEps,
    ),
    ("quarterlyNetIncome", PeriodType::Quarter, Metric::NetIncome),
    (
        "quarterlyNetIncomeCommonStockholders",
        PeriodType::Quarter,
        Metric::CommonNetIncome,
    ),
    (
        "quarterlyDilutedNIAvailtoComStockholders",
        PeriodType::Quarter,
        Metric::DilutedNetIncome,
    ),
    (
        "quarterlyDilutedAverageShares",
        PeriodType::Quarter,
        Metric::DilutedShares,
    ),
    (
        "quarterlyOrdinarySharesNumber",
        PeriodType::Quarter,
        Metric::OrdinaryShares,
    ),
    ("annualTotalRevenue", PeriodType::Annual, Metric::Revenue),
    ("annualNetIncome", PeriodType::Annual, Metric::NetIncome),
    (
        "annualNetIncomeCommonStockholders",
        PeriodType::Annual,
        Metric::CommonNetIncome,
    ),
    (
        "annualDilutedNIAvailtoComStockholders",
        PeriodType::Annual,
        Metric::DilutedNetIncome,
    ),
    ("annualDilutedEPS", PeriodType::Annual, Metric::DilutedEps),
    (
        "trailingTotalRevenue",
        PeriodType::Trailing,
        Metric::Revenue,
    ),
    ("trailingNetIncome", PeriodType::Trailing, Metric::NetIncome),
    (
        "trailingDilutedEPS",
        PeriodType::Trailing,
        Metric::DilutedEps,
    ),
    ("trailingMarketCap", PeriodType::Evidence, Metric::MarketCap),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundamentalsRequest {
    pub symbol: String,
    pub end: NaiveDate,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListingDetails {
    pub security_type: Option<String>,
    pub splits: Vec<SplitEvent>,
}

#[async_trait]
pub trait FundamentalsProvider: Send + Sync {
    async fn fetch(&self, request: &FundamentalsRequest) -> ProviderResult<Vec<FundamentalValue>>;
    async fn listing(&self, request: &FundamentalsRequest) -> ProviderResult<ListingDetails>;
}

#[derive(Clone)]
pub struct YahooFundamentalsClient {
    client: Client,
    timeseries_url: String,
    chart_url: String,
}
impl Default for YahooFundamentalsClient {
    fn default() -> Self {
        Self::new()
    }
}
impl YahooFundamentalsClient {
    pub fn new() -> Self {
        Self::with_client(
            super::http::build_client(),
            TIMESERIES_ENDPOINT,
            CHART_ENDPOINT,
        )
    }
    pub fn with_client(
        client: Client,
        timeseries_url: impl Into<String>,
        chart_url: impl Into<String>,
    ) -> Self {
        Self {
            client,
            timeseries_url: timeseries_url.into(),
            chart_url: chart_url.into(),
        }
    }
    fn url(&self, request: &FundamentalsRequest, listing: bool) -> String {
        let mut url = Url::parse(if listing {
            &self.chart_url
        } else {
            &self.timeseries_url
        })
        .expect("Yahoo URL");
        url.path_segments_mut()
            .expect("path segments")
            .push(&request.symbol);
        let end = request
            .end
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_utc()
            .timestamp()
            + 86400;
        url.query_pairs_mut()
            .append_pair("period1", "1420070400")
            .append_pair("period2", &end.to_string());
        if listing {
            url.query_pairs_mut()
                .append_pair("interval", "1mo")
                .append_pair("events", "splits");
        } else {
            url.query_pairs_mut().append_pair(
                "type",
                &SERIES
                    .iter()
                    .map(|(name, _, _)| *name)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        url.to_string()
    }
    async fn get(&self, url: &str) -> ProviderResult<String> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| ProviderError::transport("YAHOO", format!("{url}: {e}")))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ProviderError::transport("YAHOO", format!("{url}: {e}")))?;
        if !status.is_success() {
            return Err(ProviderError::with_http_status(
                "YAHOO",
                status.as_u16(),
                format!("{url}: {body}"),
            ));
        }
        Ok(body)
    }
    pub fn parse_response(body: &str) -> ProviderResult<Vec<FundamentalValue>> {
        let root: Value = serde_json::from_str(body).map_err(|e| bad(e.to_string()))?;
        let envelope = root
            .get("timeseries")
            .ok_or_else(|| bad("missing timeseries"))?;
        check_error(&envelope["error"])?;
        let results = envelope["result"]
            .as_array()
            .ok_or_else(|| bad("missing timeseries result"))?;
        let mut values = Vec::new();
        for series in results {
            let name = series["meta"]["type"][0]
                .as_str()
                .ok_or_else(|| bad("missing series type"))?;
            let Some((_, period_type, metric)) = SERIES.iter().find(|(code, _, _)| *code == name)
            else {
                continue;
            };
            let Some(entries) = series.get(name) else {
                continue;
            };
            for entry in entries
                .as_array()
                .ok_or_else(|| bad("series values must be an array"))?
            {
                if entry.is_null() || entry["reportedValue"].is_null() {
                    continue;
                }
                let expected = match period_type {
                    PeriodType::Quarter => "3M",
                    PeriodType::Annual => "12M",
                    _ => "TTM",
                };
                if entry["periodType"].as_str() != Some(expected) {
                    return Err(bad(format!("unexpected period type for {name}")));
                }
                let period_end = entry["asOfDate"]
                    .as_str()
                    .ok_or_else(|| bad("missing asOfDate"))?
                    .parse()
                    .map_err(|e| bad(format!("invalid period date: {e}")))?;
                let raw = &entry["reportedValue"]["raw"];
                if raw.is_null() {
                    continue;
                }
                let value = decimal(raw)?;
                values.push(FundamentalValue {
                    period_type: *period_type,
                    period_end,
                    metric: *metric,
                    value,
                    currency: entry["currencyCode"].as_str().map(str::to_owned),
                });
            }
        }
        Ok(values)
    }
    pub fn parse_listing(body: &str) -> ProviderResult<ListingDetails> {
        let root: Value = serde_json::from_str(body).map_err(|e| bad(e.to_string()))?;
        check_error(&root["chart"]["error"])?;
        let result = root["chart"]["result"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| bad("missing chart result"))?;
        let mut splits = Vec::new();
        if let Some(events) = result["events"]["splits"].as_object() {
            for event in events.values() {
                let ts = event["date"]
                    .as_i64()
                    .ok_or_else(|| bad("missing split date"))?;
                let offset = result["meta"]["gmtoffset"].as_i64().unwrap_or(0);
                let date = DateTime::<Utc>::from_timestamp(ts + offset, 0)
                    .ok_or_else(|| bad("invalid split date"))?
                    .date_naive();
                let ratio = event["splitRatio"]
                    .as_str()
                    .and_then(|v| v.split_once(':'))
                    .ok_or_else(|| bad("missing split ratio"))?;
                let numerator = Decimal::from_str(ratio.0).map_err(|e| bad(e.to_string()))?;
                let denominator = Decimal::from_str(ratio.1).map_err(|e| bad(e.to_string()))?;
                if numerator <= Decimal::ZERO || denominator <= Decimal::ZERO {
                    return Err(bad("non-positive split ratio"));
                }
                splits.push(SplitEvent {
                    date,
                    numerator,
                    denominator,
                });
            }
        }
        splits.sort_by_key(|s| s.date);
        Ok(ListingDetails {
            security_type: result["meta"]["instrumentType"].as_str().map(str::to_owned),
            splits,
        })
    }
}
fn bad(message: impl Into<String>) -> ProviderError {
    ProviderError::provider_error("YAHOO", message)
}
fn decimal(value: &Value) -> ProviderResult<Decimal> {
    let text = if let Some(text) = value.as_str() {
        text.to_owned()
    } else {
        value.to_string()
    };
    Decimal::from_str(&text)
        .or_else(|_| Decimal::from_scientific(&text))
        .map_err(|e| bad(format!("invalid reportedValue {text}: {e}")))
}
fn check_error(value: &Value) -> ProviderResult<()> {
    if value.is_null() {
        return Ok(());
    }
    let text = value.to_string();
    let lower = text.to_ascii_lowercase();
    let reason = if lower.contains("rate limit") || lower.contains("too many") {
        ProviderMissingReason::RateLimited
    } else if lower.contains("not found")
        || lower.contains("not listed")
        || lower.contains("invalid symbol")
    {
        ProviderMissingReason::NotListed
    } else {
        ProviderMissingReason::ProviderError
    };
    Err(ProviderError::new("YAHOO", reason, text))
}
#[async_trait]
impl FundamentalsProvider for YahooFundamentalsClient {
    async fn fetch(&self, request: &FundamentalsRequest) -> ProviderResult<Vec<FundamentalValue>> {
        Self::parse_response(&self.get(&self.url(request, false)).await?)
    }
    async fn listing(&self, request: &FundamentalsRequest) -> ProviderResult<ListingDetails> {
        Self::parse_listing(&self.get(&self.url(request, true)).await?)
    }
}

#[derive(Default)]
struct FakeState {
    responses: VecDeque<ProviderResult<Vec<FundamentalValue>>>,
    listings: VecDeque<ProviderResult<ListingDetails>>,
    calls: Vec<FundamentalsRequest>,
    listing_calls: Vec<FundamentalsRequest>,
    gate: Option<Arc<Notify>>,
}
#[derive(Clone, Default)]
pub struct FakeFundamentalsProvider {
    state: Arc<Mutex<FakeState>>,
}
impl FakeFundamentalsProvider {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push_response(&self, response: ProviderResult<Vec<FundamentalValue>>) {
        self.state
            .lock()
            .expect("fake mutex")
            .responses
            .push_back(response);
    }
    pub fn push_listing(&self, response: ProviderResult<ListingDetails>) {
        self.state
            .lock()
            .expect("fake mutex")
            .listings
            .push_back(response);
    }
    pub fn calls(&self) -> Vec<FundamentalsRequest> {
        self.state.lock().expect("fake mutex").calls.clone()
    }
    pub fn listing_calls(&self) -> Vec<FundamentalsRequest> {
        self.state.lock().expect("fake mutex").listing_calls.clone()
    }
    pub fn block_next_call_on(&self, gate: Arc<Notify>) {
        self.state.lock().expect("fake mutex").gate = Some(gate);
    }
}
#[async_trait]
impl FundamentalsProvider for FakeFundamentalsProvider {
    async fn fetch(&self, request: &FundamentalsRequest) -> ProviderResult<Vec<FundamentalValue>> {
        let (response, gate) = {
            let mut state = self.state.lock().expect("fake mutex");
            state.calls.push(request.clone());
            (
                state
                    .responses
                    .pop_front()
                    .unwrap_or_else(|| Err(bad("no fake fundamentals response"))),
                state.gate.take(),
            )
        };
        if let Some(gate) = gate {
            gate.notified().await;
        }
        response
    }
    async fn listing(&self, request: &FundamentalsRequest) -> ProviderResult<ListingDetails> {
        let mut state = self.state.lock().expect("fake mutex");
        state.listing_calls.push(request.clone());
        state
            .listings
            .pop_front()
            .unwrap_or_else(|| Err(bad("no fake listing response")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/fundamentals")
                .join(name),
        )
        .unwrap()
    }
    #[test]
    fn all_recorded_yahoo_series_and_chart_fixtures_parse() {
        for symbol in [
            "msft",
            "asml_as",
            "evo_st",
            "volv_b_st",
            "volv_a_st",
            "tsm",
            "orly",
            "iqqk_de",
        ] {
            let values = YahooFundamentalsClient::parse_response(&fixture(&format!(
                "yahoo_timeseries_{symbol}.json"
            )))
            .unwrap();
            let listing = YahooFundamentalsClient::parse_listing(&fixture(&format!(
                "yahoo_chart_{symbol}.json"
            )))
            .unwrap();
            assert_eq!(
                listing.security_type.as_deref(),
                Some(if symbol == "iqqk_de" { "ETF" } else { "EQUITY" })
            );
            if symbol == "iqqk_de" {
                assert!(values.is_empty());
            } else {
                assert!(values
                    .iter()
                    .any(|v| v.period_type == PeriodType::Quarter
                        && v.metric == Metric::DilutedShares));
                assert!(values.iter().any(|v| v.period_type == PeriodType::Evidence));
            }
            if symbol == "tsm" {
                assert!(values
                    .iter()
                    .filter(|v| v.period_type == PeriodType::Quarter)
                    .all(|v| v.currency.as_deref() == Some("TWD")));
            }
            if symbol == "evo_st" {
                assert!(!values.iter().any(|v| v.period_type == PeriodType::Quarter
                    && v.period_end.to_string() == "2025-09-30"));
            }
        }
        let splits =
            YahooFundamentalsClient::parse_listing(&fixture("yahoo_chart_orly_split.json"))
                .unwrap()
                .splits;
        assert!(splits
            .iter()
            .any(|s| s.numerator == Decimal::from(15) && s.denominator == Decimal::ONE));
    }
    #[test]
    fn search_fixtures_preserve_security_type_and_empty_results() {
        for file in [
            "yahoo_search_etf_isin.json",
            "yahoo_search_evo_st_isin.json",
            "yahoo_search_tsm_isin_no_match.json",
            "yahoo_search_tsm_symbol.json",
        ] {
            let rows =
                super::super::YahooSearchClient::parse_response("fixture", &fixture(file)).unwrap();
            if file.contains("no_match") {
                assert!(rows.is_empty());
            } else {
                assert!(!rows.is_empty());
            }
        }
        let rows = super::super::YahooSearchClient::parse_response(
            "TEST",
            r#"{"quotes":[{"symbol":"TEST"}]}"#,
        )
        .unwrap();
        assert_eq!(rows[0].quote_type, None);
    }
    #[test]
    fn request_includes_exact_types_and_dates_and_parser_rejects_bad_values() {
        let client = YahooFundamentalsClient::new();
        let url = Url::parse(&client.url(
            &FundamentalsRequest {
                symbol: "EVO.ST".to_owned(),
                end: "2026-11-07".parse().unwrap(),
            },
            false,
        ))
        .unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(query["period1"], "1420070400");
        assert_eq!(query["type"].split(',').count(), 16);
        assert!(query["type"].contains("annualDilutedNIAvailtoComStockholders"));
        assert!(YahooFundamentalsClient::parse_response("{}").is_err());
        assert_eq!(
            YahooFundamentalsClient::parse_response(
                r#"{"timeseries":{"error":{"description":"Too many requests"}}}"#
            )
            .unwrap_err()
            .reason(),
            ProviderMissingReason::RateLimited
        );
        assert_eq!(
            decimal(&serde_json::json!(3.6629E10)).unwrap(),
            Decimal::from(36_629_000_000i64)
        );
    }
}
