use async_trait::async_trait;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

pub mod frankfurter;
pub mod yahoo;

pub use frankfurter::FrankfurterClient;
pub use yahoo::YahooChartClient;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketDataProvider {
    Yahoo,
    TwelveData,
    Manual,
}

impl MarketDataProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yahoo => "YAHOO",
            Self::TwelveData => "TWELVE_DATA",
            Self::Manual => "MANUAL",
        }
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "YAHOO" => Some(Self::Yahoo),
            "TWELVE_DATA" => Some(Self::TwelveData),
            "MANUAL" => Some(Self::Manual),
            _ => None,
        }
    }
}

impl fmt::Display for MarketDataProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FxProvider {
    Frankfurter,
    Yahoo,
    Manual,
}

impl FxProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Frankfurter => "FRANKFURTER",
            Self::Yahoo => "YAHOO",
            Self::Manual => "MANUAL",
        }
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "FRANKFURTER" => Some(Self::Frankfurter),
            "YAHOO" => Some(Self::Yahoo),
            "MANUAL" => Some(Self::Manual),
            _ => None,
        }
    }
}

impl fmt::Display for FxProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderMissingReason {
    SymbolUnmapped,
    NotListed,
    MarketClosed,
    RateLimited,
    ProviderError,
    NoDataInRange,
}

impl ProviderMissingReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SymbolUnmapped => "symbol_unmapped",
            Self::NotListed => "not_listed",
            Self::MarketClosed => "market_closed",
            Self::RateLimited => "rate_limited",
            Self::ProviderError => "provider_error",
            Self::NoDataInRange => "no_data_in_range",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DailyClose {
    pub provider: MarketDataProvider,
    pub provider_symbol: String,
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FxRate {
    pub provider: FxProvider,
    pub base: String,
    pub quote: String,
    pub date: NaiveDate,
    pub rate: Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderError {
    provider: String,
    reason: ProviderMissingReason,
    message: String,
    status: Option<u16>,
}

impl ProviderError {
    pub fn new(
        provider: impl Into<String>,
        reason: ProviderMissingReason,
        message: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            reason,
            message: message.into(),
            status: None,
        }
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn reason(&self) -> ProviderMissingReason {
        self.reason
    }

    pub fn reason_code(&self) -> &'static str {
        self.reason.as_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn status(&self) -> Option<u16> {
        self.status
    }

    #[allow(clippy::self_named_constructors)]
    pub fn provider_error(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(provider, ProviderMissingReason::ProviderError, message)
    }

    pub fn with_http_status(
        provider: impl Into<String>,
        status: u16,
        message: impl Into<String>,
    ) -> Self {
        Self::new(provider, status_to_reason(status), message).with_status(status)
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.status {
            Some(status) => write!(
                f,
                "{} [{}:{}]: {}",
                self.provider,
                self.reason_code(),
                status,
                self.message
            ),
            None => write!(
                f,
                "{} [{}]: {}",
                self.provider,
                self.reason_code(),
                self.message
            ),
        }
    }
}

impl std::error::Error for ProviderError {}

pub type ProviderResult<T> = Result<T, ProviderError>;

#[async_trait]
pub trait PriceProvider: Send + Sync {
    async fn daily_history(
        &self,
        symbol: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> ProviderResult<Vec<DailyClose>>;
}

#[async_trait]
pub trait FxRateProvider: Send + Sync {
    async fn fx_history(
        &self,
        base: &str,
        quote: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> ProviderResult<Vec<FxRate>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceHistoryRequest {
    pub symbol: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FxHistoryRequest {
    pub base: String,
    pub quote: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
}

#[derive(Clone)]
pub struct FakePriceProvider {
    provider: MarketDataProvider,
    state: Arc<Mutex<FakePriceProviderState>>,
}

#[derive(Default)]
struct FakePriceProviderState {
    responses: VecDeque<ProviderResult<Vec<DailyClose>>>,
    calls: Vec<PriceHistoryRequest>,
    next_call_gate: Option<Arc<Notify>>,
}

impl FakePriceProvider {
    pub fn new() -> Self {
        Self::with_provider(MarketDataProvider::Manual)
    }

    pub fn with_provider(provider: MarketDataProvider) -> Self {
        Self {
            provider,
            state: Arc::new(Mutex::new(FakePriceProviderState::default())),
        }
    }

    pub fn push_response(&self, response: ProviderResult<Vec<DailyClose>>) {
        self.state
            .lock()
            .expect("fake price provider mutex poisoned")
            .responses
            .push_back(response);
    }

    pub fn block_next_call_on(&self, gate: Arc<Notify>) {
        self.state
            .lock()
            .expect("fake price provider mutex poisoned")
            .next_call_gate = Some(gate);
    }

    pub fn calls(&self) -> Vec<PriceHistoryRequest> {
        self.state
            .lock()
            .expect("fake price provider mutex poisoned")
            .calls
            .clone()
    }
}

impl Default for FakePriceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PriceProvider for FakePriceProvider {
    async fn daily_history(
        &self,
        symbol: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> ProviderResult<Vec<DailyClose>> {
        let (gate, response) = {
            let mut state = self
                .state
                .lock()
                .expect("fake price provider mutex poisoned");
            state.calls.push(PriceHistoryRequest {
                symbol: symbol.to_owned(),
                start,
                end,
            });
            let gate = state.next_call_gate.take();
            let response = state.responses.pop_front().unwrap_or_else(|| {
                Err(ProviderError::provider_error(
                    self.provider.as_str(),
                    format!("no fake price response configured for {symbol}"),
                ))
            });
            (gate, response)
        };

        if let Some(gate) = gate {
            gate.notified().await;
        }

        response
    }
}

#[derive(Clone)]
pub struct FakeFxRateProvider {
    provider: FxProvider,
    state: Arc<Mutex<FakeFxRateProviderState>>,
}

#[derive(Default)]
struct FakeFxRateProviderState {
    responses: VecDeque<ProviderResult<Vec<FxRate>>>,
    calls: Vec<FxHistoryRequest>,
}

impl FakeFxRateProvider {
    pub fn new() -> Self {
        Self::with_provider(FxProvider::Manual)
    }

    pub fn with_provider(provider: FxProvider) -> Self {
        Self {
            provider,
            state: Arc::new(Mutex::new(FakeFxRateProviderState::default())),
        }
    }

    pub fn push_response(&self, response: ProviderResult<Vec<FxRate>>) {
        self.state
            .lock()
            .expect("fake fx provider mutex poisoned")
            .responses
            .push_back(response);
    }

    pub fn calls(&self) -> Vec<FxHistoryRequest> {
        self.state
            .lock()
            .expect("fake fx provider mutex poisoned")
            .calls
            .clone()
    }
}

impl Default for FakeFxRateProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FxRateProvider for FakeFxRateProvider {
    async fn fx_history(
        &self,
        base: &str,
        quote: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> ProviderResult<Vec<FxRate>> {
        let mut state = self.state.lock().expect("fake fx provider mutex poisoned");
        state.calls.push(FxHistoryRequest {
            base: base.to_owned(),
            quote: quote.to_owned(),
            start,
            end,
        });
        state.responses.pop_front().unwrap_or_else(|| {
            Err(ProviderError::provider_error(
                self.provider.as_str(),
                format!("no fake fx response configured for {base}/{quote}"),
            ))
        })
    }
}

fn status_to_reason(status: u16) -> ProviderMissingReason {
    match status {
        404 => ProviderMissingReason::NotListed,
        429 => ProviderMissingReason::RateLimited,
        _ => ProviderMissingReason::ProviderError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn fake_providers_record_calls_and_return_queued_results() {
        let price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        let fx = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

        price.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
            close: dec!(390.74),
            currency: "USD".to_owned(),
        }]));
        fx.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
            rate: dec!(9.47),
        }]));

        let prices = price
            .daily_history(
                "MSFT",
                NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid"),
                NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
            )
            .await
            .expect("fake price call should succeed");
        assert_eq!(prices.len(), 1);

        let rates = fx
            .fx_history(
                "USD",
                "SEK",
                NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid"),
                NaiveDate::from_ymd_opt(2026, 6, 12).expect("date should be valid"),
            )
            .await
            .expect("fake fx call should succeed");
        assert_eq!(rates.len(), 1);

        assert_eq!(price.calls().len(), 1);
        assert_eq!(price.calls()[0].symbol, "MSFT");
        assert_eq!(fx.calls().len(), 1);
        assert_eq!(fx.calls()[0].base, "USD");
    }

    #[test]
    fn provider_error_codes_are_stable() {
        let error = ProviderError::with_http_status(
            MarketDataProvider::Yahoo.as_str(),
            429,
            "too many requests",
        );

        assert_eq!(error.reason(), ProviderMissingReason::RateLimited);
        assert_eq!(error.reason_code(), "rate_limited");
        assert_eq!(error.status(), Some(429));
    }
}
